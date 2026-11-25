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
    let (m, rx) = connected(h).await;
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
    assert!(!s.registration.message().contains("Storage"));
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

fn snapshot(pi: &str, start: u64) -> Value {
    json!({"protocol":1,"instanceId":pi,"pid":1,"cwd":"/fixture","origin":"pc","abducoSession":null,"startedAt":start,"session":{"id":"same"},"status":"idle","model":null,"thinking":null,"pendingUI":[{"requestId":"q","kind":"confirm","title":"Continue?"}],"seq":1,"entryImages":true,"fileImages":true,"providerAuth":true,"slashCommands":true,"sessionTree":true,"promptImages":true,"promptImageLimit":4})
}
async fn connected(host: Host) -> (Machine, async_channel::Receiver<crate::model::Batch>) {
    let (tx, rx) = crate::model::Delivery::channel();
    let conn = Core::new(Arc::new(Memory(host.clone())))
        .connect(host.id.clone(), Arc::new(tx))
        .unwrap();
    let mut m = Machine {
        registration: host,
        rows: Rows::snapshot(conn.snapshot()),
        conn,
    };
    while m.rows.get("host", "connection") != "connected" {
        m.rows.apply(rx.recv().await.unwrap(), || m.conn.snapshot());
    }
    (m, rx)
}
pub(super) async fn fixture(
    id: &'static str,
) -> (
    Machine,
    async_channel::Receiver<crate::model::Batch>,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let sock = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = Host {
        id: id.into(),
        name: id.into(),
        device_id: "device".into(),
        address: sock.local_addr().unwrap().to_string(),
        needs_pairing: false,
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let mut ws = accept_async(sock.accept().await.unwrap().0).await.unwrap();
        let mut files = 0;
        let mut started = false;
        while let Some(Ok(msg)) = ws.next().await {
            if !msg.is_text() {
                continue;
            }
            let v: Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
            let reply = match v["type"].as_str() {
                Some("auth") => {
                    json!({"type":"auth_ok","deviceId":"device","host":{"hostId":id,"name":id,"version":"1","protocol":1,"platform":"linux","authSession":true,"providerAuth":true}})
                }
                Some("subscribe") => {
                    if started {
                        continue;
                    }
                    started = true;
                    json!({"type":"subscribed","instanceId":"pi","snapshot":snapshot("pi",1),"history":{"session":{"id":"same"},"items":[]}})
                }
                Some("auth_session") => {
                    json!({"type":"auth_session_result","requestId":v["requestId"],"ok":true,"instanceId":"login"})
                }
                Some("auth_session_end") => {
                    tx.send("released".into()).unwrap();
                    continue;
                }
                Some("rc") => {
                    let req = &v["request"];
                    let m = req["method"].as_str().unwrap();
                    tx.send(m.to_owned()).unwrap();
                    let mut flow = json!({"id":"flow","provider":"openai","method":"api_key","revision":1,"expiresAt":9999999999999_u64,"status":"input","prompt":{"kind":"api_key","id":"prompt"}});
                    let result = match m {
                        "ui_respond" => {
                            assert!(
                                req["params"]["confirmed"] == true
                                    && req["params"]["requestId"] == "q"
                            );
                            json!({})
                        }
                        "session_op" => {
                            assert!(req["params"]["op"] == "new");
                            ws.send(Message::text(json!({"type":"event","instanceId":"pi","sessionId":"same","seq":2,"event":{"type":"session_changed","reason":"new","session":{"id":"next"}}}).to_string())).await.unwrap();
                            json!({})
                        }
                        "commands" => json!({"commands":[]}),
                        "file_image" => {
                            files += 1;
                            json!({"image":{"mimeType":"image/jpeg","data":if files==1{"YQ=="}else{"Yg=="}}})
                        }
                        "hello" => json!({"snapshot":snapshot("login",1)}),
                        "auth_list" => json!({"providers":[],"flow":flow}),
                        "auth_respond" => {
                            assert!(req["params"]["value"] == "fixture-answer");
                            flow["status"] = json!("succeeded");
                            flow["revision"] = json!(2);
                            flow.as_object_mut().unwrap().remove("prompt");
                            json!({"flow":flow})
                        }
                        _ => panic!("unexpected fixture method"),
                    };
                    json!({"type":"rc_result","instanceId":v["instanceId"],"response":{"type":"response","id":req["id"],"ok":true,"result":result}})
                }
                Some("ping") => json!({"type":"pong","t":v["t"]}),
                _ => continue,
            };
            if ws.send(Message::text(reply.to_string())).await.is_err() {
                break;
            }
        }
    });
    let (m, changes) = connected(host).await;
    (m, changes, rx, server)
}
#[tokio::test]
async fn two_hosts_question_image_and_login_use_the_pinned_connection() {
    timeout(Duration::from_secs(8), two_host_journey())
        .await
        .unwrap();
}
async fn two_host_journey() {
    use pimate_protocol as p;
    let (mut a, ar, mut aw, server_a) = fixture("alpha").await;
    let (mut b, br, mut bw, server_b) = fixture("beta").await;
    for (m, changes) in [(&mut a, &ar), (&mut b, &br)] {
        m.conn.subscribe(Some("pi".into())).unwrap();
        while m.rows.editor(&m.registration.id, "pi").is_none() {
            m.rows
                .apply(changes.recv().await.unwrap(), || m.conn.snapshot());
        }
    }
    let mut s = DesktopState::default();
    let old = s.open("alpha", &a, a.rows.editor("alpha", "pi"));
    let page = s.open("beta", &b, b.rows.editor("beta", "pi"));
    let q = b.rows.get("question", "q");
    let params = || {
        serde_json::from_value::<p::UiRespondParams>(
            json!({"requestId":"ignored","confirmed":true}),
        )
        .unwrap()
    };
    assert_eq!(old.answer("q", q, params()).await, Err(Error::StaleTarget));
    assert_eq!(
        page.answer("q", &json!({"title":"different"}), params())
            .await,
        Err(Error::StaleTarget)
    );
    let mut question = page.question("q", q).unwrap();
    assert!(question.fields.confirmed.is_none());
    question.fields.confirmed = Some(true);
    question.answer().await.unwrap();
    assert_eq!(bw.recv().await.unwrap(), "ui_respond");
    page.sheet(SheetAction::Commands).await.unwrap();
    assert_eq!(bw.recv().await.unwrap(), "commands");
    let source = |answer| images::Source::File {
        answer,
        path: "same.jpg",
    };
    assert_eq!(&*page.image(source("answer1"), true).await.unwrap(), b"a");
    assert_eq!(&*page.image(source("answer1"), true).await.unwrap(), b"a");
    assert_eq!(&*page.image(source("answer2"), true).await.unwrap(), b"b");
    for _ in 0..2 {
        assert_eq!(bw.recv().await.unwrap(), "file_image");
    }
    assert!(bw.try_recv().is_err() && aw.try_recv().is_err());
    let page = s.open("alpha", &a, None);
    let login = page.login().await.unwrap();
    for method in ["hello", "auth_list"] {
        assert_eq!(aw.recv().await.unwrap(), method);
    }
    let action = || {
        AuthAction::Respond(p::AuthRespondParams {
            flow_id: "flow".into(),
            prompt_id: "prompt".into(),
            value: "fixture-answer".into(),
        })
    };
    login.request(action()).await.unwrap();
    let snapshot = a.conn.snapshot();
    assert!(
        snapshot
            .items
            .iter()
            .all(|i| !i.json.contains("fixture-answer"))
    );
    let r = Rows::snapshot(snapshot);
    assert_eq!(r.get("auth", "login")["revision"], 2);
    assert_eq!(aw.recv().await.unwrap(), "auth_respond");
    s.close_page();
    assert_eq!(login.request(action()).await, Err(Error::StaleTarget));
    drop(login);
    assert_eq!(aw.recv().await.unwrap(), "released");
    assert!(aw.try_recv().is_err());
    server_a.abort();
    server_b.abort();
}
#[test]
fn same_batch_selection_does_not_transfer_move_to_next_pi() {
    let mut r = Rows::default();
    r.apply(
        (
            7,
            vec![
                row("view", "instanceId", json!("first")),
                row("host", "selection", json!("first")),
                row(
                    "notice",
                    "sessionChanged",
                    json!({"from":{"id":"x"},"to":{"id":"y"}}),
                ),
                row("host", "selection", json!("second")),
                row("view", "instanceId", json!("second")),
            ],
        ),
        || panic!(),
    );
    let key = |i: &str| EditorKey {
        host: "h".into(),
        instance: i.into(),
        session: Some("y".into()),
    };
    assert_eq!(r.pending_move_with(&key("first"), |_| true), Some(7));
    assert_eq!(r.pending_move_with(&key("second"), |_| true), None);
}
#[test]
fn rename_preserves_page_but_roundtrip_to_another_session_invalidates_it() {
    let mut r = Rows::default();
    let session = |s: &str, name: &str| row("view", "session", json!({"id":s,"name":name}));
    r.apply(
        (
            1,
            vec![
                row("view", "phase", json!("ready")),
                row("view", "instanceId", json!("pi")),
                session("id", "before"),
            ],
        ),
        || panic!(),
    );
    let page = Arc::downgrade(&r.view_life);
    r.apply((2, vec![session("id", "after")]), || panic!());
    assert!(page.upgrade().is_some());
    r.apply(
        (3, vec![session("other", "after"), session("id", "after")]),
        || panic!(),
    );
    assert!(page.upgrade().is_none());
}

