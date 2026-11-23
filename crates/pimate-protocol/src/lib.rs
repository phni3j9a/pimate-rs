//! PiMate v1 JSON wire types. Validate privileged requests on the original JSON
//! before deserializing: unknown object fields are deliberately ignored here.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use ts_rs::TS;
mod auth;
mod domain;
mod helpers;
mod host;
mod images;
mod rpc;
mod subagents;
pub use {auth::*, domain::*, helpers::*, host::*, images::*, rpc::*, subagents::*};

// Missing fields use Default::default(); a present value must deserialize as T.
// For Option<Option<T>> this keeps absent, explicit null and T distinct.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
