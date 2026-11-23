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
    holders: usize,
    deadline: Option<Instant>,
    closed: bool,
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
        operations::ready(dir, &id).await?;
        Ok(id)
    }
    pub async fn refresh(&self, dir: &Path, observed: &[String]) {
        let Ok(_lock) = self.transition.try_lock() else {
            return;
        };
        let (id, due) = {
            let s = self.state.lock().unwrap();
            (
                s.id.clone(),
                s.deadline.is_some_and(|t| Instant::now() >= t),
            )
        };
        for orphan in observed.iter().filter(|o| Some(*o) != id.as_ref()) {
            let _ = shutdown(dir, orphan).await;
        }
        if due
            && let Some(id) = id
            && shutdown(dir, &id).await
        {
            self.state.lock().unwrap().id = None
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
