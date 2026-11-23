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
fn socket_files(run: &Path) -> PathBuf {
    use nix::unistd::{Uid, User};
    let uid = Uid::current();
    run.join("t/pt").join(
        User::from_uid(uid)
            .unwrap()
            .map(|u| u.name)
            .unwrap_or_else(|| uid.to_string()),
    )
}
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
        // A release v0.6 list has no keeper PID. Stop our registered Pi first and
        // let list consume its exit notification, even if an assertion interrupted the test.
        let run = self.dir.join("run");
        for e in std::fs::read_dir(&run).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json")
                && let Ok(bytes) = std::fs::read(&p)
                && let Ok(meta) = serde_json::from_slice::<Value>(&bytes)
                && let (Some(id), Some(n), Some(pid)) = (
                    meta["instanceId"].as_str(),
                    meta["abducoSession"].as_str(),
                    meta["pid"]
                        .as_i64()
                        .and_then(|n| i32::try_from(n).ok())
                        .filter(|p| *p > 0),
                )
                && uuid::Uuid::parse_str(id).is_ok()
                && p.file_stem().is_some_and(|f| f == id)
                && n == format!("pimate-{}", &id[..8])
            {
                if cfg!(target_os = "linux")
                    && std::fs::read_to_string(format!("/proc/{pid}/cgroup")).is_ok_and(|groups| {
                        groups.lines().any(|g| g.ends_with(&format!("/{n}.scope")))
                    })
                    && std::process::Command::new("systemctl")
                        .args(["--user", "stop", &format!("{n}.scope")])
                        .status()
                        .is_ok_and(|s| s.success())
                {
                    continue;
                }
                let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
            }
        }
        for _ in 0..10 {
            if std::fs::read_dir(socket_files(&run)).is_ok_and(|mut d| d.next().is_none()) {
                break;
            }
            let _ = std::process::Command::new(env!("CARGO_BIN_EXE_pimate"))
                .arg("list")
                .env("PIMATE_ABDUCO", &self.ab)
                .env("PIMATE_RUN_DIR", &run)
                .env("PIMATE_CONFIG_DIR", self.dir.join("config"))
                .env("PI_CODING_AGENT_DIR", self.dir.join("agent"))
                .env("ABDUCO_SOCKET_DIR", &self.dir)
                .env("HOME", &self.dir)
                .output();
            std::thread::sleep(Duration::from_millis(100));
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
            if !loginmeta.exists()
                && serde_json::from_slice::<Value>(&o.stdout)? == json!([])
                && std::fs::read_dir(socket_files(&path.join("run")))?
                    .next()
                    .is_none()
            {
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

fn script(path: &Path, body: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn scoped_spawn_uses_host_path_and_finishes_prompt_after_disconnect() -> Result<()> {
    use tokio_util::codec::{Framed, LinesCodec};
    let t = std::sync::Arc::new(tempfile::tempdir()?);
    let root = t.path();
    let project = root.join("project");
    let bin = root.join("bin");
    std::fs::create_dir(&bin)?;
    std::fs::create_dir_all(project.join("bin"))?;
    script(&bin.join("systemctl"), "exit 0")?;
    script(
        &bin.join("systemd-run"),
        r#"if [ "$1" = --help ]; then echo "$0" > "$HOME/help"; echo --expand-environment=; exit 0; fi
 echo "$0" > "$HOME/runner"; echo "$PATH" > "$HOME/child-path"
 while [ "$1" != -- ]; do shift; done; shift; exec "$@""#,
    )?;
    script(&project.join("bin/systemd-run"), "exit 42")?;
    script(&bin.join("pi"), "exit 1")?;
    script(
        &bin.join("abduco"),
        r#"if [ "$#" -eq 0 ]; then exit 0; fi
 echo "$ABDUCO_SOCKET_DIR" > "$HOME/ns"; printf '%s\n' "$@" > "$HOME/args"; echo "$PIMATE_INSTANCE_ID" > "$HOME/started""#,
    )?;
    let s = Server::with(
        t.clone(),
        &[
            ("PATH", Path::new("bin")),
            ("PIMATE_EXTENSION", Path::new("npm:@phni3j9a/pimate")),
            ("PIMATE_ABDUCO", Path::new("abduco")),
        ],
    )
    .await?;
    let mut ws = s.auth(&s.paired().await?["token"]).await?;
    send(&mut ws,json!({"type":"spawn","requestId":"launch","params":{"cwd":project,"message":"deliver once"}})).await?;
    let id = timeout(Duration::from_secs(4), async {
        loop {
            if let Ok(s) = std::fs::read_to_string(root.join("started"))
                && !s.trim().is_empty()
            {
                break s.trim().to_owned();
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await?;
    ws.close(None).await?;
    drop(ws);
    for file in ["help", "runner"] {
        assert_eq!(
            Path::new(std::fs::read_to_string(root.join(file))?.trim()),
            bin.join("systemd-run")
        );
    }
    assert_eq!(
        Path::new(std::fs::read_to_string(root.join("child-path"))?.trim()),
        bin
    );
    let args = std::fs::read_to_string(root.join("args"))?;
    let args: Vec<_> = args.lines().collect();
    assert_eq!(Path::new(args[2]), bin.join("pi"));
    assert_eq!(
        Path::new(args[1]).parent(),
        Some(socket_files(&root.join("run")).as_path())
    );
    assert_eq!(
        std::fs::read_to_string(root.join("ns"))?.trim(),
        root.join("run/t").to_str().unwrap()
    );
    assert_eq!(&args[3..], &["-e", "npm:@phni3j9a/pimate"]);
    let listener = tokio::net::UnixListener::bind(root.join("run").join(format!("{id}.sock")))?;
    let mut io = Framed::new(
        timeout(Duration::from_secs(4), listener.accept()).await??.0,
        LinesCodec::new(),
    );
    let hello: Value = serde_json::from_str(&io.next().await.unwrap()?)?;
    io.send(json!({"type":"response","id":hello["id"],"ok":true,"result":{"snapshot":{"instanceId":id,"protocol":1}}}).to_string()).await?;
    let prompt: Value =
        serde_json::from_str(&timeout(Duration::from_secs(3), io.next()).await?.unwrap()?)?;
    assert_eq!(prompt["method"], "prompt");
    assert_eq!(prompt["params"]["text"], "deliver once");
    io.send(json!({"type":"response","id":prompt["id"],"ok":true}).to_string())
        .await?;
    Ok(())
}

#[tokio::test]
async fn native_list_formats_reap_only_private_sockets() -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use tokio::net::UnixListener;
    let tmp = tempfile::tempdir()?;
    let root = tmp.path();
    let run = root.join("a");
    let private = socket_files(&run);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&private)?;
    let external = root.join("old");
    std::fs::create_dir(&external)?;
    let host = nix::unistd::gethostname()?.into_string().unwrap();
    let own = private.join(format!("pimate-aaaaaaaa@{host}"));
    let foreign = external.join(format!("pimate-bbbbbbbb@{host}"));
    let _listener = UnixListener::bind(&own)?;
    let _foreign = UnixListener::bind(&foreign)?;
    // A hostname change hides the row from abduco list but its literal path is still usable.
    let renamed = private.join("pimate-cccccccc@previous-host");
    let _renamed = UnixListener::bind(&renamed)?;
    std::fs::set_permissions(&renamed, std::fs::Permissions::from_mode(0o610))?;
    let stale = private.join("pimate-dddddddd@previous-host");
    drop(UnixListener::bind(&stale)?);
    let fake = root.join("abduco");
    script(
        &fake,
        r#"if [ "$#" = 0 ]; then cat "$ABDUCO_SOCKET_DIR/fixture"; else printf '%s\n' "$3" >> "$ABDUCO_SOCKET_DIR/calls"; fi"#,
    )?;
    let cmd = || {
        let mut c = cli(root);
        c.env("PIMATE_ABDUCO", &fake)
            .env("PIMATE_RUN_DIR", &run)
            .env("ABDUCO_SOCKET_DIR", &external);
        c
    };
    for (column, pid) in [("", Value::Null), ("12345\t", json!(12345))] {
        std::fs::write(
            run.join("t/fixture"),
            format!(
                "Active sessions (on host example)\n+ Mon\t 2026-11-23 09:45:00\t{column}pimate-aaaaaaaa\n"
            ),
        )?;
        let rows: Vec<Value> = serde_json::from_slice(&cmd().arg("attach").output().await?.stdout)?;
        ensure!(
            rows.len() == 2
                && rows.iter().any(|s| s["name"] == "pimate-aaaaaaaa"
                    && s["pid"] == pid
                    && s["terminated"] == true)
        );
        assert!(!stale.exists());
        ensure!(cmd().arg("list").output().await?.status.success());
        ensure!(
            cmd()
                .args(["attach", "pimate-cccccccc"])
                .output()
                .await?
                .status
                .success()
        );
        let calls = std::fs::read_to_string(run.join("t/calls"))?;
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            [
                own.to_str().unwrap(),
                renamed.to_str().unwrap(),
                renamed.to_str().unwrap()
            ]
        );
        std::fs::remove_file(run.join("t/calls"))?;
        ensure!(
            !cmd()
                .args(["attach", "pimate-bbbbbbbb"])
                .output()
                .await?
                .status
                .success()
        );
    }
    Ok(())
}

#[tokio::test]
async fn orphan_wait_does_not_block_discovery_or_repeat_shutdown() -> Result<()> {
    use tokio_util::codec::{Framed, LinesCodec};
    let s = Server::start().await?;
    let run = s.dir.path().join("run");
    let meta = |id: &str| json!({"protocol":1,"instanceId":id,"pid":std::process::id(),"cwd":"/test","origin":"mobile","abducoSession":null,"session":null,"startedAt":1,"status":"idle"});
    let mut orphan = meta("orphan");
    orphan["purpose"] = "auth".into();
    let listener = tokio::net::UnixListener::bind(run.join("orphan.sock"))?;
    std::fs::write(run.join("orphan.json"), orphan.to_string())?;
    let mut ws = s.auth(&s.paired().await?["token"]).await?;
    let mut io = Framed::new(
        timeout(Duration::from_secs(4), listener.accept()).await??.0,
        LinesCodec::new(),
    );
    let hello: Value = serde_json::from_str(&io.next().await.unwrap()?)?;
    io.send(json!({"type":"response","id":hello["id"],"ok":true,"result":{"snapshot":{"instanceId":"orphan","protocol":1}}}).to_string()).await?;
    let list: Value = serde_json::from_str(&io.next().await.unwrap()?)?;
    assert_eq!(list["method"], "auth_list");
    std::fs::write(run.join("normal.json"), meta("normal").to_string())?;
    timeout(
        Duration::from_secs(5),
        until(&mut ws, |v| {
            v["instances"]
                .as_array()
                .is_some_and(|a| a.iter().any(|i| i["instanceId"] == "normal"))
        }),
    )
    .await??;
    assert!(
        timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn completion_collates_before_cutoff() -> Result<()> {
    let t = std::sync::Arc::new(tempfile::tempdir()?);
    let root = t.path();
    let folder = root.join("complete");
    std::fs::create_dir(&folder)?;
    for n in 0..1000 {
        std::fs::create_dir(folder.join(format!("Zebra{n:04}")))?;
    }
    std::fs::create_dir(folder.join("äpple"))?;
    for (locale, first) in [("en_US.UTF-8", "äpple"), ("sv_SE.UTF-8", "Zebra0000")] {
        let s = Server::with(t.clone(), &[("LANG", Path::new(locale))]).await?;
        let mut ws = s.auth(&s.paired().await?["token"]).await?;
        send(
            &mut ws,
            json!({"type":"dirs","requestId":"d","path":folder}),
        )
        .await?;
        let v = until(&mut ws, |v| v["type"] == "dirs_result").await?;
        assert_eq!(v["dirs"][0], first);
        assert_eq!(v["dirs"].as_array().unwrap().len(), 1000);
    }
    Ok(())
}

#[tokio::test]
async fn released_or_disconnected_auth_waiters_never_launch_another_pi() -> Result<()> {
    for disconnect in [false, true] {
        let tmp = std::sync::Arc::new(tempfile::tempdir()?);
        let root = tmp.path();
        let fake = root.join("abduco");
        script(
            &fake,
            r#"echo started >> "$HOME/launches"
 for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
   [ -e "$HOME/proceed" ] && break; /bin/sleep .1
 done
 echo finished >> "$HOME/completions"; exit 1"#,
        )?;
        let s = Server::with(
            tmp.clone(),
            &[
                ("PIMATE_PI", Path::new("/bin/true")),
                ("PIMATE_ABDUCO", &fake),
                ("PATH", root),
            ],
        )
        .await?;
        let token = s.paired().await?["token"].clone();
        let mut ws = s.auth(&token).await?;
        let ask = |n| json!({"type":"auth_session","holdId":"h","requestId":format!("login{n}")});
        send(&mut ws, ask(0)).await?;
        timeout(Duration::from_secs(2), async {
            while !root.join("launches").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await
            }
        })
        .await?;
        for n in 1..4 {
            send(&mut ws, ask(n)).await?
        }
        send(&mut ws, json!({"type":"ping","t":1})).await?;
        until(&mut ws, |v| v["type"] == "pong").await?;
        if disconnect {
            ws.close(None).await?;
            drop(ws);
            ws = s.auth(&token).await?;
        } else {
            send(&mut ws, json!({"type":"auth_session_end","holdId":"h"})).await?;
            send(&mut ws, json!({"type":"ping","t":2})).await?;
            until(&mut ws, |v| v["type"] == "pong").await?;
        }
        send(&mut ws, ask(4)).await?;
        send(&mut ws, json!({"type":"ping","t":3})).await?;
        until(&mut ws, |v| v["type"] == "pong").await?;
        std::fs::write(root.join("proceed"), "")?;
        until(&mut ws, |v| v["requestId"] == "login4").await?;
        assert_eq!(
            std::fs::read_to_string(root.join("launches"))?
                .lines()
                .count(),
            2
        );
        assert_eq!(
            std::fs::read_to_string(root.join("completions"))?
                .lines()
                .count(),
            2
        );
    }
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn native_abduco_prunes_only_refused_local_sockets() -> Result<()> {
    use std::os::unix::net::UnixListener;
    let t = tempfile::tempdir()?;
    let root = t.path();
    let run = root.join("run");
    let ab = env::var_os("PIMATE_TEST_ABDUCO")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../bin/linux-x64/abduco"
            ))
        });
    let list = || {
        let mut c = cli(root);
        c.env("PIMATE_ABDUCO", &ab).arg("list");
        c
    };
    ensure!(list().output().await?.status.success());
    let dir = socket_files(&run);
    let host = nix::unistd::gethostname()?.into_string().unwrap();
    let stale = dir.join(format!("pimate-00000000@{host}"));
    let regular = dir.join(format!("pimate-11111111@{host}"));
    let other = root.join(format!("pimate-22222222@{host}"));
    drop(UnixListener::bind(&stale)?);
    drop(UnixListener::bind(&other)?);
    std::fs::write(&regular, "unchanged")?;
    ensure!(list().output().await?.status.success());
    assert!(!stale.exists());
    assert_eq!(std::fs::read_to_string(&regular)?, "unchanged");
    assert!(other.exists());
    Ok(())
}

#[tokio::test]
async fn long_rc_path_rejects_ws_spawn_before_launcher_runs() -> Result<()> {
    let t = std::sync::Arc::new(tempfile::tempdir()?);
    let run = t.path().join("x".repeat(70));
    let fake = t.path().join("abduco");
    script(
        &fake,
        "if [ \"$1\" = -n ]; then echo unexpected > \"$HOME/launched\"; fi",
    )?;
    let s = Server::with(
        t.clone(),
        &[
            ("PIMATE_RUN_DIR", &run),
            ("PIMATE_ABDUCO", &fake),
            ("PIMATE_PI", Path::new("/bin/true")),
        ],
    )
    .await?;
    let mut ws = s.auth(&s.paired().await?["token"]).await?;
    send(
        &mut ws,
        json!({"type":"spawn","requestId":"long","params":{"cwd":t.path()}}),
    )
    .await?;
    let reply = until(&mut ws, |v| v["type"] == "spawn_result").await?;
    assert_eq!(reply["ok"], false);
    assert!(
        reply["message"]
            .as_str()
            .unwrap()
            .contains("RC socket path too long")
    );
    assert!(!t.path().join("launched").exists());
    Ok(())
}
