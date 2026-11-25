use super::*;
use futures_util::{SinkExt, StreamExt};
use pimate_core::{Core, Credential, Host, SecretStore};
use serde_json::json;
use tokio::{
    net::TcpListener,
    time::{Duration, timeout},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
struct Memory(Host);
impl SecretStore for Memory {
    fn hosts(&self) -> Result<Vec<Host>, Error> {
        Ok(vec![self.0.clone()])
    }
    fn load(&self, _: String) -> Result<Credential, Error> {
        Ok(Credential {
            host: self.0.clone(),
            token: "fixture-only".into(),
        })
    }
    fn save(&self, _: Host, _: String) -> Result<(), Error> {
        unreachable!()
    }
    fn invalidate(&self, _: String, _: String) -> Result<(), Error> {
        unreachable!()
    }
    fn forget(&self, _: String) -> Result<(), Error> {
        unreachable!()
    }
}
#[tokio::test]
async fn closed_page_receives_no_late_machine_ack() {
    timeout(Duration::from_secs(8), machine_ack_journey())
        .await
        .unwrap();
}
async fn machine_ack_journey() {
    let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap().to_string();
    let (reached, notice) = tokio::sync::oneshot::channel();
    let (release, gate) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let mut ws = accept_async(socket.accept().await.unwrap().0)
            .await
            .unwrap();
        let mut signal = Some(reached);
        let mut gate = Some(gate);
        let mut queried = 0;
        while let Some(Ok(msg)) = ws.next().await {
            if !msg.is_text() {
                continue;
            }
            let v: Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
            let reply = match v["type"].as_str() {
                Some("auth") => {
                    json!({"type":"auth_ok","deviceId":"device","host":{"hostId":"test","name":"Fixture","version":"1","protocol":1,"platform":"linux","pairedDevices":true}})
                }
                Some("devices") => {
                    queried += 1;
                    signal.take().unwrap().send(()).unwrap();
                    gate.take().unwrap().await.unwrap();
                    json!({"type":"devices_result","requestId":v["requestId"],"devices":[]})
                }
                Some("ping") => json!({"type":"pong","t":v["t"]}),
                _ => continue,
            };
            ws.send(Message::text(reply.to_string())).await.unwrap();
            if queried == 1 {
                break;
            }
        }
        queried
    });
    let h = Host {
        id: "test".into(),
        name: "Fixture".into(),
        address: addr,
        device_id: "device".into(),
        needs_pairing: false,
    };
    let core = Core::new(Arc::new(Memory(h.clone())));
    let (tx, rx) = crate::model::Delivery::channel();
    let conn = core.connect(h.id.clone(), Arc::new(tx)).unwrap();
    let mut m = Machine {
        registration: h,
        rows: Rows::snapshot(conn.snapshot()),
        conn,
    };
    while m.rows.get("host", "connection") != "connected" {
        m.rows.apply(rx.recv().await.unwrap(), || m.conn.snapshot());
    }
    let mut state = DesktopState::default();
    let old = state.open("test", &m, None);
    let pending = tokio::spawn(async move { old.query(MachineQuery::Devices).await });
    notice.await.unwrap();
    state.close_page();
    release.send(()).unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::OutcomeUnknown)));
    assert_eq!(server.await.unwrap(), 1);
    drop(rx);
}

