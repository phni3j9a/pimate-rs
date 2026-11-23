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
