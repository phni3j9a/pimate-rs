use indexmap::IndexMap;
use serde_json::{Value, json};
/// `position` is relative to the collection; `json` contains one item, never a whole conversation.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub collection: String,
    pub id: String,
    pub position: u64,
    pub json: String,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    ResyncRequired,
    Upsert(Item),
    Remove { collection: String, id: String },
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub revision: u64,
    pub items: Vec<Item>,
}
/// Called in revision order on a dedicated delivery thread. On ResyncRequired replace the mirror with snapshot().
pub trait Listener: Send + Sync {
    fn changed(&self, revision: u64, changes: Vec<Change>);
}
#[derive(Default)]
pub(crate) struct State {
    pub revision: u64,
    pub auth_targets: std::collections::HashMap<String, String>,
    rows: IndexMap<String, IndexMap<String, Value>>,
    pub changes: Vec<Change>,
    pub selected: Option<String>,
    seq: Option<u64>,
    effect_seq: Option<u64>,
    pub caps: Value,
    pub drafts: Vec<Draft>,
}
pub(crate) struct Draft {
    pub id: String,
    pub instance: String,
    pub session: Value,
    pub params: Value,
    pub known: std::collections::HashSet<String>,
}
fn item(c: &str, id: &str, pos: usize, v: &Value) -> Item {
    Item {
        collection: c.into(),
        id: id.into(),
        position: pos as u64,
        json: v.to_string(),
    }
}
impl State {
    pub(crate) fn models(&self) -> IndexMap<String, Value> {
        self.rows.get("model").cloned().unwrap_or_default()
    }
    pub fn snapshot(&self) -> Vec<Item> {
        self.rows
            .iter()
            .flat_map(|(c, rows)| {
                rows.iter()
                    .enumerate()
                    .map(move |(i, (id, v))| item(c, id, i, v))
            })
            .collect()
    }
    pub fn get(&self, c: &str, id: &str) -> &Value {
        self.rows
            .get(c)
            .and_then(|r| r.get(id))
            .unwrap_or(&Value::Null)
    }
    pub fn put(&mut self, c: &str, id: &str, v: Value) {
        let rows = self.rows.entry(c.into()).or_default();
        if rows.get(id) == Some(&v) {
            return;
        }
        let (pos, _) = rows.insert_full(id.into(), v);
        self.changes
            .push(Change::Upsert(item(c, id, pos, &rows[id])))
    }
    pub fn remove(&mut self, c: &str, id: &str) {
        if self
            .rows
            .get_mut(c)
            .is_some_and(|r| r.shift_remove(id).is_some())
        {
            self.changes.push(Change::Remove {
                collection: c.into(),
                id: id.into(),
            })
        }
    }
    pub(crate) fn clear(&mut self, c: &str) {
        if let Some(r) = self.rows.shift_remove(c) {
            self.changes.extend(r.into_keys().map(|id| Change::Remove {
                collection: c.into(),
                id,
            }))
        }
    }
    pub fn reset(&mut self) {
        self.effect_seq = None;
        self.clear("busy");
        self.clear("operationResult");
        self.clear("notice");
        self.reset_view()
    }
    fn reset_view(&mut self) {
        self.seq = None;
        self.remove("busy", "answer");
        for c in [
            "entry", "question", "quota", "run", "tool", "stream", "view", "command", "session",
            "model", "node",
        ] {
            self.clear(c)
        }
    }
    pub fn ready(&self) -> bool {
        self.seq.is_some()
    }
    pub fn known(&self) -> std::collections::HashSet<String> {
        self.rows
            .get("entry")
            .into_iter()
            .flat_map(|r| r.keys().cloned())
            .collect()
    }
    pub fn draft_status(&mut self, id: &str, status: &str) {
        let mut v = self.get("draft", id).clone();
        if !v.is_null() {
            v["status"] = json!(status);
            self.put("draft", id, v);
            if matches!(status, "unconfirmed" | "queued")
                && self.drafts.iter().find(|d| d.id == id).is_some_and(|d| {
                    self.rows
                        .get("entry")
                        .is_some_and(|r| r.values().any(|v| self.matches_entry(d, v)))
                })
            {
                self.forget_draft(id)
            }
        }
    }
    pub fn release(&mut self, id: &str) {
        for lock in ["operation", "answer"] {
            if self.get("busy", lock)["id"] == id {
                self.remove("busy", lock)
            }
        }
    }
    pub fn forget_draft(&mut self, id: &str) {
        self.drafts.retain(|d| d.id != id);
        self.remove("draft", id)
    }
    fn entry(&mut self, v: Value) {
        let id = v["entryId"].as_str().unwrap_or_default().to_owned();
        self.put("entry", &id, v.clone());
        if v["kind"] != "user" {
            return;
        }

        if let Some(i) = self.drafts.iter().position(|d| {
            matches!(
                self.get("draft", &d.id)["status"].as_str(),
                Some("queued" | "unconfirmed")
            ) && self.matches_entry(d, &v)
        }) {
            let id = self.drafts.remove(i).id;
            self.remove("draft", &id)
        }
    }
    fn matches_entry(&self, d: &Draft, v: &Value) -> bool {
        v["kind"] == "user"
            && self.selected.as_ref() == Some(&d.instance)
            && self.get("view", "session")["id"] == d.session
            && v["entryId"]
                .as_str()
                .is_some_and(|id| !d.known.contains(id))
            && v["text"].as_str().map(str::trim) == d.params["text"].as_str().map(str::trim)
            && same_images(v, &d.params)
    }
    pub fn prompt_ack(&mut self, id: &str, delivery: &Value) {
        if self.get("draft", id)["status"] != "sending" {
            return;
        }
        if matches!(delivery.as_str(), Some("steer" | "followUp")) {
            self.draft_status(id, "queued")
        } else {
            self.forget_draft(id)
        }
    }
    pub(crate) fn keyed(&mut self, c: &str, key: &str, v: &Value) {
        if let Some(id) = v[key].as_str() {
            self.put(c, id, v.clone())
        }
    }
    /// Frames have already passed HostMessage validation and normalization.
    pub fn apply(&mut self, m: &Value) -> bool {
        let t = m["type"].as_str().unwrap_or("");
        if t == "instances" {
            self.put("host", "listKnown", json!(true));
            let mut list = m["instances"].as_array().cloned().unwrap_or_default();
            list.sort_by_key(|v| {
                (
                    !attention(v),
                    !attention(v) && v["origin"] == "grok-bot",
                    std::cmp::Reverse(v["updatedAt"].as_u64()),
                )
            });
            let next: IndexMap<_, _> = list
                .into_iter()
                .filter_map(|mut v| {
                    v["section"] = json!(if attention(&v) {
                        "waiting"
                    } else if v["origin"] == "grok-bot" {
                        "bot"
                    } else {
                        "normal"
                    });
                    Some((v["instanceId"].as_str()?.to_owned(), v))
                })
                .collect();
            let mut prev = self
                .rows
                .insert("instance".into(), next)
                .unwrap_or_default();
            let next = &self.rows["instance"];
            for id in prev.keys().filter(|id| !next.contains_key(*id)) {
                self.changes.push(Change::Remove {
                    collection: "instance".into(),
                    id: id.clone(),
                })
            }
            prev.retain(|id, _| next.contains_key(id));
            for (i, (id, v)) in next.iter().enumerate() {
                let changed = prev.get(id) != Some(v);
                let pos = prev.get_index_of(id).unwrap_or_else(|| {
                    prev.insert(id.clone(), Value::Null);
                    prev.len() - 1
                });
                if pos != i || changed {
                    prev.move_index(pos, i);
                    self.changes
                        .push(Change::Upsert(item("instance", id, i, v)))
                }
            }
            return false;
        }
        if self.selected.is_none() || self.selected.as_deref() != m["instanceId"].as_str() {
            return false;
        }
        match t {
            "subscribed" => {
                // A fresh snapshot is not proof that a pending operation completed.
                self.finish_missing_op(&m["snapshot"]["session"]["id"]);
                self.reset_view();
                let s = &m["snapshot"];
                self.seq = s["seq"].as_u64();
                self.effect_seq = self.seq;
                for (k, v) in s.as_object().into_iter().flatten() {
                    match k.as_str() {
                        "pendingUI" => {
                            for q in v.as_array().into_iter().flatten() {
                                self.keyed("question", "requestId", q)
                            }
                        }
                        "quotas" => {
                            for q in v.as_array().into_iter().flatten() {
                                self.keyed("quota", "source", q)
                            }
                        }
                        "subagents" => self.runs(v),
                        "streaming" => {
                            for k in ["text", "thinking"] {
                                self.put("stream", k, v[k].clone())
                            }
                        }
                        _ => self.put("view", k, v.clone()),
                    }
                }
                if !m["history"]["session"].is_null() {
                    self.put("view", "session", m["history"]["session"].clone())
                }
                let entries: IndexMap<_, _> = m["history"]["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| Some((v["entryId"].as_str()?, v)))
                    .collect();
                if s["agentActive"].is_null() {
                    let active = match s["status"].as_str() {
                        Some("running") => true,
                        Some("waiting_input") => {
                            ["text", "thinking"]
                                .iter()
                                .any(|k| s["streaming"][k].as_str().is_some_and(|t| !t.is_empty()))
                                || entries
                                    .values()
                                    .rev()
                                    .find(|e| e["kind"] != "other")
                                    .is_some_and(|e| {
                                        e["kind"] != "assistant" || e["stopReason"] == "toolUse"
                                    })
                        }
                        _ => false,
                    };
                    self.put("view", "agentActive", json!(active))
                }
                for v in entries.into_values() {
                    self.entry(v.clone())
                }
                self.builtins();
                self.put("view", "phase", json!("ready"))
            }
            "subscribe_error" | "instance_gone" => {
                if t == "instance_gone" {
                    self.reset()
                } else {
                    self.reset_view()
                }
                self.put("view", "phase", json!(t));
                if t == "subscribe_error" {
                    self.put("view", "error", m["code"].clone())
                }
            }
            "event" => {
                let Some(seq) = m["seq"].as_u64() else {
                    return false;
                };
                let e = &m["event"];
                let k = e["type"].as_str().unwrap_or_default();
                // Host forwards these before `subscribed`: they are not represented in history.
                if matches!(
                    k,
                    "session_op_done" | "compact_done" | "compact_error" | "notify"
                ) {
                    if self.seq.max(self.effect_seq).is_some_and(|n| seq <= n) {
                        return false;
                    }
                    self.effect_seq = Some(seq);
                    if k == "notify" {
                        self.put("notice", "latest", json!({"seq":seq,"event":e}));
                        return false;
                    }
                    let op = self.get("busy", "operation");
                    if !(k == "session_op_done"
                        && op["method"] == "session_op"
                        && op["op"] == e["op"]
                        || k.starts_with("compact_")
                            && op["method"] == "compact"
                            && op["sessionId"] == m["sessionId"])
                    {
                        return false;
                    }
                    let mut result = json!({"id":op["id"],"sessionId":m["sessionId"],"event":e});
                    if e["op"] == "tree"
                        && (op["sessionId"] != m["sessionId"]
                            || self.ready() && op["sessionId"] != self.get("view", "session")["id"]
                            || !op["arrival"].is_null() && op["arrival"] != op["sessionId"])
                    {
                        result = json!({"id":op["id"],"outcome":"unknown"})
                    }
                    self.put("operationResult", "latest", result);
                    self.remove("busy", "operation");
                    return false;
                }
                if self.seq.is_none_or(|n| seq <= n) {
                    return false;
                }
                if m["event"]["type"] != "session_changed"
                    && m["sessionId"] != self.get("view", "session")["id"]
                {
                    return false;
                }
                self.seq = Some(seq);
                let e = &m["event"];
                match e["type"].as_str().unwrap_or("") {
                    "session_changed" => {
                        let mut op = self.get("busy", "operation").clone();
                        if !op.is_null()
                            && op["method"] == "session_op"
                            && op["op"] == e["reason"]
                            && op["sessionId"] == self.get("view", "session")["id"]
                        {
                            if op["op"] == "reload" {
                                self.put(
                                    "operationResult",
                                    "latest",
                                    json!({"id":op["id"],"outcome":"done"}),
                                );
                                self.remove("busy", "operation")
                            } else {
                                op["arrival"] = e["session"]["id"].clone();
                                self.put("busy", "operation", op)
                            }
                        }
                        self.reset_view();
                        self.put("view", "phase", json!("resyncing"));
                        return true;
                    }
                    "session_info" => {
                        let mut s = self.get("view", "session").clone();
                        if !s.is_null() {
                            s["name"] = e["name"].clone();
                            self.put("view", "session", s)
                        }
                    }
                    "message_end" => {
                        if e["item"]["kind"] == "assistant" {
                            self.clear("stream")
                        }
                        self.entry(e["item"].clone())
                    }
                    "text_delta" | "thinking_delta" => {
                        let k = if e["type"] == "text_delta" {
                            "text"
                        } else {
                            "thinking"
                        };
                        let mut text = self
                            .get("stream", k)
                            .as_str()
                            .unwrap_or_default()
                            .to_owned();
                        text.push_str(e["delta"].as_str().unwrap_or_default());
                        self.put("stream", k, json!(text))
                    }
                    "ui_request" => {
                        let req = &e["request"];
                        if mirrored(req, self.get("view", "pcPrompt")) {
                            self.remove("view", "pcPrompt")
                        }
                        self.keyed("question", "requestId", req)
                    }
                    "ui_request_end" => {
                        self.remove("question", e["requestId"].as_str().unwrap_or_default())
                    }
                    "ui_prompt" => self.put(
                        "view",
                        "pcPrompt",
                        if e["phase"] == "end"
                            || self
                                .rows
                                .get("question")
                                .is_some_and(|r| r.values().any(|q| mirrored(q, e)))
                        {
                            Value::Null
                        } else {
                            e.clone()
                        },
                    ),
                    "agent_start" => {
                        if self.get("view", "status") == "idle" {
                            self.put("view", "status", json!("running"))
                        }
                        self.put("view", "agentActive", json!(true))
                    }
                    "agent_settled" => {
                        self.settle_drafts(&m["instanceId"], &m["sessionId"], e["aborted"] == true);
                        let tools = self
                            .rows
                            .get("tool")
                            .into_iter()
                            .flat_map(|r| r.iter())
                            .filter(|(_, v)| v["status"] == "running")
                            .map(|(id, v)| (id.clone(), v.clone()))
                            .collect::<Vec<_>>();
                        for (id, mut v) in tools {
                            v["status"] = json!("done");
                            self.put("tool", &id, v)
                        }
                        self.clear("stream");
                        self.put("view", "agentActive", json!(false))
                    }
                    "thinking" => self.put("view", "thinking", e["level"].clone()),
                    "quota" => self.keyed("quota", "source", &e["quota"]),
                    "subagent_run" => self.run(&e["run"]),
                    "subagents_reset" => self.runs(&e["state"]),
                    k @ ("context" | "status" | "model") => {
                        if k == "status" && e[k] != "waiting_input" {
                            self.put("view", "agentActive", json!(e[k] == "running"))
                        }
                        self.put("view", k, e[k].clone())
                    }
                    "tool_start" | "tool_update" | "tool_end" => {
                        let id = e["toolCallId"].as_str().unwrap_or_default();
                        let mut tool = self
                            .get("tool", id)
                            .as_object()
                            .cloned()
                            .unwrap_or_default();
                        if tool.is_empty() && e["type"] != "tool_start" {
                            return false;
                        }
                        tool.extend(e.as_object().unwrap().clone());
                        tool.insert(
                            "status".into(),
                            json!(if e["type"] != "tool_end" {
                                "running"
                            } else if e["isError"] == true {
                                "error"
                            } else {
                                "done"
                            }),
                        );
                        self.put("tool", id, Value::Object(tool))
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }
    pub fn settle_drafts(&mut self, instance: &Value, session: &Value, aborted: bool) {
        let ids = self
            .drafts
            .iter()
            .filter(|d| {
                d.instance == *instance
                    && d.session == *session
                    && (self.get("draft", &d.id)["status"] == "queued"
                        || aborted && self.get("draft", &d.id)["status"] == "sending")
            })
            .map(|d| d.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            if aborted {
                self.draft_status(&id, "unconfirmed")
            } else {
                self.forget_draft(&id)
            }
        }
    }
    fn finish_missing_op(&mut self, session: &Value) {
        let op = self.get("busy", "operation");
        if op["accepted"] == true || !op["arrival"].is_null() {
            self.put(
                "operationResult",
                "latest",
                json!({"id":op["id"],"outcome":if op["op"]!="tree"&&!op["arrival"].is_null()&&op["arrival"]==*session&&op["sessionId"]!=*session{"done"}else{"unknown"}}),
            );
            self.remove("busy", "operation")
        }
    }
    fn runs(&mut self, v: &Value) {
        self.clear("run");
        self.put("view", "backends", v["backends"].clone());
        for r in v["runs"].as_array().into_iter().flatten() {
            self.run(r)
        }
    }
    fn run(&mut self, r: &Value) {
        if self.get("view", "backends").as_array().is_some_and(|b| {
            b.iter()
                .any(|b| b["backend"] == r["backend"] && b["supported"] == false)
        }) {
            return;
        }
        let mut r = r.clone();
        let active = |v: &Value| {
            matches!(
                v["state"].as_str(),
                Some("running" | "queued" | "waiting" | "stopping")
            )
        };
        let children = r["children"].as_array().cloned().unwrap_or_default();
        let is_active = active(&r) || children.iter().any(active);
        r["active"] = json!(is_active);
        r["open"] = json!(
            is_active || r["state"] == "paused" || children.iter().any(|c| c["state"] == "paused")
        );
        r["stopAction"] = json!(["stop", "interrupt", "abort_parent"].into_iter().find(|a| {
            r["actions"]
                .as_array()
                .is_some_and(|l| l.iter().any(|v| v == a))
        }));
        self.keyed("run", "id", &r)
    }
    pub fn questions(&self) -> bool {
        self.rows.get("question").is_some_and(|r| !r.is_empty())
    }
}

fn attention(v: &Value) -> bool {
    v["status"] == "waiting_input" || v["subagents"]["waiting"].as_u64().unwrap_or(0) > 0
}

fn mirrored(req: &Value, p: &Value) -> bool {
    !p.is_null()
        && (req["kind"] == "questionnaire"
            || req["kind"] == p["kind"] && (p["title"].is_null() || p["title"] == req["title"]))
}
#[derive(Clone, Debug, Default)]
pub struct SessionStats {
    pub messages: u64,
    pub tokens: f64,
    pub cost: f64,
}
#[derive(Clone, Debug)]
pub struct QuotaWindow {
    pub minutes: u64,
    pub used_percent: Option<f64>,
    pub resets_at: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct Allowance {
    pub source: String,
    pub observed_at: Option<u64>,
    pub gauge: Option<f64>,
    pub windows: Vec<QuotaWindow>,
}
impl State {
    pub fn stats(&self) -> SessionStats {
        let mut stats = SessionStats::default();
        for item in self.rows.get("entry").into_iter().flat_map(|r| r.values()) {
            if matches!(item["kind"].as_str(), Some("user" | "assistant")) {
                stats.messages += 1
            }
            if item["kind"] == "assistant" {
                let usage = &item["usage"];
                stats.tokens += ["input", "output", "cacheRead", "cacheWrite"]
                    .iter()
                    .filter_map(|k| usage[k].as_f64())
                    .sum::<f64>();
                stats.cost += usage["cost"].as_f64().unwrap_or_default()
            }
        }
        stats
    }
    pub fn allowances(&self, now: u64) -> Vec<Allowance> {
        ["claude", "codex"]
            .into_iter()
            .map(|source| {
                let q = self.get("quota", source);
                let mut windows = q["windows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|w| {
                        let resets_at = w["resetsAt"].as_u64();
                        QuotaWindow {
                            minutes: w["minutes"].as_u64().unwrap_or_default(),
                            used_percent: if resets_at.is_some_and(|t| t <= now) {
                                None
                            } else {
                                w["usedPercent"].as_f64().map(f64::round)
                            },
                            resets_at,
                        }
                    })
                    .collect::<Vec<_>>();
                windows.sort_by_key(|w| w.minutes);
                Allowance {
                    source: source.into(),
                    observed_at: q["observedAt"].as_u64(),
                    gauge: windows.first().and_then(|w| w.used_percent),
                    windows,
                }
            })
            .collect()
    }
    pub fn suggestions(&self, text: &str) -> Vec<String> {
        let Some(prefix) = text.strip_prefix('/') else {
            return vec![];
        };
        if prefix.chars().any(char::is_whitespace) {
            return vec![];
        }
        self.rows
            .get("command")
            .into_iter()
            .flat_map(|r| r.keys())
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect()
    }
}

fn same_images(item: &Value, params: &Value) -> bool {
    let images = item["images"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let sent = params["images"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if images.len() != sent.len()
        || item["imageCount"].as_u64().unwrap_or(images.len() as u64) != sent.len() as u64
    {
        return false;
    }
    let mut seen = 0u8;
    images.iter().enumerate().all(|(i, img)| {
        let pos = item["imageIndices"]
            .get(i)
            .map_or(Some(i as u64), Value::as_u64);
        let Some(pos) = pos.filter(|n| *n < sent.len() as u64 && *n < 8) else {
            return false;
        };
        let bit = 1 << pos;
        let fresh = seen & bit == 0;
        seen |= bit;
        fresh && &sent[pos as usize] == img
    })
}
