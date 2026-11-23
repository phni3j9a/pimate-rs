//! PiMate v1 JSON wire types. For Host forwarding use the original JSON and
//! [`relay::HostFrame`], not the endpoint types: those normalize unknown data.
//! Validate privileged requests on their original JSON before deserializing.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};
use ts_rs::TS;
mod auth;
mod domain;
mod helpers;
mod host;
mod images;
pub mod relay;
mod rpc;
mod subagents;
pub use {auth::*, domain::*, helpers::*, host::*, images::*, rpc::*, subagents::*};

// Missing fields use Default::default(); a present value must deserialize as T.
// For Option<Option<T>> this keeps absent, explicit null and T distinct.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// Open only on receive: a new string value must not invalidate the rest of a report.
/// The generated v1 TS types and typed outbound requests still use the known vocabulary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum Received<T> {
    Known(T),
    #[ts(skip)]
    Unknown(String),
}

fn known_values<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(Vec::<Received<T>>::deserialize(d)?
        .into_iter()
        .filter_map(|v| match v {
            Received::Known(x) => Some(x),
            Received::Unknown(_) => None,
        })
        .collect())
}
