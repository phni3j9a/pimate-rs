use async_channel::{Receiver, Sender};
use pimate_core::{Change, Item, Listener, Snapshot, Target};
use serde_json::Value;
use std::collections::HashMap;

pub type Batch = (u64, Vec<Change>);
pub struct Delivery(Sender<Batch>);
impl Delivery {
    pub fn channel() -> (Self, Receiver<Batch>) {
        let (tx, rx) = async_channel::bounded(1);
        (Self(tx), rx)
    }
}
impl Listener for Delivery {
    fn changed(&self, revision: u64, changes: Vec<Change>) {
        // Only the dedicated delivery thread waits. Core coalesces overflow into ResyncRequired.
        let _ = self.0.send_blocking((revision, changes));
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct EditorKey {
    pub host: String,
    pub instance: String,
    pub session: Option<String>,
}
impl EditorKey {
    pub fn target(&self) -> Target {
        Target {
            instance: self.instance.clone(),
            session: self.session.clone(),
        }
    }
}
#[derive(Default)]
pub struct Rows {
    revision: u64,
    groups: HashMap<String, Vec<(String, Value)>>,
}
impl Rows {
    pub fn snapshot(s: Snapshot) -> Self {
        let mut rows = Self {
            revision: s.revision,
            ..Default::default()
        };
        for i in s.items {
            rows.groups
                .entry(i.collection)
                .or_default()
                .push((i.id, serde_json::from_str(&i.json).expect("core JSON")));
        }
        rows
    }
    pub fn apply(&mut self, (rev, batch): Batch, snapshot: impl FnOnce() -> Snapshot) {
        if rev <= self.revision {
            return;
        }
        if batch.len() > 128 || batch.contains(&Change::ResyncRequired) {
            *self = Self::snapshot(snapshot());
            return;
        }
        for change in batch {
            match change {
                Change::Upsert(Item {
                    collection,
                    id,
                    position,
                    json,
                }) => {
                    let g = self.groups.entry(collection).or_default();
                    g.retain(|(k, _)| k != &id);
                    g.insert(
                        (position as usize).min(g.len()),
                        (id, serde_json::from_str(&json).expect("core JSON")),
                    );
                }
                Change::Remove { collection, id } => {
                    if let Some(g) = self.groups.get_mut(&collection) {
                        g.retain(|(k, _)| k != &id);
                    }
                }
                Change::ResyncRequired => unreachable!(),
            }
        }
        self.revision = rev;
    }
    pub fn group(&self, col: &str) -> &[(String, Value)] {
        self.groups.get(col).map(Vec::as_slice).unwrap_or_default()
    }
    pub fn get(&self, col: &str, key: &str) -> &Value {
        self.group(col)
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap_or(&Value::Null)
    }
    pub fn editor(&self, host: &str, instance: &str) -> Option<EditorKey> {
        if self.get("view", "phase") != "ready" || self.get("view", "instanceId") != instance {
            return None;
        }
        let session = self.get("view", "session");
        Some(EditorKey {
            host: host.into(),
            instance: instance.into(),
            session: if session.is_null() {
                None
            } else {
                Some(session["id"].as_str()?.into())
            },
        })
    }
}

/// Clear only the originating, still-unchanged draft, never the newly visible editor.
pub fn clear_accepted(
    drafts: &mut HashMap<EditorKey, String>,
    origin: &EditorKey,
    text: &str,
    accepted: bool,
) -> bool {
    if accepted && drafts.get(origin).is_some_and(|draft| draft == text) {
        drafts.remove(origin);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_receipts_never_touch_other_hosts_or_new_edits() {
        let a = EditorKey {
            host: "one".into(),
            instance: "p".into(),
            session: None,
        };
        let b = EditorKey {
            host: "two".into(),
            ..a.clone()
        };
        let mut drafts = HashMap::from([(a.clone(), "first".into()), (b.clone(), "first".into())]);
        assert!(!clear_accepted(&mut drafts, &a, "first", false));
        assert!(!clear_accepted(&mut drafts, &a, "older", true));
        assert_eq!(drafts.len(), 2);
        assert!(clear_accepted(&mut drafts, &a, "first", true));
        assert_eq!(drafts.get(&b).unwrap(), "first");
    }
    #[test]
    fn delta_moves_replaces_and_removes_in_order() {
        let mut r = Rows::default();
        r.apply(
            (
                1,
                vec![
                    Change::Upsert(field("a", "1", 0)),
                    Change::Upsert(field("b", "2", 1)),
                ],
            ),
            || panic!(),
        );
        r.apply((2, vec![Change::Upsert(field("b", "3", 0))]), || panic!());
        assert_eq!(r.group("view")[0], ("b".into(), 3.into()));
        r.apply(
            (
                3,
                vec![Change::Remove {
                    collection: "view".into(),
                    id: "a".into(),
                }],
            ),
            || panic!(),
        );
        assert_eq!(r.group("view").len(), 1);
    }
    fn field(k: &str, v: &str, pos: u64) -> Item {
        Item {
            collection: "view".into(),
            id: k.into(),
            json: v.into(),
            position: pos,
        }
    }
    #[test]
    fn delivery_resync_drops_old_batch_and_null_session_is_valid() {
        let (listener, rx) = Delivery::channel();
        let thread = std::thread::spawn(move || {
            listener.changed(2, vec![Change::ResyncRequired]);
            listener.changed(
                3,
                vec![Change::Remove {
                    collection: "view".into(),
                    id: "phase".into(),
                }],
            );
        });
        let mut rows = Rows::default();
        rows.apply(rx.recv_blocking().unwrap(), || Snapshot {
            revision: 4,
            items: vec![
                field("phase", "\"ready\"", 0),
                field("instanceId", "\"pi\"", 1),
            ],
        });
        rows.apply(rx.recv_blocking().unwrap(), || panic!());
        thread.join().unwrap();
        assert!(rows.editor("a", "other").is_none());
        assert_eq!(rows.editor("a", "pi").unwrap().session, None);
        assert_ne!(rows.editor("a", "pi"), rows.editor("b", "pi"));
    }
}