#[gpui::test]
fn registration_and_chat_notices_stay_visible_in_both_completion_orders(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(gpui_component::init);
    // No credentials or network needed to render completion of an unrelated pairing.
    let h = Host {
        id: "other".into(),
        name: "Other".into(),
        address: String::new(),
        device_id: "test".into(),
        needs_pairing: true,
    };
    let core = Arc::new(Core::new(Arc::new(Memory(h.clone()))));
    let (app, w) = cx.add_window_view(|w, cx| Desktop::new(core, w, cx));
    for pair_succeeded in [true, false] {
        app.update(w, |app, cx| {
            if pair_succeeded {
                app.finish_send_notice(Err(Error::OutcomeUnknown));
                app.finish_pair(Ok(h.clone()), false, cx);
            } else {
                app.finish_pair(Err(Error::OutcomeUnknown), false, cx);
                app.finish_send_notice(Ok("{}".into()));
            }
        });
        w.run_until_parked();
        app.read_with(w, |app, _| {
            if pair_succeeded {
                assert_eq!(app.state.registration.message(), "登録しました");
                assert!(app.notice.contains("再送前"));
            } else {
                assert!(
                    app.state
                        .registration
                        .message()
                        .contains("保存の結果が不明")
                );
                assert!(app.notice.contains("受付"));
            }
        });
        let registration = w.debug_bounds("registration-notice").unwrap();
        let chat = w.debug_bounds("chat-notice").unwrap();
        assert!(registration.right() <= chat.left());
    }
}

