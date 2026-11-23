use crate::config::private;
use anyhow::{Result, anyhow, ensure};
use futures_util::{SinkExt, StreamExt};
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
use pimate_protocol::{InstanceMeta, PROTOCOL_VERSION, Received, decode_json};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use tokio::{net::UnixStream, time::timeout};
use tokio_util::codec::{Framed, LinesCodec};
use uuid::Uuid;
pub fn valid_id(id: &str) -> bool {
    (1..=100).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
pub struct Rc {
    io: Framed<UnixStream, LinesCodec>,
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
        mut req: Value,
        mut event: impl FnMut(Value) -> Result<()>,
    ) -> Result<Value> {
        req.as_object_mut()
            .ok_or_else(|| anyhow!("request"))?
            .entry("expectInstanceId")
            .or_insert(json!(self.id));
        self.io.send(req.to_string()).await?;
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
pub fn request(method: &str) -> Value {
    json!({"id":Uuid::new_v4().to_string(),"method":method,"params":{}})
}
pub async fn list(dir: &Path) -> Result<Vec<Value>> {
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
        let Ok(m) = serde_json::from_value::<InstanceMeta>(v.clone()) else {
            continue;
        };
        let Ok(pid) = i32::try_from(m.pid) else {
            continue;
        };
        if pid <= 0
            || m.instance_id != id
            || m.protocol != PROTOCOL_VERSION
            || !matches!(m.origin, Received::Known(_))
            || !matches!(m.status, Received::Known(_))
        {
            continue;
        }
        if kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH) {
            let _ = fs::remove_file(dir.join(format!("{id}.sock")));
            let _ = fs::remove_file(p);
            continue;
        }
        if m.purpose.is_some() {
            continue;
        }
        let probe = async { Rc::open(dir, id).await?.hello(|_| Ok(())).await };
        let s = timeout(Duration::from_secs(1), probe).await;
        v["reachable"] = json!(matches!(s, Ok(Ok(_))));
        if let Ok(Ok(s)) = s {
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
    entries.sort_by(|a, b| a["instanceId"].as_str().cmp(&b["instanceId"].as_str()));
    Ok(entries)
}
