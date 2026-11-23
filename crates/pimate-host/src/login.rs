use crate::{
    operations,
    rc::{self, Rc},
    terminal::Terminal,
};
use anyhow::{Result, ensure};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::{Instant, timeout};
const GRACE: Duration = Duration::from_secs(60);
#[derive(Default)]
struct State {
    id: Option<String>,
    pid: Option<nix::unistd::Pid>,
    holders: usize,
    deadline: Option<Instant>,
    closed: bool,
    orphans: std::collections::HashSet<String>,
}
#[derive(Default)]
pub struct Login {
    state: Mutex<State>,
    transition: tokio::sync::Mutex<()>,
}
pub struct Hold(Arc<Login>);
impl Drop for Hold {
    fn drop(&mut self) {
        let mut s = self.0.state.lock().unwrap();
        s.holders -= 1;
        if s.holders == 0 {
            s.deadline = Some(Instant::now() + GRACE)
        }
    }
}
impl Login {
    pub fn hold(self: &Arc<Self>) -> Hold {
        let mut s = self.state.lock().unwrap();
        s.holders += 1;
        s.deadline = None;
        Hold(self.clone())
    }
    pub fn owns(&self, id: &str) -> bool {
        self.state.lock().unwrap().id.as_deref() == Some(id)
    }
    pub async fn acquire(&self, t: &Terminal, dir: &Path) -> Result<String> {
        let _lock = self.transition.lock().await;
        let old = {
            let s = self.state.lock().unwrap();
            ensure!(!s.closed, "Host stopping");
            s.id.clone()
        };
        if let Some(id) = old
            && matches!(
                timeout(Duration::from_secs(2), async {
                    Rc::open(dir, &id).await?.hello(|_| Ok(())).await
                })
                .await,
                Ok(Ok(_))
            )
        {
            return Ok(id);
        }
        let args = [
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-context-files",
            "--no-prompt-templates",
            "--no-themes",
            "--no-tools",
            "--no-mcp",
            "--no-approve",
        ]
        .map(String::from);
        let (id, _) = t
            .launch(dir, &operations::home()?, false, true, &args)
            .await?;
        self.state.lock().unwrap().id = Some(id.clone());
        let ready = operations::ready(dir, &id).await;
        let mut state = self.state.lock().unwrap();
        let (_, snapshot) = ready.inspect_err(|_| state.id = None)?;
        state.pid = snapshot["pid"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .filter(|&p| p > 0)
            .map(nix::unistd::Pid::from_raw);
        Ok(id)
    }
    pub fn observe(self: &Arc<Self>, dir: &Path, observed: Vec<String>) {
        let Ok(_lock) = self.transition.try_lock() else {
            return;
        };
        let (id, due) = {
            let mut s = self.state.lock().unwrap();
            if s.pid
                .is_some_and(|p| nix::sys::signal::kill(p, None) == Err(nix::errno::Errno::ESRCH))
            {
                s.id = None;
                s.pid = None;
            }
            (
                s.id.clone(),
                s.deadline.is_some_and(|t| Instant::now() >= t),
            )
        };
        for orphan in observed.into_iter().filter(|o| Some(o) != id.as_ref()) {
            if self.state.lock().unwrap().orphans.insert(orphan.clone()) {
                let login = self.clone();
                let dir = dir.to_owned();
                tokio::spawn(async move {
                    shutdown(&dir, &orphan).await;
                    login.state.lock().unwrap().orphans.remove(&orphan);
                });
            }
        }
        if due
            && self.state.lock().unwrap().holders == 0
            && let Some(id) = id
        {
            let login = self.clone();
            let dir = dir.to_owned();
            tokio::spawn(async move {
                let Ok(_lock) = login.transition.try_lock() else {
                    return;
                };
                let due = {
                    let s = login.state.lock().unwrap();
                    s.id.as_ref() == Some(&id)
                        && !s.closed
                        && s.deadline.is_some_and(|t| Instant::now() >= t)
                };
                if due && shutdown(&dir, &id).await {
                    login.state.lock().unwrap().id = None;
                }
            });
        }
    }
    pub async fn close(&self, dir: &Path) {
        self.state.lock().unwrap().closed = true;
        let _lock = self.transition.lock().await;
        let id = self.state.lock().unwrap().id.clone();
        if let Some(id) = id {
            shutdown(dir, &id).await;
        }
    }
}
async fn shutdown(dir: &Path, id: &str) -> bool {
    matches!(
        timeout(Duration::from_secs(35), async {
            let mut c = Rc::open(dir, id).await?;
            c.hello(|_| Ok(())).await?;
            let list = c.send(rc::request("auth_list"), |_| Ok(())).await?;
            ensure!(list["ok"] == true);
            let flow = &list["result"]["flow"];
            match flow["status"].as_str() {
                Some("saving") => return Ok(false),
                Some("working" | "input") => {
                    let mut req = rc::request("auth_cancel");
                    req["params"] = json!({"flowId":flow["id"]});
                    if c.send(req, |_| Ok(())).await?["error"]["code"] == "busy" {
                        return Ok(false);
                    }
                }
                _ => (),
            }
            Ok::<_, anyhow::Error>(c.send(rc::request("shutdown"), |_| Ok(())).await?["ok"] == true)
        })
        .await,
        Ok(Ok(true))
    )
}
pub fn allowed(method: &str) -> bool {
    matches!(
        method,
        "hello"
            | "auth_list"
            | "auth_begin"
            | "auth_wait"
            | "auth_respond"
            | "auth_cancel"
            | "auth_logout"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio_util::codec::{Framed, LinesCodec};
    #[tokio::test]
    async fn defer_while_saving_including_cancel_race() -> Result<()> {
        for (status, busy, stopped) in [
            ("saving", false, false),
            ("working", true, false),
            ("input", false, true),
        ] {
            let tmp = tempfile::tempdir()?;
            let dir = tmp.path().join("run");
            crate::config::private(&dir)?;
            let listener = tokio::net::UnixListener::bind(dir.join("login.sock"))?;
            let fake = tokio::spawn(async move {
                let mut io = Framed::new(listener.accept().await?.0, LinesCodec::new());
                let mut methods = vec![];
                while let Some(line) = io.next().await {
                    let req: serde_json::Value = serde_json::from_str(&line?)?;
                    let method = req["method"].as_str().unwrap();
                    methods.push(method.to_owned());
                    let mut r = json!({"type":"response","id":req["id"],"ok":true});
                    match method {
                        "hello" => {
                            r["result"] = json!({"snapshot":{"instanceId":"login","protocol":1}})
                        }
                        "auth_list" => r["result"] = json!({"flow":{"id":"flow","status":status}}),
                        "auth_cancel" => {
                            assert_eq!(req["params"]["flowId"], "flow");
                            if busy {
                                r["ok"] = false.into();
                                r["error"] = json!({"code":"busy"})
                            }
                        }
                        "shutdown" => (),
                        _ => panic!(),
                    };
                    io.send(r.to_string()).await?;
                }
                Ok::<_, anyhow::Error>(methods)
            });
            assert_eq!(shutdown(&dir, "login").await, stopped);
            let calls = fake.await??;
            assert_eq!(calls.iter().any(|m| m == "auth_cancel"), status != "saving");
            assert_eq!(calls.iter().any(|m| m == "shutdown"), stopped);
        }
        Ok(())
    }
}
