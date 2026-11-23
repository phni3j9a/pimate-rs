use crate::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Empty {}

// Serde's internally tagged enums support strings, but not JSON boolean tags.
macro_rules! literal_bool {
    ($name:ident, $v:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, TS)]
        #[ts(type = "boolean")]
        pub struct $name;
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bool($v)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                if bool::deserialize(d)? == $v {
                    Ok(Self)
                } else {
                    Err(serde::de::Error::custom(concat!(
                        "expected ",
                        stringify!($v)
                    )))
                }
            }
        }
    };
}
literal_bool!(True, true);
literal_bool!(False, false);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    bound(deserialize = "M: Deserialize<'de>, P: Deserialize<'de>"),
    rename_all = "camelCase"
)]
#[ts(rename_all = "camelCase")]
pub struct WireRequest<M, P> {
    pub id: String,
    pub method: M,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub params: Option<P>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub expect_instance_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub expect_session_id: Option<String>,
}
pub type Request<P = Value> = WireRequest<RcMethod, P>;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum ResponseType {
    #[serde(rename = "response")]
    Response,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum WireResponse<R> {
    Success {
        r#type: ResponseType,
        id: String,
        ok: True,
        result: R,
    },
    Failure {
        r#type: ResponseType,
        #[serde(deserialize_with = "Deserialize::deserialize")]
        id: Option<String>,
        ok: False,
        error: ProtocolError,
    },
}
pub type Response<R = Value> = WireResponse<R>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum ServerMessage {
    #[ts(type = "Response")]
    Response(Response),
    Event(Box<EventMessage>),
    #[ts(skip)]
    Unknown(UnknownServerMessage),
}
// Don't let malformed known envelopes fall through to Unknown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "UnknownTag")]
pub struct UnknownServerMessage {
    #[serde(rename = "type")]
    pub message_type: String,
}
#[derive(Deserialize)]
struct UnknownTag {
    #[serde(rename = "type")]
    tag: String,
}
impl TryFrom<UnknownTag> for UnknownServerMessage {
    type Error = &'static str;
    fn try_from(v: UnknownTag) -> Result<Self, Self::Error> {
        match v.tag.as_str() {
            "event" | "response" => Err("malformed server message"),
            _ => Ok(Self {
                message_type: v.tag,
            }),
        }
    }
}

/// Type-level method table, shared with the extension bindings.
#[derive(TS)]
pub struct Method<P, R> {
    pub params: P,
    pub result: R,
}
pub trait RpcMethod {
    type Params;
    type Result;
}
pub type ParamsOf<M> = <M as RpcMethod>::Params;
pub type ResultOf<M> = <M as RpcMethod>::Result;
/// Typed envelope for clients with a statically known method; use Request for routing.
pub type RequestOf<M> = WireRequest<M, ParamsOf<M>>;

macro_rules! methods {
    (auth { $($auth:ident : $an:ident ($ap:ident, $ar:ident)),+ } rc { $($rc:ident : $rn:ident ($p:ident, $r:ident)),+ }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all="snake_case")]
        pub enum RcMethod { $($an,)+ $($rn,)+ }
        pub mod method {
            use super::*;
            $(one_method!($an, super::$ap, super::$ar);)+
            $(one_method!($rn, super::$p, super::$r);)+
        }
        #[derive(TS)] pub struct AuthMethods { $(pub $auth: Method<$ap, $ar>,)+ }
        #[derive(TS)] pub struct RcMethods { #[ts(flatten)] pub auth: AuthMethods, $(pub $rc: Method<$p, $r>,)+ }
    }
}
macro_rules! one_method {
    ($name:ident, $params:ty, $result:ty) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $name,
        }
        impl RpcMethod for $name {
            type Params = $params;
            type Result = $result;
        }
    };
}
methods! {
auth {
    auth_list: AuthList(Empty, AuthListResult),
    auth_begin: AuthBegin(AuthBeginParams, AuthBeginResult),
    auth_wait: AuthWait(AuthWaitParams, AuthWaitResult),
    auth_respond: AuthRespond(AuthRespondParams, AuthRespondResult),
    auth_cancel: AuthCancel(AuthCancelParams, AuthCancelResult),
    auth_logout: AuthLogout(AuthLogoutParams, AuthLogoutResult)
}
rc {
    hello: Hello(Empty, HelloResult),
    history: History(Empty, HistoryResult),
    entry_image: EntryImage(EntryImageParams, EntryImageResult),
    file_image: FileImage(FileImageParams, FileImageResult),
    prompt: Prompt(PromptParams, PromptResult),
    abort: Abort(Empty, Empty),
    ui_respond: UiRespond(UiRespondParams, Empty),
    set_thinking: SetThinking(SetThinkingParams, SetThinkingResult),
    set_model: SetModel(ModelRef, SetModelResult),
    compact: Compact(CompactParams, Empty),
    session_op: SessionOp(SessionOpParams, Empty),
    session_tree: SessionTree(Empty, SessionTree),
    set_label: SetLabel(SetLabelParams, Empty),
    commands: Commands(Empty, CommandsResult),
    models: Models(Empty, ModelsResult),
    set_default_model: SetDefaultModel(SetDefaultModelParams, SetDefaultModelResult),
    set_scoped_models: SetScopedModels(SetScopedModelsParams, SetScopedModelsResult),
    sessions: Sessions(SessionsParams, SessionsResult),
    shutdown: Shutdown(Empty, Empty),
    subagent_transcript: SubagentTranscript(SubagentTranscriptParams, SubagentTranscript),
    subagent_control: SubagentControl(SubagentControlParams, SubagentControlResult),
    subagents_rpc: SubagentsRpc(SubagentsRpcParams, SubagentsRpcResult)
}
}
