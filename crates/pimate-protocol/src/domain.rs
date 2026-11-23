use crate::*;

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum InstanceStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "waiting_input")]
    WaitingInput,
}

/// Where the Pi process was started: terminal, phone, or Host CLI (including Grok Bot).
#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum Origin {
    #[serde(rename = "pc")]
    Pc,
    #[serde(rename = "mobile")]
    Mobile,
    #[serde(rename = "grok-bot")]
    GrokBot,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum ThinkingLevel {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "minimal")]
    Minimal,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "xhigh")]
    Xhigh,
    #[serde(rename = "max")]
    Max,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SessionRef {
    pub id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub file: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ModelRef {
    pub provider: String,
    pub id: String,
}

/// Pi's global default for new Pis (`defaultProvider`/`defaultModel`/`defaultThinkingLevel`, as Pi's /model "set as default").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ModelDefault {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub model: Option<ModelRef>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub thinking: Option<Received<ThinkingLevel>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: Number,
    pub output: Number,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub cache_read: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub cache_write: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub cost: Option<Number>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ToolCallRef {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    #[ts(type = "unknown")]
    pub args: Value,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PimateWriteDetailsPimateKind {
    #[serde(rename = "write_diff")]
    WriteDiff,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PimateWriteDetailsPimateOmitted {
    #[serde(rename = "too_large")]
    TooLarge,
    #[serde(rename = "binary")]
    Binary,
    #[serde(rename = "unreadable")]
    Unreadable,
    #[serde(rename = "concurrent")]
    Concurrent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PimateWriteDetailsPimate {
    pub kind: PimateWriteDetailsPimateKind,
    pub existed: bool,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub patch: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub omitted: Option<PimateWriteDetailsPimateOmitted>,
}

/// Display-only diff the RC adds to a successful built-in `write` result (`details.pimate`), because Pi's write
/// tool records no details. `patch` is a standard unified diff against the content before the write
/// (`existed: false` = new file). Omitted when the file was too large or not text, or when another write/edit to
/// the same file ran in the same parallel tool batch so the prior content is not exact (`omitted` says why).
/// The edit tool already provides `details.patch` (unified diff) itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PimateWriteDetails {
    pub pimate: PimateWriteDetailsPimate,
}

/// Display-only previews. Omitted/unsupported images remain counted for on-demand retrieval.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptImages {
    /// JPEG/PNG/WebP previews, each <= 512 KiB decoded; not subject to the prompt total/count limit.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub images: Option<Vec<PromptImage>>,
    /// Original zero-based image index for each preview; absent on old RCs means array position.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub image_indices: Option<Vec<u64>>,
    /// Total original images, including omitted previews and GIFs.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub image_count: Option<u64>,
}

/// One entry of the current session branch, normalized for display.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
pub enum TranscriptItem {
    #[serde(rename = "user")]
    User {
        #[serde(rename = "entryId")]
        entry_id: String,
        text: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        timestamp: Option<Number>,
        #[serde(flatten)]
        images: TranscriptImages,
    },
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(rename = "entryId")]
        entry_id: String,
        text: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        thinking: Option<String>,
        #[serde(rename = "toolCalls")]
        tool_calls: Vec<ToolCallRef>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        model: Option<ModelRef>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        usage: Option<Usage>,
        #[serde(
            rename = "stopReason",
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        stop_reason: Option<String>,
        #[serde(
            rename = "errorMessage",
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        error_message: Option<String>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        timestamp: Option<Number>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(rename = "entryId")]
        entry_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        text: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        truncated: Option<bool>,
        #[serde(rename = "isError")]
        is_error: bool,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional, type = "unknown")]
        details: Option<Value>,
        #[serde(
            rename = "detailsOmitted",
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        details_omitted: Option<bool>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        timestamp: Option<Number>,
        #[serde(flatten)]
        images: TranscriptImages,
    },
    #[serde(rename = "other")]
    Other {
        #[serde(rename = "entryId")]
        entry_id: String,
        label: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        text: Option<String>,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        timestamp: Option<Number>,
    },
    #[serde(other, skip_serializing)]
    #[ts(skip)]
    Unknown,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum UIRequestKind {
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "confirm")]
    Confirm,
    #[serde(rename = "input")]
    Input,
    #[serde(rename = "editor")]
    Editor,
    #[serde(rename = "questionnaire")]
    Questionnaire,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct UIQuestionOptionsItem {
    pub label: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub description: Option<String>,
}

