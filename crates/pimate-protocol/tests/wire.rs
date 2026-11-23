use pimate_protocol::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn roundtrip<T: DeserializeOwned + Serialize>(v: &Value) {
    let typed: T = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), *v);
}
fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/wire.json")).unwrap()
}
#[test]
fn upstream_wire_fixtures() {
    let f = fixtures();
    for v in f["app"].as_array().unwrap() {
        roundtrip::<AppMessage>(v)
    }
    for v in f["host"].as_array().unwrap() {
        roundtrip::<HostMessage>(v)
    }
    for v in f["server"].as_array().unwrap() {
        roundtrip::<ServerMessage>(v)
    }
    roundtrip::<RequestOf<method::AuthBegin>>(&f["begin"]);
    roundtrip::<Response<ResultOf<method::AuthBegin>>>(&f["receipt"]);
    roundtrip::<PairingLink>(&f["pairing"]);
}
#[test]
fn null_is_not_absent() {
    let mut s = fixtures()["host"][0]["snapshot"].clone();
    for v in [None, Some(Value::Null), Some(json!({"kind":"custom"}))] {
        s.as_object_mut().unwrap().remove("pcPrompt");
        if let Some(v) = v {
            s["pcPrompt"] = v
        }
        roundtrip::<InstanceSnapshot>(&s);
    }
    s.as_object_mut().unwrap().remove("session");
    assert!(serde_json::from_value::<InstanceSnapshot>(s).is_err());
    assert!(serde_json::from_value::<SessionRef>(json!({"id":"s","name":null})).is_err());
}
#[test]
fn unknown_messages_are_skippable_not_malformed_known_messages() {
    let v = json!({"type":"future","extra":true});
    assert!(matches!(
        serde_json::from_value(v.clone()),
        Ok(HostMessage::Unknown)
    ));
    assert!(matches!(
        serde_json::from_value(v.clone()),
        Ok(AppMessage::Unknown)
    ));
    assert!(matches!(
        serde_json::from_value(v.clone()),
        Ok(InstanceEvent::Unknown)
    ));
    assert!(matches!(
        serde_json::from_value(v),
        Ok(ServerMessage::Unknown(_))
    ));
    assert!(serde_json::from_value::<ServerMessage>(json!({"type":"event"})).is_err());
    assert!(
        serde_json::from_value::<HostMessage>(
            json!({"type":"host_restart_result","requestId":"r","ok":false})
        )
        .is_err()
    );
    let p: AppMessage = serde_json::from_value(json!({"type":"ping","t":1,"extra":2})).unwrap();
    assert_eq!(
        serde_json::to_value(p).unwrap(),
        json!({"type":"ping","t":1})
    );
}

#[test]
fn truncated_js_tool_output_survives_event_and_history() {
    // JSON.stringify(("x".repeat(32767) + "🙂").slice(0, 32768)) ends in \ud83d.
    let prefix = "x".repeat(32767);
    let tool = json!({"kind":"tool_result", "entryId":"e", "toolName":"read", "toolCallId":"c", "text":"TRUNCATED", "isError":false,"truncated":true});
    let event = json!({"type":"event","instanceId":"i","sessionId":null,"seq":1,"event":{"type":"message_end","item":tool}});
    let history =
        json!({"type":"response","ok":true,"id":"h","result":{"items":[tool],"session":null}});
    let mut d = LineDecoder::default();
    for (msg, path) in [
        (event, "/event/item/text"),
        (history, "/result/items/0/text"),
    ] {
        let line = encode_line(&msg)
            .unwrap()
            .replace("TRUNCATED", &format!("{prefix}\\ud83d"));
        let mut output = vec![];
        for chunk in line.as_bytes().chunks(1000) {
            d.push(std::str::from_utf8(chunk).unwrap(), |v| {
                output.push(v.unwrap())
            });
        }
        assert_eq!(output.len(), 1);
        let got = &output[0];
        assert_eq!(got.pointer(path).unwrap(), &format!("{prefix}�"));
        roundtrip::<ServerMessage>(got);
        if got["type"] == "response" {
            roundtrip::<Response<ResultOf<method::History>>>(got);
        }
    }
    let edge = r#"{"\udfff":"\uD800\ud83d\ude42\\uD800\uDC01"}"#;
    assert_eq!(decode_json(edge).unwrap(), json!({"�":"�🙂\\uD800�"}));
    for bad in [r#""\ud800\u12xx""#, r#"[\ud800]"#, r#""\ud800"#] {
        assert!(decode_json(bad).is_err());
    }
}

