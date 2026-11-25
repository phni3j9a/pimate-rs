use super::*;
use pimate_protocol::RcMethod as M;
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, PartialEq, Eq)]
pub struct Child {
    pub run: String,
    pub child: String,
}
impl Rows {
    pub fn original_run(&self, call: &str) -> Option<&str> {
        self.group("run")
            .iter()
            .find(|(_, v)| !call.is_empty() && v["toolCallId"] == call)
            .map(|(id, _)| id.as_str())
    }
    pub fn subagent_counts(&self) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        if self.group("runWatch").is_empty() {
            return counts;
        }
        for (_, r) in self.group("run") {
            let units = r["children"]
                .as_array()
                .filter(|a| !a.is_empty())
                .map(Vec::as_slice)
                .unwrap_or(std::slice::from_ref(r));
            let started = units.iter().any(|v| v["state"] != "queued");
            for u in units {
                let state = match u["state"].as_str() {
                    Some("queued") if started => continue,
                    Some("queued" | "running" | "stopping") => "running",
                    Some(s @ ("complete" | "waiting" | "paused" | "failed" | "stopped")) => s,
                    _ => continue,
                };
                *counts.entry(state).or_default() += 1;
            }
        }
        counts
    }
    fn child(&self, unit: &Child) -> Option<&Value> {
        self.get("run", &unit.run)["children"]
            .as_array()?
            .iter()
            .find(|c| c["id"] == unit.child)
    }
    fn stop_action<'a>(&'a self, run: &str, child: Option<&str>) -> Option<&'a str> {
        let r = self.get("run", run);
        if r["active"] != true {
            return None;
        }
        if let Some(id) = child {
            let c = r["children"].as_array()?.iter().find(|c| c["id"] == id)?;
            if !matches!(c["state"].as_str(), Some("running" | "waiting" | "queued")) {
                return None;
            }
            if c["actions"].as_array()?.iter().any(|a| a == "stop_child") {
                return Some("stop_child");
            }
        }
        r["stopAction"]
            .as_str()
            .filter(|a| matches!(*a, "stop" | "interrupt" | "abort_parent"))
    }
}
pub struct Stop {
    page: Page,
    run: String,
    viewed_child: Option<String>,
    action: String,
}
impl Page {
    pub fn stop(&self, run: &str, child: Option<&str>) -> Result<Stop, Error> {
        self.editor()?;
        if self.start.as_u64().is_none() {
            return Err(Error::StaleTarget);
        }
        let rows = self.current()?;
        let a = rows.stop_action(run, child).ok_or(Error::StaleTarget)?;
        Ok(Stop {
            page: self.clone(),
            run: run.into(),
            viewed_child: child.map(str::to_owned),
            action: a.into(),
        })
    }
    pub fn work_record(&self, child: Child) -> Result<WorkRecord, Error> {
        if self.current()?.child(&child).is_none() {
            return Err(Error::StaleTarget);
        }
        Ok(WorkRecord {
            page: self.clone(),
            child,
            cursor: None,
            items: vec![],
            task: None,
            truncated: false,
            more: false,
        })
    }
    pub async fn instruct(
        &self,
        child: Child,
        action: Instruction,
        text: String,
    ) -> Result<Sent, Error> {
        let s = text.trim();
        if s.is_empty() {
            return Err(Error::BadRequest);
        }
        let rows = self.current()?;
        let c = rows.child(&child).ok_or(Error::StaleTarget)?;
        let a = match action {
            Instruction::Steer => "steer",
            Instruction::FollowUp => "follow_up",
            Instruction::Resume => "resume",
        };
        if c["state"] == "running" && a != "steer" {
            return Err(Error::Blocked);
        }
        let index = c["index"].as_u64();
        let result = self
            .rc(
                M::SubagentControl,
                json!({"runId":child.run,"childId":child.child,"action":a,"text":s}),
            )
            .await?;
        Ok(Sent {
            editor: self.editor()?.clone(),
            start: self.start.clone(),
            connection: self.conn.clone(),
            life: self.life.clone(),
            child,
            action,
            text: s.into(),
            new_run: result["newRunId"].as_str().map(str::to_owned),
            index,
            destination: None,
        })
    }
}
impl Stop {
    pub fn scope(&self) -> (&str, Option<&str>, &str) {
        (&self.run, self.viewed_child.as_deref(), &self.action)
    }
    pub async fn confirm(self) -> Result<Value, Error> {
        let r = self.page.current()?;
        if r.stop_action(&self.run, self.viewed_child.as_deref()) != Some(&self.action) {
            return Err(Error::StaleTarget);
        }
        let mut p = json!({"runId":self.run,"action":self.action});
        if self.action == "stop_child" {
            p["childId"] = json!(self.viewed_child);
        }
        self.page.rc(M::SubagentControl, p).await
    }
}
pub struct WorkRecord {
    page: Page,
    child: Child,
    cursor: Option<String>,
    pub items: Vec<Value>,
    pub task: Option<String>,
    pub truncated: bool,
    pub more: bool,
}
impl WorkRecord {
    pub async fn load(&mut self) -> Result<(), Error> {
        for _ in 0..8 {
            let mut q = json!({"runId":self.child.run,"childId":self.child.child});
            if let Some(c) = &self.cursor {
                q["cursor"] = json!(c);
            }
            let v = self.page.rc(M::SubagentTranscript, q).await?;
            let items = v["items"].as_array().ok_or(Error::InvalidMessage)?;
            if self.cursor.is_none() {
                self.items.clear();
            }
            for i in items {
                let id = i["entryId"].as_str().ok_or(Error::InvalidMessage)?;
                if let Some(old) = self.items.iter_mut().find(|e| e["entryId"] == id) {
                    *old = i.clone()
                } else if self.items.len() < 2048 {
                    self.items.push(i.clone())
                } else {
                    self.truncated = true;
                    self.more = false;
                    return Ok(());
                }
            }
            if let Some(t) = v["task"].as_str() {
                self.task = Some(t.into());
            }
            self.truncated |= v["truncated"] == true;
            let next = v["cursor"].as_str().filter(|c| !c.is_empty());
            self.more = next.is_some() && next != self.cursor.as_deref();
            if let Some(c) = next {
                self.cursor = Some(c.into());
            }
            if !self.more {
                break;
            }
        }
        Ok(())
    }
}
pub enum Instruction {
    Steer,
    FollowUp,
    Resume,
}
pub struct Sent {
    editor: EditorKey,
    start: Value,
    connection: Weak<Connection>,
    life: Weak<()>,
    pub child: Child,
    pub action: Instruction,
    pub text: String,
    pub new_run: Option<String>,
    index: Option<u64>,
    destination: Option<Child>,
}
impl Sent {
    fn matches(&self, page: &Page) -> bool {
        Some(&self.editor) == page.key.as_ref()
            && self.start == page.start
            && self.connection.ptr_eq(&page.conn)
            && (self.editor.session.is_some() || self.life.ptr_eq(&page.life))
    }
    fn size(&self) -> usize {
        self.editor.host.len()
            + self.editor.instance.len()
            + self.editor.session.as_ref().map_or(0, String::len)
            + self.child.run.len()
            + self.child.child.len()
            + self.text.len()
            + self.new_run.as_ref().map_or(0, String::len)
            + self
                .destination
                .as_ref()
                .map_or(0, |d| d.child.len() + d.run.len())
    }
    fn resumed_child(&mut self, rows: &Rows) -> Option<&Child> {
        if self.destination.is_none() {
            let run = self.new_run.as_deref()?;
            let cs = rows.get("run", run)["children"].as_array()?;
            let id = cs
                .iter()
                .find(|c| self.index.is_some() && self.index == c["index"].as_u64())
                .or_else(|| cs.first())?["id"]
                .as_str()?;
            self.destination = Some(Child {
                run: run.into(),
                child: id.into(),
            });
        }
        self.destination.as_ref()
    }
}
#[derive(Default)]
pub struct Instructions(VecDeque<Sent>);
impl Instructions {
    fn accept(&mut self, s: Sent, m: &Machine) {
        if !s.connection.ptr_eq(&Arc::downgrade(&m.conn))
            || s.editor.host != m.registration.id
            || s.size() > 65536
        {
            return;
        }
        self.0.push_back(s);
        self.resolve(m);
        self.trim();
    }
    fn trim(&mut self) {
        while self.0.len() > 128 || self.0.iter().map(Sent::size).sum::<usize>() > 65536 {
            self.0.pop_front();
        }
    }
    fn resolve(&mut self, m: &Machine) {
        if self.0.is_empty() {
            return;
        }
        let r = &m.rows;
        let Some(k) = r
            .get("view", "instanceId")
            .as_str()
            .and_then(|pi| r.editor(&m.registration.id, pi))
        else {
            return;
        };
        let start = r.get("view", "startedAt");
        if !start.is_u64() {
            return;
        }
        let conn = Arc::downgrade(&m.conn);
        self.0.retain(|s| {
            s.editor.host != k.host
                || s.editor.instance != k.instance
                || (s.connection.ptr_eq(&conn) && s.start == *start)
        });
        for s in &mut self.0 {
            if s.editor == k
                && (k.session.is_some() || s.life.ptr_eq(&Arc::downgrade(&r.view_life)))
            {
                s.resumed_child(r);
            }
        }
        self.trim();
    }
    pub fn resumed(&self, page: &Page, source: &Child, run: &str) -> Option<&Child> {
        self.0
            .iter()
            .rev()
            .find(|s| s.matches(page) && &s.child == source && s.new_run.as_deref() == Some(run))?
            .destination
            .as_ref()
    }
    pub fn for_child(&self, page: &Page, child: &Child) -> Vec<&Sent> {
        let origin = self
            .0
            .iter()
            .rev()
            .find(|s| s.matches(page) && s.destination.as_ref() == Some(child))
            .map(|s| &s.child);
        self.0
            .iter()
            .filter(|s| s.matches(page) && &s.child == child)
            .chain(
                self.0
                    .iter()
                    .filter(|s| s.matches(page) && &s.child != child && Some(&s.child) == origin),
            )
            .collect()
    }
    pub(super) fn forget(&mut self, h: &str) {
        self.0.retain(|s| s.editor.host != h)
    }
}
impl Machine {
    pub(crate) fn apply(&mut self, batch: crate::model::Batch, history: &mut Instructions) {
        self.rows.apply(batch, || self.conn.snapshot());
        history.resolve(self);
    }
}
impl Desktop {
    pub fn accept_instruction(&mut self, s: Sent) {
        if let Some(m) = self.machines.get(&s.editor.host) {
            self.state.instructions.accept(s, m)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::super::tests::row;
    use super::*;
    #[test]
    fn completed_child_never_becomes_parent_stop() {
        let mut r = Rows::default();
        for (rev, state, actions, expected) in [
            (1, "running", json!([]), Some("abort_parent")),
            (2, "running", json!(["stop_child"]), Some("stop_child")),
            (3, "complete", json!([]), None),
        ] {
            r.apply((rev,vec![row("run","r",json!({"active":true,"stopAction":"abort_parent","children":[{"id":"c","state":state,"actions":actions}]})),row("runWatch","r",json!(true))]),||panic!());
            assert_eq!(r.stop_action("r", Some("c")), expected);
            assert!(r.stop_action("r", Some("not-c")).is_none());
        }
        assert_eq!(r.subagent_counts()["complete"], 1);
    }
}
#[cfg(test)]
#[tokio::test]
async fn resume_binds_late_child_once_without_open_detail() {
    use super::tests::{fixture, row};
    let (mut m, _rx, _wire, server) = fixture("one").await;
    let mut history = Instructions::default();
    let mut state = DesktopState::default();
    let update = |rev, changes| (rev, changes);
    let ready = |id: &str, start: u64| {
        vec![
            row("view", "phase", json!("ready")),
            row("view", "instanceId", json!("pi")),
            row("view", "session", json!({"id":id})),
            row("view", "startedAt", json!(start)),
        ]
    };
    let base = m.conn.snapshot().revision + 1;
    m.apply(update(base, ready("s", 41)), &mut history);
    let k = m.rows.editor("one", "pi").unwrap();
    let old = Child {
        run: "source".into(),
        child: "x".into(),
    };
    let page = state.open("one", &m, Some(k.clone()));
    history.accept(
        Sent {
            editor: k.clone(),
            start: json!(41),
            connection: page.conn.clone(),
            life: page.life.clone(),
            child: old.clone(),
            action: Instruction::Resume,
            text: "go".into(),
            new_run: Some("new".into()),
            index: Some(7),
            destination: None,
        },
        &m,
    );
    state.close_page();
    let children = |id: &str| {
        row(
            "run",
            "new",
            json!({"children":[{"id":"a","index":1},{"id":id,"index":7}]}),
        )
    };
    let mut wrong = ready("other", 41);
    wrong.push(children("other-session"));
    m.apply(update(base + 1, wrong), &mut history);
    assert!(history.0[0].destination.is_none());
    let mut correct = ready("s", 41);
    correct.push(children("b"));
    m.apply(update(base + 2, correct), &mut history);
    m.apply(update(base + 3, vec![children("later")]), &mut history);
    let page = state.open("one", &m, Some(k));
    let dest = history.resumed(&page, &old, "new").unwrap();
    assert_eq!(dest.child, "b");
    assert_eq!(history.for_child(&page, dest)[0].text, "go");
    m.apply(update(base + 4, ready("s", 42)), &mut history);
    assert!(history.0.is_empty());
    server.abort();
}
