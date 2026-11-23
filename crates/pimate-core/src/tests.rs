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

#[test]
fn stop_before_or_after_steer_ack_preserves_only_unconsumed_input() {
    for ack_first in [true, false] {
        for consumed in [true, false] {
            let mut s = view();
            s.prepare("p", send("direction", vec![]), true).unwrap();
            if ack_first {
                s.prompt_ack("p", &json!("steer"))
            }
            if consumed {
                event(
                    &mut s,
                    6,
                    json!({"type":"message_end","item":{"kind":"user","entryId":"u","text":"direction"}}),
                );
            }
            event(&mut s, 7, json!({"type":"agent_settled","aborted":true}));
            if !ack_first {
                s.prompt_ack("p", &json!("steer"))
            }
            event(&mut s, 9, json!({"type":"agent_settled","aborted":false}));
            assert_eq!(
                s.get("draft", "p")["status"],
                if consumed {
                    Value::Null
                } else {
                    json!("unconfirmed")
                }
            );
        }
    }
}
#[test]
fn pc_history_cannot_confirm_a_rejected_request_or_unlock_a_pending_send() {
    let mut s = view();
    s.prepare("p", send("yes", vec![]), true).unwrap();
    event(
        &mut s,
        6,
        json!({"type":"message_end","item":{"kind":"user","entryId":"pc","text":"yes"}}),
    );
    assert_eq!(
        s.prepare("q", send("next", vec![]), true),
        Err(Error::Blocked)
    );
    s.draft_status("p", "rejected");
    sync(&mut s, json!([{"kind":"user","entryId":"pc","text":"yes"}]));
    assert!(s.get("draft", "p").is_null());
    assert!(s.drafts.is_empty());
    for status in ["unknown", "accepted"] {
        let mut s = view();
        s.prepare("p", send("yes", vec![]), true).unwrap();
        event(
            &mut s,
            6,
            json!({"type":"message_end","item":{"kind":"user","entryId":"new","text":"yes"}}),
        );
        if status == "unknown" {
            s.draft_status("p", "unconfirmed")
        } else {
            s.prompt_ack("p", &json!("steer"))
        }
        assert!(s.get("draft", "p").is_null())
    }
}
#[test]
fn multiple_stopped_images_are_independent_of_manual_retry() {
    let mut s = view();
    for (id, data) in [("first", "YQ=="), ("second", "Yg==")] {
        s.prepare(
            id,
            send(
                id,
                vec![Image {
                    mime_type: "image/png".into(),
                    data: data.into(),
                }],
            ),
            true,
        )
        .unwrap();
        s.prompt_ack(id, &json!("steer"));
    }
    event(&mut s, 6, json!({"type":"agent_settled","aborted":true}));
    s.prepare("other", send("unrelated", vec![]), true).unwrap();
    s.prompt_ack("other", &json!("prompt"));
    s.prepare(
        "retry",
        send(
            "first",
            vec![Image {
                mime_type: "image/png".into(),
                data: "YQ==".into(),
            }],
        ),
        true,
    )
    .unwrap();
    s.prompt_ack("retry", &json!("prompt"));
    assert_eq!(s.drafts.len(), 1);
    assert_eq!(s.get("draft", "second")["status"], "unconfirmed");
}
#[test]
fn published_instance_edits_reconstruct_every_snapshot() {
    let mut s = view();
    let mut mirror: Vec<(String, String)> = vec![];
    let mut seed = 5u32;
    let mut rand = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed >> 16
    };
    for _ in 0..1000 {
        let list=(0..12).filter_map(|id|{let r=rand();(r%5!=0).then(||json!({"instanceId":id.to_string(),"updatedAt":r%8,"origin":if r%2==0{"grok-bot"}else{"pc"},"status":if r%3==0{"waiting_input"}else{"idle"}}))}).collect::<Vec<_>>();
        s.apply(&json!({"type":"instances","instances":list}));
        for change in s.changes.drain(..) {
            match change {
                Change::Remove { collection, id } if collection == "instance" => {
                    mirror.retain(|(k, _)| k != &id)
                }
                Change::Upsert(i) if i.collection == "instance" => {
                    mirror.retain(|(k, _)| k != &i.id);
                    mirror.insert(i.position as usize, (i.id, i.json))
                }
                _ => {}
            }
        }
        assert_eq!(
            mirror,
            s.snapshot()
                .into_iter()
                .filter(|i| i.collection == "instance")
                .map(|i| (i.id, i.json))
                .collect::<Vec<_>>()
        );
    }
}
#[test]
fn sync_forwards_only_matching_operation_results_and_keeps_them_after_snapshot() {
    for changed in [true, false] {
        for (op, session) in [("tree", "s"), ("new", "new")] {
            let mut s = view();
            s.prepare(
                "op",
                rc(
                    pimate_protocol::RcMethod::SessionOp,
                    json!({"op":op,"arg":"node"}),
                ),
                true,
            )
            .unwrap();
            if changed {
                event(
                    &mut s,
                    6,
                    json!({"type":"session_changed","reason":op,"session":{"id":session}}),
                );
            }
            event(&mut s, 7, json!({"type":"session_op_done","op":"fork"}));
            assert_eq!(s.get("busy", "operation")["id"], "op");
            let e = json!({"type":"session_op_done","op":op,"cancelled":false,"editorText":"kept"});
            s.apply(
                &json!({"type":"event","instanceId":"i","sessionId":session,"seq":8,"event":e}),
            );
            assert!(s.get("busy", "operation").is_null());
            sync(&mut s, json!([]));
            assert_eq!(s.get("operationResult", "latest")["event"], e);
        }
    }
}
#[test]
fn unknown_session_cannot_be_aborted_and_old_rc_has_no_command_candidates() {
    let mut s = view();
    s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":5,"session":null},"history":{"items":[]}}));
    assert_eq!(
        s.prepare(
            "x",
            Operation::Abort {
                target: Target {
                    instance: "i".into(),
                    session: None
                }
            },
            true
        ),
        Err(Error::StaleTarget)
    );
    assert!(s.suggestions("/new").is_empty());
    let msg = s
        .prepare(
            "p",
            Operation::Send {
                target: Target {
                    instance: "i".into(),
                    session: None,
                },
                text: "/new".into(),
                images: vec![],
            },
            true,
        )
        .unwrap();
    assert_eq!(msg["request"]["params"]["text"], "/new");
}
#[test]
fn model_preference_receipts_publish_only_changed_rows() {
    let mut s = view();
    let i = json!("i");
    s.receipt(
        "models",
        &i,
        &mut json!({"models":[{"id":"a","provider":"p"},{"id":"b","provider":"p"}]}),
    )
    .unwrap();
    s.changes.clear();
    s.receipt(
        "set_scoped_models",
        &i,
        &mut json!({"scoped":[{"id":"a","provider":"p"}]}),
    )
    .unwrap();
    assert!(matches!(&s.changes[..],[Change::Upsert(i)] if i.id==r#"["p","a"]"#));
    assert_eq!(s.get("model", r#"["p","a"]"#)["favorite"], true);
    let default = json!({"model":{"id":"b","provider":"p"}});
    s.receipt("set_default_model", &i, &mut json!({"default":default}))
        .unwrap();
    assert_eq!(s.get("view", "defaultModel"), &default);
}
#[test]
fn buffered_compaction_error_and_notification_survive_subscription_retry() {
    let mut s = view();
    s.prepare(
        "compact",
        rc(pimate_protocol::RcMethod::Compact, json!({})),
        true,
    )
    .unwrap();
    s.apply(&json!({"type":"subscribe_error","instanceId":"i","code":"unreachable"}));
    event(&mut s, 7, json!({"type":"compact_error","error":"failed"}));
    event(
        &mut s,
        8,
        json!({"type":"notify","message":"hi","level":"info"}),
    );
    sync(&mut s, json!([]));
    assert_eq!(
        s.get("operationResult", "latest")["event"]["error"],
        "failed"
    );
    assert_eq!(s.get("notice", "latest")["event"]["message"], "hi");
}
#[test]
fn only_observed_session_arrival_is_proof_of_a_missing_result() {
    for (op, dest, expected) in [
        ("reload", "s", "done"),
        ("fork", "next", "done"),
        ("fork", "another", "unknown"),
    ] {
        let mut s = view();
        s.prepare(
            "op",
            rc(
                pimate_protocol::RcMethod::SessionOp,
                json!({"op":op,"arg":"u"}),
            ),
            true,
        )
        .unwrap();
        s.accept_op("op");
        event(
            &mut s,
            6,
            json!({"type":"session_changed","reason":op,"session":{"id":if op=="reload"{"s"}else{"next"}}}),
        );
        s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":8,"session":{"id":dest}},"history":{"items":[]}}));
        if op == "fork" {
            assert!(s.get("operationResult", "latest").is_null());
            s.expire_result(s.result_deadline.unwrap());
        }
        assert_eq!(s.get("operationResult", "latest")["outcome"], expected);
        assert!(s.get("busy", "operation").is_null());
    }
}
#[test]
fn a_history_entry_confirms_at_most_one_identical_steer() {
    for history_before_stop in [false, true] {
        let mut s = view();
        for id in ["one", "two"] {
            s.prepare(id, send("again", vec![]), true).unwrap();
            s.prompt_ack(id, &json!("steer"));
        }
        let history = json!({"kind":"user","entryId":"entry","text":"again"});
        if history_before_stop {
            event(&mut s, 3, json!({"type":"message_end","item":history}));
        }
        s.settle_drafts(&json!("i"), &json!("s"), true);
        sync(&mut s, json!([history]));
        sync(&mut s, json!([history]));
        assert_eq!(s.drafts.len(), 1);
        assert_eq!(s.get("draft", "two")["status"], "unconfirmed");
        sync(
            &mut s,
            json!([history,{"kind":"user","entryId":"second","text":"again"}]),
        );
        assert!(s.drafts.is_empty());
    }
}
fn accepted(s: &mut State, op: &str) {
    use pimate_protocol::RcMethod::*;
    let cmd = if op == "compact" {
        rc(Compact, json!({}))
    } else {
        rc(SessionOp, json!({"op":op,"arg":"node"}))
    };
    s.prepare("request", cmd, true).unwrap();
    s.accept_op("request");
}
#[test]
fn operation_receipts_on_either_side_of_snapshot_have_identical_effect() {
    for (op, e) in [
        (
            "tree",
            json!({"type":"session_op_done","op":"tree","editorText":"original","cancelled":false}),
        ),
        ("compact", json!({"type":"compact_error","error":"failed"})),
        ("compact", json!({"type":"compact_done"})),
    ] {
        for before in [false, true] {
            let mut s = view();
            accepted(&mut s, op);
            event(
                &mut s,
                6,
                json!({"type":"session_changed","reason":"tree","session":{"id":"s"}}),
            );
            if before {
                event(&mut s, 9, e.clone());
            }
            s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":10,"session":{"id":"s"}},"history":{"items":[]}}));
            if !before {
                assert!(s.get("operationResult", "latest").is_null());
                assert_eq!(s.get("busy", "operation")["id"], "request");
                event(&mut s, 11, e.clone());
            }
            assert_eq!(s.get("operationResult", "latest")["event"], e);
            assert_eq!(s.get("operationResult", "latest")["id"], "request");
            assert!(s.get("busy", "operation").is_null());
        }
    }
}
#[test]
fn switch_arrival_uses_resume_reason() {
    let mut s = view();
    accepted(&mut s, "switch");
    event(
        &mut s,
        6,
        json!({"type":"session_changed","reason":"resume","session":{"id":"dest"}}),
    );
    s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":10,"session":{"id":"dest"}},"history":{"items":[]}}));
    assert_eq!(s.get("operationResult", "latest")["outcome"], "done");
}
#[test]
fn accepted_operation_disconnect_reports_unknown_and_keeps_it_on_resubscription() {
    for op in ["compact", "tree"] {
        let mut s = view();
        accepted(&mut s, op);
        s.changes.clear();
        s.reset();
        assert!(
            s.changes
                .iter()
                .any(|c| matches!(c,Change::Upsert(i) if i.collection=="operationResult"))
        );
        s.reset();
        sync(&mut s, json!([]));
        assert_eq!(
            s.get("operationResult", "latest"),
            &json!({"id":"request","instanceId":"i","outcome":"unknown"})
        );
    }
}
#[test]
fn grace_expires_once_even_without_more_events() {
    use pimate_protocol::RcMethod::*;
    for ack_before_snapshot in [true, false] {
        let mut s = view();
        s.prepare("A", rc(Compact, json!({})), true).unwrap();
        if ack_before_snapshot {
            s.accept_op("A")
        }
        sync(&mut s, json!([]));
        if !ack_before_snapshot {
            s.accept_op("A")
        };
        let deadline = s.result_deadline.unwrap();
        s.expire_result(deadline - std::time::Duration::from_millis(1));
        assert_eq!(
            s.prepare("B", rc(Compact, json!({})), true),
            Err(Error::Blocked)
        );
        sync(&mut s, json!([]));
        assert_eq!(s.result_deadline, Some(deadline));
        s.expire_result(deadline);
        assert_eq!(
            s.get("operationResult", "latest"),
            &json!({"id":"A","outcome":"unknown"})
        );
        s.prepare("B", rc(Compact, json!({})), true).unwrap();
        s.accept_op("B");
        event(&mut s, 3, json!({"type":"compact_done"}));
        assert_eq!(s.get("operationResult", "latest")["id"], "B");
        assert!(s.get("busy", "operation").is_null());
        event(&mut s, 4, json!({"type":"compact_done"}));
        s.expire_result(deadline);
        assert_eq!(result_count(&s, "A"), 1);
        assert_eq!(result_count(&s, "B"), 1);
    }
}
#[test]
fn late_v1_completion_never_credits_the_next_request() {
    use pimate_protocol::RcMethod::*;
    for (method, params, terminal) in [
        (Compact, json!({}), json!({"type":"compact_done"})),
        (
            Compact,
            json!({}),
            json!({"type":"compact_error","error":"old"}),
        ),
        (
            SessionOp,
            json!({"op":"tree","arg":"node"}),
            json!({"type":"session_op_done","op":"tree","cancelled":false}),
        ),
    ] {
        for ack_first in [true, false] {
            for round_trip in [true, false] {
                let mut s = view();
                s.prepare("A", rc(method, params.clone()), true).unwrap();
                s.accept_op("A");
                sync(&mut s, json!([]));
                s.expire_result(s.result_deadline.unwrap());
                if round_trip {
                    for dst in ["s2", "s"] {
                        s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":2,"session":{"id":dst},"status":"idle","sessionTree":true},"history":{"items":[]}}));
                    }
                }
                s.prepare("B", rc(method, params.clone()), true).unwrap();
                if ack_first {
                    s.accept_op("B");
                }
                event(&mut s, 3, terminal.clone());
                if !ack_first {
                    s.accept_op("B");
                }
                assert_eq!(
                    s.get("operationResult", "latest"),
                    &json!({"id":"B","outcome":"unknown"})
                );
                assert!(s.get("busy", "operation").is_null());
                assert_eq!(result_count(&s, "B"), 1);
                // The second terminal drains B's remaining mark, so C can complete normally.
                event(&mut s, 4, terminal.clone());
                s.prepare("C", rc(method, params.clone()), true).unwrap();
                s.accept_op("C");
                event(&mut s, 5, terminal.clone());
                assert_eq!(s.get("operationResult", "latest")["event"], terminal);
            }
        }
    }
}
#[test]
fn session_move_before_mark_creation_cannot_credit_next_session() {
    let mut s = view();
    accepted(&mut s, "compact");
    s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":3,"session":{"id":"s2"}},"history":{"items":[]}}));
    s.expire_result(s.result_deadline.unwrap());
    assert_eq!(s.get("operationResult", "latest")["outcome"], "unknown");
    let b = Operation::Rc {
        target: Target {
            instance: "i".into(),
            session: Some("s2".into()),
        },
        method: pimate_protocol::RcMethod::Compact,
        params: "{}".into(),
    };
    s.prepare("B", b, true).unwrap();
    s.accept_op("B");
    s.apply(&json!({"type":"event","instanceId":"i","seq":4,"sessionId":"s2","event":{"type":"compact_done"}}));
    assert_eq!(
        s.get("operationResult", "latest"),
        &json!({"id":"B","outcome":"unknown"})
    );
    assert!(s.get("busy", "operation").is_null());
}
#[test]
fn stale_or_absent_discovery_generation_cannot_retire_subscribed_operation() {
    for instances in [json!([{"instanceId":"i","startedAt":1}]), json!([])] {
        let mut s = view();
        let g2 = json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":2,"startedAt":2,"session":{"id":"s"}},"history":{"items":[]}});
        s.apply(&g2);
        accepted(&mut s, "compact");
        let stale = json!({"type":"instances","instances":instances});
        assert!(!s.apply(&stale));
        assert_eq!(s.generation("i"), Some(2));
        assert_eq!(s.get("busy", "operation")["id"], "request");
        s.apply(&g2);
        s.expire_result(s.result_deadline.unwrap());
        assert_eq!(s.get("operationResult", "latest")["outcome"], "unknown");
        s.apply(&stale);
        s.prepare("B", rc(pimate_protocol::RcMethod::Compact, json!({})), true)
            .unwrap();
        s.accept_op("B");
        event(&mut s, 3, json!({"type":"compact_done"}));
        assert_eq!(
            s.get("operationResult", "latest"),
            &json!({"id":"B","outcome":"unknown"})
        );
        assert!(s.get("busy", "operation").is_null());
    }
}
#[test]
fn replacement_during_grace_drops_the_old_operation_and_its_marks() {
    for list_first in [true, false] {
        let mut s = view();
        let snap = |start| json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":2,"startedAt":start,"session":{"id":"s"}},"history":{"items":[]}});
        s.apply(&snap(1));
        accepted(&mut s, "compact");
        s.apply(&snap(1));
        let old_deadline = s.result_deadline.unwrap();
        if list_first {
            assert!(!s.apply(
                &json!({"type":"instances","instances":[{"instanceId":"i","startedAt":2}]})
            ));
            assert!(s.ready());
            assert_eq!(s.generation("i"), Some(1));
        }
        s.apply(&snap(2));
        assert_eq!(
            s.get("operationResult", "latest"),
            &json!({"id":"request","outcome":"unknown"})
        );
        assert!(s.get("busy", "operation").is_null());
        s.expire_result(old_deadline);
        s.prepare(
            "new",
            rc(pimate_protocol::RcMethod::Compact, json!({})),
            true,
        )
        .unwrap();
        s.accept_op("new");
        event(&mut s, 3, json!({"type":"compact_done"}));
        assert_eq!(
            s.get("operationResult", "latest")["event"]["type"],
            "compact_done"
        );
    }
}
#[test]
fn missing_marks_end_on_expiry_process_replacement_or_instance_removal() {
    for cleanup in ["expire", "replace", "remove"] {
        let mut s = view();
        let snap = |start| json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":2,"startedAt":start,"session":{"id":"s"}},"history":{"items":[]}});
        s.apply(&snap(1));
        accepted(&mut s, "compact");
        sync(&mut s, json!([]));
        let deadline = s.result_deadline.unwrap();
        s.expire_result(deadline);
        match cleanup {
            "expire" => s.expire_result(deadline + state::MISSING_TTL),
            "replace" => {
                s.apply(&snap(2));
            }
            _ => {
                s.apply(&json!({"type":"instance_gone","instanceId":"i"}));
                s.apply(&snap(1));
            }
        }
        sync(&mut s, json!([]));
        accepted(&mut s, "compact");
        event(&mut s, 3, json!({"type":"compact_done"}));
        assert_eq!(
            s.get("operationResult", "latest")["event"]["type"],
            "compact_done"
        );
    }
}
#[test]
fn pre_ack_switch_move_requires_both_ack_and_matching_snapshot() {
    for ack_first in [true, false] {
        let mut s = view();
        s.prepare(
            "sw",
            rc(
                pimate_protocol::RcMethod::SessionOp,
                json!({"op":"switch","arg":"dest"}),
            ),
            true,
        )
        .unwrap();
        event(
            &mut s,
            3,
            json!({"type":"session_changed","reason":"resume","session":{"id":"dest"}}),
        );
        if ack_first {
            s.accept_op("sw");
        }
        s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":4,"session":{"id":"dest"}},"history":{"items":[]}}));
        if !ack_first {
            assert!(s.get("operationResult", "latest").is_null());
            s.accept_op("sw");
        }
        assert_eq!(
            s.get("operationResult", "latest"),
            &json!({"id":"sw","outcome":"done","sessionId":"dest"})
        );
    }
}
fn result_count(s: &State, id: &str) -> usize {
    s.changes.iter().filter(|c|matches!(c,Change::Upsert(i) if i.collection=="operationResult"&&serde_json::from_str::<Value>(&i.json).unwrap()["id"]==id)).count()
}
#[test]
fn another_tree_move_during_or_after_grace_cannot_publish_its_editor_text() {
    for (expired, resyncing) in [(false, false), (true, false), (false, true)] {
        let mut s = view();
        accepted(&mut s, "tree");
        event(
            &mut s,
            6,
            json!({"type":"session_changed","reason":"tree","session":{"id":"s"}}),
        );
        sync(&mut s, json!([]));
        let deadline = s.result_deadline.unwrap();
        if resyncing {
            s.apply(&json!({"type":"subscribe_error","instanceId":"i","code":"unreachable"}));
        }
        if expired {
            s.expire_result(deadline)
        }
        event(
            &mut s,
            9,
            json!({"type":"session_changed","reason":"tree","session":{"id":"s"}}),
        );
        sync(&mut s, json!([]));
        event(
            &mut s,
            10,
            json!({"type":"session_op_done","op":"tree","cancelled":false,"editorText":"another phone"}),
        );
        assert_eq!(
            s.get("operationResult", "latest"),
            &json!({"id":"request","outcome":"unknown"})
        );
        s.expire_result(deadline);
        assert_eq!(result_count(&s, "request"), 1);
        assert!(s.get("busy", "operation").is_null());
    }
}
#[test]
fn only_matching_unresolved_drafts_accumulate_consumed_ids() {
    let mut s = view();
    for (id, text, status) in [
        ("reject", "same", "rejected"),
        ("different", "different", "unconfirmed"),
        ("image", "same", "unconfirmed"),
        ("first", "same", "queued"),
        ("second", "same", "sending"),
    ] {
        let images = if id == "image" {
            vec![Image {
                mime_type: "image/png".into(),
                data: "YQ==".into(),
            }]
        } else {
            vec![]
        };
        s.prepare(id, send(text, images), true).unwrap();
        s.draft_status(id, status);
    }
    event(
        &mut s,
        3,
        json!({"type":"message_end","item":{"entryId":"new","kind":"user","text":"same"}}),
    );
    for d in &s.drafts {
        assert_eq!(d.known.contains("new"), d.id == "second");
    }
    s.draft_status("second", "rejected");
    assert!(s.drafts.iter().all(|d| d.known.is_empty()));
}
#[test]
fn fork_waits_for_real_event_or_uses_proven_arrival_and_source_text() {
    for delivered in [true, false] {
        let mut s = view();
        sync(
            &mut s,
            json!([{"kind":"user","entryId":"node","text":"original"}]),
        );
        accepted(&mut s, "fork");
        event(
            &mut s,
            6,
            json!({"type":"session_changed","reason":"fork","session":{"id":"dst"}}),
        );
        s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":8,"session":{"id":"dst"}},"history":{"items":[]}}));
        let deadline = s.result_deadline.unwrap();
        let m = json!({"type":"event","sessionId":"dst","instanceId":"i","seq":9,"event":{"type":"session_op_done","op":"fork","cancelled":false,"editorText":"from wire"}});
        if delivered {
            s.apply(&m);
        }
        s.expire_result(deadline);
        s.apply(&m);
        let result = s.get("operationResult", "latest");
        assert_eq!(result["forkText"], "original");
        if delivered {
            assert_eq!(result["event"]["editorText"], "from wire")
        } else {
            assert_eq!(result["outcome"], "done");
            assert!(result["event"].is_null())
        }
        assert_eq!(result_count(&s, "request"), 1);
        assert!(s.get("busy", "operation").is_null());
    }
}

#[test]
fn take_only_matching_unconfirmed_draft_once() {
    let mut s = view();
    s.prepare(
        "id",
        send(
            "body",
            vec![Image {
                mime_type: "image/png".into(),
                data: "YQ==".into(),
            }],
        ),
        true,
    )
    .unwrap();
    assert!(s.take_unconfirmed(&target(), "id").is_err());
    s.draft_status("id", "unconfirmed");
    for t in [
        Target {
            instance: "other".into(),
            ..target()
        },
        Target {
            session: None,
            ..target()
        },
    ] {
        assert!(s.take_unconfirmed(&t, "id").is_err());
    }
    // The displayed session is now current, but the named draft belongs to its predecessor.
    s.put("view", "session", json!({"id":"next"}));
    assert!(
        s.take_unconfirmed(
            &Target {
                session: Some("next".into()),
                ..target()
            },
            "id"
        )
        .is_err()
    );
    s.put("view", "session", json!({"id":"s"}));
    let payload = s.take_unconfirmed(&target(), "id").unwrap();
    assert_eq!(payload.text, "body");
    assert_eq!(payload.images[0].data, "YQ==");
    assert_eq!(payload.images[0].mime_type, "image/png");
    assert!(s.take_unconfirmed(&target(), "id").is_err());
    s.prepare("late", send("accepted", vec![]), true).unwrap();
    s.draft_status("late", "unconfirmed");
    sync(
        &mut s,
        json!([{"kind":"user","entryId":"received","text":"accepted"}]),
    );
    assert!(s.take_unconfirmed(&target(), "late").is_err());
}

#[test]
fn observed_timing_and_partial_survive_reopening_only_an_unanswered_call() {
    let mut s = view();
    let events = [
        json!({"type":"agent_start"}),
        json!({"type":"thinking_delta","delta":"a"}),
        json!({"type":"text_delta","delta":"b"}),
        json!({"type":"message_end","item":{"kind":"assistant","entryId":"e","thinking":"a","toolCalls":[{"id":"t"}]}}),
        json!({"type":"tool_start","toolCallId":"t"}),
        json!({"type":"tool_update","toolCallId":"t","partial":{"content":[{"type":"text","text":"test"}]}}),
    ];
    for (i, e) in events.into_iter().enumerate() {
        s.apply_at(
            &json!({"type":"event","instanceId":"i","sessionId":"s","seq":3+i,"event":e}),
            1000 + i as u64 * 50,
        );
    }
    let item = s.get("entry", "e").clone();
    let mut snapshot = json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":10,"session":{"id":"s"},"status":"running"},"history":{"items":[item]}});
    s.reset();
    s.apply_at(&snapshot, 5000);
    assert_eq!(s.get("view", "runningSince"), 1000);
    assert_eq!(s.get("thinkingMs", "e"), 50);
    assert_eq!(s.get("tool", "t")["startedAt"], 1200);
    assert_eq!(s.get("tool", "t")["partial"]["content"][0]["text"], "test");
    snapshot["history"]["items"]
        .as_array_mut()
        .unwrap()
        .push(json!({"entryId":"result","kind":"tool_result","toolCallId":"t"}));
    s.apply_at(&snapshot, 6000);
    assert!(s.get("tool", "t").is_null());
}

#[test]
fn rc_time_and_session_move_are_not_lost_in_resync() {
    let mut s = view();
    s.apply_at(&json!({"type":"event","sessionId":"s","instanceId":"i","seq":3,"timestamp":100,"event":{"type":"agent_start"}}), 999);
    assert_eq!(s.get("view", "runningSince"), 100);
    event(
        &mut s,
        4,
        json!({"type":"session_changed","reason":"resume","session":{"id":"b","name":"next"}}),
    );
    s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":5,"status":"idle","session":{"id":"b"}},"history":{"items":[]}}));
    assert_eq!(s.get("notice", "sessionChanged")["reason"], "resume");
    assert_eq!(s.get("notice", "sessionChanged")["from"]["id"], "s");
    assert_eq!(s.get("notice", "sessionChanged")["to"]["id"], "b");
}

#[test]
fn past_child_failure_is_not_attention_and_new_questions_supersede_observed_errors() {
    let mut s = view();
    let mut list = json!({"type":"instances","instances":[{"instanceId":"i","session":{"id":"s"},"status":"idle","subagents":{"failed":1}}]});
    s.apply(&list);
    assert_eq!(s.get("instance", "i")["section"], "normal");
    sync(
        &mut s,
        json!([{"entryId":"prev","kind":"assistant","stopReason":"error","errorMessage":"not observed"}]),
    );
    assert_eq!(
        s.get("instance", "i")["failureText"],
        "Pi の実行に失敗しました"
    );
    for (seq, message, expected) in [
        (3, "", "Pi の実行に失敗しました"),
        (4, "out of memory", "out of memory"),
    ] {
        event(
            &mut s,
            seq,
            json!({"type":"message_end","item":{"entryId":"a","kind":"assistant","stopReason":"error","errorMessage":message}}),
        );
        s.apply(&list);
        assert_eq!(s.get("instance", "i")["failureText"], expected);
        assert_eq!(s.get("instance", "i")["section"], "waiting");
    }
    list["instances"][0]["status"] = json!("waiting_input");
    s.apply(&list);
    assert!(s.get("instance", "i")["failureText"].is_null());
    event(&mut s, 5, json!({"type":"agent_start"}));
    list["instances"][0]["status"] = json!("idle");
    s.apply(&list);
    assert_eq!(s.get("instance", "i")["section"], "normal");
}

#[test]
fn same_seq_restores_observed_turn_start_without_tools() {
    let mut s = view();
    event(&mut s, 3, json!({"type":"agent_start"}));
    let start = s.get("view", "runningSince").clone();
    let mut snap = json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":3,"session":{"id":"s"},"status":"running"},"history":{"items":[]}});
    s.reset();
    s.apply_at(&snap, 100);
    assert_eq!(s.get("view", "runningSince"), &start);
    snap["snapshot"]["seq"] = json!(6);
    s.reset();
    s.apply_at(&snap, 200);
    assert_eq!(s.get("view", "runningSince"), 200);
}

#[test]
fn failure_in_new_session_waits_for_matching_discovery() {
    let mut s = view();
    let mut list = json!({"type":"instances","instances":[{"instanceId":"i","startedAt":1,"session":{"id":"s"},"status":"idle"}]});
    s.apply(&list);
    event(
        &mut s,
        3,
        json!({"type":"session_changed","reason":"resume","session":{"id":"b"}}),
    );
    s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"startedAt":1,"seq":4,"status":"idle","session":{"id":"b"}},"history":{"items":[]}}));
    s.apply(&json!({"type":"event","sessionId":"b","instanceId":"i","seq":5,"event":{"type":"message_end","item":{"kind":"assistant","entryId":"new","stopReason":"error","errorMessage":"B only"}}}));
    s.apply(&list);
    assert!(s.get("instance", "i")["failureText"].is_null());
    assert_eq!(s.get("instance", "i")["section"], "normal");
    s.apply(&json!({"type":"instances","instances":[]}));
    list["instances"][0]["session"]["id"] = json!("b");
    s.apply(&list);
    assert_eq!(s.get("instance", "i")["section"], "waiting");
    assert_eq!(s.get("instance", "i")["failureText"], "B only");
}

