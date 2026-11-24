//! Terminal keeper boundary: only this module knows the launcher and its list format.
use anyhow::{Context, Result, ensure};
use futures_util::{StreamExt, stream};
use nix::unistd::{AccessFlags, access};
use serde::Serialize;
use std::{
    env, fs,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;
use tokio::{process::Command, time::timeout};
use uuid::Uuid;

pub(crate) fn executable(name: &Path) -> Option<PathBuf> {
    let dirs = if name.components().count() > 1 || name.is_absolute() {
        vec![PathBuf::new()]
    } else {
        env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect()
    };
    dirs.into_iter()
        .map(|d| d.join(name))
        .find(|p| p.metadata().is_ok_and(|m| m.is_file()) && access(p, AccessFlags::X_OK).is_ok())
        .and_then(|p| std::path::absolute(p).ok())
}
pub struct Terminal;
#[derive(Serialize)]
struct Session {
    name: String,
    attached: bool,
    terminated: bool,
}
struct Sockets(PathBuf);
impl Sockets {
    fn open(run: &Path) -> Result<Self> {
        crate::config::private(run)?;
        let dir = std::path::absolute(run)?.join("t");
        crate::config::private(&dir)?;
        Ok(Self(dir))
    }
    fn path(&self, name: &str) -> Result<PathBuf> {
        ensure!(managed(name), "Unknown session");
        let path = self.0.join(name);
        nix::sys::socket::UnixAddr::new(&path)
            .context("Terminal socket path too long: choose a shorter PIMATE_RUN_DIR")?;
        Ok(path)
    }
    async fn sessions(&self) -> Result<Vec<Session>> {
        let candidates = fs::read_dir(&self.0)?.flatten().filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !managed(&name) {
                return None;
            }
            let path = entry.path();
            let meta = path.symlink_metadata().ok()?;
            meta.file_type().is_socket().then_some((name, path))
        });
        // An unresponsive keeper is not proof of death: never unlink its socket here.
        Ok(stream::iter(candidates)
            .map(|(name, path)| async move {
                Some(Session {
                    name,
                    attached: crate::keeper::probe(&path).await.ok()?,
                    terminated: false,
                })
            })
            .buffer_unordered(16)
            .filter_map(std::future::ready)
            .collect()
            .await)
    }
}
fn validate_rc_path(run: &Path, id: &str) -> Result<()> {
    let p = std::path::absolute(run)?.join(format!("{id}.sock"));
    nix::sys::socket::UnixAddr::new(&p)
        .context("RC socket path too long: choose a shorter PIMATE_RUN_DIR")?;
    Ok(())
}
fn managed(s: &str) -> bool {
    s.strip_prefix("pimate-").is_some_and(|v| {
        v.len() == 8
            && v.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
async fn user_scope_runner() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let runner = executable(Path::new("systemd-run"))?;
    let ctl = executable(Path::new("systemctl"))?;
    let mut probe = Command::new(ctl);
    probe
        .args(["--user", "show-environment"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    timeout(Duration::from_secs(2), probe.status())
        .await
        .ok()?
        .ok()?
        .success()
        .then_some(runner)
}
fn extension_arg(ext: PathBuf) -> Result<PathBuf> {
    if let Some(s) = ext.to_str() {
        if [
            "npm:", "git:", "github:", "http:", "https:", "ssh:", "builtin:",
        ]
        .iter()
        .any(|p| s.trim().starts_with(p))
        {
            return Ok(ext);
        }
        if s.starts_with("file://") {
            return Ok(ext);
        }
        if s == "~" || s.starts_with("~/") {
            return Ok(
                crate::operations::home()?.join(s.trim_start_matches('~').trim_start_matches('/'))
            );
        }
    }
    Ok(std::path::absolute(ext)?)
}
impl Terminal {
    pub async fn refresh(&self, run: &Path, instances: &mut [serde_json::Value]) {
        let Ok(ns) = Sockets::open(run) else { return };
        let Ok(sessions) = ns.sessions().await else {
            return;
        };
        for s in sessions {
            for i in instances
                .iter_mut()
                .filter(|i| i["abducoSession"] == s.name)
            {
                i["pcAttached"] = s.attached.into();
            }
        }
    }
    pub async fn attach(&self, run: &Path, name: Option<String>) -> Result<u8> {
        let ns = Sockets::open(run)?;
        if let Some(n) = name {
            let path = ns.path(&n)?;
            ensure!(
                path.symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_socket()),
                "Unknown session"
            );
            eprintln!("Detach: Ctrl+\\ (Pi keeps running)");
            return crate::keeper::attach(&path).await;
        }
        println!("{}", serde_json::to_string_pretty(&ns.sessions().await?)?);
        Ok(0)
    }
    pub async fn launch(
        &self,
        run: &Path,
        cwd: &Path,
        trust: bool,
        origin: &str,
        args: &[String],
    ) -> Result<(String, String, &'static str)> {
        let keeper = env::current_exe()?;
        let pi = executable(Path::new(
            &env::var_os("PIMATE_PI").unwrap_or_else(|| "pi".into()),
        ))
        .context("pi not found")?;
        let id = Uuid::new_v4().to_string();
        validate_rc_path(run, &id)?;
        let name = format!("pimate-{}", &id[..8]);
        let ns = Sockets::open(run)?;
        let path = ns.path(&name)?;
        let runner = user_scope_runner().await;
        let isolation = if runner.is_some() {
            "systemd-scope"
        } else {
            "detached"
        };
        let mut c = if let Some(runner) = runner {
            let mut c = Command::new(&runner);
            let help = timeout(
                Duration::from_secs(2),
                Command::new(&runner)
                    .arg("--help")
                    .kill_on_drop(true)
                    .output(),
            )
            .await??;
            if String::from_utf8_lossy(&help.stdout).contains("--expand-environment=") {
                c.arg("--expand-environment=no");
            }
            c.args([
                "--user",
                "--scope",
                "--quiet",
                &format!("--unit={name}"),
                "--",
            ])
            .arg(&keeper);
            c
        } else {
            Command::new(keeper)
        };
        c.arg("__hold").arg(path).arg(pi);
        let ext = env::var_os("PIMATE_EXTENSION")
            .map(PathBuf::from)
            .unwrap_or(
                env::current_exe()?
                    .parent()
                    .context("bin directory")?
                    .join("../../dist/extension.js"),
            );
        if ext != Path::new("none") {
            c.arg("-e").arg(extension_arg(ext)?);
        }
        c.args(args)
            .env("PIMATE_INSTANCE_ID", &id)
            .env("PIMATE_RUN_DIR", std::path::absolute(run)?)
            .env(
                "PIMATE_ORIGIN",
                if origin == "auth" { "mobile" } else { origin },
            )
            .env("PIMATE_TRUST", if trust { "yes" } else { "no" })
            .env("PIMATE_ABDUCO_SESSION", &name)
            .env_remove("PIMATE_PURPOSE");
        if origin == "auth" {
            c.env("PIMATE_PURPOSE", "auth");
        }
        // Pin relative entries for child interpreters too (e.g. Pi's env-node shebang).
        if let Some(path) = env::var_os("PATH") {
            let host_cwd = env::current_dir()?;
            c.env(
                "PATH",
                env::join_paths(env::split_paths(&path).map(|entry| host_cwd.join(entry)))?,
            );
        }
        c.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        unsafe {
            c.pre_exec(|| {
                nix::unistd::setsid()?;
                Ok(())
            });
        }
        let mut child = c.spawn()?;
        let mut ready = child.stdout.take().unwrap();
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        ensure!(
            timeout(Duration::from_secs(8), ready.read_u8()).await?? == b'R',
            "Pi launcher failed"
        );
        Ok((id, name, isolation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extension_package_or_relative_file() -> Result<()> {
        for src in [
            "npm:@name/pkg",
            "git:example.git",
            "https://example.invalid/a",
            "http://example.invalid/a",
            "ssh:repo",
            "builtin:foo",
            "github:org/repo",
            "file:///tmp/a%2Fb.ts",
            "file:///tmp/space%20name",
        ] {
            assert_eq!(extension_arg(src.into())?, Path::new(src));
        }
        for src in ["extensions/login.ts", "team:rc.ts", "git+ssh:repo", "NPM:x"] {
            assert_eq!(extension_arg(src.into())?, env::current_dir()?.join(src));
        }
        Ok(())
    }
    #[test]
    fn rc_socket_includes_uuid_and_extension() -> Result<()> {
        let limit = if cfg!(target_os = "macos") { 104 } else { 108 };
        let id = Uuid::new_v4().to_string();
        let run = PathBuf::from(format!("/{}", "a".repeat(limit - id.len() - 6 - 1)));
        assert!(nix::sys::socket::UnixAddr::new(&run.join("t/pimate-1234abcd@pc")).is_ok());
        assert!(
            validate_rc_path(&run, &id)
                .unwrap_err()
                .to_string()
                .contains("RC socket path too long")
        );
        Ok(())
    }
    #[test]
    fn reject_long_socket_path_before_launch() -> Result<()> {
        let t = tempfile::tempdir()?;
        let run = t.path().join("あ".repeat(34));
        let msg = Sockets::open(&run)?
            .path("pimate-00000000")
            .err()
            .context("accepted long path")?
            .to_string();
        assert!(msg.contains("shorter PIMATE_RUN_DIR"));
        Ok(())
    }
}
