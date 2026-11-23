use crate::{config, rc::Rc, webhook as hook};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, broadcast, watch},
    time::timeout,
};
use tokio_util::task::AbortOnDropHandle;
struct Tracker {
    seq: u64,
    session: Value,
    started: bool,
    waiting: bool,
    stop: Value,
    children: HashMap<String, Value>,
}
impl Tracker {
    fn new(s: &Value) -> Self {
        let mut t = Self {
            seq: s["seq"].as_u64().unwrap_or_default(),
            session: s["session"]["id"].clone(),
            started: false,
            waiting: s["status"] == "waiting_input",
            stop: Value::Null,
            children: Default::default(),
        };
        t.seed(&s["subagents"]);
        t
    }
    fn seed(&mut self, s: &Value) {
        self.children.clear();
        for r in s["runs"].as_array().into_iter().flatten() {
            self.remember(r);
        }
    }
    fn remember(&mut self, r: &Value) -> Option<&'static str> {
        let mut rank = 0;
        let mut one = |id: &Value, state: &Value| {
            let key = json!([r["id"], id]).to_string();
            if self
                .children
                .insert(key, state.clone())
                .is_some_and(|prev| prev != *state)
            {
                rank = rank.max(match state.as_str() {
                    Some("failed") => 3,
                    Some("paused") => 2,
                    Some("waiting") => 1,
                    _ => 0,
                })
            }
        };
        if let Some(cs) = r["children"].as_array().filter(|a| !a.is_empty()) {
            for c in cs {
                one(&c["id"], &c["state"])
            }
        } else {
            one(&Value::Null, &r["state"])
        }
        [
            None,
            Some("child_waiting"),
            Some("child_paused"),
            Some("child_failed"),
        ][rank]
    }
    fn next(&mut self, m: &Value) -> Option<Value> {
        let seq = m["seq"].as_u64()?;
        if seq <= self.seq {
            return None;
        }
        self.seq = seq;
        let e = &m["event"];
        if m["sessionId"] != self.session || e["type"] == "session_changed" {
            self.session = m["sessionId"].clone();
            self.started = false;
            self.stop = Value::Null;
            self.waiting = false;
            self.children.clear();
            return None;
        }
        let mut kind = None;
        match e["type"].as_str() {
            Some("agent_start") => {
                self.started = true;
                self.stop = Value::Null
            }
            Some("message_end") if e["item"]["kind"] == "assistant" => {
                self.stop = e["item"]["stopReason"].clone()
            }
            Some("agent_settled") => {
                if self.started && e["aborted"] != true {
                    kind = match self.stop.as_str() {
                        Some("stop") => Some("complete"),
                        Some("error") => Some("failed"),
                        _ => None,
                    }
                }
                self.started = false;
                self.stop = Value::Null
            }
            Some("status") => {
                let w = e["status"] == "waiting_input";
                if w && !self.waiting {
                    kind = Some("input")
                }
                self.waiting = w
            }
            Some("subagent_run") => kind = self.remember(&e["run"]),
            Some("subagents_reset") => self.seed(&e["state"]),
            _ => (),
        };
        let id = m["instanceId"].as_str()?;
        let session = self.session.as_str()?;
        Some(json!({"id":format!("{id}:{seq}"),"instanceId":id,"sessionId":session,"kind":kind?}))
    }
}
#[derive(Default)]
struct State {
    opened: HashMap<String, i64>,
    pending: HashMap<String, Option<Value>>,
}
// Opened is durable only after delivery succeeds. Hold a racing close until then.
struct Delivery {
    dir: PathBuf,
    state: Mutex<State>,
    initial: Arc<Semaphore>,
    retries: Semaphore,
}
impl Delivery {
    fn send(self: &Arc<Self>, v: Value, k: Option<String>) {
        let Ok(slot) = self.initial.clone().try_acquire_owned() else {
            eprintln!("webhook queue full");
            if let Some(k) = k {
                self.state.lock().unwrap().pending.remove(&k);
            }
            return;
        };
        let this = self.clone();
        tokio::spawn(async move {
            let ok = hook::deliver(&this.dir, v, slot, &this.retries).await;
            if let Some(k) = k {
                let next = {
                    let mut s = this.state.lock().unwrap();
                    let next = s.pending.remove(&k).flatten();
                    if ok {
                        s.opened
                            .insert(k.clone(), chrono::Utc::now().timestamp_millis());
                        if s.opened.len() > 1000
                            && let Some(old) = s
                                .opened
                                .iter()
                                .min_by_key(|(_, v)| *v)
                                .map(|(k, _)| k.clone())
                        {
                            s.opened.remove(&old);
                        }
                        if crate::service::write(
                            &this.dir.join("webhook-state.json"),
                            json!(s.opened).to_string().as_bytes(),
                        )
                        .is_err()
                        {
                            eprintln!("webhook state write failed")
                        }
                    }
                    next
                };
                if ok
                    && !hook::answered(&this.dir, &k)
                    && let Some(v) = next
                {
                    this.send(v, None);
                }
            }
        });
    }
    fn open(self: &Arc<Self>, k: String, v: Value) {
        let mut s = self.state.lock().unwrap();
        if s.opened.contains_key(&k) || s.pending.contains_key(&k) {
            return;
        }
        s.pending.insert(k.clone(), None);
        drop(s);
        self.send(v, Some(k))
    }
    fn close(self: &Arc<Self>, k: String, v: Value) {
        if hook::answered(&self.dir, &k) {
            return;
        }
        let mut s = self.state.lock().unwrap();
        if let Some(p) = s.pending.get_mut(&k) {
            *p = Some(v)
        } else if s.opened.contains_key(&k) {
            drop(s);
            self.send(v, None)
        }
    }
}
struct Stream<'a> {
    tx: &'a broadcast::Sender<Value>,
    delivery: &'a Arc<Delivery>,
    row: Value,
    latest: watch::Receiver<Value>,
    c: Option<Value>,
    requests: HashMap<String, Value>,
    user: String,
    last: String,
    error: String,
}
fn refresh_session(row: &mut Value, latest: &mut watch::Receiver<Value>, m: &Value) {
    if latest.has_changed().unwrap_or(false) {
        let latest = latest.borrow_and_update();
        // An in-flight discovery may still describe the session before an RC switch.
        if latest["session"]["id"] == m["sessionId"] {
            *row = latest.clone()
        }
    }
    if m["event"]["type"] == "session_changed" {
        row["session"] = m["event"]["session"].clone()
    }
    if row["session"]["id"] != m["sessionId"] {
        row["session"] = Value::Null
    }
}
impl Stream<'_> {
    fn hook(&mut self, event: &str, session: &Value, r: &str, d: Value) {
        if let Some(c) = &self.c {
            let v = hook::payload(c, event, &self.row, session, r, d);
            let k = hook::key(json!([self.row["instanceId"], r]));
            match event {
                "request.opened" => self.delivery.open(k, v),
                "request.resolved" => self.delivery.close(k, v),
                _ => self.delivery.send(v, None),
            }
        }
    }
    fn open(&mut self, session: &Value, r: &Value) {
        let Some(id) = r["requestId"].as_str() else {
            return;
        };
        self.requests.insert(id.into(), r["kind"].clone());
        let mut d = r.clone();
        if let Some(t) = r.get("title") {
            d["requestTitle"] = t.clone()
        }
        if !self.last.is_empty() {
            d["context"] = self.last.chars().take(400).collect::<String>().into()
        } else if !self.user.is_empty() {
            d["context"] =
                format!("Task: {}", self.user.chars().take(394).collect::<String>()).into()
        }
        self.hook("request.opened", session, id, d)
    }
    fn event(&mut self, t: &mut Tracker, m: &Value) {
        if m["instanceId"] != self.row["instanceId"]
            || m["type"] != "event"
            || m["seq"].as_u64().is_none_or(|n| n <= t.seq)
        {
            return;
        }
        refresh_session(&mut self.row, &mut self.latest, m);
        let a = t.next(m);
        if let Some(a) = &a {
            let _ = self.tx.send(a.clone());
        }
        if self.c.is_none() {
            return;
        }
        let s = &m["sessionId"];
        let e = &m["event"];
        match e["type"].as_str() {
            Some("session_changed") => {
                self.last.clear();
                self.user.clear();
                self.error.clear();
                self.requests.clear()
            }
            Some("ui_request") => self.open(s, &e["request"]),
            Some("ui_request_end") => {
                if let Some(r) = e["requestId"].as_str() {
                    let mut d = json!({"requestId":r,"by":e["by"]});
                    if let Some(k) = self.requests.remove(r) {
                        d["kind"] = k
                    }
                    self.hook("request.resolved", s, r, d)
                }
            }
            Some("message_end") => {
                let i = &e["item"];
                let text = i["text"].as_str().unwrap_or("");
                if i["kind"] == "user" {
                    self.user = text.chars().take(400).collect();
                    self.last.clear()
                } else if i["kind"] == "assistant" {
                    self.last = text.chars().take(600).collect();
                    self.error = i["errorMessage"].as_str().unwrap_or("").into()
                }
            }
            _ => (),
        }
        if let Some(a) = a {
            let mut d = json!({});
            let ev = match a["kind"].as_str() {
                Some("input") => return,
                Some("complete") => "pi.finished",
                Some("failed") => {
                    if !self.error.is_empty() {
                        d["error"] = json!(self.error)
                    }
                    "pi.failed"
                }
                _ => {
                    d["kind"] = a["kind"].clone();
                    "pi.child"
                }
            };
            if !self.last.is_empty() {
                d["lastReply"] = json!(self.last)
            }
            self.hook(ev, s, &t.seq.to_string(), d)
        }
    }
}
struct Task {
    handle: AbortOnDropHandle<Result<()>>,
    config: Option<Value>,
    row: watch::Sender<Value>,
}
pub struct Monitor {
    tx: broadcast::Sender<Value>,
    delivery: Arc<Delivery>,
    tasks: HashMap<String, Task>,
}
impl Monitor {
    pub fn new() -> Result<Self> {
        let dir = config::config_dir()?;
        let opened = fs::read(dir.join("webhook-state.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Ok(Self {
            tx: broadcast::channel(256).0,
            delivery: Arc::new(Delivery {
                dir,
                state: Mutex::new(State {
                    opened,
                    ..Default::default()
                }),
                initial: Arc::new(Semaphore::new(100)),
                retries: Semaphore::new(100),
            }),
            tasks: Default::default(),
        })
    }
    pub fn subscribe(&mut self, run: &Path, rows: &[Value]) -> broadcast::Receiver<Value> {
        let rx = self.tx.subscribe();
        self.refresh(run, rows);
        rx
    }
    pub fn refresh(&mut self, run: &Path, rows: &[Value]) {
        let d = &self.delivery.dir;
        let c = hook::saved(d);
        let watched: HashSet<_> = hook::watches(d).into_iter().collect();
        self.tasks.retain(|id, task| {
            !task.handle.is_finished() && rows.iter().any(|r| r["instanceId"] == *id)
        });
        for row in rows {
            let Some(id) = row["instanceId"].as_str() else {
                continue;
            };
            let c = c.clone().filter(|_| watched.contains(id));
            if c.is_none() && self.tx.receiver_count() == 0 {
                self.tasks.remove(id);
                continue;
            }
            if let Some(task) = self.tasks.get(id).filter(|task| task.config == c) {
                task.row.send_if_modified(|prev| {
                    if prev != row {
                        *prev = row.clone();
                        true
                    } else {
                        false
                    }
                });
                continue;
            }
            let config = c.clone();
            let (latest_tx, latest) = watch::channel(row.clone());
            let mut row = row.clone();
            let tx = self.tx.clone();
            let delivery = self.delivery.clone();
            let dir = run.to_owned();
            let task = tokio::spawn(async move {
                let mut rc =
                    Rc::open(&dir, row["instanceId"].as_str().context("missing id")?).await?;
                let mut early = vec![];
                let mut bytes = 0;
                let s = timeout(
                    Duration::from_secs(5),
                    rc.hello(|v| {
                        bytes += v.to_string().len();
                        ensure!(bytes <= crate::server::LIMIT);
                        early.push(v);
                        Ok(())
                    }),
                )
                .await??;
                ensure!(s["purpose"].is_null());
                let mut t = Tracker::new(&s);
                row["session"] = s["session"].clone();
                let mut stream = Stream {
                    row,
                    latest,
                    tx: &tx,
                    delivery: &delivery,
                    c: config,
                    requests: Default::default(),
                    user: String::new(),
                    last: String::new(),
                    error: String::new(),
                };
                for r in s["pendingUI"].as_array().into_iter().flatten() {
                    stream.open(&s["session"]["id"], r)
                }
                for v in early {
                    stream.event(&mut t, &v)
                }
                loop {
                    stream.event(&mut t, &rc.next().await?)
                }
            });
            self.tasks.insert(
                id.into(),
                Task {
                    handle: AbortOnDropHandle::new(task),
                    config: c,
                    row: latest_tx,
                },
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_rename_updates_a_live_stream() {
        let mut row = json!({"session":{"id":"s","name":"before"}});
        let (tx, mut rx) = watch::channel(row.clone());
        tx.send_replace(json!({"session":{"id":"s","name":"after"}}));
        refresh_session(
            &mut row,
            &mut rx,
            &json!({"sessionId":"s","event":{"type":"ui_request"}}),
        );
        assert_eq!(row["session"]["name"], "after");
    }
    #[test]
    fn live_only_edges_and_seq() {
        let mut t = Tracker::new(
            &json!({"seq":4,"session":{"id":"s"},"status":"waiting_input","subagents":{"runs":[{"id":"r","children":[{"id":"c","state":"running"}]}]}}),
        );
        let mut seq = 4;
        let mut next = |v: Value| {
            seq += 1;
            t.next(&json!({"instanceId":"i","sessionId":"s","seq":seq,"event":v}))
                .map(|a| {
                    assert_eq!(a.as_object().unwrap().len(), 4);
                    a["kind"].clone()
                })
        };
        assert_eq!(
            next(json!({"type":"status","status":"waiting_input"})),
            None
        );
        assert_eq!(next(json!({"type":"agent_settled"})), None);
        next(json!({"type":"agent_start"}));
        next(json!({"type":"message_end","item":{"kind":"assistant","stopReason":"error"}}));
        assert_eq!(next(json!({"type":"agent_settled"})), Some(json!("failed")));
        assert_eq!(
            next(
                json!({"type":"subagent_run","run":{"id":"r","children":[{"id":"c","state":"paused"}]}})
            ),
            Some(json!("child_paused"))
        );
    }
}