#[test]
fn relay_preserves_future_methods_but_typed_dispatch_is_strict() {
    let req = json!({"id":"r","method":"future_method","params":{"x":[false,null,12.5]},"expectInstanceId":"i","expectSessionId":"s","futureGuard":7});
    roundtrip::<AppMessage>(&json!({"type":"rc","instanceId":"i","request":req}));
    assert!(serde_json::from_value::<RequestOf<method::Hello>>(req).is_err());
}
#[test]
fn mixed_subagent_snapshot_keeps_known_rows_and_actions() {
    let f = fixtures();
    let mut snapshot = f["host"][0].clone();
    let mut run = f["server"][2]["event"]["run"].clone();
    run["actions"] = json!(["future_op", "stop"]);
    run["children"][0]["actions"] = json!(["steer", "future_op", "resume"]);
    snapshot["snapshot"]["subagents"] = json!({"backends":[{"backend":"future_backend","supported":false},{"backend":"example","supported":true}],"runs":[{"backend":"future_backend","opaque":true},run]});
    let got =
        serde_json::to_value(serde_json::from_value::<HostMessage>(snapshot.clone()).unwrap())
            .unwrap();
    let state = &got["snapshot"]["subagents"];
    assert_eq!(
        state["backends"],
        json!([{"backend":"example","supported":true}])
    );
    run["actions"] = json!(["stop"]);
    run["children"][0]["actions"] = json!(["steer", "resume"]);
    assert_eq!(state["runs"], json!([run]));
    snapshot["snapshot"]["subagents"]["runs"][0]["backend"] = json!("example");
    assert!(serde_json::from_value::<HostMessage>(snapshot).is_err());
    assert!(
        serde_json::from_value::<SubagentControlParams>(json!({"runId":"r","action":"future_op"}))
            .is_err()
    );
}
#[test]
fn echo_preserves_json_number() {
    for t in [json!(-1), json!(123.5), json!(9007199254740991u64)] {
        let AppMessage::Ping { t: received } =
            serde_json::from_value(json!({"type":"ping","t":t})).unwrap()
        else {
            panic!("not ping")
        };
        let pong = json!({"type":"pong","t":t});
        assert_eq!(
            serde_json::to_value(HostMessage::Pong { t: received }).unwrap(),
            pong
        );
        roundtrip::<HostMessage>(&pong);
    }
}
#[test]
fn fractional_rpc_timeout() {
    roundtrip::<RequestOf<method::SubagentsRpc>>(
        &json!({"id":"r","method":"subagents_rpc","params":{"method":"status","timeoutMs":12.5}}),
    );
}
#[test]
fn future_report_values_do_not_expand_command_vocabulary() {
    let mut s = fixtures()["host"][0]["snapshot"].clone();
    s["origin"] = json!("desktop");
    s["thinking"] = json!("future");
    s["quotas"][0]["source"] = json!("future_provider");
    roundtrip::<InstanceSnapshot>(&s);
    let mut r = fixtures()["server"][2]["event"]["run"].clone();
    r["startedAt"] = json!(123.5);
    r["usage"] = json!({"total":1.25});
    roundtrip::<SubagentRun>(&r);
    roundtrip::<TranscriptItem>(&json!({"kind":"user","entryId":"e","text":"x","timestamp":-1.25}));
    let mut p = json!({"id":"future_provider","methods":["oauth","future_method"],"stored":"future_method","configured":true});
    let decoded: AuthProviderInfo = serde_json::from_value(p.clone()).unwrap();
    p["methods"] = json!(["oauth"]);
    assert_eq!(serde_json::to_value(decoded).unwrap(), p);
    assert!(
        serde_json::from_value::<AuthBeginParams>(
            json!({"provider":"future_provider","method":"oauth"})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<SetThinkingParams>(json!({"level":"future"})).is_err());
}
