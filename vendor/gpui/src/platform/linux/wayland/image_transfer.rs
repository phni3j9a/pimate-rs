use std::{
    io::{self, Read},
    os::unix::net::UnixStream,
    time::Instant,
};

pub(super) fn read(mut socket: UnixStream, limit: usize, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        socket.set_read_timeout(Some(remaining))?;
        let capacity = chunk.len().min((limit - bytes.len()).saturating_add(1));
        let n = match socket.read(&mut chunk[..capacity]) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            r => r?,
        };
        if n == 0 {
            return Ok(bytes);
        }
        if n > limit - bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "clipboard image too large",
            ));
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
}