/// One question of a `questionnaire` request (PiMate's own `pimate_ask` tool).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UIQuestion {
    pub question: String,
    pub options: Vec<UIQuestionOptionsItem>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub multi_select: Option<bool>,
}

/// Answer to one UIQuestion: chosen option labels (at most one unless multiSelect) and/or the user's own text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct UIQuestionAnswer {
    pub selected: Vec<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub custom: Option<String>,
}

/// A dialog opened by Pi or any extension through ctx.ui, answerable from the phone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UIRequest {
    pub request_id: String,
    pub kind: Received<UIRequestKind>,
    pub title: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub message: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub options: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub placeholder: Option<String>,
    /// editor: the TUI editor stays open after a remote answer (Pi's editor() has no AbortSignal).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub tui_stays_open: Option<bool>,
    /// questionnaire: all questions, answered together (`message` is the optional context).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub questions: Option<Vec<UIQuestion>>,
}

/// Context window usage (ctx.getContextUsage()). `tokens`/`percent` are null right after compaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextInfo {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub tokens: Option<u64>,
    pub context_window: u64,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub percent: Option<Number>,
}

/// One subscription allowance window, read passively from an ordinary model response (never polled).
/// Codex keeps serving on credits at 100%, so `usedPercent` is not an availability verdict.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// Window length: 300 = 5 hours, 10080 = 7 days.
    pub minutes: Number,
    /// 0-100.
    pub used_percent: Number,
    /// Epoch ms; absent when the provider did not report it.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub resets_at: Option<Number>,
}

/// `codex`: ChatGPT/Codex OAuth (`openai-codex`). `claude`: Claude Pro/Max OAuth (`anthropic`).
#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum QuotaSource {
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "claude")]
    Claude,
}

/// Latest allowance this Pi saw for one subscription. Other Pi processes or devices may have used more since.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionQuota {
    pub source: Received<QuotaSource>,
    /// Shortest window first.
    pub windows: Vec<QuotaWindow>,
    /// RC clock (epoch ms) when the response carrying it arrived.
    pub observed_at: u64,
}

/// A TUI-only prompt (ctx.ui.custom() or Pi's own selectors) the phone cannot answer: "needs the PC".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PcPrompt {
    pub kind: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub title: Option<String>,
}

/// One selectable model (ctx.modelRegistry, models with configured auth only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub id: String,
    pub name: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub context_window: Option<u64>,
    /// Thinking levels this model accepts; ["off"] for non-reasoning models.
    #[serde(deserialize_with = "known_values")]
    pub thinking_levels: Vec<ThinkingLevel>,
}

/// A saved session of the instance's cwd (SessionManager.list).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionListItem {
    pub id: String,
    /// Session file; pass as `session_op` `switch` arg.
    pub path: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub name: Option<String>,
    pub first_message: String,
    pub message_count: u64,
    pub created: i64,
    pub modified: i64,
    /// The instance's current session.
    pub current: bool,
}

