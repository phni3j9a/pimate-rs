use crate::{github::output, operations::home, service};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf, time::Duration};
const NAME: &str = "@phni3j9a/pimate";
fn meta() -> Result<Value> {
    let root: PathBuf = env::current_exe()?
        .ancestors()
        .nth(3)
        .context("missing package root")?
        .into();
    let v: Value = serde_json::from_slice(&fs::read(root.join("package.json"))?)?;
    ensure!(v["name"] == NAME);
    Ok(v)
}
fn version(v: &Value) -> Option<&str> {
    let s = v.as_str()?;
    Some(s).filter(|s| {
        s.split('.').count() == 3
            && s.split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
    })
}
pub async fn run(check: bool) -> Value {
    static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let reason = if meta().is_err() {
        Some("not_package")
    } else if !service::managed() {
        Some("not_service")
    } else {
        None
    };
    let current = env!("CARGO_PKG_VERSION");
    if check {
        let mut v = json!({"current":current,"canUpdate":reason.is_none()});
        if let Some(r) = reason {
            v["reason"] = r.into()
        }
        let latest = async {
            let d: Value = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()?
                .get("https://registry.npmjs.org/@phni3j9a/pimate/latest")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            Ok::<_, anyhow::Error>(
                version(&d["version"])
                    .context("invalid version")?
                    .to_string(),
            )
        }
        .await;
        match latest {
            Ok(s) => v["latest"] = s.into(),
            Err(_) => v["error"] = "Registry check failed".into(),
        }
        return v;
    }
    if let Some(r) = reason {
        return json!({"ok":false,"code":"unsupported","message":r});
    }
    let Ok(lock) = BUSY.try_lock() else {
        return json!({"ok":false,"code":"busy","message":"Another update is running"});
    };
    let r = async {
        let mut c = service::command(&env::var("PIMATE_PI").unwrap_or_else(|_| "pi".into()))?;
        c.current_dir(home()?)
            .args(["update", "npm:@phni3j9a/pimate"]);
        let (code, out, err) = output(c, 600, 1 << 20).await?;
        ensure!(
            code == Some(0),
            "{}",
            if err.is_empty() { out } else { err }
        );
        let m = meta()?;
        let to = version(&m["version"]).context("invalid installed version")?;
        let restarting = current != to;
        if restarting {
            tokio::spawn(async move {
                let _guard = lock;
                tokio::time::sleep(Duration::from_millis(300)).await;
                if service::restart().await.is_err() {
                    eprintln!("PiMate Host restart failed")
                }
            });
        }
        Ok::<_, anyhow::Error>(json!({"ok":true,"from":current,"to":to,"restarting":restarting}))
    }
    .await;
    r.unwrap_or_else(|e|json!({"ok":false,"code":"failed","message":e.to_string().trim().chars().rev().take(2048).collect::<Vec<_>>().into_iter().rev().collect::<String>()}))
}
