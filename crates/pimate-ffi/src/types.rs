use pimate_core::{
    Allowance, Change, Credential, Error as CoreError, Host, Image, Item, Operation, QuotaWindow,
    SessionStats, Snapshot, Target,
};
use pimate_protocol::RcMethod;

#[uniffi::remote(Error)]
pub enum CoreError {
    BuiltinCommand,
    Rejected,
    InvalidLink,
    InvalidCode,
    ExpiredCode,
    BadRequest,
    HostMismatch,
    Storage,
    Offline,
    Timeout,
    InvalidMessage,
    OutcomeUnknown,
    Blocked,
    Unsupported,
    StaleTarget,
    Revoked,
    InvalidToken,
    ProtocolMismatch,
    AuthFailed,
}

#[uniffi::remote(Record)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub address: String,
    pub device_id: String,
    pub needs_pairing: bool,
}

#[uniffi::remote(Record)]
pub struct Credential {
    pub host: Host,
    pub token: String,
}

#[uniffi::remote(Record)]
pub struct Item {
    pub collection: String,
    pub id: String,
    pub position: u64,
    pub json: String,
}

#[uniffi::remote(Enum)]
pub enum Change {
    ResyncRequired,
    Upsert(Item),
    Remove { collection: String, id: String },
}

#[uniffi::remote(Record)]
pub struct Snapshot {
    pub revision: u64,
    pub items: Vec<Item>,
}

#[uniffi::remote(Record)]
pub struct SessionStats {
    pub messages: u64,
    pub tokens: f64,
    pub cost: f64,
}

#[uniffi::remote(Record)]
pub struct QuotaWindow {
    pub minutes: u64,
    pub used_percent: Option<f64>,
    pub resets_at: Option<u64>,
}

#[uniffi::remote(Record)]
pub struct Allowance {
    pub source: String,
    pub observed_at: Option<u64>,
    pub gauge: Option<f64>,
    pub windows: Vec<QuotaWindow>,
}

#[uniffi::remote(Record)]
pub struct Target {
    pub instance: String,
    pub session: Option<String>,
}

#[uniffi::remote(Record)]
pub struct Image {
    pub mime_type: String,
    pub data: String,
}

#[uniffi::remote(Enum)]
pub enum Operation {
    DismissDraft {
        id: String,
    },
    Send {
        target: Target,
        text: String,
        images: Vec<Image>,
    },
    Abort {
        target: Target,
    },
    Answer {
        target: Target,
        params: String,
    },
    Rc {
        target: Target,
        method: RcMethod,
        params: String,
    },
    Folders,
    Dirs {
        path: String,
    },
    Spawn {
        params: String,
    },
    AuthSession {
        hold_id: String,
    },
    EndAuthSession {
        hold_id: String,
    },
    Github {
        instance: String,
        query: String,
    },
    GithubStatus,
    Devices,
    Revoke {
        device_id: String,
    },
    UpdateCheck,
    Update,
    Restart,
}

#[uniffi::remote(Enum)]
pub enum RcMethod {
    AuthList,
    AuthBegin,
    AuthWait,
    AuthRespond,
    AuthCancel,
    AuthLogout,
    Hello,
    History,
    EntryImage,
    FileImage,
    Prompt,
    Abort,
    UiRespond,
    SetThinking,
    SetModel,
    Compact,
    SessionOp,
    SessionTree,
    SetLabel,
    Commands,
    Models,
    SetDefaultModel,
    SetScopedModels,
    Sessions,
    Shutdown,
    SubagentTranscript,
    SubagentControl,
    SubagentsRpc,
}