#[test]
fn live_update_after_oversized_preview_rebuilds_only_open_history_call() {
    let mut s = view();
    let item = json!({"entryId":"e","kind":"assistant","toolCalls":[{"id":"t","name":"bash","args":{"command":"build"}}]});
    event(&mut s, 3, json!({"type":"message_end","item":item}));
    event(
        &mut s,
        4,
        json!({"type":"tool_start","toolName":"bash","toolCallId":"t"}),
    );
    let update = |text: &str| json!({"type":"tool_update","toolName":"bash","toolCallId":"t","partial":{"content":[{"type":"text","text":text}]}});
    event(&mut s, 5, update(&"x".repeat(1024 * 1024)));
    let mut snapshot = json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":5,"session":{"id":"s"},"status":"running"},"history":{"items":[item]}});
    s.reset();
    s.apply(&snapshot);
    assert!(s.get("tool", "t").is_null());
    event(&mut s, 6, update("fresh"));
    let tool = s.get("tool", "t");
    assert_eq!(tool["status"], "running");
    assert_eq!(tool["partial"]["content"][0]["text"], "fresh");
    assert_eq!(tool["args"]["command"], "build");
    assert!(tool["startedAt"].is_null());
    snapshot["history"]["items"]
        .as_array_mut()
        .unwrap()
        .push(json!({"entryId":"r","kind":"tool_result","toolCallId":"t"}));
    snapshot["snapshot"]["seq"] = json!(7);
    s.apply(&snapshot);
    event(&mut s, 8, update("late"));
    assert!(s.get("tool", "t").is_null());
}

