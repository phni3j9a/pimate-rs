use crate::*;

/// Contents of the pairing QR code / deep link: `pimate://pair?h=<host:port>&c=<code>&n=<hostName>&id=<hostId>`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PairingLink {
    /// `host:port` reachable from the phone (Tailscale IP or MagicDNS name). IPv6 hosts are bracketed.
    pub address: String,
    /// Single-use pairing code (base64url, >=128 bits), valid for a few minutes.
    pub code: String,
    pub host_name: String,
    pub host_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PairRequest {
    pub code: String,
    pub device_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PairResponse {
    pub host_id: String,
    pub host_name: String,
    pub device_id: String,
    /// Long-lived bearer token (base64url, 256 bits). The Host stores only its hash.
    pub token: String,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PairErrorError {
    #[serde(rename = "invalid_code")]
    InvalidCode,
    #[serde(rename = "expired_code")]
    ExpiredCode,
    #[serde(rename = "bad_request")]
    BadRequest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PairError {
    pub error: PairErrorError,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PiAlertKind {
    #[serde(rename = "input")]
    Input,
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "child_waiting")]
    ChildWaiting,
    #[serde(rename = "child_paused")]
    ChildPaused,
    #[serde(rename = "child_failed")]
    ChildFailed,
}

/// Text-free, live-only alert. Never replayed from history or snapshots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PiAlert {
    pub id: String,
    pub instance_id: String,
    pub session_id: String,
    pub kind: PiAlertKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HostInfo {
    /// Only send device requests when true.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub paired_devices: Option<bool>,
    /// True only when a managed Host can be restarted.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub host_restart: Option<bool>,
    /// Only send github_status when true.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub github_status: Option<bool>,
    /// Only send update_check / update when true; older Hosts close on unknown messages.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub package_update: Option<bool>,
    /// Read-only GitHub queries via the PC's gh login; never send to older Hosts.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub github: Option<bool>,
    /// Optional capability: only send watch_alerts when true.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub alerts: Option<bool>,
    /// Auth RPC timeout/long-poll support; RC providerAuth capability is also required.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub provider_auth: Option<bool>,
    /// Optional capability: only send `auth_session` / `auth_session_end` when true (older Hosts close on unknown messages).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub auth_session: Option<bool>,
    /// Optional capability: only send `dirs` (folder completion) when true; older Hosts close on unknown messages.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub dirs: Option<bool>,
    pub host_id: String,
    pub name: String,
    pub protocol: u64,
    pub version: String,
    pub platform: String,
}

/// One Pi instance as shown in the drawer. Derived from discovery meta (+ abduco attach state).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub instance_id: String,
    pub cwd: String,
    pub origin: Origin,
    #[serde(
        rename = "abducoSession",
        deserialize_with = "Deserialize::deserialize"
    )]
    pub abduco_session: Option<String>,
    /// Whether a PC terminal is attached to the abduco session; null when not started under abduco.
    #[serde(rename = "pcAttached", deserialize_with = "Deserialize::deserialize")]
    pub pc_attached: Option<bool>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub session: Option<SessionRef>,
    pub status: InstanceStatus,
    pub started_at: u64,
    pub updated_at: u64,
    /// Subagent counts (M4); absent when the RC does not report them.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub subagents: Option<SubagentCounts>,
}

/// A folder offered on the "new Pi" screen (from Pi's saved sessions and running instances).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    /// Absolute path.
    pub path: String,
    /// Last time a Pi session was used there (ms), if known.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub last_used: Option<u64>,
    /// Number of running instances in this folder.
    pub running: u64,
    /// Saved project-trust decision (~/.pi/agent/trust.json), if any.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub trusted: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SpawnParamsModel {
    pub provider: String,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpawnParams {
    /// Absolute path or `~/...`; must be an existing directory.
    pub cwd: String,
    /// Project-trust decision for this start only (handled by RC's `project_trust` handler so the TUI prompt
    /// never blocks a phone-started Pi). Omitted = decline.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub trust: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub model: Option<SpawnParamsModel>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub thinking: Option<ThinkingLevel>,
    /// PiMate's `pimate_ask` tool: true adds it (`--tools +pimate_ask`), false removes it, omitted = Pi settings.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub ask_tool: Option<bool>,
    /// First prompt, sent once the Pi is ready.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub message: Option<String>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum SpawnErrorCode {
    #[serde(rename = "bad_request")]
    BadRequest,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "not_directory")]
    NotDirectory,
    #[serde(rename = "spawn_failed")]
    SpawnFailed,
    #[serde(rename = "unsupported")]
    Unsupported,
}

