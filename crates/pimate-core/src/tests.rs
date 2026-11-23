use super::*;
use serde_json::{Value, json};
use state::State;
fn view() -> State {
    let mut s = State::default();
    s.selected = Some("i".into());
    sync(&mut s, json!([]));
    s
}
fn sync(s: &mut State, items: Value) {
    s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":2,"session":{"id":"s"},"status":"idle","sessionTree":true,"slashCommands":true,"promptImages":true,"promptImageLimit":4},"history":{"session":null,"items":items}}));
}
fn event(s: &mut State, seq: u64, e: Value) -> bool {
    s.apply(&json!({"type":"event","sessionId":"s","instanceId":"i","seq":seq,"event":e}))
}
fn target() -> Target {
    Target {
        instance: "i".into(),
        session: Some("s".into()),
    }
}
fn send(text: &str, images: Vec<Image>) -> Operation {
    Operation::Send {
        target: target(),
        text: text.into(),
        images,
    }
}
fn rc(m: pimate_protocol::RcMethod, v: Value) -> Operation {
    Operation::Rc {
        target: target(),
        method: m,
        params: v.to_string(),
    }
}
#[test]
fn stream_updates_only_the_target_and_history_uses_first_position_last_content() {
    let mut s = view();
    sync(
        &mut s,
        json!([{"entryId":"a","kind":"user","text":"1"},{"entryId":"b","kind":"assistant","usage":{"input":5,"output":1}},{"entryId":"a","kind":"user","text":"2"}]),
    );
    assert_eq!(s.get("entry", "a")["text"], "2");
    assert_eq!(s.stats().messages, 2);
    assert_eq!(s.stats().tokens, 6.);
    s.changes.clear();
    event(&mut s, 5, json!({"type":"text_delta","delta":"x"}));
    event(&mut s, 3, json!({"type":"text_delta","delta":"y"}));
    assert!(
        matches!(&s.changes[..],[Change::Upsert(i)] if i.collection=="stream"&&i.json=="\"x\"")
    );
    assert!(event(&mut s, 6, json!({"type":"session_changed"})));
    assert!(!s.ready());
    event(&mut s, 7, json!({"type":"text_delta","delta":"z"}));
    assert!(s.get("stream", "text").is_null());
}
#[test]
fn attention_includes_waiting_children_then_normal_then_bot() {
    let mut s = view();
    s.apply(&json!({"type":"instances","instances":[{"instanceId":"bot","origin":"grok-bot","updatedAt":500},{"instanceId":"waiting","subagents":{"waiting":1}},{"instanceId":"pc"}]}));
    assert_eq!(
        s.snapshot()
            .into_iter()
            .filter(|r| r.collection == "instance")
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        ["waiting", "pc", "bot"]
    );
}
#[test]
fn mirrored_questions_block_send_and_cannot_be_answered_twice() {
    let mut s = view();
    event(
        &mut s,
        3,
        json!({"type":"ui_prompt","phase":"start","kind":"select","title":"?"}),
    );
    event(
        &mut s,
        4,
        json!({"type":"ui_request","request":{"kind":"select","title":"?","requestId":"r"}}),
    );
    assert!(s.get("view", "pcPrompt").is_null());
    assert_eq!(
        s.prepare("p", send("hello", vec![]), true),
        Err(Error::Blocked)
    );
    let answer = Operation::Answer {
        target: target(),
        params: r#"{"requestId":"r","value":"ok"}"#.into(),
    };
    assert!(s.prepare("a", answer.clone(), true).is_ok());
    assert_eq!(s.prepare("b", answer.clone(), true), Err(Error::Blocked));
    s.release("a");
    event(&mut s, 5, json!({"type":"ui_request_end","requestId":"r"}));
    assert_eq!(s.prepare("c", answer, true), Err(Error::StaleTarget));
}
#[test]
fn uncertain_images_require_new_id_same_session_and_exact_images() {
    let mut s = view();
    let image = json!({"type":"image","mimeType":"image/png","data":"YQ=="});
    let user = json!({"kind":"user","entryId":"old","text":"","images":[image]});
    sync(&mut s, json!([user]));
    let op = send(
        "",
        vec![Image {
            mime_type: "image/png".into(),
            data: "YQ==".into(),
        }],
    );
    assert!(s.prepare("p", op, true).is_ok());
    s.draft_status("p", "unconfirmed");
    sync(
        &mut s,
        json!([user,{"kind":"user","entryId":"new","text":"","images":[image],"imageCount":2,"imageIndices":[1]}]),
    );
    assert!(!s.get("draft", "p").is_null());
    event(
        &mut s,
        3,
        json!({"type":"message_end","item":{"kind":"user","entryId":"new","text":"","images":[image]}}),
    );
    assert!(s.get("draft", "p").is_null());
}
#[test]
fn caps_and_explicit_target_guard_side_effects() {
    let mut s = view();
    assert_eq!(
        s.prepare("1", Operation::Devices, true),
        Err(Error::Unsupported)
    );
    assert_eq!(
        s.prepare(
            "2",
            Operation::Abort {
                target: Target {
                    instance: "i".into(),
                    session: Some("previous".into())
                }
            },
            true
        ),
        Err(Error::StaleTarget)
    );
    event(&mut s, 3, json!({"type":"agent_start"}));
    assert_eq!(
        s.prepare(
            "t",
            rc(
                pimate_protocol::RcMethod::SessionOp,
                json!({"op":"tree","arg":"node"})
            ),
            true
        ),
        Err(Error::Blocked)
    );
    s.put("view", "promptImages", json!(false));
    assert_eq!(
        s.prepare(
            "x",
            send(
                "",
                vec![Image {
                    mime_type: "image/png".into(),
                    data: "YQ==".into()
                }]
            ),
            true
        ),
        Err(Error::Unsupported)
    );
    assert!(s.drafts.is_empty());
}
#[test]
fn commands_prefer_local_and_do_not_turn_unknown_commands_into_prompts() {
    let mut s = view();
    s.receipt("commands",&json!("i"),&mut json!({"commands":[{"name":"new","source":"extension"},{"name":"review","source":"prompt"}]})).unwrap();
    assert_eq!(s.suggestions("/rev"), ["review"]);
    assert!(s.suggestions("/review ").is_empty());
    assert_eq!(
        s.prepare("x", send("/new", vec![]), false),
        Err(Error::BuiltinCommand)
    );
    assert_eq!(
        s.prepare("x", send("/typo", vec![]), false),
        Err(Error::Unsupported)
    );
    assert_eq!(
        s.prepare("x", send("/review here", vec![]), false).unwrap()["request"]["params"]["expandTemplates"],
        true
    );
    assert!(s.drafts.is_empty());
}
#[test]
fn auth_strips_untrusted_url_and_ignores_late_revision() {
    let mut s = view();
    for (rev, url, valid) in [
        (1, "https://auth.openai.com/codex/device", true),
        (2, "https://evil.com/codex/device", false),
        (1, "https://auth.openai.com/codex/device", false),
    ] {
        let mut body = json!({"flow":{"id":"f","revision":rev,"provider":"openai-codex","authorization":{"url":url,"userCode":"AB-CD"}}});
        s.receipt("auth_wait", &json!("login"), &mut body).unwrap();
        assert_eq!(body["flow"]["authorization"].is_object(), valid);
    }
    assert_eq!(s.get("auth", "login")["revision"], 2);
}
#[test]
fn quota_gauge_expires_without_an_event() {
    let mut s = view();
    event(
        &mut s,
        4,
        json!({"type":"quota","quota":{"source":"codex","windows":[{"minutes":300,"usedPercent":42.2,"resetsAt":1000}]}}),
    );
    assert_eq!(s.allowances(900)[1].gauge, Some(42.));
    assert_eq!(s.allowances(1000)[1].gauge, None);
}
#[test]
fn subagent_controls_are_limited_to_supported_backends_and_actions() {
    let mut s = view();
    let mut r = json!({"id":"run","backend":"a","state":"running","actions":["stop"],"children":[{"id":"x","actions":["steer"]}]});
    event(
        &mut s,
        4,
        json!({"type":"subagents_reset","state":{"runs":[r],"backends":[{"backend":"a","supported":false}]}}),
    );
    assert!(s.get("run", "run").is_null());
    r["backend"] = json!("b");
    event(&mut s, 5, json!({"type":"subagent_run","run":r}));
    assert_eq!(s.get("run", "run")["stopAction"], "stop");
    assert_eq!(
        s.prepare(
            "x",
            rc(
                pimate_protocol::RcMethod::SubagentControl,
                json!({"runId":"run","childId":"x","action":"stop_child"})
            ),
            true
        ),
        Err(Error::Unsupported)
    );
}
#[test]
fn image_receipts_reject_garbage() {
    let mut s = view();
    for data in ["x", "YQ=", "", "Y Q="] {
        assert_eq!(
            s.receipt(
                "entry_image",
                &json!("i"),
                &mut json!({"image":{"mimeType":"image/png","data":data}})
            ),
            Err(Error::InvalidMessage)
        );
    }
}
