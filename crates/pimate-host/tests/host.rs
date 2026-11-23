use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    env,
    fs::File,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::{Child, Command},
    time::timeout,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
fn cli(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pimate"));
    c.env_clear();
    for k in [
        "PATH",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "LANG",
    ] {
        if let Some(v) = env::var_os(k) {
            c.env(k, v);
        }
    }
    c.current_dir(dir)
        .env("HOME", dir)
        .env("ABDUCO_SOCKET_DIR", dir)
        .env("PI_CODING_AGENT_DIR", dir.join("agent"))
        .env("PIMATE_CONFIG_DIR", dir.join("config"))
        .env("PIMATE_RUN_DIR", dir.join("run"))
        .kill_on_drop(true);
    c
}
struct Server {
    _child: Child,
    addr: String,
    dir: std::sync::Arc<tempfile::TempDir>,
}
impl Server {
    async fn start() -> Result<Self> {
        Self::with(tempfile::tempdir()?.into(), &[]).await
    }
    async fn with(dir: std::sync::Arc<tempfile::TempDir>, vars: &[(&str, &Path)]) -> Result<Self> {
        let mut child = cli(dir.path())
            .envs(vars.iter().copied())
            .args(["serve", "--listen", "127.0.0.1", "--port", "0"])
            .stdout(Stdio::piped())
            .spawn()?;
        let mut line = String::new();
        timeout(
            Duration::from_secs(8),
            BufReader::new(child.stdout.take().unwrap()).read_line(&mut line),
        )
        .await??;
        let addr = line
            .trim()
            .strip_prefix("Listening http://")
            .context("server startup")?
            .to_owned();
        Ok(Self {
            _child: child,
            addr,
            dir,
        })
    }
    async fn post(&self, headers: &str, body: &str) -> Result<(u16, Value)> {
        let mut tcp = TcpStream::connect(&self.addr).await?;
        tcp.write_all(format!("POST /pair HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n\r\n{body}",self.addr,body.len()).as_bytes()).await?;
        let mut s = String::new();
        tcp.read_to_string(&mut s).await?;
        let (h, b) = s.split_once("\r\n\r\n").context("HTTP response")?;
        Ok((
            h.split_whitespace().nth(1).unwrap().parse()?,
            serde_json::from_str(b)?,
        ))
    }
    async fn paired(&self) -> Result<Value> {
        let o = cli(self.dir.path())
            .args(["pair", "--address", &self.addr])
            .output()
            .await?;
        ensure!(o.status.success());
        let code = pimate_protocol::parse_pairing_link(
            std::str::from_utf8(&o.stdout)?.lines().next().unwrap(),
        )
        .unwrap()
        .code;
        let body = json!({"code":code,"deviceName":"試験"}).to_string();
        let h = "Content-Type: application/json";
        assert_eq!(
            self.post(&format!("{h}\r\nOrigin: https://example.com"), &body)
                .await?
                .0,
            400
        );
        let (status, v) = self.post(h, &body).await?;
        assert_eq!(status, 200);
        assert_eq!(self.post(h, &body).await?.0, 401);
        Ok(v)
    }
    async fn ws(&self) -> Result<Ws> {
        Ok(connect_async(format!("ws://{}/ws", self.addr)).await?.0)
    }
    async fn auth(&self, token: &Value) -> Result<Ws> {
        let mut ws = self.ws().await?;
        send(&mut ws,json!({"type":"auth","protocol":1,"token":token,"app":{"platform":"test","version":"1"}})).await?;
        Ok(ws)
    }
}
async fn send(ws: &mut Ws, v: Value) -> Result<()> {
    ws.send(Message::Text(v.to_string().into())).await?;
    Ok(())
}
async fn next(ws: &mut Ws) -> Result<Value> {
    let msg = timeout(Duration::from_secs(20), ws.next())
        .await?
        .context("WS closed")??;
    ensure!(msg.is_text(), "{msg:?}");
    Ok(serde_json::from_str(msg.to_text()?)?)
}
async fn until(ws: &mut Ws, f: impl Fn(&Value) -> bool) -> Result<Value> {
    timeout(Duration::from_secs(20), async {
        loop {
            let v = next(ws).await?;
            if f(&v) {
                return Ok(v);
            }
        }
    })
    .await?
}
async fn closed(ws: &mut Ws, code: u16) -> Result<()> {
    let msg = timeout(Duration::from_secs(7), ws.next())
        .await?
        .context("missing close")??;
    assert!(matches!(msg,Message::Close(Some(c)) if u16::from(c.code)==code));
    Ok(())
}
#[tokio::test]
async fn pairing_auth_and_revocation() -> Result<()> {
    let s = Server::start().await?;
    let v = s.paired().await?;
    let mut bad = s.auth(&json!("wrong")).await?;
    assert_eq!(next(&mut bad).await?["code"], "invalid_token");
    closed(&mut bad, 4401).await?;
    let mut ws = s.auth(&v["token"]).await?;
    assert_eq!(next(&mut ws).await?["type"], "auth_ok");
    assert_eq!(next(&mut ws).await?["instances"], json!([]));
    send(&mut ws, json!({"type":"devices","requestId":"a"})).await?;
    let ds = next(&mut ws).await?;
    assert_eq!(ds["devices"][0]["current"], true);
    assert!(ds["devices"][0].get("tokenHash").is_none());
    send(
        &mut ws,
        json!({"type":"device_revoke","requestId":"b","deviceId":v["deviceId"]}),
    )
    .await?;
    assert_eq!(next(&mut ws).await?["ok"], true);
    closed(&mut ws, 4403).await?;
    closed(&mut s.ws().await?, 4408).await
}
#[tokio::test]
#[ignore = "requires Pi and a TS PiMate checkout with dependencies"]
async fn real_pi() -> Result<()> {
    let pi = PathBuf::from(env::var("PIMATE_TEST_PI")?).canonicalize()?;
    let pkg = PathBuf::from(env::var("PIMATE_TEST_TS")?)
        .canonicalize()?
        .join("packages/pimate");
    let s = Server::start().await?;
    let v = s.paired().await?;
    let dir = s.dir.path();
    let pty = nix::pty::openpty(
        Some(&nix::pty::Winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )?;
    let slave = File::from(pty.slave);
    let mut pi = Command::new(pi);
    pi.current_dir(dir)
        .env_clear()
        .env("PATH", env::var_os("PATH").unwrap())
        .env("ABDUCO_SOCKET_DIR", dir)
        .env("TERM", "xterm-256color")
        .env("HOME", dir)
        .env("PI_CODING_AGENT_DIR", dir.join("agent"))
        .env("PIMATE_CONFIG_DIR", dir.join("config"))
        .env("PIMATE_RUN_DIR", dir.join("run"))
        .env("PIMATE_ORIGIN", "mobile")
        .env("PIMATE_TRUST", "no")
        .env("PIMATE_INSTANCE_ID", "real-e2e")
        .env("PIMATE_E2E", "1")
        .env("PI_OFFLINE", "1")
        .args(["--no-extensions", "-e"])
        .arg(pkg.join("src/extension/index.ts"))
        .arg("-e")
        .arg(pkg.join("test/fixtures/faux.ts"))
        .args([
            "--model",
            "pimate-faux/scripted",
            "--no-skills",
            "--no-context-files",
            "--no-prompt-templates",
            "--no-themes",
            "--thinking",
            "off",
        ])
        .stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave)
        .kill_on_drop(true);
    let mut child = pi.spawn()?;
    let master = File::from(pty.master);
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut &master, &mut std::io::sink());
    });
    let mut ws = s.auth(&v["token"]).await?;
    until(&mut ws, |v| {
        v["instances"]
            .as_array()
            .is_some_and(|a| a.iter().any(|i| i["instanceId"] == "real-e2e"))
    })
    .await?;
    send(&mut ws, json!({"type":"subscribe","instanceId":"real-e2e"})).await?;
    let sub = until(&mut ws, |v| v["type"] == "subscribed").await?;
    assert_eq!(sub["snapshot"]["model"]["provider"], "pimate-faux");
    assert!(sub["history"]["items"].is_array());
    send(&mut ws,json!({"type":"rc","instanceId":"real-e2e","request":{"id":"prompt1","method":"prompt","params":{"text":"say hello"}}})).await?;
    let mut acknowledged = false;
    let mut answered = false;
    timeout(Duration::from_secs(20), async {
        while !(acknowledged && answered) {
            let m = next(&mut ws).await?;
            acknowledged |= m["response"]["id"] == "prompt1" && m["response"]["ok"] == true;
            answered |= m["event"]["item"]["text"] == "Hello from the PiMate faux model.";
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    child.kill().await?;
    Ok(())
}

// Exercise the Host without Pi in CI. Unknown payloads intentionally cannot be decoded as endpoint types.
#[tokio::test]
async fn relay_sync_reconnect_and_unknown_fields() -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::net::UnixListener;
    use tokio_util::codec::{Framed, LinesCodec};
    let s = Server::start().await?;
    let run = s.dir.path().join("run");
    let meta = json!({"protocol":1,"instanceId":"stub","pid":std::process::id(),"cwd":"/test","origin":"pc","abducoSession":null,"session":{"id":"s"},"startedAt":1,"updatedAt":1,"status":"idle"});
    std::fs::write(run.join("stub.json"), meta.to_string())?;
    let listener = UnixListener::bind(run.join("stub.sock"))?;
    let sends = Arc::new(AtomicUsize::new(0));
    let count = sends.clone();
    let fake = tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let count = count.clone();
            tokio::spawn(async move {
                let mut io = Framed::new(sock, LinesCodec::new());
                let mut batch = vec![];
                let snap = json!({"instanceId":"stub","protocol":1,"seq":2,"session":{"id":"s"},"newField":true});
                let event = |seq: u64, kind: &str, delta: &str| json!({"type":"event","instanceId":"stub","sessionId":"s","seq":seq,"extra":1,"event":{"type":kind,"delta":delta}});
                while let Some(Ok(l)) = io.next().await {
                    let r: Value = serde_json::from_str(&l)?;
                    let result = match r["method"].as_str() {
                        Some("hello") => json!({"snapshot":snap}),
                        Some("history") => {
                            ensure!(r["expectSessionId"] == "s");
                            for (seq, kind) in [(1, "status"), (2, "notify"), (3, "future")] {
                                io.send(event(seq, kind, "").to_string()).await?
                            }
                            json!({"session":{"id":"s"},"future":[1,2]})
                        }
                        Some("future_op") => {
                            count.fetch_add(1, Ordering::SeqCst);
                            return Ok::<_, anyhow::Error>(());
                        }
                        Some("ordered") => {
                            batch.push(r["id"].clone());
                            if batch.len() == 30 {
                                for id in batch.iter().rev() {
                                    io.send(
                                        json!({"type":"response","id":id,"ok":true,"result":batch})
                                            .to_string(),
                                    )
                                    .await?
                                }
                            }
                            continue;
                        }
                        Some("echo") => r.clone(),
                        _ => bail!("unexpected RC request"),
                    };
                    io.send(json!({"type":"response","id":r["id"],"ok":true,"extra_response":42,"result":result}).to_string()).await?;
                    if r["method"] == "history" {
                        for (n, k, d) in [
                            (4, "text_delta", "a"),
                            (5, "text_delta", "b"),
                            (6, "settled", ""),
                        ] {
                            io.send(event(n, k, d).to_string()).await?
                        }
                        return Ok(());
                    }
                }
                Ok(())
            });
        }
    });
    let token = s.paired().await?["token"].clone();
    let mut ws = s.auth(&token).await?;
    until(&mut ws, |m| m["instances"][0]["instanceId"] == "stub").await?;
    let order: Vec<_> = (0..30).map(|i| i.to_string()).collect();
    for id in &order {
        send(
            &mut ws,
            json!({"type":"rc","instanceId":"stub","request":{"id":id,"method":"ordered"}}),
        )
        .await?
    }
    let mut replies = std::collections::HashSet::new();
    for _ in &order {
        let m = next(&mut ws).await?;
        assert_eq!(m["response"]["result"], json!(order));
        assert!(replies.insert(m["response"]["id"].as_str().unwrap().to_owned()));
    }
    let r = json!({"id":"raw","method":"echo","params":{"future":{}},"expectSessionId":"original","expectInstanceId":"stub","future_flag":true});
    send(
        &mut ws,
        json!({"type":"rc","instanceId":"stub","request":r}),
    )
    .await?;
    let response = next(&mut ws).await?;
    assert_eq!(response["response"]["result"], r);
    assert_eq!(response["response"]["extra_response"], 42);
    send(
        &mut ws,
        json!({"type":"rc","instanceId":"stub","request":{"id":"once","method":"future_op"}}),
    )
    .await?;
    assert_eq!(
        next(&mut ws).await?["response"]["error"]["code"],
        "internal"
    );
    send(&mut ws, json!({"type":"subscribe","instanceId":"stub"})).await?;
    assert_eq!(next(&mut ws).await?["event"]["type"], "notify");
    let sub = next(&mut ws).await?;
    assert_eq!(sub["snapshot"]["newField"], true);
    assert_eq!(sub["history"]["future"], json!([1, 2]));
    let future = next(&mut ws).await?;
    assert_eq!(future["seq"], 3);
    assert_eq!(future["extra"], 1);
    let delta = next(&mut ws).await?;
    assert_eq!(delta["seq"], 5);
    assert_eq!(delta["event"]["delta"], "ab");
    assert_eq!(next(&mut ws).await?["seq"], 6);
    until(&mut ws, |m| m["type"] == "subscribed").await?;
    std::fs::remove_file(run.join("stub.json"))?;
    until(&mut ws, |m| m["type"] == "instance_gone").await?;
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    fake.abort();
    Ok(())
}

