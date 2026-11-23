use crate::*;

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum AuthProviderId {
    #[serde(rename = "openai")]
    Openai,
    #[serde(rename = "openai-codex")]
    OpenaiCodex,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "opencode")]
    Opencode,
    #[serde(rename = "opencode-go")]
    OpencodeGo,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum ProviderAuthType {
    #[serde(rename = "oauth")]
    Oauth,
    #[serde(rename = "api_key")]
    ApiKey,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum AuthProviderInfoSource {
    #[serde(rename = "stored")]
    Stored,
    #[serde(rename = "runtime")]
    Runtime,
    #[serde(rename = "environment")]
    Environment,
    #[serde(rename = "fallback")]
    Fallback,
    #[serde(rename = "models_json_key")]
    ModelsJsonKey,
    #[serde(rename = "models_json_command")]
    ModelsJsonCommand,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthProviderInfo {
    pub id: AuthProviderId,
    /// OpenAI and OpenCode (Zen / Go) support API keys only; OAuth is available for Codex and Anthropic.
    pub methods: Vec<ProviderAuthType>,
    /// Stored credential metadata only; not proof that it is valid.
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub stored: Option<ProviderAuthType>,
    /// Active registry snapshot; environment/models.json/runtime auth can survive logout.
    pub configured: bool,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub source: Option<AuthProviderInfoSource>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum AuthFlowStatus {
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "input")]
    Input,
    #[serde(rename = "saving")]
    Saving,
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "expired")]
    Expired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthFlowAuthorization {
    pub url: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub user_code: Option<String>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum AuthFlowPromptKind {
    #[serde(rename = "api_key")]
    ApiKey,
    #[serde(rename = "code")]
    Code,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthFlowPrompt {
    pub id: String,
    pub kind: AuthFlowPromptKind,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum AuthFlowError {
    #[serde(rename = "login_failed")]
    LoginFailed,
    #[serde(rename = "storage_failed")]
    StorageFailed,
}

/// RPC-only, transient state. Never put auth inputs or this state in app persistence or chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthFlow {
    pub id: String,
    pub provider: AuthProviderId,
    pub method: ProviderAuthType,
    pub revision: u64,
    /// Interactive deadline; an already-started commit/refresh can finish up to 60s later.
    pub expires_at: u64,
    pub status: AuthFlowStatus,
    /// Public authorization URL and optional user-facing device code, never tokens.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub authorization: Option<AuthFlowAuthorization>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub prompt: Option<AuthFlowPrompt>,
    /// Fixed safe messages only. storage_failed may have unknown persistence outcome: inspect metadata, never auto-resend.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub error: Option<AuthFlowError>,
    /// Credential saved but local registry refresh failed. Reopen models/reload Pi; do not resend credentials.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub refresh_required: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthListResult {
    pub providers: Vec<AuthProviderInfo>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub flow: Option<AuthFlow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthBeginParams {
    pub provider: AuthProviderId,
    pub method: ProviderAuthType,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthBeginResult {
    pub flow: AuthFlow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthWaitParams {
    pub flow_id: String,
    pub after_revision: u64,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub wait_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthWaitResult {
    pub flow: AuthFlow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthRespondParams {
    pub flow_id: String,
    pub prompt_id: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthRespondResult {
    pub flow: AuthFlow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthCancelParams {
    pub flow_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthCancelResult {
    pub flow: AuthFlow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AuthLogoutParams {
    pub provider: AuthProviderId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthLogoutResult {
    pub refresh_required: bool,
}
