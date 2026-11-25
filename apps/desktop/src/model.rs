use async_channel::{Receiver, Sender};
use pimate_core::{Change, Host, Item, Listener, Snapshot, Target};
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
    pub host_life: std::sync::Arc<()>,
    pub view_life: std::sync::Arc<()>,
    revision: u64,
    moves: HashMap<String, (String, String, u64)>,
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
        rows.remember_move();
        rows
    }
    pub fn apply(&mut self, (rev, batch): Batch, snapshot: impl FnOnce() -> Snapshot) {
        if rev <= self.revision {
            return;
        }
        let missed = batch.contains(&Change::ResyncRequired);
        // Check each change, including a disconnect/reconnect in one delivery.
        let mut host_changed = missed;
        let mut view_changed = missed;
        for change in &batch {
            let (c, id, changed) = match change {
                Change::Upsert(i) => (&i.collection, &i.id, || {
                    self.get(&i.collection, &i.id)
                        != &serde_json::from_str::<Value>(&i.json).expect("core JSON")
                }),
                Change::Remove { collection, id } => {
                    host_changed |= collection == "host" && id == "connection";
                    view_changed |= collection == "view"
                        && matches!(
                            id.as_str(),
                            "instanceId" | "startedAt" | "phase" | "session"
                        );
                    continue;
                }
                _ => continue,
            };
            if c == "host" && id == "connection" {
                host_changed |= changed();
            }
            if c == "view"
                && matches!(
                    id.as_str(),
                    "instanceId" | "startedAt" | "phase" | "session"
                )
            {
                view_changed |= changed();
            }
        }
        if host_changed {
            self.host_life = Default::default();
        }
        if host_changed || view_changed {
            self.view_life = Default::default();
        }
        if batch.len() > 128 || missed {
            let mut next = Self::snapshot(snapshot());
            if next.revision == rev {
                std::mem::swap(&mut next.host_life, &mut self.host_life);
                std::mem::swap(&mut next.view_life, &mut self.view_life);
            }
            // Identical endpoints after a delivery gap do not identify the same move.
            let changed = missed
                || next.revision > rev
                || self.get("notice", "sessionChanged") != next.get("notice", "sessionChanged")
                || self.get("host", "selection") != next.get("host", "selection");
            let prior = std::mem::take(&mut self.moves);
            *self = next;
            self.moves = prior;
            if changed {
                self.remember_move();
            }
            return;
        }
        let mut moved = false;
        for change in batch {
            match change {
                Change::Upsert(Item {
                    collection,
                    id,
                    position,
                    json,
                }) => {
                    moved |= collection == "notice" && id == "sessionChanged";
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
        if moved {
            self.remember_move();
        }
    }
    fn remember_move(&mut self) {
        let n = self.get("notice", "sessionChanged");
        if let (Some(pi), Some(from), Some(to)) = (
            self.get("host", "selection").as_str(),
            n["from"]["id"].as_str(),
            n["to"]["id"].as_str(),
        ) {
            self.moves
                .insert(pi.into(), (from.into(), to.into(), self.revision));
        }
    }
    pub fn pending_move(&self, k: &EditorKey, drafts: &HashMap<EditorKey, String>) -> Option<u64> {
        self.pending_move_with(k, |key| drafts.get(key).is_some_and(|s| !s.is_empty()))
    }
    pub fn pending_move_with(
        &self,
        k: &EditorKey,
        has_draft: impl FnOnce(&EditorKey) -> bool,
    ) -> Option<u64> {
        let (from, to, rev) = self.moves.get(&k.instance)?;
        if Some(to) != k.session.as_ref() {
            return None;
        }
        has_draft(&EditorKey {
            session: Some(from.clone()),
            ..k.clone()
        })
        .then_some(*rev)
    }
    pub fn confirm_move(&mut self, k: &EditorKey, shown: u64) {
        if self
            .moves
            .get(&k.instance)
            .is_some_and(|(_, to, rev)| Some(to) == k.session.as_ref() && *rev == shown)
        {
            self.moves.remove(&k.instance);
        }
    }
    pub fn subscription(&self, pi: &str) -> String {
        if self.get("host", "selection") == pi {
            match self.get("view", "phase").as_str() {
                Some("instance_gone") => {
                    return "Pi は終了しました。左の一覧から別の Pi を選んでください。".into();
                }
                Some("subscribe_error") => {
                    return format!(
                        "購読に失敗しました（{}）。同じ Pi を選び直して再試行できます。",
                        self.get("view", "error").as_str().unwrap_or("unknown")
                    );
                }
                _ => (),
            }
        }
        format!("{pi} に接続しています…")
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

/// A connection is valid only for the registration that created it.
pub fn registered(previous: &Host, current: &[Host]) -> bool {
    current.iter().any(|h| !h.needs_pairing && h == previous)
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
    #[test]
    fn registrations_drop_deleted_revoked_and_replaced_connections() {
        let h = Host {
            id: "h".into(),
            name: "N".into(),
            address: "localhost:80".into(),
            device_id: "one".into(),
            needs_pairing: false,
        };
        assert!(registered(&h, std::slice::from_ref(&h)));
        assert!(!registered(&h, &[]));
        assert!(!registered(
            &h,
            &[Host {
                needs_pairing: true,
                ..h.clone()
            }]
        ));
        assert!(!registered(
            &h,
            &[Host {
                device_id: "two".into(),
                ..h.clone()
            }]
        ));
    }
    fn group(col: &str, k: &str, v: serde_json::Value) -> Change {
        Change::Upsert(Item {
            collection: col.into(),
            id: k.into(),
            position: 0,
            json: v.to_string(),
        })
    }
    #[test]
    fn external_move_blocks_only_at_destination_and_confirmation_is_for_one_move() {
        use serde_json::json;
        let mut r = Rows::default();
        let dst = EditorKey {
            host: "h".into(),
            instance: "p".into(),
            session: Some("B".into()),
        };
        let src = EditorKey {
            session: Some("A".into()),
            ..dst.clone()
        };
        let drafts = HashMap::from([(src.clone(), "keep".to_owned())]);
        let notice = group(
            "notice",
            "sessionChanged",
            json!({"from":{"id":"A"},"to":{"id":"B"}}),
        );
        r.apply(
            (
                1,
                vec![group("host", "selection", json!("p")), notice.clone()],
            ),
            || panic!(),
        );
        assert_eq!(r.pending_move(&src, &drafts), None);
        assert_eq!(r.pending_move(&dst, &HashMap::new()), None);
        assert_eq!(r.pending_move(&dst, &drafts), Some(1));
        r.apply((2, vec![notice]), || panic!());
        r.confirm_move(&dst, 1);
        assert_eq!(r.pending_move(&dst, &drafts), Some(2));
        r.confirm_move(&dst, 2);
        assert_eq!(r.pending_move(&dst, &drafts), None);
    }
    #[test]
    fn resync_forgets_confirmation_even_with_identical_move_notice() {
        let snap = Snapshot {
            revision: 1,
            items: vec![
                Item {
                    collection: "host".into(),
                    ..field("selection", r#""p""#, 0)
                },
                Item {
                    collection: "notice".into(),
                    ..field(
                        "sessionChanged",
                        r#"{"from":{"id":"A"},"to":{"id":"B"}}"#,
                        0,
                    )
                },
            ],
        };
        let mut rows = Rows::snapshot(snap.clone());
        let dest = EditorKey {
            host: "h".into(),
            instance: "p".into(),
            session: Some("B".into()),
        };
        let drafts = HashMap::from([(
            EditorKey {
                session: Some("A".into()),
                ..dest.clone()
            },
            "keep".into(),
        )]);
        rows.confirm_move(&dest, 1);
        assert_eq!(rows.pending_move(&dest, &drafts), None);
        rows.apply((4, vec![Change::ResyncRequired]), || Snapshot {
            revision: 5,
            ..snap
        });
        rows.confirm_move(&dest, 1);
        assert_eq!(rows.pending_move(&dest, &drafts), Some(5));
        rows.confirm_move(&dest, 5);
        assert_eq!(rows.pending_move(&dest, &drafts), None);
    }
    #[test]
    fn subscription_error_is_only_shown_for_its_own_selection() {
        use serde_json::json;
        let mut r = Rows::default();
        r.apply(
            (
                1,
                vec![
                    group("host", "selection", json!("p")),
                    group("view", "phase", json!("subscribe_error")),
                    group("view", "error", json!("unreachable")),
                ],
            ),
            || panic!(),
        );
        assert!(r.subscription("p").contains("unreachable"));
        assert!(!r.subscription("other").contains("unreachable"));
        r.apply(
            (2, vec![group("view", "phase", json!("instance_gone"))]),
            || panic!(),
        );
        assert!(r.subscription("p").contains("終了"));
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
