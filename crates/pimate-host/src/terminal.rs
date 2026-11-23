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
    io::Write,
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
fn extension_arg(ext: PathBuf) -> std::io::Result<PathBuf> {
    if ext
        .to_str()
        .and_then(|s| s.split_once(':'))
        .is_some_and(|(scheme, _)| {
            scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
        })
    {
        Ok(ext)
    } else {
        std::path::absolute(ext)
    }
}
impl Terminal {
    pub fn resolve() -> Self {
        if let Some(p) = env::var_os("PIMATE_ABDUCO").filter(|s| !s.is_empty()) {
            return Self(executable(Path::new(&p)));
        }
        Self::fallback(cfg!(all(target_os = "linux", target_arch = "x86_64")))
    }
    fn fallback(supported: bool) -> Self {
        let bundled = supported
            .then(env::current_exe)
            .and_then(Result::ok)
            .and_then(|p| Some(p.parent()?.join("abduco")))
            .and_then(|p| executable(&p));
        if let Some(ref p) = bundled
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
        ensure!(output.status.success(), "abduco list failed");
        Ok(parse(&String::from_utf8_lossy(&output.stdout)))
    }
    pub async fn refresh(&self, run: &Path, instances: &mut [serde_json::Value]) {
        // Snapshot records first: never prune a launch that followed the abduco listing.
        let records: Vec<_> = fs::read_dir(run.join("terminals"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                if !managed(&name) {
                    return None;
                }
                let pid = fs::read_to_string(e.path()).ok()?.parse::<i32>().ok()?;
                Some((e.path(), name, pid))
            })
            .collect();
        let Ok(sessions) = self.sessions().await else {
            return;
        };
        for (record, name, pid) in records {
            match sessions.iter().find(|s| s.name == name && s.pid == pid) {
                Some(s) if s.terminated => {
                    let _ = kill(Pid::from_raw(s.pid), Signal::SIGTERM);
                }
                Some(_) => (),
                None => {
                    let _ = fs::remove_file(record);
                }
            }
        }
        for s in sessions {
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
        let mut c = if let Some(runner) = user_scope_runner().await {
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
            c.arg("-e").arg(extension_arg(ext)?);
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
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            c.pre_exec(|| {
                nix::unistd::setsid()?;
                Ok(())
            });
        }
        let records = run.join("terminals");
        crate::config::private(&records)?;
        let mut record = tempfile::NamedTempFile::new_in(&records)?;
        ensure!(c.status().await?.success(), "Pi launcher failed");
        // Only claim a successfully launched, identifiable keeper. Never retry a launch
        // merely because its keeper exited before listing or its record could not be saved.
        if let Err(e) = async {
            let s = self
                .sessions()
                .await?
                .into_iter()
                .find(|s| s.name == name)
                .context("keeper not listed")?;
            write!(record, "{}", s.pid)?;
            record.as_file().sync_all()?;
            record.persist(records.join(&name))?;
            Ok::<_, anyhow::Error>(())
        }
        .await
        {
            eprintln!("Could not record {name} for cleanup: {e}");
        }
        Ok((id, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_platform_keeps_sibling_on_path() -> Result<()> {
        if env::var_os("PIMATE_TEST_UNBUNDLED").is_some() {
            assert_eq!(
                Terminal::fallback(false).0,
                Some(env::current_exe()?.with_file_name("abduco"))
            );
            return Ok(());
        }
        let t = tempfile::tempdir()?;
        let exe = t.path().join("host-test");
        fs::copy(env::current_exe()?, &exe)?;
        fs::copy("/bin/true", t.path().join("abduco"))?;
        let status = std::process::Command::new(exe)
            .env_clear()
            .env("HOME", t.path())
            .env("PATH", t.path())
            .env("PIMATE_TEST_UNBUNDLED", "1")
            .env("ABDUCO_SOCKET_DIR", t.path())
            .env("PIMATE_RUN_DIR", t.path().join("run"))
            .env("PIMATE_CONFIG_DIR", t.path().join("config"))
            .env("PI_CODING_AGENT_DIR", t.path().join("agent"))
            .args([
                "--exact",
                "terminal::tests::unsupported_platform_keeps_sibling_on_path",
            ])
            .status()?;
        assert!(status.success());
        Ok(())
    }
    #[test]
    fn extension_package_or_relative_file() -> Result<()> {
        for src in [
            "npm:@name/pkg",
            "git:example.git",
            "https://example.invalid/a",
            "git+ssh:repo",
        ] {
            assert_eq!(extension_arg(src.into())?, Path::new(src));
        }
        assert_eq!(
            extension_arg("extensions/login.ts".into())?,
            env::current_dir()?.join("extensions/login.ts")
        );
        Ok(())
    }
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
