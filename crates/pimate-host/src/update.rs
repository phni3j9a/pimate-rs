use crate::{
    operations::home,
    process::{command, output},
    service,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf, time::Duration};
const NAME: &str = "@phni3j9a/pimate";
fn root() -> Result<PathBuf> {
    Ok(env::current_exe()?
        .ancestors()
        .nth(3)
        .context("missing package root")?
        .into())
}
fn meta(root: &std::path::Path) -> Result<Value> {
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
    run_in(
        check,
        &root().unwrap_or_default(),
        service::managed(),
        "https://registry.npmjs.org/@phni3j9a/pimate/latest",
    )
    .await
}
async fn run_in(check: bool, root: &std::path::Path, managed: bool, registry: &str) -> Value {
    static BUSY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let reason = if !root.is_absolute() || meta(root).is_err() {
        Some("not_package")
    } else if !managed {
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
                .get(registry)
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
        let mut c = command(&env::var("PIMATE_PI").unwrap_or_else(|_| "pi".into()))?;
        c.current_dir(home()?)
            .args(["update", "npm:@phni3j9a/pimate"]);
        let (code, out, err) = output(c, 600, 1 << 20).await?;
        ensure!(
            code == Some(0),
            "{}",
            if err.is_empty() { out } else { err }
        );
        let m = meta(root)?;
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
#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, process::Command};
    #[tokio::test]
    async fn fake_update() -> Result<()> {
        if let Some(root) = env::var_os("PIMATE_UPDATE_FIXTURE") {
            let root = PathBuf::from(root);
            let url = &env::var("PIMATE_UPDATE_REGISTRY")?;
            let old = env!("CARGO_PKG_VERSION");
            let check = run_in(true, &root, true, url).await;
            assert_eq!(
                check,
                json!({"current":old,"latest":"2.0.0","canUpdate":true})
            );
            assert_eq!(
                run_in(false, &root, false, url).await["message"],
                "not_service"
            );
            assert_eq!(
                run_in(false, &root, true, url).await,
                json!({"ok":true,"from":old,"to":"2.0.0","restarting":true})
            );
            assert_eq!(run_in(false, &root, true, url).await["code"], "busy");
            for _ in 0..100 {
                if root.join("restart").exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            assert_eq!(
                fs::read_to_string(root.join("restart"))?,
                "--user restart --no-block pimate-host.service\n"
            );
            assert_eq!(
                fs::read_to_string(root.join("args"))?,
                "update\nnpm:@phni3j9a/pimate\n"
            );
            return Ok(());
        }
        let t = tempfile::tempdir()?;
        let root = t.path();
        fs::write(
            root.join("package.json"),
            json!({"name":NAME,"version":"2.0.0"}).to_string(),
        )?;
        for (k, s) in [
            ("pi", "printf '%s\\n' \"$@\" > args"),
            (
                "systemctl",
                "[ \"$2\" = restart ] || exit 0; echo \"$*\" > \"$HOME/restart\"",
            ),
        ] {
            let p = root.join(k);
            fs::write(&p, format!("#!/bin/sh\n{s}\n"))?;
            fs::set_permissions(p, fs::Permissions::from_mode(0o700))?
        }
        let units = root.join("xdg/systemd/user");
        fs::create_dir_all(&units)?;
        fs::write(
            units.join("pimate-host.service"),
            format!(
                "# Managed by PiMate (Rust)\nEnvironment=\"PIMATE_CONFIG_DIR={}\"\n",
                root.join("config").display()
            ),
        )?;
        let l = std::net::TcpListener::bind("127.0.0.1:0")?;
        let addr = l.local_addr()?;
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            if let Ok((mut sock, _)) = l.accept() {
                let _ = sock.read(&mut [0; 2048]);
                let _ = sock.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 19\r\n\r\n{\"version\":\"2.0.0\"}",
                );
            }
        });
        let status = Command::new(env::current_exe()?)
            .args(["--exact", "update::tests::fake_update"])
            .env_clear()
            .env("HOME", root)
            .env("PIMATE_RUN_DIR", root.join("run"))
            .env("PIMATE_CONFIG_DIR", root.join("config"))
            .env("PI_CODING_AGENT_DIR", root.join("agent"))
            .env("XDG_CONFIG_HOME", root.join("xdg"))
            .env("PATH", root)
            .env("PIMATE_UPDATE_REGISTRY", format!("http://{addr}"))
            .env("PIMATE_UPDATE_FIXTURE", root)
            .status()?;
        ensure!(status.success());
        Ok(())
    }
}
