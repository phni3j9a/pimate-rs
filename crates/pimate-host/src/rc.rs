use crate::config::private;
use anyhow::{Result, anyhow, ensure};
use futures_util::{SinkExt, StreamExt};
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
pub use pimate_protocol::valid_instance_id as valid_id;
use pimate_protocol::{PROTOCOL_VERSION, decode_json};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use tokio::{net::UnixStream, time::timeout};
use tokio_util::codec::{Framed, LinesCodec};
use uuid::Uuid;
pub struct Rc {
    pub(super) io: Framed<UnixStream, LinesCodec>,
    id: String,
}
impl Rc {
    pub async fn open(dir: &Path, id: &str) -> Result<Self> {
        private(dir)?;
        ensure!(valid_id(id));
        let sock = UnixStream::connect(dir.join(format!("{id}.sock"))).await?;
        Ok(Self {
            io: Framed::new(sock, LinesCodec::new_with_max_length(16 << 20)),
            id: id.into(),
        })
    }
    pub async fn next(&mut self) -> Result<Value> {
        let s = self.io.next().await.ok_or_else(|| anyhow!("RC closed"))??;
        Ok(decode_json(&s)?)
    }
    pub async fn send(
        &mut self,
        req: Value,
        mut event: impl FnMut(Value) -> Result<()>,
    ) -> Result<Value> {
        self.io.send(request_line(req.clone(), &self.id)?).await?;
        loop {
            let v = self.next().await?;
            if v["type"] == "response" && v["id"] == req["id"] {
                return Ok(v);
            }
            if v["type"] == "event" && v["instanceId"] == self.id {
                event(v)?
            }
        }
    }
    pub async fn hello(&mut self, event: impl FnMut(Value) -> Result<()>) -> Result<Value> {
        let v = self.send(request("hello"), event).await?;
        let s = &v["result"]["snapshot"];
        ensure!(
            v["ok"] == true && s["instanceId"] == self.id && s["protocol"] == PROTOCOL_VERSION,
            "RC identity mismatch"
        );
        Ok(s.clone())
    }
}
pub fn request_line(mut req: Value, id: &str) -> Result<String> {
    req.as_object_mut()
        .ok_or_else(|| anyhow!("request"))?
        .entry("expectInstanceId")
        .or_insert(json!(id));
    Ok(req.to_string())
}
pub fn request(method: &str) -> Value {
    json!({"id":Uuid::new_v4().to_string(),"method":method,"params":{}})
}
pub struct Discovery {
    pub login: Vec<String>,
    pub instances: Vec<Value>,
    pub incompatible: serde_json::Map<String, Value>,
}
// Discovery already probes hello, including instances with no phone subscription.
fn attention_details(row: &mut Value, snapshot: &Value) {
    let runs = snapshot["subagents"]["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .rev();
    let agents = runs.flat_map(|r| {
        r["children"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(std::iter::once(r))
    });
    let question = snapshot["pendingUI"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find_map(|q| q["title"].as_str())
        .or_else(|| {
            agents
                .clone()
                .filter(|a| a["state"] == "waiting")
                .find_map(|a| a["question"].as_str())
        });
    let failure = agents
        .filter(|a| a["state"] == "failed")
        .find_map(|a| a["error"].as_str());
    for (key, value) in [("questionTitle", question), ("failureText", failure)] {
        if let Some(value) = value.filter(|s| !s.is_empty()) {
            row[key] = json!(value.chars().take(240).collect::<String>());
        }
    }
}

type FailureCache = std::collections::HashMap<String, (Value, Option<String>)>;
pub async fn list(dir: &Path) -> Result<Discovery> {
    discover(dir, None).await
}
pub async fn discover(dir: &Path, mut failures: Option<&mut FailureCache>) -> Result<Discovery> {
    let mut login = vec![];
    let mut incompatible = serde_json::Map::new();
    private(dir)?;
    let mut entries = vec![];
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        let Some(id) = p
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix(".json"))
            .filter(|id| valid_id(id))
        else {
            continue;
        };
        let Ok(st) = p.symlink_metadata() else {
            continue;
        };
        if !st.is_file() || st.len() > 1 << 20 {
            continue;
        }
        let Ok(bytes) = fs::read(&p) else { continue };
        let Ok(mut v) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if v["protocol"].is_number() && v["protocol"] != PROTOCOL_VERSION {
            incompatible.insert(id.to_owned(), v["protocol"].clone());
            continue;
        }
        let Some(pid) = v["pid"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .filter(|n| *n > 0)
        else {
            continue;
        };
        if v["instanceId"] != id
            || v["protocol"] != PROTOCOL_VERSION
            || !v["cwd"].is_string()
            || !matches!(v["origin"].as_str(), Some("pc" | "mobile" | "grok-bot"))
            || !v
                .get("abducoSession")
                .is_some_and(|a| a.is_null() || a.is_string())
            || !v["startedAt"].is_number()
            || !matches!(
                v["status"].as_str(),
                Some("idle" | "running" | "waiting_input")
            )
            || !v
                .get("session")
                .is_some_and(|s| s.is_null() || s["id"].is_string())
            || v.get("purpose").is_some_and(|p| p != "auth")
        {
            continue;
        }
        if kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH) {
            let _ = fs::remove_file(dir.join(format!("{id}.sock")));
            let _ = fs::remove_file(p);
            continue;
        }
        if v.get("purpose").is_some() {
            login.push(id.to_owned());
            continue;
        }
        let probe = async {
            let mut rc = Rc::open(dir, id).await?;
            let snapshot = rc.hello(|_| Ok(())).await?;
            anyhow::Ok((rc, snapshot))
        };
        let s = timeout(Duration::from_secs(1), probe).await;
        v["reachable"] = json!(matches!(s, Ok(Ok(_))));
        if let Ok(Ok((mut rc, s))) = s {
            if let Some(cache) = failures.as_deref_mut()
                && s["status"] == "idle"
                && s["session"]["id"].is_string()
            {
                // v1 has no last-error summary. Fetch idle history once per observed RC revision, never per phone.
                let key = json!([s["startedAt"], s["session"]["id"], s["seq"]]);
                if !cache.get(id).is_some_and(|(k, _)| k == &key) {
                    let mut req = request("history");
                    req["id"] = json!(format!("drawer-{}", Uuid::new_v4()));
                    req["expectSessionId"] = s["session"]["id"].clone();
                    if let Ok(Ok(r)) =
                        timeout(Duration::from_secs(1), rc.send(req, |_| Ok(()))).await
                        && r["ok"] == true
                        && r["result"]["session"]["id"] == s["session"]["id"]
                    {
                        let failure = r["result"]["items"]
                            .as_array()
                            .and_then(|a| a.iter().rev().find(|i| i["kind"] != "other"))
                            .filter(|i| i["kind"] == "assistant" && i["stopReason"] == "error")
                            .map(|i| {
                                i["errorMessage"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("Pi の実行に失敗しました")
                                    .chars()
                                    .take(240)
                                    .collect()
                            });
                        cache.insert(id.to_owned(), (key.clone(), failure));
                    }
                }
                if let Some((k, Some(text))) = cache.get(id)
                    && *k == key
                {
                    v["failureText"] = json!(text);
                }
            }
            attention_details(&mut v, &s);
            for k in ["status", "session"] {
                if let Some(x) = s.get(k) {
                    v[k] = x.clone()
                }
            }
            if let Some(runs) = s["subagents"]["runs"].as_array() {
                let runs: Vec<_> = runs
                    .iter()
                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                    .collect();
                v["subagents"] = json!(pimate_protocol::count_subagents(&runs));
            }
        }
        v["pcAttached"] = Value::Null;
        entries.push(v)
    }
    if let Some(cache) = failures {
        cache.retain(|id, _| entries.iter().any(|v| v["instanceId"] == *id));
    }
    entries.sort_by(|a, b| a["instanceId"].as_str().cmp(&b["instanceId"].as_str()));
    Ok(Discovery {
        login,
        instances: entries,
        incompatible,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cleanup_only_valid_stale_entries() -> Result<()> {
        let t = tempfile::tempdir()?;
        let dir = t.path().join("run");
        private(&dir)?;
        let victim = t.path().join("untouched");
        fs::write(&victim, "")?;
        for (id, pid, origin) in [
            ("stale", i32::MAX, "pc"),
            ("broken", i32::MAX, "bad"),
            ("live", std::process::id() as i32, "grok-bot"),
        ] {
            fs::write(dir.join(format!("{id}.json")),json!({"protocol":1,"instanceId":id,"pid":pid,"origin":origin,"socket":victim,"cwd":".","status":"idle","session":null,"abducoSession":null,"startedAt":1}).to_string())?
        }
        fs::write(dir.join("stale.sock"), "")?;
        let v = list(&dir).await?.instances;
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["reachable"], false);
        assert!(victim.exists() && dir.join("broken.json").exists());
        assert!(!dir.join("stale.sock").exists() && !dir.join("stale.json").exists());
        assert!(!valid_id("../stale"));
        Ok(())
    }
}