#[tokio::test]
async fn bad_requests_never_reach_rc() -> Result<()> {
    let s = Server::start().await?;
    let h = "Content-Type: application/json";
    for b in ["null".into(), "{".into(), "x".repeat(4097)] {
        assert_eq!(s.post(h, &b).await?.0, 400)
    }
    assert_eq!(s.post("Content-Type: text/plain", "{}").await?.0, 400);
    let v = s.paired().await?;
    for msg in [
        json!({"type":"devices","requestId":"a","extra":true}),
        json!({"type":"device_revoke","requestId":"b","deviceId":"../bad"}),
        json!({"type":"rc","instanceId":"../../bad","request":{}}),
        json!({"type":"rc","instanceId":"live","request":{"id":"q","method":"hello","params":[]}}),
    ] {
        let mut w = s.auth(&v["token"]).await?;
        until(&mut w, |m| m["type"] == "instances").await?;
        send(&mut w, msg).await?;
        closed(&mut w, 4400).await?
    }
    let mut ws = s.auth(&v["token"]).await?;
    until(&mut ws, |m| m["type"] == "instances").await?;
    send(&mut ws, json!({"type":"ping","t":123})).await?;
    assert_eq!(next(&mut ws).await?, json!({"type":"pong","t":123}));
    let ok = cli(s.dir.path())
        .args(["revoke", v["deviceId"].as_str().unwrap()])
        .status()
        .await?;
    assert!(ok.success());
    closed(&mut ws, 4403).await?;
    let mut huge = s.ws().await?;
    // The length alone is rejected. Writing the payload races the server's close on small TCP buffers.
    let mut header = vec![0x81, 0xff];
    header.extend_from_slice(&((1u64 << 20) + 1).to_be_bytes());
    header.extend_from_slice(&[0; 4]);
    huge.get_mut().write_all(&header).await?;
    closed(&mut huge, 1009).await?;
    let mut mismatch = s.ws().await?;
    send(
        &mut mismatch,
        json!({"type":"auth","token":"t","app":{"platform":"x","version":"1"},"protocol":2}),
    )
    .await?;
    assert_eq!(next(&mut mismatch).await?["code"], "protocol_mismatch");
    closed(&mut mismatch, 4400).await?;
    Ok(())
}
#[tokio::test]
async fn reject_public_bind() -> Result<()> {
    let t = tempfile::tempdir()?;
    let out = timeout(
        Duration::from_secs(8),
        cli(t.path())
            .args(["serve", "--port", "0", "--listen", "0.0.0.0"])
            .output(),
    )
    .await??;
    assert!(!out.status.success());
    assert!(String::from_utf8(out.stderr)?.contains("not loopback or a local Tailscale address"));
    Ok(())
}