#[gpui::test]
fn reload_retires_only_the_recovered_hosts_connection_error(cx: &mut gpui::TestAppContext) {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Retry {
        hosts: Vec<Host>,
        allow_first: AtomicBool,
    }
    impl SecretStore for Retry {
        fn hosts(&self) -> Result<Vec<Host>, Error> {
            Ok(self.hosts.clone())
        }
        fn load(&self, id: String) -> Result<Credential, Error> {
            if id == self.hosts[0].id && self.allow_first.load(Ordering::Relaxed) {
                Ok(Credential {
                    host: self.hosts[0].clone(),
                    token: "fixture-only".into(),
                })
            } else {
                Err(Error::Storage)
            }
        }
        fn save(&self, _: Host, _: String) -> Result<(), Error> {
            unreachable!()
        }
        fn forget(&self, _: String) -> Result<(), Error> {
            unreachable!()
        }
        fn invalidate(&self, _: String, _: String) -> Result<(), Error> {
            unreachable!()
        }
    }
    cx.update(gpui_component::init);
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let store = Arc::new(Retry {
        hosts: ["a", "b"]
            .map(|id| Host {
                id: id.into(),
                name: "Same display name".into(),
                address: socket.local_addr().unwrap().to_string(),
                device_id: "fixture".into(),
                needs_pairing: false,
            })
            .into(),
        allow_first: AtomicBool::new(false),
    });
    let core = Arc::new(Core::new(store.clone()));
    let (app, w) = cx.add_window_view(|w, cx| Desktop::new(core, w, cx));
    app.read_with(w, |app, _| {
        assert_eq!(app.state.registration.connection_errors.len(), 2)
    });
    store.allow_first.store(true, Ordering::Relaxed);
    app.update(w, |app, cx| app.reload(cx));
    w.run_until_parked();
    app.read_with(w, |app, _| {
        assert!(app.machines.contains_key("a"));
        assert_eq!(app.state.registration.connection_errors.len(), 1);
        assert_eq!(
            app.state.registration.connection_errors["b"],
            Error::Storage
        );
    });
}
