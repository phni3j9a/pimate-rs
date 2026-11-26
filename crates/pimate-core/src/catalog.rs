use crate::{Error, state::State};
use serde_json::{Value, json};
// Keep authorization links out of the UI unless they lead to the exact provider endpoint.
fn safe_url(flow: &Value) -> bool {
    let Some(url) = flow["authorization"]["url"].as_str() else {
        return false;
    };
    if url.len() > 8192
        || url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '#' | '\\'))
    {
        return false;
    }
    match flow["provider"].as_str() {
        Some("anthropic") => url.starts_with("https://claude.ai/oauth/authorize?"),
        Some("openai-codex") => {
            url == "https://auth.openai.com/codex/device"
                && flow["authorization"]["userCode"].as_str().is_some_and(|s| {
                    !s.is_empty()
                        && s.len() <= 64
                        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
        }
        _ => false,
    }
}
impl State {
    pub(crate) fn receipt(
        &mut self,
        method: &str,
        instance: &Value,
        body: &mut Value,
    ) -> Result<(), Error> {
        match method {
            "hello" => self.put(
                "loginCapability",
                instance.as_str().ok_or(Error::InvalidMessage)?,
                json!(body["snapshot"]["providerAuth"] == true),
            ),
            "commands" => {
                self.clear("command");
                self.builtins();
                for c in body["commands"].as_array().into_iter().flatten() {
                    if let Some(name) = c["name"].as_str()
                        && self.get("command", name).is_null()
                    {
                        self.put("command", name, c.clone())
                    }
                }
            }
            "models" => {
                self.clear("model");
                for m in body["models"].as_array().into_iter().flatten() {
                    let id = json!([m["provider"], m["id"]]).to_string();
                    let mut m = m.clone();
                    m["favorite"] = json!(body["scoped"].as_array().is_some_and(|s| {
                        s.iter()
                            .any(|f| f["id"] == m["id"] && f["provider"] == m["provider"])
                    }));
                    self.put("model", &id, m)
                }
                self.put("view", "defaultModel", body["default"].clone());
            }
            "sessions" | "session_tree" => {
                let (key, col) = if method == "sessions" {
                    ("sessions", "session")
                } else {
                    ("nodes", "node")
                };
                self.clear(col);
                for row in body[key].as_array().into_iter().flatten() {
                    self.keyed(col, "id", row)
                }
            }
            "set_default_model" => self.put("view", "defaultModel", body["default"].clone()),
            "set_scoped_models" => {
                for (id, mut m) in self.models() {
                    m["favorite"] = json!(body["scoped"].as_array().is_some_and(|s| {
                        s.iter()
                            .any(|f| f["id"] == m["id"] && f["provider"] == m["provider"])
                    }));
                    self.put("model", &id, m)
                }
            }
            "set_model" => self.put("view", "model", body["model"].clone()),
            "set_thinking" => self.put("view", "thinking", body["thinking"].clone()),
            "auth_list" | "auth_begin" | "auth_wait" | "auth_cancel" | "auth_respond" => {
                let id = instance.as_str().ok_or(Error::InvalidMessage)?;
                if method == "auth_list" {
                    let mut providers = body["providers"].clone();
                    for p in providers.as_array_mut().into_iter().flatten() {
                        let (vendor, member) = match p["id"].as_str() {
                            Some("openai") => ("OpenAI", "API キー"),
                            Some("openai-codex") => ("OpenAI", "ChatGPT"),
                            Some("anthropic") => ("Anthropic", ""),
                            Some("opencode") => ("OpenCode", "Zen"),
                            Some("opencode-go") => ("OpenCode", "Go"),
                            Some(other) => (other, ""),
                            None => continue,
                        };
                        p["vendor"] = json!(vendor);
                        p["member"] = json!(member);
                        if p["id"] == "openai"
                            && let Some(methods) = p["methods"].as_array_mut()
                        {
                            methods.retain(|m| m != "oauth");
                        }
                    }
                    self.put("provider", id, providers)
                }
                let flow = &mut body["flow"];
                if !safe_url(flow)
                    && let Some(obj) = flow.as_object_mut()
                {
                    obj.remove("authorization");
                }
                let old = self.get("auth", id);
                if old["id"] == flow["id"] && old["revision"].as_u64() > flow["revision"].as_u64() {
                    *flow = old.clone()
                } else {
                    self.put("auth", id, flow.clone())
                }
            }
            "entry_image" | "file_image" => {
                let img = &body["image"];
                let data = img["data"].as_str().ok_or(Error::InvalidMessage)?;
                use base64::{Engine, engine::general_purpose::STANDARD};
                if data.is_empty()
                    || data.len() > (5 * 1024 * 1024usize).div_ceil(3) * 4
                    || STANDARD
                        .decode(data)
                        .ok()
                        .is_none_or(|v| v.len() > 5 * 1024 * 1024)
                {
                    return Err(Error::InvalidMessage);
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn builtins(&mut self) {
        if self.get("view", "slashCommands") != true {
            return;
        }
        for name in [
            "model", "reload", "new", "resume", "fork", "compact", "tree",
        ] {
            if name != "tree" || self.get("view", "sessionTree") == true {
                self.put("command", name, json!({"name":name,"source":"builtin"}))
            }
        }
    }
}