#[tokio::test]
async fn lock_contention_and_unreadable_db_are_not_revocation() -> Result<()> {
    let s = Server::start().await?;
    let paired = s.paired().await?;
    let cfg = s.dir.path().join("config");
    let db = cfg.join("devices.json");
    let lock = cfg.join("devices.lock");
    let mut devices: Value = serde_json::from_slice(&std::fs::read(&db)?)?;
    devices[0]["lastSeenAt"] = json!(0);
    std::fs::write(&db, devices.to_string())?;
    std::fs::write(&lock, "")?;
    let mut ws = s.auth(&paired["token"]).await?;
    until(&mut ws, |v| v["type"] == "instances").await?;
    send(&mut ws, json!({"type":"ping","t":1})).await?;
    assert_eq!(next(&mut ws).await?["type"], "pong");
    std::fs::remove_file(lock)?;
    timeout(Duration::from_secs(5), async {
        loop {
            let ds: Value = serde_json::from_slice(&std::fs::read(&db)?)?;
            if ds[0]["lastSeenAt"] != 0 {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await
        }
    })
    .await??;
    std::fs::write(&db, "broken")?;
    closed(&mut s.auth(&paired["token"]).await?, 1011).await?;
    closed(&mut ws, 1011).await
}
#[tokio::test]
async fn ws_revoke_immediately_closes_all_target_peers() -> Result<()> {
    let s = Server::start().await?;
    let a = s.paired().await?;
    let b = s.paired().await?;
    let mut owner = s.auth(&a["token"]).await?;
    until(&mut owner, |m| m["type"] == "instances").await?;
    for target in [&b, &a] {
        let mut peers = vec![];
        for _ in 0..2 {
            let mut w = s.auth(&target["token"]).await?;
            until(&mut w, |m| m["type"] == "instances").await?;
            peers.push(w)
        }
        send(
            &mut owner,
            json!({"type":"device_revoke","requestId":"x","deviceId":target["deviceId"]}),
        )
        .await?;
        assert_eq!(next(&mut owner).await?["ok"], true);
        timeout(Duration::from_millis(750), async {
            for w in &mut peers {
                closed(w, 4403).await?
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
    }
    closed(&mut owner, 4403).await
}
#[tokio::test]
async fn pair_rejects_unusable_ports_before_issuing_code() -> Result<()> {
    let t = tempfile::tempdir()?;
    for args in [
        ["--port", "0"],
        ["--port", "65536"],
        ["--address", "localhost:0"],
        ["--address", "localhost:65536"],
    ] {
        assert!(
            !cli(t.path())
                .arg("pair")
                .args(args)
                .output()
                .await?
                .status
                .success()
        );
    }
    assert_eq!(
        std::fs::read_dir(t.path().join("config/pairing"))?.count(),
        0
    );
    assert!(
        cli(t.path())
            .args(["pair", "--address", "[::1]:65535"])
            .output()
            .await?
            .status
            .success()
    );
    Ok(())
}
#[tokio::test]
async fn normal_stop_finishes_locked_pair_update() -> Result<()> {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    for sig in [Signal::SIGINT, Signal::SIGTERM] {
        let mut s = Server::start().await?;
        let dir = s.dir.path();
        let cfg = dir.join("config");
        let db = cfg.join("devices.json");
        let o = cli(dir)
            .args(["pair", "--address", &s.addr])
            .output()
            .await?;
        let code = pimate_protocol::parse_pairing_link(
            std::str::from_utf8(&o.stdout)?.lines().next().unwrap(),
        )
        .unwrap()
        .code;
        // A FIFO holds the read inside Config::update after devices.lock is acquired.
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&db)
                .status()?
                .success()
        );
        let mut tcp = TcpStream::connect(&s.addr).await?;
        let body = json!({"code":code,"deviceName":"signal"}).to_string();
        tcp.write_all(format!("POST /pair HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",s.addr,body.len()).as_bytes()).await?;
        let mut writer = timeout(Duration::from_secs(4), async {
            loop {
                if let Ok(f) = std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(nix::libc::O_NONBLOCK)
                    .open(&db)
                {
                    break f;
                }
                tokio::time::sleep(Duration::from_millis(10)).await
            }
        })
        .await?;
        assert!(cfg.join("devices.lock").exists());
        kill(Pid::from_raw(s._child.id().unwrap() as i32), sig)?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(s._child.try_wait()?.is_none());
        writer.write_all(b"[]")?;
        drop(writer);
        assert!(
            timeout(Duration::from_secs(4), s._child.wait())
                .await??
                .success()
        );
        assert!(!cfg.join("devices.lock").exists());
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(db)?)?[0]["name"],
            "signal"
        );
    }
    Ok(())
}