/// Only the selected Host-known instance repository; no paths, repos or CLI arguments from the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
pub enum GitHubQuery {
    #[serde(rename = "list")]
    List,
    #[serde(rename = "issue")]
    Issue { number: u64 },
    #[serde(rename = "pr")]
    Pr { number: u64 },
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum GitHubCheckState {
    #[serde(rename = "passed")]
    Passed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "pending")]
    Pending,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct GitHubCheck {
    pub name: String,
    pub state: GitHubCheckState,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum GitHubSummaryKind {
    #[serde(rename = "issue")]
    Issue,
    #[serde(rename = "pr")]
    Pr,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSummary {
    pub kind: GitHubSummaryKind,
    pub number: u64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub url: String,
    pub updated_at: String,
    pub labels: Vec<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub is_draft: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub review_decision: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub ci: Option<GitHubCi>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitHubDetailCommentsItem {
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub body_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitHubDetail {
    #[serde(flatten)]
    pub summary: GitHubSummary,
    pub body: String,
    pub body_truncated: bool,
    pub created_at: String,
    pub comments: Vec<GitHubDetailCommentsItem>,
    pub comments_truncated: bool,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub head_ref_name: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub base_ref_name: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub additions: Option<u64>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub deletions: Option<u64>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub changed_files: Option<u64>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub checks: Option<Vec<GitHubCheck>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub checks_truncated: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct GitHubDataListRepo {
    pub name: String,
    pub url: String,
}

/// Lists are open only, at most 30 each. Details include at most 30 comments; truncation is explicit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind")]
pub enum GitHubData {
    #[serde(rename = "list")]
    List {
        repo: GitHubDataListRepo,
        issues: Vec<GitHubSummary>,
        prs: Vec<GitHubSummary>,
    },
    #[serde(rename = "detail")]
    Detail { item: Box<GitHubDetail> },
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum GitHubErrorCode {
    #[serde(rename = "gh_missing")]
    GhMissing,
    #[serde(rename = "gh_auth")]
    GhAuth,
    #[serde(rename = "not_github")]
    NotGithub,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum GitHubResult {
    Success {
        ok: True,
        data: GitHubData,
    },
    Failure {
        ok: False,
        code: GitHubErrorCode,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub device_id: String,
    pub name: String,
    pub created_at: u64,
    pub last_seen_at: u64,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitHubStatus {
    pub installed: bool,
    pub logged_in: bool,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub login: Option<String>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum HostOperationResultFalseCode {
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum HostOperationResult {
    Success {
        ok: True,
    },
    Failure {
        ok: False,
        code: HostOperationResultFalseCode,
    },
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum GitHubStatusResultFalseCode {
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum GitHubStatusResult {
    Success {
        ok: True,
        status: GitHubStatus,
    },
    Failure {
        ok: False,
        code: GitHubStatusResultFalseCode,
    },
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PackageUpdateCheckReason {
    #[serde(rename = "not_service")]
    NotService,
    #[serde(rename = "not_package")]
    NotPackage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PackageUpdateCheck {
    pub current: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub latest: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub error: Option<String>,
    pub can_update: bool,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub reason: Option<PackageUpdateCheckReason>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum PackageUpdateResultFalseCode {
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "busy")]
    Busy,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum PackageUpdateResult {
    Success {
        ok: True,
        from: String,
        to: String,
        restarting: bool,
    },
    Failure {
        ok: False,
        code: PackageUpdateResultFalseCode,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        message: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AppMessageAuthApp {
    pub version: String,
    pub platform: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum AppMessage {
    #[serde(rename = "auth")]
    Auth {
        token: String,
        protocol: u64,
        app: AppMessageAuthApp,
    },
    #[serde(rename = "watch_alerts")]
    WatchAlerts { enabled: bool },
    #[serde(rename = "subscribe")]
    Subscribe {
        #[serde(rename = "instanceId")]
        instance_id: String,
    },
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        #[serde(rename = "instanceId")]
        instance_id: String,
    },
    #[serde(rename = "rc")]
    Rc {
        #[serde(rename = "instanceId")]
        instance_id: String,
        #[ts(type = "Request")]
        request: Request,
    },
    #[serde(rename = "ping")]
    Ping { t: u64 },
    #[serde(rename = "folders")]
    Folders {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "update_check")]
    UpdateCheck {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "update")]
    Update {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "devices")]
    Devices {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "host_restart")]
    HostRestart {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "github_status")]
    GithubStatus {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    #[serde(rename = "device_revoke")]
    DeviceRevoke {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    #[serde(rename = "github")]
    Github {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "instanceId")]
        instance_id: String,
        query: GitHubQuery,
    },
    #[serde(rename = "dirs")]
    Dirs {
        #[serde(rename = "requestId")]
        request_id: String,
        path: String,
    },
    #[serde(rename = "spawn")]
    Spawn {
        #[serde(rename = "requestId")]
        request_id: String,
        params: SpawnParams,
    },
    #[serde(rename = "auth_session")]
    AuthSession {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "holdId")]
        hold_id: String,
    },
    #[serde(rename = "auth_session_end")]
    AuthSessionEnd {
        #[serde(rename = "holdId")]
        hold_id: String,
    },
    #[serde(other, skip_serializing)]
    #[ts(skip)]
    Unknown,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum HostMessageAuthErrorCode {
    #[serde(rename = "invalid_token")]
    InvalidToken,
    #[serde(rename = "protocol_mismatch")]
    ProtocolMismatch,
    #[serde(rename = "bad_request")]
    BadRequest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HostMessageSubscribedHistory {
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub session: Option<SessionRef>,
    pub items: Vec<TranscriptItem>,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum HostMessageSubscribeErrorCode {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "unreachable")]
    Unreachable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum SpawnResultOutcome {
    Success {
        ok: True,
        #[serde(rename = "instanceId")]
        instance_id: String,
        #[serde(rename = "abducoSession")]
        abduco_session: String,
        #[serde(
            rename = "messageError",
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        message_error: Option<String>,
    },
    Failure {
        ok: False,
        code: SpawnErrorCode,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        message: Option<String>,
    },
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum AuthSessionResultOutcomeFalseCode {
    #[serde(rename = "spawn_failed")]
    SpawnFailed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum AuthSessionResultOutcome {
    Success {
        ok: True,
        #[serde(rename = "instanceId")]
        instance_id: String,
    },
    Failure {
        ok: False,
        code: AuthSessionResultOutcomeFalseCode,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        message: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum HostMessage {
    #[serde(rename = "alert")]
    Alert { alert: PiAlert },
    #[serde(rename = "auth_ok")]
    AuthOk {
        host: HostInfo,
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    #[serde(rename = "auth_error")]
    AuthError { code: HostMessageAuthErrorCode },
    #[serde(rename = "instances")]
    Instances { instances: Vec<InstanceSummary> },
    #[serde(rename = "subscribed")]
    Subscribed {
        #[serde(rename = "instanceId")]
        instance_id: String,
        snapshot: Box<InstanceSnapshot>,
        history: HostMessageSubscribedHistory,
    },
    #[serde(rename = "subscribe_error")]
    SubscribeError {
        #[serde(rename = "instanceId")]
        instance_id: String,
        code: HostMessageSubscribeErrorCode,
        #[serde(
            default,
            deserialize_with = "present",
            skip_serializing_if = "Option::is_none"
        )]
        #[ts(optional)]
        message: Option<String>,
    },
    #[serde(rename = "event")]
    Event {
        #[serde(rename = "instanceId")]
        instance_id: String,
        #[serde(rename = "sessionId", deserialize_with = "Deserialize::deserialize")]
        session_id: Option<String>,
        seq: u64,
        event: Box<InstanceEvent>,
    },
    #[serde(rename = "rc_result")]
    RcResult {
        #[serde(rename = "instanceId")]
        instance_id: String,
        #[ts(type = "Response")]
        response: Response,
    },
    #[serde(rename = "instance_gone")]
    InstanceGone {
        #[serde(rename = "instanceId")]
        instance_id: String,
    },
    #[serde(rename = "pong")]
    Pong { t: u64 },
    #[serde(rename = "folders_result")]
    FoldersResult {
        #[serde(rename = "requestId")]
        request_id: String,
        home: String,
        folders: Vec<FolderInfo>,
    },
    #[serde(rename = "update_check_result")]
    UpdateCheckResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        check: PackageUpdateCheck,
    },
    #[serde(rename = "update_result")]
    UpdateResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        outcome: PackageUpdateResult,
    },
    #[serde(rename = "github_result")]
    GithubResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        outcome: GitHubResult,
    },
    #[serde(rename = "devices_result")]
    DevicesResult {
        #[serde(rename = "requestId")]
        request_id: String,
        devices: Vec<PairedDevice>,
    },
    #[serde(rename = "device_revoke_result")]
    DeviceRevokeResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        outcome: HostOperationResult,
    },
    #[serde(rename = "host_restart_result")]
    HostRestartResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        outcome: HostOperationResult,
    },
    #[serde(rename = "github_status_result")]
    GithubStatusResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        outcome: GitHubStatusResult,
    },
    #[serde(rename = "dirs_result")]
    DirsResult {
        #[serde(rename = "requestId")]
        request_id: String,
        dirs: Vec<String>,
    },
    #[serde(rename = "spawn_result")]
    SpawnResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        outcome: SpawnResultOutcome,
    },
    #[serde(rename = "auth_session_result")]
    AuthSessionResult {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(flatten)]
        outcome: AuthSessionResultOutcome,
    },
    #[serde(other, skip_serializing)]
    #[ts(skip)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct GitHubCi {
    pub passed: u64,
    pub failed: u64,
    pub pending: u64,
}
