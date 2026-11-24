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
    fs::{self, File, Permissions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, PermissionsExt},
            process::CommandExt,
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
const MAGIC: &[u8; 4] = b"PMT1";
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
pub async fn run() -> Result<()> {
    tokio::task::LocalSet::new().run_until(hold()).await
}
async fn hold() -> Result<()> {
    let mut args = env::args_os().skip(2);
    let path = PathBuf::from(args.next().context("missing terminal socket")?);
    let mut cmd = std::process::Command::new(args.next().context("missing command")?);
    cmd.args(args);
    let listener = UnixListener::bind(&path)?;
    let _cleanup = SocketFile(path.clone(), path.symlink_metadata()?.ino());
    fs::set_permissions(&path, Permissions::from_mode(0o600))?;
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
    io::stdout().write_all(b"R")?;
    io::stdout().flush()?;
    let mut buf = [0; 8192];
    loop {
        tokio::select! {
            status = child.wait() => { status?; break },
            result = pty.async_io(tokio::io::Interest::READABLE, |mut f| f.read(&mut buf)) => match result {
                Ok(0) => break,
                Ok(n) => if tx.receiver_count() > 0 { let _ = tx.send(buf[..n].to_vec()); },
                Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
                Err(e) => return Err(e.into()),
            },
            result = listener.accept() => {
                let (mut sock, _) = result?;
                let (pty, lock, path, tx) = (pty.clone(), lock.clone(), path.clone(), tx.clone());
                tokio::task::spawn_local(async move {
                    let mut magic = [0; 4];
                    if !matches!(timeout(Duration::from_secs(2), sock.read_exact(&mut magic)).await, Ok(Ok(_))) || &magic != MAGIC { return }
                    let Ok(_guard) = lock.try_lock() else { return };
                    if fs::set_permissions(&path, Permissions::from_mode(0o700)).is_ok() {
                        let _ = bridge(sock, &pty, pid, tx.subscribe()).await;
                        let _ = fs::set_permissions(path, Permissions::from_mode(0o600));
                    }
                });
            }
        }
    }
    child.wait().await?;
    Ok(())
}
async fn bridge(
    mut sock: UnixStream,
    pty: &AsyncFd<File>,
    pid: i32,
    mut rx: broadcast::Receiver<Vec<u8>>,
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
                    nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGWINCH,
                    )?;
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
            w.write_all(&rx.recv().await?).await?;
        }
    };
    tokio::select! {r=input=>r,r=output=>r}
}
struct Raw(termios::Termios);
impl Drop for Raw {
    fn drop(&mut self) {
        let _ = termios::tcsetattr(io::stdin(), termios::SetArg::TCSANOW, &self.0);
    }
}
pub async fn attach(path: &Path) -> Result<()> {
    let mut sock = UnixStream::connect(path).await?;
    sock.write_all(MAGIC).await?;
    let mut ack = [0; 4];
    timeout(Duration::from_secs(2), sock.read_exact(&mut ack)).await??;
    ensure!(&ack == MAGIC, "incompatible terminal");
    let (mut r, mut w) = sock.into_split();
    let mut winch = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let orig = termios::tcgetattr(io::stdin())?;
    let mut raw = orig.clone();
    termios::cfmakeraw(&mut raw);
    termios::tcsetattr(io::stdin(), termios::SetArg::TCSANOW, &raw)?;
    let _restore = Raw(orig);
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
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
                    if n == 0 || len < n { return Ok(()) }
                    resize = false;
                }
            }
        }
    };
    tokio::select! {
        r = input => r,
        r = tokio::io::copy(&mut r, &mut stdout) => {r?; stdout.flush().await?; Ok(())},
        _ = tokio::signal::ctrl_c() => Ok(()),
        _ = terminate.recv() => Ok(()),
        _ = hangup.recv() => Ok(()),
    }
}