#[tokio::test]
async fn stop_during_initial_probe_never_starts_listening() -> Result<()> {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    use std::os::unix::fs::DirBuilderExt;
    for signal in [Signal::SIGINT, Signal::SIGTERM] {
        let tmp = tempfile::tempdir()?;
        let run = tmp.path().join("run");
        std::fs::DirBuilder::new().mode(0o700).create(&run)?;
        let listener = tokio::net::UnixListener::bind(run.join("slow.sock"))?;
        std::fs::write(run.join("slow.json"),json!({"instanceId":"slow","pid":std::process::id(),"protocol":1,"cwd":".","startedAt":1,"abducoSession":null,"session":null,"status":"idle","origin":"pc"}).to_string())?;
        let child = cli(tmp.path())
            .args(["serve", "--listen", "127.0.0.1", "--port", "0"])
            .stdout(Stdio::piped())
            .spawn()?;
        let (socket, _) = timeout(Duration::from_secs(4), listener.accept()).await??;
        let mut hello = String::new();
        let mut reader = BufReader::new(socket);
        reader.read_line(&mut hello).await?;
        kill(Pid::from_raw(child.id().unwrap() as i32), signal)?;
        let o = timeout(Duration::from_secs(3), child.wait_with_output()).await??;
        assert!(o.status.success());
        assert!(o.stdout.is_empty(), "started listening after stop");
    }
    Ok(())
}

