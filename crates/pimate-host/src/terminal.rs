//! Terminal keeper boundary: only this module knows the launcher and its list format.
use anyhow::{Context, Result, ensure};
use nix::{
    sys::signal::{Signal, kill},
    unistd::{AccessFlags, Pid, access},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::timeout};
use uuid::Uuid;

fn executable(name: &Path) -> Option<PathBuf> {
    search(name, None)
}
fn search(name: &Path, rejected: Option<&fs::Metadata>) -> Option<PathBuf> {
    let dirs = if name.components().count() > 1 || name.is_absolute() {
        vec![PathBuf::new()]
    } else {
        env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect()
    };
    dirs.into_iter()
        .map(|d| d.join(name))
        .find(|p| {
            p.metadata().is_ok_and(|m| {
                m.is_file()
                    && rejected.is_none_or(|bad| (m.dev(), m.ino()) != (bad.dev(), bad.ino()))
            }) && access(p, AccessFlags::X_OK).is_ok()
        })
        .and_then(|p| std::path::absolute(p).ok())
}
#[derive(Default)]
pub struct Terminal(pub Option<PathBuf>);
#[derive(Serialize)]
pub struct Session {
    pub name: String,
    pid: i32,
    pub attached: bool,
    terminated: bool,
}
fn parse(s: &str) -> Vec<Session> {
    s.lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let mark = cols.next()?.trim();
            cols.next()?;
            let pid = cols.next()?.trim().parse().ok()?;
            let name = cols.next()?.trim();
            (pid > 0 && !name.is_empty()).then(|| Session {
                name: name.into(),
                pid,
                attached: mark.starts_with('*'),
                terminated: mark.starts_with('+'),
            })
        })
        .collect()
}
fn managed(s: &str) -> bool {
    s.strip_prefix("pimate-").is_some_and(|v| {
        v.len() == 8
            && v.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
impl Terminal {
    pub fn resolve() -> Self {
        if let Some(p) = env::var_os("PIMATE_ABDUCO").filter(|s| !s.is_empty()) {
            return Self(executable(Path::new(&p)));
        }
        let bundled = env::current_exe()
            .ok()
            .and_then(|p| Some(p.parent()?.join("abduco")))
            .and_then(|p| executable(&p));
        if cfg!(all(target_os = "linux", target_arch = "x86_64"))
            && let Some(ref p) = bundled
            && let Ok(b) = fs::read(p)
        {
            let sums: serde_json::Value =
                serde_json::from_str(include_str!("../../../bin/SHA256SUMS.json")).unwrap();
            if sums["bin/linux-x64/abduco"] == format!("{:x}", Sha256::digest(b)) {
                return Self(Some(p.clone()));
            }
        }
        eprintln!("Bundled abduco unavailable or checksum mismatch; trying PATH");
        let rejected = bundled.and_then(|p| p.metadata().ok());
        Self(search(Path::new("abduco"), rejected.as_ref()))
    }
    pub async fn sessions(&self) -> Result<Vec<Session>> {
        let output = timeout(
            Duration::from_secs(2),
            Command::new(self.0.as_ref().context("abduco not found")?)
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        Ok(parse(&String::from_utf8_lossy(&output.stdout)))
    }
    pub async fn refresh(&self, instances: &mut [serde_json::Value]) {
        for s in self.sessions().await.unwrap_or_default() {
            if s.terminated && managed(&s.name) {
                let _ = kill(Pid::from_raw(s.pid), Signal::SIGTERM);
            }
            for i in instances
                .iter_mut()
                .filter(|i| i["abducoSession"] == s.name)
            {
                i["pcAttached"] = s.attached.into();
            }
        }
    }
    pub async fn attach(&self, name: Option<String>) -> Result<()> {
        let sessions = self.sessions().await?;
        if let Some(n) = name {
            ensure!(sessions.iter().any(|s| s.name == n), "Unknown session");
            eprintln!("Detach: Ctrl+\\ (Pi keeps running)");
            return Err(std::process::Command::new(self.0.as_ref().unwrap())
                .args(["-a", "--", &n])
                .exec()
                .into());
        }
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        Ok(())
    }
    pub async fn launch(
        &self,
        run: &Path,
        cwd: &Path,
        trust: bool,
        auth: bool,
        args: &[String],
    ) -> Result<(String, String)> {
        let ab = self.0.as_ref().context("abduco not found")?;
        let pi = executable(Path::new(
            &env::var_os("PIMATE_PI").unwrap_or_else(|| "pi".into()),
        ))
        .context("pi not found")?;
        let id = Uuid::new_v4().to_string();
        let name = format!("pimate-{}", &id[..8]);
        let mut probe = Command::new("systemctl");
        probe
            .args(["--user", "show-environment"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let scoped = cfg!(target_os = "linux")
            && executable(Path::new("systemd-run")).is_some()
            && matches!(timeout(Duration::from_secs(2),probe.status()).await,Ok(Ok(s)) if s.success());
        let mut c = if scoped {
            let mut c = Command::new("systemd-run");
            // Older systemd never expands scope argv; newer versions offer an explicit opt-out.
            let help = Command::new("systemd-run").arg("--help").output().await?;
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
            .arg(ab);
            c
        } else {
            Command::new(ab)
        };
        c.args(["-n", &name]).arg(pi);
        let ext = env::var_os("PIMATE_EXTENSION")
            .map(PathBuf::from)
            .unwrap_or(
                env::current_exe()?
                    .parent()
                    .context("bin directory")?
                    .join("../../dist/extension.js"),
            );
        if ext != Path::new("none") {
            c.arg("-e").arg(std::path::absolute(ext)?);
        }
        c.args(args)
            .env("PIMATE_INSTANCE_ID", &id)
            .env("PIMATE_RUN_DIR", std::path::absolute(run)?)
            .env("PIMATE_ORIGIN", "mobile")
            .env("PIMATE_TRUST", if trust { "yes" } else { "no" })
            .env("PIMATE_ABDUCO_SESSION", &name)
            .env_remove("PIMATE_PURPOSE");
        if auth {
            c.env("PIMATE_PURPOSE", "auth");
        }
        c.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            c.pre_exec(|| {
                nix::unistd::setsid()?;
                Ok(())
            });
        }
        ensure!(c.status().await?.success(), "Pi launcher failed");
        Ok((id, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_list_markers_and_cleanup_boundary() {
        let sessions = parse(
            "Active sessions (on host test)\n* Mon\t 2026-11-23 00:00:00\t100\tpimate-0123abcd\n+ Mon\t 2026-11-23 00:01:00\t101\tother\n+ invalid\tdate\t-1\tpimate-deadbeef",
        );
        assert_eq!(sessions.len(), 2);
        assert!(sessions[0].attached && !sessions[0].terminated);
        assert!(sessions[1].terminated && !managed(&sessions[1].name));
        assert!(managed(&sessions[0].name));
        assert!(!managed("pimate-0123abcd@other-host"));
    }
}
