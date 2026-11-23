//! Terminal keeper boundary: only this module knows the launcher and its list format.
use anyhow::{Context, Result, ensure};
use nix::unistd::{AccessFlags, access};
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
pub struct Terminal {
    pub executable: Option<PathBuf>,
    launching: tokio::sync::RwLock<()>,
}
#[derive(Serialize)]
struct Session {
    #[serde(skip)]
    meta: fs::Metadata,
    name: String,
    #[serde(skip)]
    path: PathBuf,
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
    fn sessions(&self) -> Result<Vec<Session>> {
        Ok(fs::read_dir(&self.0)?
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                if !managed(&name) {
                    return None;
                }
                let path = entry.path();
                let meta = path.symlink_metadata().ok()?;
                meta.file_type().is_socket().then_some(Session {
                    name,
                    path,
                    attached: meta.mode() & 0o100 != 0,
                    terminated: meta.mode() & 0o010 != 0,
                    meta,
                })
            })
            .collect())
    }
}
async fn probe(path: &Path) -> std::io::Result<()> {
    timeout(
        Duration::from_millis(100),
        tokio::net::UnixStream::connect(path),
    )
    .await??;
    Ok(())
}
fn old_socket(m: &fs::Metadata) -> bool {
    m.file_type().is_socket()
        && m.modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= Duration::from_secs(30))
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
            return Self {
                executable: executable(Path::new(&p)),
                ..Default::default()
            };
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
                return Self {
                    executable: Some(p.clone()),
                    ..Default::default()
                };
            }
        }
        eprintln!("Bundled abduco unavailable or checksum mismatch; trying PATH");
        let rejected = bundled.and_then(|p| p.metadata().ok());
        Self {
            executable: search(Path::new("abduco"), rejected.as_ref()),
            ..Default::default()
        }
    }
    pub async fn refresh(&self, run: &Path, instances: &mut [serde_json::Value]) {
        let Some(ab) = &self.executable else { return };
        let Ok(ns) = Sockets::open(run) else { return };
        let Ok(sessions) = ns.sessions() else {
            return;
        };
        for s in sessions {
            if self.clean(ab, &s).await {
                continue;
            }
            for i in instances
                .iter_mut()
                .filter(|i| i["abducoSession"] == s.name)
            {
                i["pcAttached"] = s.attached.into();
            }
        }
    }
    // A fresh socket may still be between bind and listen in another Host process.
    async fn clean(&self, ab: &Path, s: &Session) -> bool {
        let exited = s.terminated && !s.attached;
        if !exited && !old_socket(&s.meta) {
            return false;
        }
        let Ok(_idle) = self.launching.try_write() else {
            return false;
        };
        match probe(&s.path).await {
            Err(e)
                if e.kind() == std::io::ErrorKind::ConnectionRefused
                    && old_socket(&s.meta)
                    && s.path.symlink_metadata().is_ok_and(|m| {
                        m.dev() == s.meta.dev() && m.ino() == s.meta.ino() && old_socket(&m)
                    }) =>
            {
                return fs::remove_file(&s.path).is_ok();
            }
            Ok(()) if exited => (),
            _ => return false,
        }
        // /dev/null delivers EOF before the bundled client acknowledges MSG_EXIT.
        if let Ok(mut child) = Command::new(ab)
            .args(["-a", "--"])
            .arg(&s.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            let _input = child.stdin.take();
            let _ = timeout(Duration::from_secs(2), child.wait()).await;
        }
        false
    }
    pub async fn attach(&self, run: &Path, name: Option<String>) -> Result<()> {
        let ab = self.executable.as_ref().context("abduco not found")?;
        let ns = Sockets::open(run)?;
        if let Some(n) = name {
            let path = ns.path(&n)?;
            ensure!(
                path.symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_socket()),
                "Unknown session"
            );
            probe(&path).await.context("Terminal is not listening")?;
            eprintln!("Detach: Ctrl+\\ (Pi keeps running)");
            return Err(std::process::Command::new(ab)
                .args(["-a", "--"])
                .arg(path)
                .exec()
                .into());
        }
        println!("{}", serde_json::to_string_pretty(&ns.sessions()?)?);
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
        let ab = self.executable.as_ref().context("abduco not found")?;
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
        c.arg("-n").arg(path).arg(pi);
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
        let _launching = self.launching.read().await;
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
                Terminal::fallback(false).executable,
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
    #[tokio::test]
    async fn skip_old_refused_socket_while_launch_in_progress() -> Result<()> {
        use nix::sys::{
            stat::{UtimensatFlags, utimensat},
            time::TimeSpec,
        };
        let t = tempfile::tempdir()?;
        let run = t.path().join("run");
        let ns = Sockets::open(&run)?;
        let path = ns.path("pimate-00000001")?;
        drop(std::os::unix::net::UnixListener::bind(&path)?);
        let old = TimeSpec::new(1, 0);
        utimensat(
            fs::File::open(&run)?,
            &path,
            &old,
            &old,
            UtimensatFlags::NoFollowSymlink,
        )?;
        let term = Terminal {
            executable: Some("/bin/true".into()),
            ..Default::default()
        };
        let active = term.launching.read().await;
        term.refresh(&run, &mut []).await;
        assert!(path.exists());
        drop(active);
        term.refresh(&run, &mut []).await;
        assert!(!path.exists());
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