#[test]
fn machine_screen_operations_do_not_require_a_chat_subscription() {
    let mut s = State::default();
    s.caps = json!({"dirs":true,"github":true,"pairedDevices":true,"githubStatus":true,"hostRestart":true,"packageUpdate":true});
    let spawn = json!({"cwd":"/tmp/mock","trust":true,"askTool":true,"model":{"provider":"demo","id":"small"},"thinking":"low","message":"hello"});
    for (op, ty) in [
        (Operation::Folders, "folders"),
        (Operation::Dirs { path: "~/".into() }, "dirs"),
        (
            Operation::Spawn {
                params: spawn.to_string(),
            },
            "spawn",
        ),
        (Operation::GithubStatus, "github_status"),
        (Operation::Devices, "devices"),
        (
            Operation::Revoke {
                device_id: "other".into(),
            },
            "device_revoke",
        ),
        (Operation::UpdateCheck, "update_check"),
        (Operation::Update, "update"),
        (Operation::Restart, "host_restart"),
    ] {
        let frame = s.prepare("r", op, false).unwrap();
        assert_eq!(frame["type"], ty);
        if ty == "spawn" {
            assert_eq!(frame["params"], spawn);
        }
    }
    for query in [
        json!({"kind":"list"}),
        json!({"kind":"issue","number":3}),
        json!({"kind":"pr","number":2}),
    ] {
        let frame = s
            .prepare(
                "r",
                Operation::Github {
                    instance: "pi".into(),
                    query: query.to_string(),
                },
                false,
            )
            .unwrap();
        assert_eq!(frame["query"], query);
        assert_eq!(frame["instanceId"], "pi");
    }
    let models = || Operation::HostModels {
        instance: "pi".into(),
    };
    assert_eq!(s.prepare("r", models(), false), Err(Error::StaleTarget));
    s.apply(&json!({"type":"instances","instances":[{"instanceId":"pi"}]}));
    assert_eq!(
        s.prepare("r", models(), true).unwrap()["request"],
        json!({"id":"r","method":"models","params":{},"expectInstanceId":"pi"})
    );
    assert_eq!(s.selected, None);
}

