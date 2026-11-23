use pimate_protocol::*;
use serde_json::{Value, json};
#[test]
fn leading_slash_commands() {
    for name in [
        "model",
        "name_2",
        "skill:foo.bar",
        "review:1",
        "release-notes",
    ] {
        for tail in ["", " arg", "\targ", "\narg", "\u{feff}arg"] {
            assert_eq!(slash_command_name(&format!("/{name}{tail}")), Some(name))
        }
    }
    for s in [
        "/home/x",
        "/home/x arg",
        " /model",
        "hi /model",
        "/",
        "/x?",
        "/日本語",
        "",
    ] {
        assert_eq!(slash_command_name(s), None)
    }
}
#[test]
fn pairing() {
    for addr in [
        "100.90.41.53:47800",
        "[fd7a:115c:a1e0::1]:47800",
        "server-macmini.tail1234.ts.net:47800",
    ] {
        let link = PairingLink {
            address: addr.into(),
            code: "AbCdEfGhIjKlMnOpQrStUv".into(),
            host_name: "server Mac&mini🙂".into(),
            host_id: "h1".into(),
        };
        assert_eq!(parse_pairing_link(&format_pairing_link(&link)), Some(link))
    }
    for s in [
        "https://example.com",
        "pimate://pair?h=x:1&c=short&id=h",
        "pimate://pair?h=bad host:1&c=AbCdEfGhIjKlMnOpQrStUv&id=h",
        "pimate://pair?c=AbCdEfGhIjKlMnOpQrStUv&id=h",
    ] {
        assert_eq!(parse_pairing_link(s), None)
    }
    let s = "pimate://pair?h=x:1&c=AbCdEfGhIjKlMnOpQrStUv&id=h";
    assert_eq!(
        parse_pairing_link(&format!("{s}&n=%ZZ")).unwrap().host_name,
        "x:1"
    );
    assert_eq!(
        parse_pairing_link(&format!("{s}&n=a+b")).unwrap().host_name,
        "a b"
    );
}
#[test]
fn ndjson_chunks_errors_and_recovery() {
    let mut d = LineDecoder::default();
    let mut parsed = vec![];
    let mut bad = vec![];
    let mut rx = |r: Result<Value, &str>| match r {
        Ok(v) => parsed.push(v),
        Err(s) => bad.push(s.to_owned()),
    };
    let s = format!(
        "{}\n{{oops\n{}",
        encode_line(&json!({"a":1})).unwrap(),
        encode_line(&json!({"b":"x\ny"})).unwrap()
    );
    d.push(&s[..3], &mut rx);
    d.push(&s[3..], &mut rx);
    assert_eq!(parsed, [json!({"a":1}), json!({"b":"x\ny"})]);
    assert_eq!(bad, ["{oops"]);
    let mut d = LineDecoder::new(2);
    d.push("🙂", |_| panic!());
    d.push("x", |r| assert_eq!(r.unwrap_err(), "🙂x"));
    d.push("2\n", |r| assert_eq!(r.unwrap(), 2));
    d.push(&"🙂".repeat(150), |r| {
        assert_eq!(r.unwrap_err().encode_utf16().count(), 200)
    });
}
fn image(data: &str) -> Value {
    json!({"type":"image","mimeType":"image/jpeg","data":data})
}
#[test]
fn image_shape_and_content() {
    let i = image("YQ==");
    for mime in ["image/jpeg", "image/png", "image/webp"] {
        let mut v = i.clone();
        v["mimeType"] = mime.into();
        assert!(valid_prompt_images(&json!([v])))
    }
    assert!(valid_prompt_images(&json!([])));
    assert!(same_prompt_images(&[], &[]));
    let a: PromptImage = serde_json::from_value(i.clone()).unwrap();
    let b = PromptImage {
        data: "Yg==".into(),
        ..a.clone()
    };
    assert!(same_prompt_images(
        std::slice::from_ref(&a),
        std::slice::from_ref(&a)
    ));
    assert!(!same_prompt_images(&[a], &[b]));
    for value in [
        Value::Null,
        json!({}),
        json!([null]),
        Value::Array(vec![i.clone(); 5]),
        json!([{ "type":"text","mimeType":"image/jpeg","data":"YQ=="}]),
        json!([{ "type":"image","mimeType":"image/gif","data":"YQ=="}]),
    ] {
        assert!(!valid_prompt_images(&value))
    }
    for s in [
        "",
        "a",
        "YWJ",
        "YQ=",
        "Y===",
        "====",
        "Y Q==",
        "YQ==\n",
        "data:image/jpeg;base64,YQ==",
        "YQ-=",
    ] {
        assert!(!valid_prompt_images(&json!([image(s)])))
    }
}
fn encoded(n: usize) -> Value {
    let pad = (3 - n % 3) % 3;
    image(&("A".repeat(n.div_ceil(3) * 4 - pad) + &"=".repeat(pad)))
}
#[test]
fn image_byte_budgets() {
    assert!(valid_prompt_images(&json!([encoded(
        MAX_PROMPT_IMAGE_BYTES
    )])));
    assert!(!valid_prompt_images(&json!([encoded(
        MAX_PROMPT_IMAGE_BYTES + 1
    )])));
    let mut images = json!([
        encoded(MAX_PROMPT_IMAGE_BYTES),
        encoded(MAX_PROMPT_TOTAL_IMAGE_BYTES - MAX_PROMPT_IMAGE_BYTES - 2),
        encoded(1),
        encoded(1)
    ]);
    assert!(valid_prompt_images(&images));
    let req = json!({"type":"rc","id":"image","instanceId":"i","request":{"id":"image","method":"prompt","params":{"text":"\0".repeat(MAX_IMAGE_PROMPT_TEXT),"images":images}}});
    assert!(serde_json::to_vec(&req).unwrap().len() < 1024 * 1024);
    images[3] = encoded(2);
    assert!(!valid_prompt_images(&images));
}
fn run(states: &[&str], state: &str) -> SubagentRun {
    serde_json::from_value(json!({"id":"r","backend":"pi-subagents","background":true,"mode":"parallel","state":state,"actions":[],"children":states.iter().enumerate().map(|(i,s)|json!({"id":i.to_string(),"index":i,"agent":"a","state":s,"hasTranscript":false,"actions":[]})).collect::<Vec<_>>()})).unwrap()
}
#[test]
fn counts_children_or_run_state() {
    assert_eq!(run(&[], "future_state").state, SubagentState::Unknown);
    let c = count_subagents(&[
        run(&["running", "waiting", "complete"], "running"),
        run(&["paused", "failed"], "running"),
    ]);
    assert_eq!((c.active, c.waiting, c.paused, c.failed), (2, 1, 1, 1));
    let c = count_subagents(&[run(&[], "queued")]);
    assert_eq!((c.active, c.waiting, c.paused, c.failed), (1, 0, 0, 0));
}
