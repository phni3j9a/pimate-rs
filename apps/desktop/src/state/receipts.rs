use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Done,
    Failed,
    Cancelled,
    Unknown,
}
pub struct Receipt {
    pub instance: String,
    pub started_at: Option<u64>,
    pub value: Value,
    dismissed: bool,
    text_handled: bool,
}
impl Receipt {
    pub fn outcome(&self) -> Outcome {
        let v = &self.value;
        let e = &v["event"];
        if v["outcome"] == "unknown" {
            Outcome::Unknown
        } else if e["error"].is_string() || e["type"] == "compact_error" {
            Outcome::Failed
        } else if e["cancelled"] == true {
            Outcome::Cancelled
        } else if v["outcome"] == "done"
            || e["type"] == "compact_done"
            || e["type"] == "session_op_done" && e["cancelled"] == false
        {
            Outcome::Done
        } else {
            Outcome::Unknown
        }
    }
}
struct Pending {
    instance: String,
    start: Option<u64>,
    value: Value,
}
#[derive(Default)]
pub struct Receipts {
    pending: Option<Pending>,
    latest: Option<Receipt>,
}
impl Receipts {
    pub fn busy(&mut self, v: &Value, instance: Option<&str>) {
        if !matches!(v["method"].as_str(), Some("session_op" | "compact"))
            || v["id"].as_str().is_none_or(str::is_empty)
        {
            self.pending = None;
            return;
        }
        if let Some(p) = &mut self.pending
            && p.value["id"] == v["id"]
        {
            p.value = v.clone();
            return;
        }
        self.pending = instance.map(|pi| Pending {
            instance: pi.into(),
            start: v["startedAt"].as_u64(),
            value: v.clone(),
        });
    }
    pub fn result(&mut self, v: &Value) {
        if !v["id"].is_string() {
            return;
        }
        if let Some(r) = &mut self.latest
            && r.value["id"] == v["id"]
            && r.value["sessionId"] == v["sessionId"]
        {
            r.value = v.clone();
            return;
        }
        if let Some(p) = &self.pending
            && p.value["id"] == v["id"]
        {
            self.latest = Some(Receipt {
                instance: p.instance.clone(),
                started_at: p.start,
                value: v.clone(),
                dismissed: false,
                text_handled: false,
            });
        }
    }
    pub fn local_move(&self, pi: &str, notice: &Value) -> bool {
        self.pending.as_ref().is_some_and(|p| {
            p.instance == pi
                && notice["from"]["id"].is_string()
                && notice["to"]["id"].is_string()
                && p.value["sessionId"] == notice["from"]["id"]
                && p.value["arrival"] == notice["to"]["id"]
                && (p.value["op"] == notice["reason"]
                    || p.value["op"] == "switch" && notice["reason"] == "resume")
        })
    }
    pub fn shown(&self, pi: &str, start: Option<u64>) -> Option<&Receipt> {
        self.latest
            .as_ref()
            .filter(|r| !r.dismissed && r.instance == pi && r.started_at == start)
    }
    pub fn dismiss(&mut self, pi: &str, id: &str) {
        if let Some(r) = &mut self.latest
            && r.instance == pi
            && r.value["id"] == id
        {
            r.dismissed = true
        }
    }
    pub fn take_text(&mut self, pi: &str, start: Option<u64>, session: &str) -> Option<String> {
        let r = self.latest.as_mut()?;
        if r.instance != pi
            || r.started_at != start
            || r.text_handled
            || r.outcome() != Outcome::Done
            || r.value["sessionId"] != session
        {
            return None;
        }
        let text = [&r.value["forkText"], &r.value["event"]["editorText"]]
            .into_iter()
            .find_map(|v| v.as_str().filter(|s| !s.is_empty()))?;
        r.text_handled = true;
        Some(text.into())
    }
}
