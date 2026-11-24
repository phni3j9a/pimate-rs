//! One current-thread process per PTY. A listing probe connects and closes without attaching.
use anyhow::{Context, Result, ensure};
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    libc,
    pty::{Winsize, openpty},
    sys::termios,
    unistd,
};
use std::{
    env,
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::MetadataExt,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, unix::AsyncFd},
    net::{UnixListener, UnixStream},
    sync::{Mutex, broadcast},
    time::timeout,
};
const MAGIC: &[u8; 4] = b"PMT2";
const PROBE: &[u8; 4] = b"PMS2";
fn verify_uid(sock: &UnixStream, expected: u32) -> Result<()> {
    // Darwin's LOCAL_PEEREPID is not always available; only the UID is needed.
    #[cfg(target_os = "macos")]
    let uid = unistd::getpeereid(sock)?.0.as_raw();
    #[cfg(not(target_os = "macos"))]
    let uid = sock.peer_cred()?.uid();
    ensure!(uid == expected, "terminal peer UID mismatch");
    Ok(())
}
async fn connect(path: &Path) -> Result<UnixStream> {
    let sock = UnixStream::connect(path).await?;
    verify_uid(&sock, unistd::geteuid().as_raw())?;
    Ok(sock)
}
pub async fn probe(path: &Path) -> Result<bool> {
    timeout(Duration::from_millis(100), async {
        let mut sock = connect(path).await?;
        sock.write_all(PROBE).await?;
        let mut reply = [0; 5];
        sock.read_exact(&mut reply).await?;
        ensure!(&reply[..4] == PROBE && reply[4] <= 1);
        Ok(reply[4] == 1)
    })
    .await?
}
#[derive(Clone)]
enum Frame {
    Output(Vec<u8>),
    Exit(u8),
}
fn winsize(rows: u16, cols: u16) -> Winsize {
    Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}
