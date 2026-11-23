//! Host-side framing only. Keep the original JSON from `decode_json` / `LineDecoder`
//! for forwarding. Read just the routing or policy fields the Host needs. An RC
//! event is already a Host event: forward the entire original frame unchanged.
//! `HostMessage`, `ServerMessage` and the snapshot/event types are endpoint views,
//! not relay buffers; decoding into them intentionally discards unknown data.
use serde::Serialize;
use serde_json::Value;

/// The two replies that need a Host envelope. Borrow the unmodified RC payloads.
#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HostFrame<'a> {
    Subscribed {
        instance_id: &'a str,
        snapshot: &'a Value,
        history: &'a Value,
    },
    RcResult {
        instance_id: &'a str,
        response: &'a Value,
    },
}
