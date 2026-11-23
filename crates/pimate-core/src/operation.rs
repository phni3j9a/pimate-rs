use crate::{
    Error,
    state::{Draft, State},
};
use pimate_protocol as p;
use serde::Deserialize;
use serde_json::{Value, json};
/// Capture from the displayed view, not from the latest session at dispatch time.
#[derive(Clone, Debug)]
pub struct Target {
    pub instance: String,
    pub session: Option<String>,
}
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub mime_type: String,
    pub data: String,
}
#[derive(Clone, Debug, serde::Deserialize)]
pub struct RestoredDraft {
    pub text: String,
    #[serde(default)]
    pub images: Vec<Image>,
}
/// Complex parameters are JSON records of the corresponding protocol method (not envelopes).
#[derive(Clone)]
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
        method: p::RcMethod,
        params: String,
    },
    HostModels {
        instance: String,
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
fn record(s: &str) -> Result<Value, Error> {
    serde_json::from_str(s).map_err(|_| Error::BadRequest)
}
fn checked<T: serde::de::DeserializeOwned + serde::Serialize>(p: Value) -> Result<Value, Error> {
    serde_json::from_value::<T>(p)
        .and_then(serde_json::to_value)
        .map_err(|_| Error::BadRequest)
}
impl State {
    pub fn prepare(&mut self, id: &str, op: Operation, commit: bool) -> Result<Value, Error> {
        let msg = self.build(id, op, commit)?;
        let method = &msg["request"]["method"];
        let lock = match method.as_str() {
            Some("auth_wait") => {
                let pi = msg["instanceId"].as_str().ok_or(Error::BadRequest)?;
                if self.auth_waits.values().any(|target| target == pi) {
                    return Err(Error::Blocked);
                }
                if commit {
                    self.auth_waits.insert(id.into(), pi.into());
                }
                return Ok(msg);
            }
            Some("ui_respond") => "answer",
            Some("compact" | "session_op" | "set_model" | "set_thinking" | "shutdown") => {
                "operation"
            }
            _ => return Ok(msg),
        };
        if !self.get("busy", lock).is_null() {
            return Err(Error::Blocked);
        }
        if commit {
            let req = &msg["request"];
            let params = &req["params"];
            let mut busy = json!({"id":id,"requestId":params["requestId"],"method":method,"op":params["op"],"sessionId":req["expectSessionId"]});
            if matches!(method.as_str(), Some("compact" | "session_op")) {
                busy["startedAt"] = json!(self.generation(msg["instanceId"].as_str().unwrap()));
            }
            if params["op"] == "fork"
                && let Some(entry) = params["arg"].as_str()
            {
                let source = self.get("entry", entry);
                if source["kind"] == "user" {
                    busy["forkText"] = source["text"].clone()
                }
            }
            self.put("busy", lock, busy);
        }
        Ok(msg)
    }
    fn build(&mut self, id: &str, op: Operation, commit: bool) -> Result<Value, Error> {
        use Operation::*;
        let mut m = match op {
            DismissDraft { .. } => return Err(Error::BadRequest),
            Send {
                target,
                text,
                images,
            } => {
                self.target(&target)?;
                if self.questions()
                    || !self.get("view", "pcPrompt").is_null()
                    || self.drafts.iter().any(|d| {
                        d.instance == target.instance
                            && d.session == json!(target.session)
                            && self.get("draft", &d.id)["status"] == "sending"
                    })
                {
                    return Err(Error::Blocked);
                }
                let images: Vec<_> = images
                    .into_iter()
                    .map(|i| json!({"type":"image","mimeType":i.mime_type,"data":i.data}))
                    .collect();
                let mut params = json!({"text":text,"deliverAs":"steer"});
                if !images.is_empty() {
                    if self.get("view", "promptImages") != true {
                        return Err(Error::Unsupported);
                    }
                    if images.len() as u64
                        > self.get("view", "promptImageLimit").as_u64().unwrap_or(1)
                        || text.encode_utf16().count() > p::MAX_IMAGE_PROMPT_TEXT
                        || !p::valid_prompt_images(&json!(images))
                    {
                        return Err(Error::BadRequest);
                    }
                    params["images"] = json!(images)
                } else if text.trim().is_empty() {
                    return Err(Error::BadRequest);
                }
                if self.get("view", "slashCommands") == true
                    && let Some(name) = p::slash_command_name(&text)
                {
                    match self.get("command", name)["source"].as_str() {
                        None => return Err(Error::Unsupported),
                        Some("builtin") => return Err(Error::BuiltinCommand),
                        Some("extension") if !params["images"].is_null() => {
                            return Err(Error::BadRequest);
                        }
                        _ => params["expandTemplates"] = json!(true),
                    }
                }
                if commit {
                    let replaced = self
                        .drafts
                        .iter()
                        .filter(|d| {
                            d.instance == target.instance
                                && d.session == json!(target.session)
                                && d.params == params
                                && self.get("draft", &d.id)["status"] == "unconfirmed"
                        })
                        .map(|d| d.id.clone())
                        .collect::<Vec<_>>();
                    for id in replaced {
                        self.forget_draft(&id)
                    }
                    self.drafts.push(Draft {
                        id: id.into(),
                        instance: target.instance.clone(),
                        session: json!(target.session),
                        params: params.clone(),
                        known: self.known(),
                    });
                    self.put("draft",id,json!({"instanceId":target.instance,"sessionId":target.session,"params":params,"status":"sending"}));
                }
                return Ok(rc(id, target, "prompt", params));
            }
            Abort { target } => {
                if target.session.as_deref().is_none_or(str::is_empty) {
                    return Err(Error::StaleTarget);
                }
                self.target(&target)?;
                return Ok(rc(id, target, "abort", json!({})));
            }
            Answer { target, params } => {
                self.target(&target)?;
                let params = checked::<p::UiRespondParams>(record(&params)?)?;
                if self
                    .get("question", params["requestId"].as_str().unwrap_or_default())
                    .is_null()
                {
                    return Err(Error::StaleTarget);
                }
                return Ok(rc(id, target, "ui_respond", params));
            }
            Rc {
                target,
                method,
                params,
            } => {
                let key = serde_json::to_value(method).unwrap();
                let name = key.as_str().unwrap();
                let dedicated = (name.starts_with("auth_") || name == "hello")
                    && self.auth_targets.values().any(|s| s == &target.instance);
                if !dedicated {
                    self.target(&target)?
                }
                if name.starts_with("auth_")
                    && (self.caps["providerAuth"] != true
                        || if dedicated {
                            self.get("loginCapability", &target.instance) != true
                        } else {
                            self.get("view", "providerAuth") != true
                        })
                {
                    return Err(Error::Unsupported);
                }
                let cap = match name {
                    "commands" => Some("slashCommands"),
                    "session_tree" | "set_label" => Some("sessionTree"),
                    "entry_image" => Some("entryImages"),
                    "file_image" => Some("fileImages"),
                    _ => None,
                };
                if cap.is_some_and(|c| self.get("view", c) != true) {
                    return Err(Error::Unsupported);
                }
                let params = rc_params(method, record(&params)?)?;
                if name == "hello" && !dedicated {
                    return Err(Error::BadRequest);
                }
                if name == "session_op"
                    && matches!(params["op"].as_str(), Some("reload" | "tree"))
                    && (self.get("view", "status") != "idle"
                        || self.get("view", "agentActive") == true)
                {
                    return Err(Error::Blocked);
                }
                if name == "session_op"
                    && params["op"] == "tree"
                    && self.get("view", "sessionTree") != true
                {
                    return Err(Error::Unsupported);
                }
                if name == "subagent_control" {
                    let run = self.get("run", params["runId"].as_str().unwrap_or_default());
                    let unit = if let Some(cid) = params["childId"].as_str() {
                        run["children"]
                            .as_array()
                            .and_then(|cs| cs.iter().find(|c| c["id"] == cid))
                            .ok_or(Error::StaleTarget)?
                    } else {
                        run
                    };
                    if !unit["actions"]
                        .as_array()
                        .is_some_and(|a| a.contains(&params["action"]))
                    {
                        return Err(Error::Unsupported);
                    }
                }
                if name.starts_with("auth_") {
                    let flow = self.get("auth", &target.instance);
                    let active = matches!(
                        flow["status"].as_str(),
                        Some("input" | "saving" | "working")
                    );
                    if matches!(name, "auth_begin" | "auth_logout") && active
                        || name == "auth_cancel" && (!active || flow["status"] == "saving")
                    {
                        return Err(Error::Blocked);
                    }
                    if !params["flowId"].is_null() && params["flowId"] != flow["id"] {
                        return Err(Error::StaleTarget);
                    }
                    if name == "auth_begin"
                        && params["provider"] == "openai"
                        && params["method"] == "oauth"
                    {
                        return Err(Error::Unsupported);
                    }
                    if name == "auth_respond"
                        && (flow["status"] != "input"
                            || params["promptId"] != flow["prompt"]["id"]
                            || params["value"].as_str().is_none_or(|s| {
                                s.trim().is_empty()
                                    || s.encode_utf16().count() > p::AUTH_MAX_INPUT_LENGTH
                            }))
                    {
                        return Err(Error::BadRequest);
                    }
                }
                return Ok(rc(id, target, name, params));
            }
            HostModels { instance } => {
                if self.get("instance", &instance).is_null() {
                    return Err(Error::StaleTarget);
                }
                return Ok(rc(
                    id,
                    Target {
                        instance,
                        session: None,
                    },
                    "models",
                    json!({}),
                ));
            }
            Folders => json!({"type":"folders"}),
            Dirs { path } => json!({"type":"dirs","path":path}),
            Spawn { params } => {
                json!({"type":"spawn","params":checked::<p::SpawnParams>(record(&params)?)?})
            }
            AuthSession { hold_id } => {
                if self.caps["providerAuth"] != true {
                    return Err(Error::Unsupported);
                }
                json!({"type":"auth_session","holdId":hold_id})
            }
            EndAuthSession { hold_id } => json!({"type":"auth_session_end","holdId":hold_id}),
            Github { instance, query } => {
                json!({"type":"github","instanceId":instance,"query":checked::<p::GitHubQuery>(record(&query)?)?})
            }
            GithubStatus => json!({"type":"github_status"}),
            Devices => json!({"type":"devices"}),
            Revoke { device_id } => json!({"type":"device_revoke","deviceId":device_id}),
            UpdateCheck => json!({"type":"update_check"}),
            Update => json!({"type":"update"}),
            Restart => json!({"type":"host_restart"}),
        };
        let cap = match m["type"].as_str().unwrap() {
            "devices" | "device_revoke" => "pairedDevices",
            "github" => "github",
            "github_status" => "githubStatus",
            "update" | "update_check" => "packageUpdate",
            "host_restart" => "hostRestart",
            "auth_session" | "auth_session_end" => "authSession",
            "dirs" => "dirs",
            _ => "",
        };
        if !cap.is_empty() && self.caps[cap] != true {
            return Err(Error::Unsupported);
        }
        if m["type"] != "auth_session_end" {
            m["requestId"] = json!(id)
        } else if commit && let Some(hold) = m["holdId"].as_str() {
            self.auth_targets.remove(hold);
        }
        Ok(m)
    }
    pub(crate) fn take_unconfirmed(
        &mut self,
        target: &Target,
        id: &str,
    ) -> Result<RestoredDraft, Error> {
        self.target(target)?;
        let draft = self.get("draft", id);
        if draft["status"] != "unconfirmed"
            || draft["instanceId"] != target.instance
            || draft["sessionId"] != json!(target.session)
        {
            return Err(Error::StaleTarget);
        }
        let payload =
            RestoredDraft::deserialize(&draft["params"]).map_err(|_| Error::InvalidMessage)?;
        self.forget_draft(id);
        Ok(payload)
    }
    fn target(&self, t: &Target) -> Result<(), Error> {
        if !self.ready()
            || self.selected.as_ref() != Some(&t.instance)
            || self.get("view", "session")["id"] != json!(t.session)
        {
            Err(Error::StaleTarget)
        } else {
            Ok(())
        }
    }
}
fn rc(id: &str, t: Target, m: &str, p: Value) -> Value {
    let mut r = json!({"id":id,"method":m,"params":p,"expectInstanceId":t.instance});
    if let Some(s) = t.session {
        r["expectSessionId"] = json!(s)
    }
    json!({"type":"rc","instanceId":t.instance,"request":r})
}
fn rc_params(m: p::RcMethod, v: Value) -> Result<Value, Error> {
    match m {
        p::RcMethod::AuthList => checked::<p::Empty>(v),
        p::RcMethod::AuthBegin => checked::<p::AuthBeginParams>(v),
        p::RcMethod::AuthWait => checked::<p::AuthWaitParams>(v),
        p::RcMethod::AuthRespond => checked::<p::AuthRespondParams>(v),
        p::RcMethod::AuthCancel => checked::<p::AuthCancelParams>(v),
        p::RcMethod::AuthLogout => checked::<p::AuthLogoutParams>(v),
        p::RcMethod::Hello => checked::<p::Empty>(v),
        p::RcMethod::History => Err(Error::BadRequest),
        p::RcMethod::EntryImage => checked::<p::EntryImageParams>(v),
        p::RcMethod::FileImage => checked::<p::FileImageParams>(v),
        p::RcMethod::Prompt => Err(Error::BadRequest),
        p::RcMethod::Abort => Err(Error::BadRequest),
        p::RcMethod::UiRespond => Err(Error::BadRequest),
        p::RcMethod::SetThinking => checked::<p::SetThinkingParams>(v),
        p::RcMethod::SetModel => checked::<p::ModelRef>(v),
        p::RcMethod::Compact => checked::<p::CompactParams>(v),
        p::RcMethod::SessionOp => checked::<p::SessionOpParams>(v),
        p::RcMethod::SessionTree => checked::<p::Empty>(v),
        p::RcMethod::SetLabel => checked::<p::SetLabelParams>(v),
        p::RcMethod::Commands => checked::<p::Empty>(v),
        p::RcMethod::Models => checked::<p::Empty>(v),
        p::RcMethod::SetDefaultModel => checked::<p::SetDefaultModelParams>(v),
        p::RcMethod::SetScopedModels => checked::<p::SetScopedModelsParams>(v),
        p::RcMethod::Sessions => checked::<p::SessionsParams>(v),
        p::RcMethod::Shutdown => checked::<p::Empty>(v),
        p::RcMethod::SubagentTranscript => checked::<p::SubagentTranscriptParams>(v),
        p::RcMethod::SubagentControl => checked::<p::SubagentControlParams>(v),
        p::RcMethod::SubagentsRpc => checked::<p::SubagentsRpcParams>(v),
    }
}
pub(crate) fn rc_result(method: &str, v: &Value) -> Result<Value, Error> {
    let out = match method {
        "auth_list" => checked::<p::AuthListResult>(v.clone()),
        "auth_begin" => checked::<p::AuthBeginResult>(v.clone()),
        "auth_wait" => checked::<p::AuthWaitResult>(v.clone()),
        "auth_respond" => checked::<p::AuthRespondResult>(v.clone()),
        "auth_cancel" => checked::<p::AuthCancelResult>(v.clone()),
        "auth_logout" => checked::<p::AuthLogoutResult>(v.clone()),
        "hello" => checked::<p::HelloResult>(v.clone()),
        "history" => checked::<p::HistoryResult>(v.clone()),
        "entry_image" => checked::<p::EntryImageResult>(v.clone()),
        "file_image" => checked::<p::FileImageResult>(v.clone()),
        "prompt" => checked::<p::PromptResult>(v.clone()),
        "abort" => checked::<p::Empty>(v.clone()),
        "ui_respond" => checked::<p::Empty>(v.clone()),
        "set_thinking" => checked::<p::SetThinkingResult>(v.clone()),
        "set_model" => checked::<p::SetModelResult>(v.clone()),
        "compact" => checked::<p::Empty>(v.clone()),
        "session_op" => checked::<p::Empty>(v.clone()),
        "session_tree" => checked::<p::SessionTree>(v.clone()),
        "set_label" => checked::<p::Empty>(v.clone()),
        "commands" => checked::<p::CommandsResult>(v.clone()),
        "models" => checked::<p::ModelsResult>(v.clone()),
        "set_default_model" => checked::<p::SetDefaultModelResult>(v.clone()),
        "set_scoped_models" => checked::<p::SetScopedModelsResult>(v.clone()),
        "sessions" => checked::<p::SessionsResult>(v.clone()),
        "shutdown" => checked::<p::Empty>(v.clone()),
        "subagent_transcript" => checked::<p::SubagentTranscript>(v.clone()),
        "subagent_control" => checked::<p::SubagentControlResult>(v.clone()),
        "subagents_rpc" => checked::<p::SubagentsRpcResult>(v.clone()),
        _ => Err(Error::InvalidMessage),
    };
    out.map_err(|_| Error::InvalidMessage)
}
