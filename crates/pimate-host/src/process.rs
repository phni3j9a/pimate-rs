use crate::terminal::executable;
use anyhow::{Context, Result, ensure};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::timeout,
};
pub fn command(name: &str) -> Result<Command> {
    let mut c =
        Command::new(executable(Path::new(name)).with_context(|| format!("{name} not found"))?);
    c.kill_on_drop(true).stdin(Stdio::null());
    Ok(c)
}
async fn bounded(io: impl AsyncRead + Unpin, max: usize) -> Result<String> {
    let mut b = vec![];
    io.take(max as u64 + 1).read_to_end(&mut b).await?;
    ensure!(b.len() <= max, "command output too large");
    Ok(String::from_utf8_lossy(&b).into_owned())
}
pub async fn output(
    mut c: Command,
    secs: u64,
    max: usize,
) -> Result<(Option<i32>, String, String)> {
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    timeout(Duration::from_secs(secs), async {
        let (status, out, err) = tokio::try_join!(
            async { Ok::<_, anyhow::Error>(child.wait().await?) },
            bounded(out, max),
            bounded(err, max)
        )?;
        Ok((status.code(), out, err))
    })
    .await?
}