// Test-owned socket tree only. Recover even when a WS assertion fails before spawn_result.
struct Cleanup {
    dir: PathBuf,
    ab: PathBuf,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::Pid,
        };
        if !self.dir.is_dir() {
            return;
        }
        if let Ok(o) = std::process::Command::new(&self.ab)
            .env("HOME", &self.dir)
            .env("ABDUCO_SOCKET_DIR", &self.dir)
            .output()
        {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let cols: Vec<_> = line.split('\t').collect();
                if cols.len() != 4 {
                    continue;
                }
                let n = cols[3].trim();
                if let Some(hex) = n.strip_prefix("pimate-")
                    && hex.len() == 8
                    && hex.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    if cfg!(target_os = "linux")
                        && std::process::Command::new("systemctl")
                            .args(["--user", "stop", &format!("{n}.scope")])
                            .status()
                            .is_ok_and(|s| s.success())
                    {
                        continue;
                    }
                    if let Ok(pid) = cols[2].parse::<i32>()
                        && pid > 0
                    {
                        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
                    }
                }
            }
        }
    }
}
#[tokio::test]
#[ignore = "requires Pi, abduco and TS PiMate faux fixture"]
async fn real_pi_spawn_attach_restart_login() -> Result<()> {
    let tmp = std::sync::Arc::new(tempfile::tempdir()?);
    let path = tmp.path().to_path_buf();
    let pi = PathBuf::from(env::var("PIMATE_TEST_PI")?).canonicalize()?;
    let pkg = PathBuf::from(env::var("PIMATE_TEST_TS")?)
        .canonicalize()?
        .join("packages/pimate");
    let ab = env::var_os("PIMATE_TEST_ABDUCO")
        .map(PathBuf::from)
        .unwrap_or_else(|| pkg.join("bin/linux-x64/abduco"))
        .canonicalize()?;
    let ext = pkg.join("test/fixtures/m2.ts");
    let vars = [
        ("PIMATE_ABDUCO", ab.as_path()),
        ("PIMATE_PI", &pi),
        ("PIMATE_EXTENSION", &ext),
        ("HOME", &path),
        ("PIMATE_E2E", Path::new("1")),
        ("PI_OFFLINE", Path::new("1")),
        ("TERM", Path::new("xterm-256color")),
    ];
    let mut s = Server::with(tmp.clone(), &vars).await?;
    let _guard = Cleanup {
        dir: path.clone(),
        ab: ab.clone(),
    };
    let token = s.paired().await?["token"].clone();
    let mut ws = s.auth(&token).await?;
    send(&mut ws,json!({"type":"spawn","requestId":"s","params":{"cwd":path,"model":{"provider":"pimate-faux","id":"scripted"},"message":"say hello"}})).await?;
    let reply = until(&mut ws, |v| v["type"] == "spawn_result").await?;
    ensure!(
        reply["ok"] == true && reply.get("messageError").is_none(),
        "{reply}"
    );
    let id = &reply["instanceId"];
    let name = reply["abducoSession"].as_str().unwrap();
    let meta = path
        .join("run")
        .join(format!("{}.json", id.as_str().unwrap()));
    let pid = serde_json::from_slice::<Value>(&std::fs::read(meta)?)?["pid"].clone();
    if cfg!(target_os = "linux")
        && std::process::Command::new("systemctl")
            .args(["--user", "show-environment"])
            .stdout(Stdio::null())
            .status()?
            .success()
    {
        ensure!(
            std::fs::read_to_string(format!("/proc/{pid}/cgroup"))?
                .contains(&format!("{name}.scope"))
        )
    }
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(s._child.id().unwrap() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )?;
    ensure!(s._child.wait().await?.success());
    drop(ws);
    let s = Server::with(s.dir, &vars).await?;
    let mut ws = s.auth(&token).await?;
    until(&mut ws, |m| {
        m["instances"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["instanceId"] == *id))
    })
    .await?;
    let pty = nix::pty::openpty(
        Some(&nix::pty::Winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )?;
    let slave = File::from(pty.slave);
    let mut attach = cli(&path)
        .envs(vars)
        .args(["attach", name])
        .stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave)
        .spawn()?;
    let mut master = tokio::fs::File::from_std(File::from(pty.master));
    timeout(Duration::from_secs(12), async {
        let mut buf = [0; 8192];
        let mut screen = vec![];
        loop {
            let n = master.read(&mut buf).await?;
            ensure!(n > 0);
            screen.extend_from_slice(&buf[..n]);
            if String::from_utf8_lossy(&screen).contains("faux") {
                break;
            }
            ensure!(screen.len() < 2 << 20)
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    master.write_all(&[0x1c]).await?;
    timeout(Duration::from_secs(3), attach.wait()).await??;
    send(
        &mut ws,
        json!({"type":"auth_session","requestId":"login","holdId":"one"}),
    )
    .await?;
    let login = until(&mut ws, |m| m["type"] == "auth_session_result").await?;
    ensure!(login["ok"] == true, "{login}");
    let login = &login["instanceId"];
    let loginmeta = path
        .join("run")
        .join(format!("{}.json", login.as_str().unwrap()));
    ensure!(loginmeta.exists());
    let mut other = s.auth(&token).await?;
    let list = until(&mut other, |m| m["type"] == "instances").await?;
    ensure!(
        !list["instances"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["instanceId"] == *login)
    );
    send(
        &mut other,
        json!({"type":"auth_session","requestId":"again","holdId":"one"}),
    )
    .await?;
    assert_eq!(
        until(&mut other, |m| m["type"] == "auth_session_result").await?["instanceId"],
        *login
    );
    drop(other);
    for (method, ok) in [("auth_list", true), ("shutdown", false)] {
        send(
            &mut ws,
            json!({"type":"rc","instanceId":login,"request":{"id":"auth","method":method}}),
        )
        .await?;
        assert_eq!(
            until(&mut ws, |m| m["type"] == "rc_result").await?["response"]["ok"],
            ok
        )
    }
    send(&mut ws, json!({"type":"auth_session_end","holdId":"one"})).await?;
    send(
        &mut ws,
        json!({"type":"rc","instanceId":id,"request":{"id":"stop","method":"shutdown"}}),
    )
    .await?;
    tokio::time::sleep(Duration::from_secs(58)).await;
    ensure!(loginmeta.exists());
    timeout(Duration::from_secs(15), async {
        loop {
            let o = cli(&path).envs(vars).arg("attach").output().await?;
            if !loginmeta.exists() && serde_json::from_slice::<Value>(&o.stdout)? == json!([]) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn folders_and_spawn_validation() -> Result<()> {
    let s = Server::start().await?;
    let root = s.dir.path();
    let project = root.join("project");
    std::fs::create_dir(&project)?;
    let saved = root.join("agent/sessions/example");
    std::fs::create_dir_all(&saved)?;
    std::fs::write(
        saved.join("first.jsonl"),
        format!(
            "{}\nnot a session header",
            json!({"type":"session","cwd":project})
        ),
    )?;
    std::fs::write(
        root.join("agent/trust.json"),
        json!({root.to_str().unwrap():false}).to_string(),
    )?;
    let mut ws = s.auth(&s.paired().await?["token"]).await?;
    send(&mut ws, json!({"type":"folders","requestId":"f"})).await?;
    let folders = until(&mut ws, |v| v["type"] == "folders_result").await?;
    assert_eq!(folders["folders"][0]["path"], json!(project));
    assert_eq!(folders["folders"][0]["trusted"], false);
    send(&mut ws, json!({"type":"dirs","requestId":"d","path":root})).await?;
    assert!(
        until(&mut ws, |v| v["type"] == "dirs_result").await?["dirs"]
            .as_array()
            .unwrap()
            .contains(&json!("project"))
    );
    for (params, code) in [
        (json!({"cwd":"relative"}), "bad_request"),
        (json!({"cwd":root,"trust":null}), "bad_request"),
        (json!({"cwd":root.join("missing")}), "not_found"),
        (json!({"cwd":saved.join("first.jsonl")}), "not_directory"),
    ] {
        send(
            &mut ws,
            json!({"type":"spawn","requestId":"s","params":params}),
        )
        .await?;
        let v = until(&mut ws, |v| v["type"] == "spawn_result").await?;
        assert_eq!(v["code"], code)
    }
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn reject_modified_bundle_even_if_on_path() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir()?;
    let root = t.path();
    let bin = root.join("bin/linux-x64");
    std::fs::create_dir_all(&bin)?;
    let host = bin.join("pimate");
    std::fs::copy(env!("CARGO_BIN_EXE_pimate"), &host)?;
    let ab = bin.join("abduco");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/linux-x64/abduco"),
        &ab,
    )?;
    let attach = || {
        let mut c = Command::new(&host);
        c.env_clear()
            .env("HOME", root)
            .env("ABDUCO_SOCKET_DIR", root)
            .env("PIMATE_RUN_DIR", root.join("run"))
            .env("PI_CODING_AGENT_DIR", root.join("agent"))
            .env("PIMATE_CONFIG_DIR", root.join("config"))
            .env("PATH", &bin)
            .arg("attach");
        c
    };
    ensure!(attach().output().await?.status.success());
    std::fs::write(&ab, "#!/bin/sh\nexit 0\n")?;
    std::fs::set_permissions(&ab, std::fs::Permissions::from_mode(0o755))?;
    ensure!(!attach().output().await?.status.success());
    ensure!(
        attach()
            .env("PIMATE_ABDUCO", &ab)
            .output()
            .await?
            .status
            .success()
    );
    Ok(())
}