/// Bounded, display-only session tree. parentId skips hidden tool/state entries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeItem {
    pub id: String,
    #[serde(rename = "parentId", deserialize_with = "Deserialize::deserialize")]
    pub parent_id: Option<String>,
    pub kind: String,
    pub preview: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub label: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub timestamp: Option<Number>,
    pub on_branch: bool,
    pub is_current: bool,
    pub is_leaf: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SessionTree {
    pub nodes: Vec<SessionTreeItem>,
    #[serde(rename = "leafId", deserialize_with = "Deserialize::deserialize")]
    pub leaf_id: Option<String>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct InstanceSnapshotStreaming {
    pub text: String,
    pub thinking: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSnapshot {
    /// PiMate package version; absent on pre-M5 RCs.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub version: Option<String>,
    pub protocol: u64,
    pub instance_id: String,
    pub pid: u64,
    pub cwd: String,
    pub origin: Received<Origin>,
    /// abduco session name when started by the Host; null for a Pi started directly in a terminal.
    #[serde(
        rename = "abducoSession",
        deserialize_with = "Deserialize::deserialize"
    )]
    pub abduco_session: Option<String>,
    pub started_at: u64,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub session: Option<SessionRef>,
    pub status: Received<InstanceStatus>,
    /// A Pi run is between agent_start and agent_settled, including extension dialogs after agent_end.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub agent_active: Option<bool>,
    /// Explicit capability: older RCs ignore unknown prompt fields. Never send images without this.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub prompt_images: Option<bool>,
    /// Maximum prompt image count; missing means 1 when promptImages is true. Shared byte budgets still apply.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub prompt_image_limit: Option<u64>,
    /// On-demand current-session user/toolResult image retrieval via entry_image.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub entry_images: Option<bool>,
    /// Local file_image retrieval; relative paths use session cwd, absolute paths are allowed.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub file_images: Option<bool>,
    /// Session tree read, navigation and labels. Absent on older RCs.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub session_tree: Option<bool>,
    /// RC command discovery, expansion and explicit command acknowledgements.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub slash_commands: Option<bool>,
    /// Auth-specific RPCs for supported providers. Absent = update PC package first.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub provider_auth: Option<bool>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub model: Option<ModelRef>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub thinking: Option<Received<ThinkingLevel>>,
    #[serde(rename = "pendingUI")]
    pub pending_ui: Vec<UIRequest>,
    /// TUI-only prompt currently open on the PC, if any.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub pc_prompt: Option<Option<PcPrompt>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub context: Option<Option<ContextInfo>>,
    /// Last event seq emitted before this snapshot was taken.
    pub seq: u64,
    /// Assistant output streamed so far in the current, not yet finished message (for late subscribers).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub streaming: Option<InstanceSnapshotStreaming>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub pi_version: Option<String>,
    /// Subagent runs of the current session (M4). Absent from older RCs.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub subagents: Option<SubagentsState>,
    /// Subscription allowances seen since this Pi started. Absent from older RCs; empty = none seen yet.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub quotas: Option<Vec<SubscriptionQuota>>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum InstanceMetaPurpose {
    #[serde(rename = "auth")]
    Auth,
}

