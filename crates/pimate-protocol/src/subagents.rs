use crate::*;

/// Which extension produced the run. Both register a tool named `subagent`; the RC tells them apart by details shape.
#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum SubagentBackend {
    #[serde(rename = "pi-subagents")]
    PiSubagents,
    #[serde(rename = "example")]
    Example,
}

/// - waiting: the child asked its parent (pi-subagents `contact_supervisor`) and waits for a reply.
/// - stopping: a stop was requested and not yet confirmed.
/// - unknown: the backend reported a state the RC does not understand.
#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum SubagentState {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "waiting")]
    Waiting,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "stopping")]
    Stopping,
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(other, rename = "unknown")]
    Unknown,
}

#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum SubagentMode {
    #[serde(rename = "single")]
    Single,
    #[serde(rename = "parallel")]
    Parallel,
    #[serde(rename = "chain")]
    Chain,
    #[serde(rename = "workflow")]
    Workflow,
    #[serde(other, rename = "unknown")]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentUsage {
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub input: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub output: Option<Number>,
    /// Total tokens as reported by the backend (no double counting of cache).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub total: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub cost_usd: Option<Number>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentActivity {
    /// Tool currently running in the child, e.g. "read".
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub tool: Option<String>,
    /// Display string of its arguments, e.g. "src/api/client.ts" (may be truncated by the backend).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub args: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub started_at: Option<Number>,
}

/// Operations the RC can perform right now for a run/child. Only these may be shown in the app.
/// - steer / follow_up: send text to a running pi-subagents child (index-targeted).
/// - interrupt: pause the whole pi-subagents run (resumable).
/// - resume: restart a paused/finished pi-subagents child with a message; starts a NEW run (`resumedAs`).
/// - stop: stop the whole background run. stop_child: stop one pending/running child of a background run.
/// - abort_parent: official example only — aborts the parent Pi turn, which stops every child.
#[derive(Clone, Copy, Eq, Debug, PartialEq, Serialize, Deserialize, TS)]
pub enum SubagentAction {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "follow_up")]
    FollowUp,
    #[serde(rename = "interrupt")]
    Interrupt,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "stop_child")]
    StopChild,
    #[serde(rename = "abort_parent")]
    AbortParent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentChild {
    /// Stable within the run (backend child id such as "step:0", or the index).
    pub id: String,
    pub index: Number,
    pub agent: String,
    /// Short human title (e.g. from the pi-subagents session name), without the agent prefix.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub title: Option<String>,
    pub state: SubagentState,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub thinking: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub started_at: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub ended_at: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub activity: Option<SubagentActivity>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub usage: Option<SubagentUsage>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub error: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub exit_code: Option<Option<Number>>,
    /// Question text while `waiting`, when readable.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub question: Option<String>,
    /// Whether `subagent_transcript` can return the child's work record.
    pub has_transcript: bool,
    /// Child-scoped actions (steer, follow_up, stop_child, resume).
    #[serde(deserialize_with = "known_values")]
    pub actions: Vec<SubagentAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SubagentRunStage {
    pub index: Number,
    pub count: Number,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRun {
    /// RC key: pi-subagents runId, or `tool:<toolCallId>` for runs only known through a tool call.
    pub id: String,
    pub backend: SubagentBackend,
    /// Keeps running after the parent turn ends (pi-subagents async).
    pub background: bool,
    pub mode: SubagentMode,
    /// Parent tool call that started (or launched) the run; links the chat tool row to the run.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub tool_call_id: Option<String>,
    pub state: SubagentState,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub started_at: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub updated_at: Option<Number>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub ended_at: Option<Number>,
    /// Chain progress (0-based index).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub stage: Option<SubagentRunStage>,
    pub children: Vec<SubagentChild>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub usage: Option<SubagentUsage>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub error: Option<String>,
    /// Run this one was resumed from / was resumed into (pi-subagents `resume` creates a new run).
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub resumed_from: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub resumed_as: Option<String>,
    /// Run-scoped actions (interrupt, stop, abort_parent).
    #[serde(deserialize_with = "known_values")]
    pub actions: Vec<SubagentAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SubagentsStateBackendsItem {
    pub backend: SubagentBackend,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub version: Option<String>,
    pub supported: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SubagentsState {
    #[serde(deserialize_with = "known_backend_rows")]
    pub runs: Vec<SubagentRun>,
    /// Backends seen in this Pi, with the version when known. `supported: false` → app shows plain tool rows only.
    #[serde(deserialize_with = "known_backend_rows")]
    pub backends: Vec<SubagentsStateBackendsItem>,
}

/// Counts for the drawer (host InstanceSummary / InstanceMeta).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SubagentCounts {
    /// queued / running / waiting / stopping children.
    pub active: u64,
    pub waiting: u64,
    pub paused: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SubagentTranscript {
    /// Parent's request to the child, unredacted when available.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub task: Option<String>,
    /// Child messages normalized like the main transcript (initial task excluded). Message granularity, not token streaming.
    pub items: Vec<TranscriptItem>,
    /// Opaque; pass back to get only newer items.
    pub cursor: String,
    /// Older records were dropped by the backend or the RC size limit.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional)]
    pub truncated: Option<bool>,
}

// Check the discriminator before parsing the payload: a new backend can have a different shape.
fn known_backend_rows<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    Vec::<Value>::deserialize(d)?
        .into_iter()
        .filter_map(|row| {
            if unknown_backend(&row) {
                return None;
            }
            Some(serde_json::from_value(row).map_err(serde::de::Error::custom))
        })
        .collect()
}

// Only a new string discriminator is skippable; missing or invalid tags are errors.
pub(crate) fn unknown_backend(row: &Value) -> bool {
    row.get("backend")
        .and_then(Value::as_str)
        .is_some_and(|name| {
            SubagentBackend::deserialize(
                serde::de::value::StrDeserializer::<serde::de::value::Error>::new(name),
            )
            .is_err()
        })
}
