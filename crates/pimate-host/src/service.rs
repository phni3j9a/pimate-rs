use crate::{
    config::{self, Config},
    operations::home,
    terminal::executable,
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::Write,
    os::unix::{
        fs::{OpenOptionsExt, symlink},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};
pub const SERVICE: &str = "pimate-host.service";
const MARKER: &str = "# Managed by PiMate (Rust)\n";
pub fn command(name: &str) -> Result<Command> {
    let mut c =
        Command::new(executable(Path::new(name)).with_context(|| format!("{name} not found"))?);
    c.kill_on_drop(true).stdin(Stdio::null());
    Ok(c)
}
async fn ctl(args: &[&str]) -> Result<String> {
    let o = timeout(
        Duration::from_secs(15),
        command("systemctl")?.arg("--user").args(args).output(),
    )
    .await??;
    ensure!(
        o.status.success(),
        "systemctl failed: {}",
        String::from_utf8_lossy(&o.stderr).trim()
    );
    Ok(String::from_utf8(o.stdout)?.trim().into())
}
pub async fn restart() -> Result<()> {
    ctl(&["restart", "--no-block", SERVICE]).await?;
    Ok(())
}
pub fn managed() -> bool {
    cfg!(target_os = "linux")
        && fs::read_to_string("/proc/self/cgroup")
            .is_ok_and(|s| s.trim_end().ends_with("/pimate-host.service"))
}
fn quote(s: &str) -> Result<String> {
    ensure!(!s.contains(['\n', '\r', '\0']), "invalid unit value");
    Ok(format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    ))
}
fn abs(p: PathBuf) -> Result<String> {
    Ok(std::path::absolute(p)?
        .to_str()
        .context("non UTF-8 path")?
        .into())
}
fn unit_path() -> Result<PathBuf> {
    Ok(env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".config"))
        .join("systemd/user")
        .join(SERVICE))
}
fn config_line() -> Result<String> {
    Ok(format!(
        "Environment={}\n",
        quote(&format!(
            "PIMATE_CONFIG_DIR={}",
            abs(config::config_dir()?)?
        ))?
    ))
}
fn port() -> Result<u16> {
    Ok(env::var("PIMATE_SERVICE_PORT")
        .map(|s| s.parse())
        .unwrap_or(Ok(pimate_protocol::DEFAULT_HOST_PORT))?)
}
fn unit() -> Result<String> {
    let mut s = format!(
        "{MARKER}[Unit]\nDescription=PiMate Host\n\n[Service]\nType=simple\nExecStart={} serve --port {}\nRestart=on-failure\nRestartSec=3\nUMask=0077\n{}",
        quote(&abs(env::current_exe()?)?)?,
        port()?,
        config_line()?
    );
    for k in [
        "PATH",
        "PIMATE_RUN_DIR",
        "PIMATE_ABDUCO",
        "PIMATE_PI",
        "PIMATE_EXTENSION",
        "PI_CODING_AGENT_DIR",
        "XDG_CONFIG_HOME",
        "XDG_RUNTIME_DIR",
    ] {
        if let Ok(v) = env::var(k)
            && !v.is_empty()
        {
            let v = if k == "PATH" {
                env::join_paths(
                    env::split_paths(&v)
                        .map(std::path::absolute)
                        .collect::<std::io::Result<Vec<_>>>()?,
                )?
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid PATH"))?
            } else if matches!(k, "PIMATE_ABDUCO" | "PIMATE_PI") && !v.contains('/')
                || k == "PIMATE_EXTENSION"
                    && (v == "none"
                        || v.split_once(':').is_some_and(|(p, _)| {
                            p.bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b"+.-".contains(&c))
                        }))
            {
                v
            } else if k == "PI_CODING_AGENT_DIR" && (v == "~" || v.starts_with("~/")) {
                abs(home()?.join(v.trim_start_matches('~').trim_start_matches('/')))?
            } else {
                abs(v.into())?
            };
            s.push_str(&format!("Environment={}\n", quote(&format!("{k}={v}"))?))
        }
    }
    s.push_str("\n[Install]\nWantedBy=default.target\n");
    Ok(s)
}
fn owned(path: &Path) -> Result<bool> {
    match path.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        m => {
            ensure!(m?.is_file(), "unmanaged unit");
            let s = fs::read_to_string(path)?;
            ensure!(
                s.starts_with(MARKER) && s.contains(&format!("\n{}", config_line()?)),
                "unmanaged unit or different config directory"
            );
            Ok(true)
        }
    }
}
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    tmp.write_all(bytes)?;
    tmp.persist(path)?;
    Ok(())
}
async fn health() -> Result<Option<Value>> {
    let c = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(1500))
        .build()?;
    let r = match c
        .get(format!("http://127.0.0.1:{}/health", port()?))
        .send()
        .await
    {
        Err(e) if e.is_connect() => return Ok(None),
        v => v?,
    };
    let h: Value = r.error_for_status()?.json().await?;
    ensure!(
        h["ok"] == true && h["protocol"].is_number(),
        "foreign service on Host port"
    );
    Ok(Some(h))
}
async fn ours(c: &Config) -> Result<Option<Value>> {
    let h = health().await?;
    ensure!(
        h.as_ref().is_none_or(|h| h["hostId"] == c.host.host_id),
        "foreign Host on port"
    );
    Ok(h)
}
async fn ready(c: &Config, want: bool) -> Result<()> {
    for _ in 0..40 {
        if ours(c).await?.is_some() == want {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await
    }
    bail!("Host did not reach requested state")
}
fn local_command(action: &str) -> Result<Value> {
    let xdg = |s| {
        env::var_os(s)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let dir = xdg("XDG_BIN_HOME")
        .or_else(|| Some(xdg("XDG_DATA_HOME")?.parent()?.join("bin")))
        .unwrap_or(home()?.join(".local/bin"));
    let path = dir.join("pimate");
    let exe = env::current_exe()?;
    let linked = fs::read_link(&path).is_ok_and(|p| p == exe);
    let conflict = path.symlink_metadata().is_ok() && !linked;
    match action {
        "command-status" => Ok(
            json!({"dir":dir,"path":path,"linked":linked,"conflict":conflict,"onPath":env::split_paths(&env::var_os("PATH").unwrap_or_default()).any(|p|std::path::absolute(p).ok().as_ref()==Some(&dir))}),
        ),
        "command-install" => {
            if !linked {
                fs::create_dir_all(dir)?;
                symlink(exe, path)?
            }
            Ok(json!({"ok":true}))
        }
        "command-remove" => {
            if linked {
                fs::remove_file(path)?
            }
            Ok(json!({"removed":linked}))
        }
        _ => bail!("invalid command action"),
    }
}
pub async fn run(action: &str) -> Result<Value> {
    if action.starts_with("command-") {
        return local_command(action);
    }
    if action == "health" {
        return Ok(health().await?.unwrap_or_default());
    }
    let path = unit_path()?;
    let systemd = cfg!(target_os = "linux") && ctl(&["show-environment"]).await.is_ok();
    if action == "has-systemd" {
        return Ok(json!(systemd));
    }
    if action == "status" {
        let mut linger = "unknown (no change made)".to_owned();
        if systemd
            && let Ok(mut cmd) = command("loginctl")
            && let Ok(Ok(o)) = timeout(
                Duration::from_secs(3),
                cmd.args([
                    "show-user",
                    &nix::unistd::Uid::current().to_string(),
                    "-p",
                    "Linger",
                    "--value",
                ])
                .output(),
            )
            .await
            && o.status.success()
        {
            linger = String::from_utf8_lossy(&o.stdout).trim().into()
        }
        return Ok(
            json!({"installed":path.exists(),"active":systemd&&ctl(&["is-active",SERVICE]).await.is_ok_and(|s|s=="active"),"linger":linger}),
        );
    }
    let installed = owned(&path)?;
    let dir = config::config_dir()?;
    let c = Config::open(dir.clone())?;
    let h = ours(&c).await?;
    let state = dir.join("host-process.json");
    match action {
        "install" | "uninstall" | "stop" => {
            if action == "install" {
                ensure!(systemd, "no systemd user manager")
            }
            if installed {
                ctl(&["stop", SERVICE]).await?;
            }
            if state.try_exists()? {
                let saved: Value = serde_json::from_slice(&fs::read(&state)?)?;
                if let Some(pid) = saved["pid"]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .filter(|n| *n > 1)
                    && h.as_ref().is_some_and(|h| h["pid"] == pid)
                {
                    nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGTERM,
                    )?;
                }
                fs::remove_file(&state)?;
            }
            ready(&c, false).await?;
            if action == "install" {
                fs::create_dir_all(path.parent().unwrap())?;
                write(&path, unit()?.as_bytes())?;
                ctl(&["daemon-reload"]).await?;
                ctl(&["enable", "--now", SERVICE]).await?;
                ready(&c, true).await?
            } else if action == "uninstall" && installed {
                ctl(&["disable", SERVICE]).await?;
                fs::remove_file(path)?;
                ctl(&["daemon-reload"]).await?;
            }
        }
        "start" => {
            if h.is_none() {
                if installed {
                    ctl(&["start", SERVICE]).await?;
                } else {
                    let log = fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .mode(0o600)
                        .open(dir.join("host.log"))?;
                    let child = std::process::Command::new(env::current_exe()?)
                        .args(["serve", "--port", &port()?.to_string()])
                        .stdin(Stdio::null())
                        .stdout(log.try_clone()?)
                        .stderr(log)
                        .process_group(0)
                        .spawn()?;
                    write(&state, json!({"pid":child.id()}).to_string().as_bytes())?;
                }
                ready(&c, true).await?
            }
        }
        _ => bail!("unknown service action"),
    };
    Ok(json!({"ok":true}))
}
