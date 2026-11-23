//! Terminal keeper boundary: only this module knows the launcher and its list format.
use anyhow::{Context, Result, ensure};
use nix::unistd::{AccessFlags, Uid, User, access};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        process::CommandExt,
    },
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
    pid: Option<i32>,
    #[serde(skip)]
    path: PathBuf,
    pub attached: bool,
    terminated: bool,
}
// abduco's list searches $ABDUCO_SOCKET_DIR/<argv[0] basename>/<user>.
// Pin argv[0] for listing even if the configured executable has another name.
const LIST_NAME: &str = "pt";
struct Sockets {
    dir: PathBuf,
    files: PathBuf,
    hostname: String,
}
impl Sockets {
    fn open(run: &Path) -> Result<Self> {
        crate::config::private(run)?;
        let dir = std::path::absolute(run)?.join("t");
        crate::config::private(&dir)?;
        let parent = dir.join(LIST_NAME);
        crate::config::private(&parent)?;
        let uid = Uid::current();
        let user = User::from_uid(uid)?
            .map(|u| u.name)
            .unwrap_or_else(|| uid.to_string());
        ensure!(
            !user.is_empty() && Path::new(&user).file_name() == Some(user.as_ref()),
            "invalid username"
        );
        let files = parent.join(user);
        crate::config::private(&files)?;
        let hostname = nix::unistd::gethostname()?
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid hostname"))?;
        ensure!(!hostname.contains('/'), "invalid hostname");
        Ok(Self {
            dir,
            files,
            hostname,
        })
    }
    fn path(&self, name: &str) -> Result<PathBuf> {
        ensure!(managed(name), "Unknown session");
        let p = self.files.join(format!("{name}@{}", self.hostname));
        nix::sys::socket::UnixAddr::new(&p)
            .context("Terminal socket path too long: choose a shorter PIMATE_RUN_DIR")?;
        Ok(p)
    }
    async fn sessions(&self, ab: &Path) -> Result<Vec<Session>> {
        if fs::read_dir(&self.files)?.next().is_none() {
            return Ok(vec![]);
        }
        // Do not ask abduco to search its fallback directories if this one is unusable.
        access(&self.files, AccessFlags::W_OK | AccessFlags::X_OK)?;
        let mut cmd = Command::new(ab);
        cmd.as_std_mut().arg0(LIST_NAME);
        let output = timeout(
            Duration::from_secs(2),
            cmd.env("ABDUCO_SOCKET_DIR", &self.dir)
                .env("HOME", &self.dir)
                .env("TMPDIR", &self.dir)
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        ensure!(output.status.success(), "abduco list failed");
        let mut rows: Vec<_> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let (name, pid, mark) = parse_list(line)?;
                let path = self.path(name).ok()?;
                path.symlink_metadata()
                    .ok()
                    .filter(|m| m.file_type().is_socket())?;
                Some(Session {
                    name: name.into(),
                    pid,
                    path,
                    attached: mark == Some('*'),
                    terminated: mark == Some('+'),
                })
            })
            .collect();
        // Native list filters by the current hostname. Old sockets still accept their absolute path.
        for e in fs::read_dir(&self.files)?.flatten() {
            let file = e.file_name();
            let Some((name, host)) = file.to_str().and_then(|s| s.split_once('@')) else {
                continue;
            };
            if !managed(name) || host.is_empty() || self.hostname == host {
                continue;
            }
            let path = e.path();
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if !meta.file_type().is_socket() {
                continue;
            }
            match timeout(
                Duration::from_millis(200),
                tokio::net::UnixStream::connect(&path),
            )
            .await
            {
                Ok(Ok(_)) => rows.push(Session {
                    name: name.into(),
                    pid: None,
                    path,
                    attached: meta.mode() & 0o100 != 0,
                    terminated: meta.mode() & 0o010 != 0,
                }),
                Ok(Err(e))
                    if e.kind() == std::io::ErrorKind::ConnectionRefused
                        && path
                            .symlink_metadata()
                            .is_ok_and(|m| m.dev() == meta.dev() && m.ino() == meta.ino()) =>
                {
                    let _ = fs::remove_file(path);
                }
                _ => (),
            }
        }
        Ok(rows)
    }
}
// v0.6 has date/name; newer builds insert a PID before the name.
fn parse_list(line: &str) -> Option<(&str, Option<i32>, Option<char>)> {
    let cols: Vec<_> = line.split('\t').collect();
    let (name, pid) = match cols.as_slice() {
        [_, _, name] => (*name, None),
        [_, _, pid, name] => (*name, Some(pid.trim().parse().ok()?)),
        _ => return None,
    };
    let name = name.trim();
    managed(name).then_some((name, pid, line.trim_start().chars().next()))
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
    pub async fn refresh(&self, run: &Path, instances: &mut [serde_json::Value]) {
        let Some(ab) = &self.0 else { return };
        let Ok(ns) = Sockets::open(run) else { return };
        let Ok(sessions) = ns.sessions(ab).await else {
            return;
        };
        for s in sessions {
            if s.terminated && !s.attached {
                // Consume the exit notification on this socket; never signal a PID supplied by a peer.
                if let Ok(mut child) = Command::new(ab)
                    .args(["-a", "--"])
                    .arg(&s.path)
                    .env("ABDUCO_SOCKET_DIR", &ns.dir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                {
                    let _input = child.stdin.take();
                    let _ = timeout(Duration::from_secs(2), child.wait()).await;
                }
            }
            for i in instances
                .iter_mut()
                .filter(|i| i["abducoSession"] == s.name)
            {
                i["pcAttached"] = s.attached.into();
            }
        }
    }
    pub async fn attach(&self, run: &Path, name: Option<String>) -> Result<()> {
        let ab = self.0.as_ref().context("abduco not found")?;
        let ns = Sockets::open(run)?;
        let sessions = ns.sessions(ab).await?;
        if let Some(n) = name {
            let mut matches = sessions.iter().filter(|s| s.name == n);
            let path = &matches.next().context("Unknown session")?.path;
            ensure!(matches.next().is_none(), "Ambiguous session name");
            eprintln!("Detach: Ctrl+\\ (Pi keeps running)");
            return Err(std::process::Command::new(ab)
                .args(["-a", "--"])
                .arg(path)
                .env("ABDUCO_SOCKET_DIR", &ns.dir)
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
        validate_rc_path(run, &id)?;
        let name = format!("pimate-{}", &id[..8]);
        let ns = Sockets::open(run)?;
        let path = ns.path(&name)?;
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
        c.arg("-n")
            .arg(path)
            .arg(pi)
            .env("ABDUCO_SOCKET_DIR", &ns.dir);
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
        ensure!(c.status().await?.success(), "Pi launcher failed");
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