fn size(fd: i32) -> io::Result<Winsize> {
    let mut w = winsize(0, 0);
    nix::errno::Errno::result(unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut w) })?;
    Ok(w)
}
struct SocketFile(PathBuf, u64);
impl Drop for SocketFile {
    fn drop(&mut self) {
        if self.0.symlink_metadata().is_ok_and(|m| m.ino() == self.1) {
            let _ = fs::remove_file(&self.0);
        }
    }
}
#[tokio::main(flavor = "current_thread")]
pub async fn run() -> Result<u8> {
    tokio::task::LocalSet::new().run_until(hold()).await
}
async fn hold() -> Result<u8> {
    let mut args = env::args_os().skip(2);
    let path = PathBuf::from(args.next().context("missing terminal socket")?);
    let mut cmd = std::process::Command::new(args.next().context("missing command")?);
    cmd.args(args);
    // This dedicated process is single-threaded; restore the caller's umask before spawn.
    let old = nix::sys::stat::umask(nix::sys::stat::Mode::from_bits_truncate(0o177));
    let listener = UnixListener::bind(&path);
    nix::sys::stat::umask(old);
    let listener = listener?;
    let _cleanup = SocketFile(path.clone(), path.symlink_metadata()?.ino());
    let pty = openpty(Some(&winsize(24, 80)), None)?;
    let master = File::from(pty.master);
    fcntl(&master, FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC))?;
    fcntl(&master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
    let slave = File::from(pty.slave);
    fcntl(&slave, FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC))?;
    cmd.stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave);
    unsafe {
        cmd.pre_exec(|| {
            unistd::setsid()?;
            nix::errno::Errno::result(libc::ioctl(0, libc::TIOCSCTTY as _, 0))?;
            Ok(())
        });
    }
    let mut child = tokio::process::Command::from(cmd)
        .kill_on_drop(true)
        .spawn()?;
    let pid = child.id().unwrap() as i32;
    // File owns this descriptor until the registration itself is dropped.
    let pty = Rc::new(unsafe { AsyncFd::register(master) }?);
    let (tx, _) = broadcast::channel(32);
    let lock = Rc::new(Mutex::new(()));
    // Rust ignores SIGPIPE. Losing the Host's ready pipe must not cancel an admitted launch.
    let _ = io::stdout()
        .write_all(b"R")
        .and_then(|()| io::stdout().flush());
    let mut buf = [0; 8192];
    loop {
        tokio::select! {
            status = child.wait() => { status?; break },
            result = pty.async_io(tokio::io::Interest::READABLE, |mut f| f.read(&mut buf)) => match result {
                Ok(0) => break,
                Ok(n) => if tx.receiver_count() > 0 { let _ = tx.send(Frame::Output(buf[..n].to_vec())); },
                Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
                Err(e) => return Err(e.into()),
            },
            result = listener.accept() => {
                let (mut sock, _) = result?;
                let (pty, lock, tx) = (pty.clone(), lock.clone(), tx.clone());
                tokio::task::spawn_local(async move {
                    if verify_uid(&sock, unistd::geteuid().as_raw()).is_err() { return }
                    let mut magic = [0; 4];
                    if !matches!(timeout(Duration::from_secs(2), sock.read_exact(&mut magic)).await, Ok(Ok(_))) { return }
                    if &magic == PROBE {
                        let attached = lock.try_lock().is_err();
                        if sock.write_all(PROBE).await.is_ok() { let _ = sock.write_u8(attached as u8).await; }
                        return;
                    }
                    if &magic != MAGIC { return }
                    let Ok(_guard) = lock.try_lock() else { return };
                    let _ = bridge(sock, &pty, pid, tx.subscribe()).await;
                });
            }
        }
    }
    let status = child.wait().await?;
    let code = status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)) as u8;
    // A descendant might keep writing after Pi exits. Drain at most the queue budget.
    if tx.receiver_count() > 0 {
        for _ in 0..31 {
            let Ok(n @ 1..) = pty.get_ref().read(&mut buf) else {
                break;
            };
            if tx.send(Frame::Output(buf[..n].to_vec())).is_err() {
                break;
            }
        }
        let _ = tx.send(Frame::Exit(code));
        let _ = timeout(Duration::from_secs(2), lock.lock()).await;
    }
    Ok(code)
}
async fn bridge(
    mut sock: UnixStream,
    pty: &AsyncFd<File>,
    pid: i32,
    mut rx: broadcast::Receiver<Frame>,
) -> Result<()> {
    sock.write_all(MAGIC).await?;
    let (mut r, mut w) = sock.split();
    let input = async {
        let mut buf = [0; 4096];
        loop {
            match r.read_u8().await? {
                b'S' => {
                    let ws = winsize(r.read_u16().await?, r.read_u16().await?);
                    nix::errno::Errno::result(unsafe {
                        libc::ioctl(pty.as_raw_fd(), libc::TIOCSWINSZ, &ws)
                    })?; // Same-size reattach needs a redraw too.
                    let pgid = unistd::tcgetpgrp(pty.get_ref())
                        .ok()
                        .filter(|p| p.as_raw() > 0)
                        .unwrap_or(unistd::Pid::from_raw(pid));
                    nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGWINCH)?;
                }
                b'I' => {
                    let len = r.read_u16().await? as usize;
                    ensure!(len <= buf.len());
                    r.read_exact(&mut buf[..len]).await?;
                    let mut bytes = &buf[..len];
                    while !bytes.is_empty() {
                        let n = pty
                            .async_io(tokio::io::Interest::WRITABLE, |f| (&*f).write(bytes))
                            .await?;
                        ensure!(n > 0);
                        bytes = &bytes[n..];
                    }
                }
                _ => anyhow::bail!("invalid terminal frame"),
            }
        }
    };
    let output = async {
        loop {
            match rx.recv().await? {
                Frame::Output(data) => {
                    w.write_u16(data.len() as u16).await?;
                    w.write_all(&data).await?;
                }
                Frame::Exit(code) => {
                    w.write_all(&[0, 0, code]).await?;
                    return Ok(());
                }
            }
        }
    };
    tokio::select! {r=input=>r,r=output=>r}
}
struct Raw(libc::termios);
impl Drop for Raw {
    fn drop(&mut self) {
        let mut out = io::stdout().lock();
        let _ = out
            .write_all(b"\x1b[?1049l\x1b[?25h")
            .and_then(|()| out.flush());
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.0);
        }
    }
}
pub async fn attach(path: &Path) -> Result<u8> {
    let sock = timeout(Duration::from_secs(2), async {
        let mut sock = connect(path).await?;
        sock.write_all(MAGIC).await?;
        let mut ack = [0; 4];
        sock.read_exact(&mut ack).await?;
        ensure!(&ack == MAGIC, "incompatible terminal");
        Ok::<_, anyhow::Error>(sock)
    })
    .await??;
    let (mut r, mut w) = sock.into_split();
    let mut winch = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let orig = termios::tcgetattr(io::stdin())?;
    // Preserve platform flags that nix truncates when writing its portable Termios wrapper.
    let saved = libc::termios::from(orig.clone());
    let mut raw = orig;
    termios::cfmakeraw(&mut raw);
    termios::tcsetattr(io::stdin(), termios::SetArg::TCSANOW, &raw)?;
    let _restore = Raw(saved);
    io::stdout().write_all(b"\x1b[?1049h")?;
    io::stdout().flush()?;
    let mut stdin = tokio::io::stdin();

    let mut buf = [0; 4096];
    let input = async {
        let mut resize = true;
        loop {
            if resize {
                let ws = size(0)?;
                w.write_u8(b'S').await?;
                w.write_u16(ws.ws_row).await?;
                w.write_u16(ws.ws_col).await?;
            }
            tokio::select! {
                _ = winch.recv() => resize = true,
                n = stdin.read(&mut buf) => {
                    let n = n?;
                    let len = buf[..n].iter().position(|b| *b == 0x1c).unwrap_or(n);
                    if len > 0 {
                        w.write_u8(b'I').await?;
                        w.write_u16(len as u16).await?;
                        w.write_all(&buf[..len]).await?;
                    }
                    if n == 0 || len < n { return Ok(0) }
                    resize = false;
                }
            }
        }
    };
    let output = async {
        let mut buf = [0; 8192];
        loop {
            let n = r.read_u16().await? as usize;
            if n == 0 {
                return Ok(r.read_u8().await?);
            }
            ensure!(n <= buf.len(), "invalid output frame");
            r.read_exact(&mut buf[..n]).await?;
            let mut out = io::stdout().lock();
            out.write_all(&buf[..n])?;
            out.flush()?;
        }
    };
    tokio::select! {
        r = input => r,
        r = output => r,
        _ = interrupt.recv() => Ok(0),
        _ = terminate.recv() => Ok(0),
        _ = hangup.recv() => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn keeper_checks_peer_uid_on_both_ends() -> Result<()> {
        let d = tempfile::tempdir_in("/tmp")?;
        let path = d.path().join("t");
        let listener = UnixListener::bind(&path)?;
        let (a, (b, _)) = tokio::try_join!(UnixStream::connect(&path), listener.accept())?;
        let uid = unistd::geteuid().as_raw();
        for sock in [&a, &b] {
            verify_uid(sock, uid)?;
            assert!(verify_uid(sock, uid.wrapping_add(1)).is_err());
        }
        Ok(())
    }
}