/// Discovery file `<runDir>/<instanceId>.json` written by RC (0600). The socket is `<runDir>/<instanceId>.sock`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstanceMeta {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub version: Option<String>,
    pub protocol: u64,
    pub instance_id: String,
    pub pid: u64,
    pub cwd: String,
    pub origin: Received<Origin>,
    #[serde(
        rename = "abducoSession",
        deserialize_with = "Deserialize::deserialize"
    )]
    pub abduco_session: Option<String>,
    pub started_at: u64,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub session: Option<SessionRef>,
    pub status: Received<InstanceStatus>,
    pub updated_at: u64,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub subagents: Option<SubagentCounts>,
    /// Host-owned login Pi (`auth_session`): never listed to apps / `pimate list`; Host forwards only hello and auth_*.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub purpose: Option<Received<InstanceMetaPurpose>>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum InstanceEventSessionChangedReason {
    #[serde(rename = "startup")]
    Startup,
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "new")]
    New,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "fork")]
    Fork,
    #[serde(rename = "tree")]
    Tree,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum InstanceEventUiRequestEndBy {
    #[serde(rename = "tui")]
    Tui,
    #[serde(rename = "remote")]
    Remote,
    #[serde(rename = "error")]
    Error,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum InstanceEventUiPromptPhase {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "end")]
    End,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum InstanceEventNotifyLevel {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

// The private serde mirror decodes known events; no public deserialization path
// can bypass the backend check. Both enums share the same wire definition.
macro_rules! instance_events {
    ($($variants:tt)*) => {
        #[derive(Clone, Debug, PartialEq, Serialize, TS)]
        #[serde(tag = "type")]
        pub enum InstanceEvent { $($variants)* }

        impl<'de> Deserialize<'de> for InstanceEvent {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(Deserialize, TS)]
                #[serde(tag = "type", remote = "InstanceEvent")]
                enum WireEvent { $($variants)* }

                let event = Value::deserialize(d)?;
                if event.get("type").and_then(Value::as_str) == Some("subagent_run")
                    && event.get("run").is_some_and(subagents::unknown_backend) {
                    return Ok(Self::Unknown);
                }
                WireEvent::deserialize(event).map_err(serde::de::Error::custom)
            }
        }
    };
}
instance_events! {
    #[serde(rename = "status")]
    Status { status: Received<InstanceStatus> },
    #[serde(rename = "session_changed")]
    SessionChanged {
        reason: Received<InstanceEventSessionChangedReason>,
        session: SessionRef,
    },
    #[serde(rename = "session_info")]
    SessionInfo {
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        name: Option<String>,
    },
    #[serde(rename = "agent_start")]
    AgentStart,
    #[serde(rename = "agent_settled")]
    AgentSettled { aborted: bool },
    #[serde(rename = "user_input")]
    UserInput {
        text: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        source: Option<String>,
    },
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { delta: String },
    #[serde(rename = "message_end")]
    MessageEnd { item: TranscriptItem },
    #[serde(rename = "tool_start")]
    ToolStart {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(deserialize_with = "Deserialize::deserialize")]
        #[ts(type = "unknown")]
        args: Value,
        #[serde(
            rename = "parentToolCallId",
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        parent_tool_call_id: Option<String>,
    },
    #[serde(rename = "tool_update")]
    ToolUpdate {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(deserialize_with = "Deserialize::deserialize")]
        #[ts(type = "unknown")]
        partial: Value,
    },
    #[serde(rename = "tool_end")]
    ToolEnd {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "isError")]
        is_error: bool,
        #[serde(
            rename = "durationMs",
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        duration_ms: Option<Number>,
    },
    #[serde(rename = "model")]
    Model { model: ModelRef },
    #[serde(rename = "thinking")]
    Thinking { level: Received<ThinkingLevel> },
    #[serde(rename = "ui_request")]
    UiRequest { request: UIRequest },
    #[serde(rename = "ui_request_end")]
    UiRequestEnd {
        #[serde(rename = "requestId")]
        request_id: String,
        by: Received<InstanceEventUiRequestEndBy>,
    },
    #[serde(rename = "ui_prompt")]
    UiPrompt {
        phase: Received<InstanceEventUiPromptPhase>,
        kind: String,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        title: Option<String>,
    },
    #[serde(rename = "notify")]
    Notify {
        message: String,
        level: Received<InstanceEventNotifyLevel>,
    },
    #[serde(rename = "context")]
    Context {
        #[serde(deserialize_with = "Deserialize::deserialize")]
        context: Option<ContextInfo>,
    },
    #[serde(rename = "session_op_done")]
    SessionOpDone {
        op: Received<SessionOp>,
        cancelled: bool,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        error: Option<String>,
        #[serde(
            rename = "editorText",
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        editor_text: Option<String>,
    },
    #[serde(rename = "compact_done")]
    CompactDone,
    #[serde(rename = "compact_error")]
    CompactError { error: String },
    #[serde(rename = "quota")]
    Quota { quota: SubscriptionQuota },
    #[serde(rename = "subagent_run")]
    SubagentRun { run: SubagentRun },
    #[serde(rename = "subagents_reset")]
    SubagentsReset { state: SubagentsState },
    #[serde(rename = "bus")]
    Bus {
        channel: String,
        #[serde(deserialize_with = "Deserialize::deserialize")]
        #[ts(type = "unknown")]
        data: Value,
    },
    #[serde(other, skip_serializing)]
    #[ts(skip)]
    Unknown,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum InstanceEventType {
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "session_changed")]
    SessionChanged,
    #[serde(rename = "session_info")]
    SessionInfo,
    #[serde(rename = "agent_start")]
    AgentStart,
    #[serde(rename = "agent_settled")]
    AgentSettled,
    #[serde(rename = "user_input")]
    UserInput,
    #[serde(rename = "text_delta")]
    TextDelta,
    #[serde(rename = "thinking_delta")]
    ThinkingDelta,
    #[serde(rename = "message_end")]
    MessageEnd,
    #[serde(rename = "tool_start")]
    ToolStart,
    #[serde(rename = "tool_update")]
    ToolUpdate,
    #[serde(rename = "tool_end")]
    ToolEnd,
    #[serde(rename = "model")]
    Model,
    #[serde(rename = "thinking")]
    Thinking,
    #[serde(rename = "ui_request")]
    UiRequest,
    #[serde(rename = "ui_request_end")]
    UiRequestEnd,
    #[serde(rename = "ui_prompt")]
    UiPrompt,
    #[serde(rename = "notify")]
    Notify,
    #[serde(rename = "context")]
    Context,
    #[serde(rename = "session_op_done")]
    SessionOpDone,
    #[serde(rename = "compact_done")]
    CompactDone,
    #[serde(rename = "compact_error")]
    CompactError,
    #[serde(rename = "quota")]
    Quota,
    #[serde(rename = "subagent_run")]
    SubagentRun,
    #[serde(rename = "subagents_reset")]
    SubagentsReset,
    #[serde(rename = "bus")]
    Bus,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum SessionOp {
    #[serde(rename = "new")]
    New,
    #[serde(rename = "switch")]
    Switch,
    #[serde(rename = "fork")]
    Fork,
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "tree")]
    Tree,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CommandInfo {
    pub name: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub description: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HelloResult {
    pub snapshot: InstanceSnapshot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HistoryResult {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub session: Option<SessionRef>,
    pub items: Vec<TranscriptItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EntryImageParams {
    pub entry_id: String,
    pub index: u64,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub thumbnail: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EntryImageResult {
    pub image: ViewImage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct FileImageParams {
    pub path: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub thumbnail: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct FileImageResult {
    pub image: ViewImage,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PromptParamsDeliverAs {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PromptParams {
    pub text: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub images: Option<Vec<PromptImage>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub deliver_as: Option<PromptParamsDeliverAs>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub expand_templates: Option<bool>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PromptResultDeliveredAs {
    #[serde(rename = "prompt")]
    Prompt,
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
    #[serde(rename = "command")]
    Command,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PromptResult {
    pub delivered_as: Received<PromptResultDeliveredAs>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UiRespondParams {
    pub request_id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub value: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub confirmed: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub cancelled: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub answers: Option<Vec<UIQuestionAnswer>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetThinkingParams {
    pub level: ThinkingLevel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetThinkingResult {
    pub thinking: Received<ThinkingLevel>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetModelResult {
    pub model: ModelRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CompactParams {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub instructions: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SessionOpParams {
    pub op: SessionOp,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub arg: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub summarize: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetLabelParams {
    pub entry_id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CommandsResult {
    pub commands: Vec<CommandInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ModelsResult {
    pub models: Vec<ModelInfo>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub current: Option<ModelRef>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub scoped: Option<Vec<ModelRef>>,
    pub default: ModelDefault,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetDefaultModelParams {
    pub model: ModelRef,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub thinking: Option<ThinkingLevel>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetDefaultModelResult {
    pub default: ModelDefault,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetScopedModelsParams {
    pub models: Vec<ModelRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetScopedModelsResult {
    pub scoped: Vec<ModelRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SessionsParams {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub limit: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SessionsResult {
    pub sessions: Vec<SessionListItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentTranscriptParams {
    pub run_id: String,
    pub child_id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentControlParams {
    pub run_id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub child_id: Option<String>,
    pub action: SubagentAction,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentControlResult {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub message: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub new_run_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentsRpcParams {
    pub method: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "unknown")]
    pub params: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub timeout_ms: Option<Number>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SubagentsRpcResult {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    #[ts(type = "unknown")]
    pub reply: Value,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum ErrorCode {
    #[serde(rename = "bad_request")]
    BadRequest,
    #[serde(rename = "unknown_method")]
    UnknownMethod,
    #[serde(rename = "not_ready")]
    NotReady,
    #[serde(rename = "busy")]
    Busy,
    #[serde(rename = "instance_mismatch")]
    InstanceMismatch,
    #[serde(rename = "session_changed")]
    SessionChanged,
    #[serde(rename = "no_such_request")]
    NoSuchRequest,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "timeout")]
    Timeout,
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "internal")]
    Internal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ProtocolError {
    pub code: Received<ErrorCode>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub message: Option<String>,
    /// e.g. `session_changed` carries the current session.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "unknown")]
    pub data: Option<Value>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum EventMessageType {
    #[serde(rename = "event")]
    Event,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EventMessage {
    pub r#type: EventMessageType,
    pub instance_id: String,
    #[serde(rename = "sessionId", deserialize_with = "Deserialize::deserialize")]
    pub session_id: Option<String>,
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timestamp: Option<u64>,
    pub event: InstanceEvent,
}