pub(super) fn row(c: &str, id: &str, v: Value) -> pimate_core::Change {
    pimate_core::Change::Upsert(pimate_core::Item {
        collection: c.into(),
        id: id.into(),
        position: 0,
        json: v.to_string(),
    })
}
#[test]
fn correlated_completion_and_snapshot_do_not_transfer_fork_text() {
    use super::receipts::Outcome;
    let mut a = Rows::default();
    let mut b = Rows::default();
    let busy = json!({"method":"session_op","id":"r","startedAt":8,"op":"fork","sessionId":"old"});
    let result = json!({"id":"r","outcome":"done","sessionId":"new","forkText":"original"});
    a.apply(
        (
            1,
            vec![
                row("view", "instanceId", json!("pi")),
                row("busy", "operation", busy),
            ],
        ),
        || panic!(),
    );
    let snapshot = pimate_core::Snapshot {
        revision: 3,
        items: vec![pimate_core::Item {
            collection: "operationResult".into(),
            id: "latest".into(),
            position: 0,
            json: result.to_string(),
        }],
    };
    for r in [&mut a, &mut b] {
        r.apply((2, vec![pimate_core::Change::ResyncRequired]), || {
            snapshot.clone()
        })
    }
    assert!(b.receipts.shown("pi", Some(8)).is_none());
    assert!(a.receipts.shown("other", Some(8)).is_none());
    assert_eq!(
        a.receipts.shown("pi", Some(8)).unwrap().outcome(),
        Outcome::Done
    );
    a.receipts.dismiss("pi", "r");
    assert!(a.receipts.shown("pi", Some(8)).is_none());
    assert_eq!(a.receipts.take_text("pi", Some(9), "new"), None);
    assert_eq!(
        a.receipts.take_text("pi", Some(8), "new").as_deref(),
        Some("original")
    );
    assert_eq!(a.receipts.take_text("pi", Some(8), "new"), None);
}
#[test]
fn same_batch_target_roundtrip_invalidates_but_catalog_does_not() {
    let mut r = Rows::default();
    let old = Arc::downgrade(&r.view_life);
    let host = Arc::downgrade(&r.host_life);
    r.apply(
        (
            1,
            vec![
                row("view", "phase", json!("offline")),
                row("view", "phase", json!("ready")),
            ],
        ),
        || panic!(),
    );
    assert!(old.upgrade().is_none());
    let view = Arc::downgrade(&r.view_life);
    r.apply(
        (
            2,
            (0..140)
                .map(|i| row("model", &i.to_string(), Value::Null))
                .collect(),
        ),
        || pimate_core::Snapshot {
            revision: 2,
            items: vec![],
        },
    );
    assert!(view.upgrade().is_some());
    r.apply(
        (
            3,
            vec![
                row("host", "connection", json!("offline")),
                row("host", "connection", json!("connected")),
            ],
        ),
        || panic!(),
    );
    assert!(host.upgrade().is_none() && view.upgrade().is_none());
}
#[test]
fn first_step_and_registration_preserve_unknown_independently_of_chat() {
    let mut r = Rows::default();
    r.apply(
        (1, vec![row("host", "connection", json!("connected"))]),
        || panic!(),
    );
    assert_eq!(r.first_step(), FirstStep::Loading);
    r.apply(
        (2, vec![row("host", "listKnown", json!(true))]),
        || panic!(),
    );
    assert_eq!(r.first_step(), FirstStep::Empty);
    let mut s = DesktopState::default();
    s.registration.save_unknown = true;
    s.registration.read_failed(Error::Storage);
    assert!(
        s.registration.message().contains("不明") && s.registration.message().contains("Storage")
    );
    s.registration.read_ok();
    assert!(s.registration.save_unknown);
    s.registration.paired("one", true);
    assert_eq!(s.registration.paired_host.take().as_deref(), Some("one"));
    s.registration.paired("two", false);
    assert!(s.registration.paired_host.is_none());
}
#[test]
fn purging_replaced_host_preserves_other_hosts_drafts() {
    let key = |h: &str| EditorKey {
        host: h.into(),
        instance: "p".into(),
        session: None,
    };
    let mut s = DesktopState::default();
    let mut drafts =
        std::collections::HashMap::from([(key("a"), "A".into()), (key("b"), "B".into())]);
    s.purge("a", &mut drafts);
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[&key("b")], "B");
}
pub(super) fn dummy_page() -> Page {
    Page {
        host: "a".into(),
        key: Some(EditorKey {
            host: "a".into(),
            instance: "pi".into(),
            session: Some("session".into()),
        }),
        start: json!(1),
        conn: Weak::new(),
        life: Weak::new(),
        page: Weak::new(),
        images: Default::default(),
    }
}
