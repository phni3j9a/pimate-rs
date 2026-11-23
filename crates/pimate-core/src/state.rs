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
    Upsert(Item),
    Remove { collection: String, id: String },
}
/// Called serially on the core runtime. Enqueue UI work and return without blocking.
pub trait Listener: Send + Sync {
    fn changed(&self, changes: Vec<Change>);
}
#[derive(Default)]
pub(crate) struct State {
    pub auth_targets: std::collections::HashMap<String, String>,
    rows: IndexMap<String, IndexMap<String, Value>>,
    pub changes: Vec<Change>,
    pub selected: Option<String>,
    seq: Option<u64>,
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
    fn clear(&mut self, c: &str) {
        if let Some(r) = self.rows.shift_remove(c) {
            self.changes.extend(r.into_keys().map(|id| Change::Remove {
                collection: c.into(),
                id,
            }))
        }
    }
    pub fn reset(&mut self) {
        self.seq = None;
        for c in [
            "entry", "question", "quota", "run", "tool", "stream", "view",
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
        let images = v["images"].as_array().cloned().unwrap_or_default();
        if let Some(i) = self.drafts.iter().position(|d| {
            self.selected.as_ref() == Some(&d.instance)
                && self.get("view", "session")["id"] == d.session
                && !d.known.contains(&id)
                && v["text"].as_str().map(str::trim) == d.params["text"].as_str().map(str::trim)
                && v["imageCount"].as_u64().unwrap_or(images.len() as u64) == images.len() as u64
                && v["imageIndices"].as_array().is_none_or(|indices| {
                    indices
                        .iter()
                        .enumerate()
                        .all(|(i, n)| n.as_u64() == Some(i as u64))
                })
                && d.params["images"].as_array().cloned().unwrap_or_default() == images
        }) {
            let id = self.drafts.remove(i).id;
            self.remove("draft", &id)
        }
    }
    fn keyed(&mut self, c: &str, key: &str, v: &Value) {
        if let Some(id) = v[key].as_str() {
            self.put(c, id, v.clone())
        }
    }
    /// Frames have already passed HostMessage validation and normalization.
    pub fn apply(&mut self, m: &Value) -> bool {
        let t = m["type"].as_str().unwrap_or("");
        if t == "instances" {
            let mut list = m["instances"].as_array().cloned().unwrap_or_default();
            list.sort_by_key(|v| {
                (
                    v["status"] != "waiting_input",
                    v["origin"] == "grok-bot",
                    std::cmp::Reverse(v["updatedAt"].as_u64()),
                )
            });
            let next: IndexMap<_, _> = list
                .into_iter()
                .filter_map(|mut v| {
                    v["section"] = json!(if v["status"] == "waiting_input" {
                        "waiting"
                    } else if v["origin"] == "grok-bot" {
                        "bot"
                    } else {
                        "normal"
                    });
                    Some((v["instanceId"].as_str()?.to_owned(), v))
                })
                .collect();
            let prev = self
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
            for (i, (id, v)) in next.iter().enumerate() {
                if prev
                    .get_full(id)
                    .is_none_or(|(p, _, old)| p != i || old != v)
                {
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
                self.reset();
                let s = &m["snapshot"];
                self.seq = s["seq"].as_u64();
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
                for v in m["history"]["items"].as_array().into_iter().flatten() {
                    self.entry(v.clone())
                }
                self.put("view", "phase", json!("ready"))
            }
            "subscribe_error" | "instance_gone" => {
                self.reset();
                self.put("view", "phase", json!(t))
            }
            "event" => {
                let Some(seq) = m["seq"].as_u64() else {
                    return false;
                };
                if self.seq.is_none_or(|n| seq <= n) {
                    return false;
                }
                self.seq = Some(seq);
                let e = &m["event"];
                match e["type"].as_str().unwrap_or("") {
                    "session_changed" => {
                        self.reset();
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
                    "ui_request" => self.keyed("question", "requestId", &e["request"]),
                    "ui_request_end" => {
                        self.remove("question", e["requestId"].as_str().unwrap_or_default())
                    }
                    "ui_prompt" => self.put(
                        "view",
                        "pcPrompt",
                        if e["phase"] == "end" {
                            Value::Null
                        } else {
                            e.clone()
                        },
                    ),
                    "agent_start" => self.put("view", "agentActive", json!(true)),
                    "agent_settled" => {
                        self.clear("stream");
                        self.put("view", "agentActive", json!(false))
                    }
                    "thinking" => self.put("view", "thinking", e["level"].clone()),
                    "quota" => self.keyed("quota", "source", &e["quota"]),
                    "subagent_run" => self.keyed("run", "id", &e["run"]),
                    "subagents_reset" => self.runs(&e["state"]),
                    k @ ("context" | "status" | "model") => self.put("view", k, e[k].clone()),
                    "tool_start" | "tool_update" | "tool_end" => {
                        let id = e["toolCallId"].as_str().unwrap_or_default();
                        let mut tool = self
                            .get("tool", id)
                            .as_object()
                            .cloned()
                            .unwrap_or_default();
                        tool.extend(e.as_object().unwrap().clone());
                        self.put("tool", id, Value::Object(tool))
                    }
                    k @ ("session_op_done" | "compact_done" | "compact_error") => {
                        self.put("view", k, e.clone())
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }
    fn runs(&mut self, v: &Value) {
        self.clear("run");
        for r in v["runs"].as_array().into_iter().flatten() {
            self.keyed("run", "id", r)
        }
        self.put("view", "backends", v["backends"].clone())
    }
    pub fn questions(&self) -> bool {
        self.rows.get("question").is_some_and(|r| !r.is_empty())
    }
}