#[test]
fn finished_child_stays_watched_until_a_new_user_entry() {
    let mut s = view();
    for (seq, state) in [(3, "running"), (4, "paused")] {
        event(
            &mut s,
            seq,
            json!({"type":"subagent_run","run":{"id":"r","state":state,"actions":["interrupt"],"children":[]}}),
        );
        assert_eq!(s.get("runWatch", "r"), true);
    }
    assert_eq!(s.get("run", "r")["open"], true);
    assert_eq!(
        s.prepare(
            "a",
            rc(
                pimate_protocol::RcMethod::SubagentControl,
                json!({"runId":"r","action":"interrupt"})
            ),
            false
        )
        .unwrap()["request"]["params"]["action"],
        "interrupt"
    );
    event(
        &mut s,
        5,
        json!({"type":"message_end","item":{"kind":"user","entryId":"next"}}),
    );
    assert!(s.get("runWatch", "r").is_null());
}

#[test]
fn reconnect_restores_only_watched_runs_in_the_same_turn() {
    for (next_pi, start, session, turn, run, restored) in [
        ("i", 1, "s", "u", "r", true),
        ("i", 1, "s", "next", "r", false),
        ("i", 1, "other", "u", "r", false),
        ("i", 2, "s", "u", "r", false),
        ("other", 1, "s", "u", "r", false),
        ("i", 1, "s", "u", "other", false),
    ] {
        let snapshot = |pi, start, session, turn, runs: Value| json!({"type":"subscribed","instanceId":pi,"snapshot":{"seq":2,"startedAt":start,"session":{"id":session},"subagents":{"runs":runs}},"history":{"items":[{"kind":"user","entryId":turn}]}});
        let mut s = State::default();
        s.selected = Some("i".into());
        s.apply(&snapshot("i", 1, "s", "u", json!([])));
        for (seq, status) in [(3, "running"), (4, "complete")] {
            event(
                &mut s,
                seq,
                json!({"type":"subagent_run","run":{"id":"r","state":status}}),
            );
        }
        s.reset();
        s.selected = None;
        s.reset();
        s.selected = Some(next_pi.into());
        s.apply(&snapshot(
            next_pi,
            start,
            session,
            turn,
            json!([{"id":run,"state":"complete"}]),
        ));
        assert_eq!(s.get("runWatch", run) == true, restored);
        assert_eq!(s.get("runWatch", "r") == true, restored);
    }
}

#[test]
fn auth_wait_is_exclusive_per_login_pi_until_the_receipt_or_timeout() {
    let mut s = view();
    s.caps = json!({"providerAuth":true});
    for pi in ["a", "b"] {
        s.auth_targets.insert(pi.into(), pi.into());
        s.put("loginCapability", pi, json!(true));
        s.put("auth", pi, json!({"id":"f"}));
    }
    let wait = |pi: &str| Operation::Rc {
        target: Target {
            instance: pi.into(),
            session: None,
        },
        method: pimate_protocol::RcMethod::AuthWait,
        params: json!({"flowId":"f","afterRevision":1}).to_string(),
    };
    s.prepare("1", wait("a"), false).unwrap();
    s.prepare("1", wait("a"), true).unwrap();
    s.reset(); // Switching chats does not cancel the login Pi's wait.
    assert_eq!(s.prepare("2", wait("a"), true), Err(Error::Blocked));
    s.prepare("3", wait("b"), true).unwrap();
    s.release("3");
    assert_eq!(s.prepare("2", wait("a"), false), Err(Error::Blocked));
    s.release_unknown("1");
    s.prepare("2", wait("a"), true).unwrap();
    s.release("2");
    s.prepare("4", wait("a"), true).unwrap();
}
