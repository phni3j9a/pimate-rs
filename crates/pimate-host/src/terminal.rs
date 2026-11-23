//! Terminal keeper boundary: only this module knows the launcher and its list format.
use anyhow::{Context, Result, ensure};
use futures_util::{StreamExt, stream};
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
use tokio::{io::AsyncReadExt, process::Command, time::timeout};
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
// Explicit absolute socket names bypass abduco's fallback to HOME and /tmp.
struct Sockets {
    dir: PathBuf,
    suffix: String,
}
impl Sockets {
    fn open(run: &Path) -> Result<Self> {
        crate::config::private(run)?;
        let dir = std::path::absolute(run)?.join("t");
        crate::config::private(&dir)?;
        let hostname = nix::unistd::gethostname()?
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid hostname"))?;
        ensure!(!hostname.contains('/'), "invalid hostname");
        let ns = Self {
            dir,
            suffix: format!("@{hostname}"),
        };
        ns.path("pimate-00000000")?;
        Ok(ns)
    }
    fn path(&self, name: &str) -> Result<PathBuf> {
        ensure!(managed(name), "Unknown session");
        let path = self.dir.join(format!("{name}{}", self.suffix));
        nix::sys::socket::UnixAddr::new(&path)
            .context("Terminal socket path too long: choose a shorter PIMATE_RUN_DIR")?;
        Ok(path)
    }
    async fn sessions(&self) -> Result<Vec<Session>> {
        let files = fs::read_dir(&self.dir)?.flatten().filter_map(|entry| {
            let name = entry
                .file_name()
                .to_str()?
                .strip_suffix(&self.suffix)?
                .to_owned();
            let path = self.path(&name).ok()?;
            Some(session(name, path))
        });
        Ok(stream::iter(files)
            .buffer_unordered(16)
            .filter_map(std::future::ready)
            .collect()
            .await)
    }
}
async fn session(name: String, path: PathBuf) -> Option<Session> {
    let meta = path.symlink_metadata().ok()?;
    if !meta.file_type().is_socket() {
        return None;
    }
    // abduco 0.6 greets even a passive list client with MSG_PID (native endian).
    let packet = timeout(Duration::from_secs(2), async {
        let mut conn = tokio::net::UnixStream::connect(&path).await?;
        let mut buf = [0; 16];
        conn.read_exact(&mut buf).await?;
        Ok::<_, std::io::Error>(buf)
    })
    .await
    .ok()?
    .ok()?;
    if packet[..4] != 5u32.to_ne_bytes() || packet[4..8] != 8u32.to_ne_bytes() {
        return None;
    }
    let pid = i32::try_from(u64::from_ne_bytes(packet[8..].try_into().ok()?)).ok()?;
    (pid > 0).then_some(Session {
        name,
        pid,
        attached: meta.mode() & 0o100 != 0,
        terminated: meta.mode() & 0o010 != 0,
    })
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
            return url::Url::parse(s)?
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid local extension URL"));
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
        let Ok(sessions) = ns.sessions().await else {
            return;
        };
        for s in sessions {
            if s.terminated
                && !s.attached
                && let Ok(path) = ns.path(&s.name)
            {
                // Consume the exit notification on this socket; never signal a PID supplied by a peer.
                if let Ok(mut child) = Command::new(ab)
                    .args(["-a", "--"])
                    .arg(path)
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
        if let Some(n) = name {
            let path = ns.path(&n)?;
            ensure!(
                path.symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_socket()),
                "Unknown session"
            );
            eprintln!("Detach: Ctrl+\\ (Pi keeps running)");
            return Err(std::process::Command::new(ab)
                .args(["-a", "--"])
                .arg(path)
                .env("ABDUCO_SOCKET_DIR", &ns.dir)
                .exec()
                .into());
        }
        println!("{}", serde_json::to_string_pretty(&ns.sessions().await?)?);
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
        ] {
            assert_eq!(extension_arg(src.into())?, Path::new(src));
        }
        for src in ["extensions/login.ts", "team:rc.ts", "git+ssh:repo", "NPM:x"] {
            assert_eq!(extension_arg(src.into())?, env::current_dir()?.join(src));
        }
        assert_eq!(
            extension_arg("file:///tmp/space%20name".into())?,
            Path::new("/tmp/space name")
        );
        Ok(())
    }
    #[test]
    fn reject_long_socket_path_before_launch() -> Result<()> {
        let t = tempfile::tempdir()?;
        let run = t.path().join("あ".repeat(34));
        let msg = Sockets::open(&run)
            .err()
            .context("accepted long path")?
            .to_string();
        assert!(msg.contains("shorter PIMATE_RUN_DIR"));
        Ok(())
    }
}
