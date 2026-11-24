//! Per-login LaunchAgent. Only operate on a job loaded from our marked plist.
use super::service::{abs, host_env, port};
use crate::{
    config,
    operations::home,
    process::{command, output},
};
use anyhow::{Context, Result, ensure};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};
pub const LABEL: &str = "dev.pimate.rs.host";
const MARK: &str = "<!-- Managed by PiMate (Rust) -->";
pub fn path() -> Result<PathBuf> {
    Ok(home()?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}
fn domain() -> String {
    format!("gui/{}", nix::unistd::Uid::current())
}
fn target() -> String {
    format!("{}/{LABEL}", domain())
}
fn xml(s: &str) -> Result<String> {
    ensure!(
        s.chars()
            .all(|c| !c.is_control() && c != '\u{fffe}' && c != '\u{ffff}'),
        "invalid plist value"
    );
    Ok(s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;"))
}
fn entry(k: &str, v: &str) -> Result<String> {
    Ok(format!(
        "<key>{}</key><string>{}</string>\n",
        xml(k)?,
        xml(v)?
    ))
}
pub fn plist() -> Result<String> {
    let mut vars = String::new();
    for pair in host_env()? {
        let (k, v) = pair.split_once('=').context("invalid environment")?;
        vars += &entry(k, v)?;
    }
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
{MARK}
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>serve</string><string>--port</string><string>{}</string></array>
<key>EnvironmentVariables</key><dict>
{vars}</dict>
<key>StandardErrorPath</key><string>{}</string>
<key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>AbandonProcessGroup</key><true/>
<key>Umask</key><integer>63</integer>
</dict></plist>
"#,
        xml(&abs(env::current_exe()?)?)?,
        port()?,
        xml(&abs(config::config_dir()?.join("host.log"))?)?
    ))
}
pub fn lock(path: &Path) -> Result<fs::File> {
    let dir = path.parent().context("missing LaunchAgents directory")?;
    fs::create_dir_all(dir)?;
    let lock = fs::File::open(dir)?;
    lock.try_lock()
        .context("another LaunchAgent operation is in progress")?;
    Ok(lock)
}
pub fn install(path: &Path, installed: bool) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    tmp.write_all(plist()?.as_bytes())?;
    if installed {
        ensure!(owned(path)?, "LaunchAgent removed during install");
        tmp.persist(path)?;
    } else {
        tmp.persist_noclobber(path)?;
    }
    Ok(())
}
pub fn owned(p: &Path) -> Result<bool> {
    match p.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        m => {
            ensure!(m?.is_file(), "unmanaged LaunchAgent");
            let text = fs::read_to_string(p)?;
            ensure!(
                text.lines().nth(1) == Some(MARK)
                    && text.contains(&format!(
                        "\n{}",
                        entry("PIMATE_CONFIG_DIR", &abs(config::config_dir()?)?)?
                    )),
                "unmanaged LaunchAgent or different config directory"
            );
            Ok(true)
        }
    }
}
async fn ctl(args: &[&str]) -> Result<(Option<i32>, String)> {
    let mut c = command("launchctl")?;
    c.args(args);
    let (code, out, _) = output(c, 15, 131072).await?;
    Ok((code, out))
}
pub async fn loaded(p: &Path) -> Result<Option<bool>> {
    let (code, out) = ctl(&["print", &target()]).await?;
    if code == Some(113) {
        return Ok(None);
    }
    ensure!(code == Some(0), "cannot inspect LaunchAgent");
    let from = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("path = "))
        .context("missing loaded plist path")?;
    ensure!(
        owned(p)? && fs::canonicalize(from)? == fs::canonicalize(p)?,
        "different loaded LaunchAgent"
    );
    Ok(Some(out.lines().any(|l| l.trim() == "state = running")))
}
pub async fn start(p: &Path) -> Result<()> {
    let (code, _) = if loaded(p).await?.is_some() {
        ctl(&["kickstart", &target()]).await?
    } else {
        ctl(&[
            "bootstrap",
            &domain(),
            p.to_str().context("non UTF-8 path")?,
        ])
        .await?
    };
    ensure!(code == Some(0), "LaunchAgent start failed");
    Ok(())
}
pub async fn stop(p: &Path) -> Result<()> {
    if loaded(p).await?.is_some() {
        ensure!(
            ctl(&["bootout", &target()]).await?.0 == Some(0),
            "LaunchAgent stop failed"
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_agent_plist() -> Result<()> {
        let s = plist()?;
        assert!(s.contains(&entry("PIMATE_CONFIG_DIR", &abs(config::config_dir()?)?)?));
        assert!(s.contains(&format!("<string>{LABEL}</string>")));
        assert!(s.contains("<string>serve</string><string>--port</string>"));
        assert!(s.contains("<key>AbandonProcessGroup</key><true/>"));
        assert!(s.contains(&entry(
            "StandardErrorPath",
            &abs(config::config_dir()?.join("host.log"))?
        )?));
        let tmp = tempfile::tempdir()?;
        let p = tmp.path().join("job.plist");
        let first = lock(&p)?;
        assert!(lock(&p).is_err());
        drop(first);
        let _second = lock(&p)?;
        fs::write(&p, "foreign")?;
        assert!(install(&p, false).is_err());
        assert_eq!(fs::read_to_string(p)?, "foreign");
        assert_eq!(xml("x&<y>")?, "x&amp;&lt;y&gt;");
        assert!(xml("bad\0").is_err());
        Ok(())
    }
}
