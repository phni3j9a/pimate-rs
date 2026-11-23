use crate::{
    Error,
    state::{Draft, State},
};
use pimate_protocol as p;
use serde_json::{Value, json};
/// Capture from the displayed view, not from the latest session at dispatch time.
#[derive(Clone, Debug)]
pub struct Target {
    pub instance: String,
    pub session: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Image {
    pub mime_type: String,
    pub data: String,
}
/// Complex parameters are JSON records of the corresponding protocol method (not envelopes).
#[derive(Clone, Debug)]
pub enum Operation {
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
    pub fn prepare(&mut self, id: &str, op: Operation) -> Result<Value, Error> {
        use Operation::*;
        let mut m = match op {
            Send {
                target,
                text,
                images,
            } => {
                self.target(&target)?;
                if self.questions()
                    || !self.get("view", "pcPrompt").is_null()
                    || self.drafts.iter().any(|d| {
                        d.instance == target.instance && d.session == json!(target.session)
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
                self.drafts.push(Draft {
                    id: id.into(),
                    instance: target.instance.clone(),
                    session: json!(target.session),
                    params: params.clone(),
                    known: self.known(),
                });
                self.put("draft",id,json!({"instanceId":target.instance,"sessionId":target.session,"params":params,"status":"sending"}));
                return Ok(rc(id, target, "prompt", params));
            }
            Abort { target } => {
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
                let dedicated = name.starts_with("auth_")
                    && self.auth_targets.values().any(|s| s == &target.instance);
                if !dedicated {
                    self.target(&target)?
                }
                if name.starts_with("auth_")
                    && (self.caps["providerAuth"] != true
                        || (!dedicated && self.get("view", "providerAuth") != true))
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
                return Ok(rc(id, target, name, params));
            }
            Folders => json!({"type":"folders"}),
            Dirs { path } => json!({"type":"dirs","path":path}),
            Spawn { params } => {
                json!({"type":"spawn","params":checked::<p::SpawnParams>(record(&params)?)?})
            }
            AuthSession { hold_id } => json!({"type":"auth_session","holdId":hold_id}),
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
        } else if let Some(hold) = m["holdId"].as_str() {
            self.auth_targets.remove(hold);
        }
        Ok(m)
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
        p::RcMethod::Hello => Err(Error::BadRequest),
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
