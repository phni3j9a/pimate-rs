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
    cli_exe(dir, Path::new(env!("CARGO_BIN_EXE_pimate")))
}
fn cli_exe(dir: &Path, exe: &Path) -> Command {
    let mut c = Command::new(exe);
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
        .env("XDG_CONFIG_HOME", dir.join("xdg"))
        .env("HOME", dir)
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
    real_turn(false).await
}
#[tokio::test]
#[ignore = "requires real Pi and faux fixture"]
async fn real_pi_agent_cli() -> Result<()> {
    real_turn(true).await
}
async fn real_turn(agent_cli: bool) -> Result<()> {
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
        .env("TERM", "xterm-256color")
        .env("XDG_CONFIG_HOME", dir.join("xdg"))
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
    if agent_cli {
        for (args, expected) in [
            (
                vec!["prompt", "real-e2e", "say hello", "--wait"],
                json!({"deliveredAs":"prompt","outcome":"done","reply":"Hello from the PiMate faux model."}),
            ),
            (
                vec!["wait", "real-e2e"],
                json!({"outcome":"idle","reply":"Hello from the PiMate faux model."}),
            ),
            (
                vec!["history", "real-e2e", "--last", "1"],
                json!([{"kind":"assistant","text":"Hello from the PiMate faux model."}]),
            ),
        ] {
            let o = timeout(Duration::from_secs(20), cli(dir).args(args).output()).await??;
            ensure!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            assert_eq!(serde_json::from_slice::<Value>(&o.stdout)?, expected);
        }
        child.kill().await?;
        return Ok(());
    }
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

// The keeper creates a new PTY process group. The test gate is independent of RC readiness.
struct Cleanup {
    dir: PathBuf,
}
impl Cleanup {
    fn install(dir: &Path) -> Result<Self> {
        std::fs::write(dir.join("live"), "")?;
        script(
            &dir.join("test-pi"),
            r#"set -eu
[ -f "$HOME/live" ] || exit 1
[ "$(ps -o pgid= -p $$ | tr -d ' ')" = "$$" ] || exit 1
(while [ -f "$HOME/live" ] && kill -0 "$$" 2>/dev/null; do sleep .1; done
 [ -f "$HOME/live" ] || kill -KILL 0) </dev/null >/dev/null 2>&1 &
exec "$PIMATE_TEST_TARGET" "$@""#,
        )?;
        Ok(Self { dir: dir.into() })
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dir.join("live"));
        let run = self.dir.join("run");
        for _ in 0..30 {
            if !run.join("t").exists()
                || std::fs::read_dir(run.join("t")).is_ok_and(|mut d| d.next().is_none())
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if std::thread::panicking() {
            eprintln!("test keeper cleanup timed out: {}", run.display());
        } else {
            panic!("test keeper cleanup timed out: {}", run.display())
        }
    }
}
#[tokio::test]
#[ignore = "requires Pi and TS PiMate faux fixture"]
async fn real_pi_spawn_attach_restart_login() -> Result<()> {
    let tmp = std::sync::Arc::new(tempfile::tempdir()?);
    let path = tmp.path().to_path_buf();
    let pi = PathBuf::from(env::var("PIMATE_TEST_PI")?).canonicalize()?;
    let pkg = PathBuf::from(env::var("PIMATE_TEST_TS")?)
        .canonicalize()?
        .join("packages/pimate");
    let ext = pkg.join("test/fixtures/m2.ts");
    let wrapper = path.join("test-pi");
    let _guard = Cleanup::install(&path)?;
    let vars = [
        ("PIMATE_PI", wrapper.as_path()),
        ("PIMATE_TEST_TARGET", &pi),
        ("PIMATE_EXTENSION", &ext),
        ("HOME", &path),
        ("PIMATE_E2E", Path::new("1")),
        ("PI_OFFLINE", Path::new("1")),
        ("TERM", Path::new("xterm-256color")),
    ];
    let mut s = Server::with(tmp.clone(), &vars).await?;
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
    let result = timeout(
        Duration::from_secs(10),
        cli(&path)
            .args(["wait", id.as_str().unwrap(), "--timeout", "8"])
            .output(),
    )
    .await??;
    ensure!(result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout)?["reply"],
        "Hello from the PiMate faux model."
    );
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
    ensure!(
        timeout(Duration::from_secs(3), attach.wait())
            .await??
            .success()
    );
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
                && std::fs::read_dir(path.join("run/t"))?.next().is_none()
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
    script(
        &bin.join("pi"),
        r#"printf '%s\n' "$0" "$@" > "$HOME/args"; echo "$PIMATE_INSTANCE_ID" > "$HOME/started""#,
    )?;
    let s = Server::with(
        t.clone(),
        &[
            ("PATH", Path::new("bin")),
            ("PIMATE_EXTENSION", Path::new("npm:@phni3j9a/pimate")),
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
    assert_eq!(Path::new(args[0]), bin.join("pi"));
    assert_eq!(&args[1..], &["-e", "npm:@phni3j9a/pimate"]);
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
async fn reachable_listing_and_direct_attach() -> Result<()> {
    use std::os::unix::{fs::DirBuilderExt, net::UnixListener};
    let t = tempfile::tempdir()?;
    let root = t.path();
    let run = root.join("run");
    let private = run.join("t");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&private)?;
    let name = "pimate-aaaaaaaa";
    let own = private.join(name);
    let _target = UnixListener::bind(&own)?;
    let hanging = UnixListener::bind(private.join("pimate-bbbbbbbb"))?;
    hanging.set_nonblocking(true)?;
    let cmd = || cli(root);
    let peer = tokio::task::spawn_blocking(move || -> Result<()> {
        use std::io::{Read, Write};
        let (mut c, _) = _target.accept()?;
        let mut magic = [0; 4];
        c.read_exact(&mut magic)?;
        ensure!(&magic == b"PMT1");
        c.write_all(b"PMT1")?;
        Ok(())
    });
    let _ = cmd().args(["attach", name]).output().await?;
    peer.await??;
    assert_eq!(
        hanging.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let _target = {
        std::fs::remove_file(&own)?;
        UnixListener::bind(own)?
    };
    let list = cmd().arg("attach").output().await?;
    assert!(list.status.success());
    let rows: Vec<Value> = serde_json::from_slice(&list.stdout)?;
    assert_eq!(rows.len(), 2);
    let alias = root.join("long".repeat(30));
    std::os::unix::fs::symlink(root, &alias)?;
    let out = cmd()
        .env("PIMATE_RUN_DIR", alias.join("run"))
        .arg("attach")
        .output()
        .await?;
    ensure!(out.status.success());
    assert!(serde_json::from_slice::<Vec<Value>>(&out.stdout)?.is_empty());
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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn released_or_disconnected_auth_waiters_never_launch_another_pi() -> Result<()> {
    for disconnect in [false, true] {
        let tmp = std::sync::Arc::new(tempfile::tempdir()?);
        let root = tmp.path();
        script(&root.join("systemctl"), "exit 0")?;
        let fake = root.join("systemd-run");
        script(
            &fake,
            r#"[ "$1" != --help ] || exit 0
echo started >> "$HOME/launches"
 for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
   [ -e "$HOME/proceed" ] && break; /bin/sleep .1
 done
 echo finished >> "$HOME/completions"; exit 1"#,
        )?;
        let s = Server::with(
            tmp.clone(),
            &[("PIMATE_PI", Path::new("/bin/true")), ("PATH", root)],
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

#[tokio::test]
async fn unreachable_sockets_are_hidden_but_never_unlinked() -> Result<()> {
    use nix::sys::{
        stat::{UtimensatFlags, utimensat},
        time::TimeSpec,
    };
    let t = tempfile::tempdir()?;
    let root = t.path();
    let run = root.join("run");
    let cmd = || {
        let mut c = cli(root);
        c.arg("list");
        c
    };
    ensure!(cmd().output().await?.status.success());
    let dir = run.join("t");
    let path = dir.join("pimate-01234567");
    let sock = tokio::net::UnixSocket::new_stream()?;
    sock.bind(&path)?;
    ensure!(cmd().output().await?.status.success());
    assert!(path.exists());
    let epoch = TimeSpec::new(1, 0);
    utimensat(
        File::open(&dir)?,
        &path,
        &epoch,
        &epoch,
        UtimensatFlags::NoFollowSymlink,
    )?;
    let other = root.join("pimate-abcdef01");
    drop(std::os::unix::net::UnixListener::bind(&other)?);
    ensure!(cmd().output().await?.status.success());
    let attach = || {
        let mut c = cli(root);
        c.arg("attach");
        c
    };
    assert_eq!(
        serde_json::from_slice::<Value>(&attach().output().await?.stdout)?,
        json!([])
    );
    assert!(path.exists() && other.exists());
    let listener = sock.listen(5)?;
    let rows: Vec<Value> = serde_json::from_slice(&attach().output().await?.stdout)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "pimate-01234567");
    drop(listener);
    assert_eq!(
        serde_json::from_slice::<Value>(&attach().output().await?.stdout)?,
        json!([])
    );
    assert!(path.exists());
    Ok(())
}

#[tokio::test]
async fn long_rc_path_rejects_ws_spawn_before_launcher_runs() -> Result<()> {
    let t = std::sync::Arc::new(tempfile::tempdir()?);
    let run = t.path().join("x".repeat(70));
    let fake = t.path().join("pi");
    script(&fake, "echo unexpected > \"$HOME/launched\"")?;
    let s = Server::with(t.clone(), &[("PIMATE_RUN_DIR", &run), ("PIMATE_PI", &fake)]).await?;
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

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn real_keeper_cleanup_before_rc_registration() -> Result<()> {
    for detached in [true, false] {
        early_cleanup(detached).await?;
    }
    Ok(())
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
async fn early_cleanup(detached: bool) -> Result<()> {
    use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
    let t = std::sync::Arc::new(tempfile::tempdir()?);
    let root = t.path();
    let cleanup = Cleanup::install(root)?;
    let target = root.join("before-rc");
    script(&target, "echo $$ > \"$HOME/test-pid\"; exec sleep 90")?;
    let pi = root.join("test-pi");
    let mut vars = vec![
        ("PIMATE_PI", pi.as_path()),
        ("PIMATE_TEST_TARGET", &target),
        ("PIMATE_EXTENSION", Path::new("none")),
    ];
    if detached {
        vars.extend([
            ("XDG_RUNTIME_DIR", root),
            (
                "DBUS_SESSION_BUS_ADDRESS",
                Path::new("unix:path=/nonexistent"),
            ),
        ]);
    }
    let s = Server::with(t.clone(), &vars).await?;
    let mut ws = s.auth(&s.paired().await?["token"]).await?;
    send(
        &mut ws,
        json!({"type":"spawn","requestId":"early","params":{"cwd":root}}),
    )
    .await?;
    let r = until(&mut ws, |v| v["type"] == "spawn_result").await?;
    ensure!(r["ok"] == true, "{r}");
    let name = r["abducoSession"].as_str().unwrap();
    let socket = root.join("run/t").join(name);
    assert!(socket.exists());
    let pid = timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(s) = std::fs::read_to_string(root.join("test-pid"))
                && let Ok(p) = s.trim().parse::<i32>()
            {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await?;
    assert!(
        !root
            .join("run")
            .join(format!("{}.json", r["instanceId"].as_str().unwrap()))
            .exists()
    );
    let scoped =
        std::fs::read_to_string(format!("/proc/{pid}/cgroup"))?.contains(&format!("/{name}.scope"));
    if detached {
        assert!(!scoped);
    }
    drop(ws);
    drop(s);
    drop(cleanup);
    assert_eq!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH));
    assert!(!socket.exists());
    if scoped {
        assert!(
            !std::process::Command::new("systemctl")
                .args(["--user", "is-active", "--quiet", &format!("{name}.scope")])
                .status()?
                .success()
        );
    }
    Ok(())
}

#[tokio::test]
async fn github_admin_fixed_commands_and_rejection() -> Result<()> {
    let t = std::sync::Arc::new(tempfile::tempdir()?);
    let root = t.path();
    let bin = root.join("bin");
    std::fs::create_dir(&bin)?;
    script(
        &bin.join("gh"),
        r#"printf '%s\n' "$PWD $*" >> "$HOME/calls"
[ "$GH_PROMPT_DISABLED" = 1 ] && [ -z "$GH_REPO" ] || exit 4
case "$1 $2" in 'api --hostname') echo dev;; 'issue view') echo 'could not find https://secret@example.invalid' >&2; exit 1;; 'pr view') echo '{"number":2,"title":"PR"}';; *) exit 1;; esac"#,
    )?;
    let s = Server::with(t.clone(), &[("PATH", &bin)]).await?;
    let repo = root.join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(root.join("run/gh-pi.json"),json!({"protocol":1,"instanceId":"gh-pi","origin":"pc","pid":std::process::id(),"cwd":repo,"status":"idle","startedAt":1,"session":null,"abducoSession":null}).to_string())?;
    let mut ws = s.auth(&s.paired().await?["token"]).await?;
    until(&mut ws, |m| {
        m["instances"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await?;
    for (msg, expected) in [
        (
            json!({"type":"github_status","requestId":"s"}),
            json!({"type":"github_status_result","requestId":"s","ok":true,"status":{"installed":true,"loggedIn":true,"login":"dev"}}),
        ),
        (
            json!({"type":"host_restart","requestId":"r"}),
            json!({"type":"host_restart_result","requestId":"r","ok":false,"code":"unsupported"}),
        ),
        (
            json!({"type":"update","requestId":"u"}),
            json!({"type":"update_result","requestId":"u","ok":false,"code":"unsupported","message":"not_package"}),
        ),
    ] {
        send(&mut ws, msg).await?;
        assert_eq!(
            until(&mut ws, |m| m["type"] == expected["type"]).await?,
            expected
        );
    }
    send(&mut ws,json!({"type":"github","requestId":"q","instanceId":"gh-pi","query":{"kind":"pr","number":2}})).await?;
    let r = until(&mut ws, |m| m["requestId"] == "q").await?;
    assert_eq!(r["data"]["item"]["author"], "（削除済み）");
    send(&mut ws,json!({"type":"github","requestId":"err","instanceId":"gh-pi","query":{"kind":"issue","number":1}})).await?;
    assert_eq!(
        until(&mut ws, |m| m["requestId"] == "err").await?,
        json!({"type":"github_result","requestId":"err","ok":false,"code":"not_found","message":"Issue or pull request not found"})
    );
    let calls = std::fs::read_to_string(root.join("calls"))?;
    assert!(calls.contains(&format!("{} pr view 2 --json", repo.display())));
    send(
        &mut ws,
        json!({"type":"host_restart","requestId":"reject","extra":1}),
    )
    .await?;
    assert!(matches!(ws.next().await, Some(Ok(Message::Close(_)))));
    Ok(())
}

#[tokio::test]
async fn webhook_cli_private_loopback() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir()?;
    let d = t.path();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/secret-path", l.local_addr()?);
    let mut setup = cli(d)
        .args(["webhook", "setup", "--machine", "fixture"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    setup
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{url}\nAuthorization: Bearer example\n").as_bytes())
        .await?;
    let o = setup.wait_with_output().await?;
    assert_eq!(o.stdout, b"saved webhook.json\n");
    assert_eq!(
        std::fs::metadata(d.join("config/webhook.json"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let o = cli(d).args(["webhook", "status"]).output().await?;
    assert_eq!(
        serde_json::from_slice::<Value>(&o.stdout)?,
        json!({"configured":true,"machine":"fixture","watched":[]})
    );
    let job = tokio::spawn(async move {
        for status in [200, 401] {
            let (mut sock, _) = l.accept().await?;
            let mut rd = BufReader::new(&mut sock);
            let mut len = 0;
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                rd.read_line(&mut line).await?;
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length: ") {
                    len = v.trim().parse()?
                }
                headers += &line;
            }
            assert!(headers.contains("authorization: Bearer example"));
            let mut body = vec![0; len];
            rd.read_exact(&mut body).await?;
            let v: Value = serde_json::from_slice(&body)?;
            assert_eq!(v["text"], "Pi webhook test on fixture.");
            sock.write_all(
                format!("HTTP/1.1 {status} Resp\r\nContent-Length: 11\r\n\r\nsecret-path")
                    .as_bytes(),
            )
            .await?;
        }
        Ok::<_, anyhow::Error>(())
    });
    let test = || {
        let mut c = cli(d);
        c.args(["webhook", "test"]);
        c
    };
    let o = test().output().await?;
    assert_eq!(
        serde_json::from_slice::<Value>(&o.stdout)?,
        json!({"ok":true,"status":200})
    );
    let o = test().output().await?;
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(o.stderr)?.trim(),
        "WEBHOOK_REJECTED HTTP 401 (key changed, run setup again)"
    );
    job.await??;
    assert!(cli(d).args(["webhook", "off"]).status().await?.success());
    assert!(!d.join("config/webhook.json").exists());
    Ok(())
}

#[tokio::test]
async fn launch_agent_or_systemd_owns_only_its_unit() -> Result<()> {
    let t = tempfile::tempdir()?;
    let d = t.path();
    let bin = d.join("bin");
    std::fs::create_dir(&bin)?;
    script(
        &bin.join("systemctl"),
        r#"printf '%s\n' "$*" >> "$HOME/calls"
case "$2" in enable) echo enabled > "$HOME/enabled";;stop) /bin/rm -f "$HOME/enabled";;is-active) echo active;;esac"#,
    )?;
    script(&bin.join("loginctl"), "echo no")?;
    script(
        &bin.join("launchctl"),
        r#"echo "$*" >> "$HOME/calls"
case "$1" in
print) [ -f "$HOME/enabled" ] || exit 113; echo "path = $(/bin/cat "$HOME/enabled")"; echo 'state = running';;
bootstrap) echo "$3" > "$HOME/enabled";; bootout) /bin/rm "$HOME/enabled";; *) exit 1;; esac"#,
    )?;
    let mac = cfg!(target_os = "macos");
    let addr = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?;
    let command = |action: &str| {
        let mut c = cli(d);
        c.env("PATH", &bin)
            .env("PIMATE_SERVICE_PORT", addr.port().to_string())
            .args(["service", action]);
        c
    };
    let unit = d.join(if mac {
        "Library/LaunchAgents/dev.pimate.rs.host.plist"
    } else {
        "xdg/systemd/user/pimate-host.service"
    });
    std::fs::create_dir_all(unit.parent().unwrap())?;
    std::fs::write(&unit, "foreign")?;
    assert!(!command("uninstall").status().await?.success());
    assert_eq!(std::fs::read_to_string(&unit)?, "foreign");
    std::fs::remove_file(&unit)?;
    let enabled = d.join("enabled");
    let host = d.join("config/host.json");
    let mock = tokio::spawn(async move {
        while !enabled.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await
        }
        let l = tokio::net::TcpListener::bind(addr).await?;
        let h: Value = serde_json::from_slice(&std::fs::read(host)?)?;
        while enabled.exists() {
            if let Ok(Ok((mut sock, _))) = timeout(Duration::from_millis(10), l.accept()).await {
                let mut buf = [0; 512];
                if sock.read(&mut buf).await? == 0 {
                    continue;
                }
                let body = json!({"ok":true,"protocol":1,"hostId":h["hostId"]}).to_string();
                sock.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await?
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    assert!(command("install").status().await?.success());
    assert!(std::fs::read_to_string(&unit)?.contains(if mac {
        "<key>Umask</key><integer>63"
    } else {
        "UMask=0077"
    }));
    if mac {
        ensure!(
            Command::new("/usr/bin/plutil")
                .arg("-lint")
                .arg(&unit)
                .status()
                .await?
                .success()
        );
    }
    let status = command("status").output().await?;
    assert_eq!(
        serde_json::from_slice::<Value>(&status.stdout)?,
        json!({"active":true,"installed":true,"linger":if mac {"not applicable"}else{"no"}})
    );
    assert!(command("uninstall").status().await?.success());
    mock.await??;
    assert!(!unit.exists());
    let log = std::fs::read_to_string(d.join("calls"))?;
    if mac {
        assert!(log.lines().any(|l| l.starts_with("bootstrap gui/")));
        assert!(log.lines().any(|l| l.starts_with("bootout gui/")));
        return Ok(());
    }
    for cmd in [
        "--user enable --now pimate-host.service",
        "--user stop pimate-host.service",
        "--user disable pimate-host.service",
        "--user daemon-reload",
    ] {
        assert!(log.lines().any(|l| l == cmd))
    }
    let record = d.join("config/host-process.json");
    std::fs::write(
        &record,
        json!({"owner":"pimate-rs","unit":"pimate-host-1-2"}).to_string(),
    )?;
    for (load, ok) in [("loaded", false), ("not-found", true)] {
        script(
            &bin.join("systemctl"),
            &format!("case \"$2\" in stop) exit 5;; show) echo {load};exit 4;; esac"),
        )?;
        assert_eq!(command("stop").status().await?.success(), ok);
        assert_eq!(record.exists(), !ok);
    }
    Ok(())
}

// Same local RC script and golden JSON for the Rust binary and (optionally) upstream Node CLI.
#[tokio::test]
async fn agent_cli_contract() -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    use tokio::net::UnixListener;
    use tokio_util::codec::{Framed, LinesCodec};
    let t = tempfile::tempdir()?;
    let root = t.path();
    let run = root.join("run");
    std::fs::DirBuilder::new().mode(0o700).create(&run)?;
    let exes = std::iter::once(PathBuf::from(env!("CARGO_BIN_EXE_pimate")))
        .chain(env::var_os("PIMATE_TEST_TS_CLI").map(PathBuf::from));
    for exe in exes {
        for (args, expected) in [
            (
                vec!["prompt", "i", "hello", "--wait"],
                json!({"deliveredAs":"prompt","outcome":"done","reply":"OK"}),
            ),
            (
                vec!["prompt", "i", "hello", "--wait", "--steer"],
                json!({"deliveredAs":"steer","outcome":"failed","error":"bad"}),
            ),
            (
                vec!["prompt", "i", "hello", "--follow-up", "--wait"],
                json!({"deliveredAs":"followUp","outcome":"aborted"}),
            ),
            (
                vec!["wait", "i"],
                json!({"outcome":"waiting_input","pending":[{"kind":"confirm","requestId":"R"}]}),
            ),
            (
                vec!["wait", "i", "--timeout", "2"],
                json!({"outcome":"done","reply":"first"}),
            ),
            (
                vec!["wait", "i", "--timeout", "1"],
                json!({"outcome":"timeout"}),
            ),
            (
                vec!["prompt", "i", "bye", "--wait"],
                json!({"deliveredAs":"prompt","outcome":"closed"}),
            ),
            (
                vec!["history", "i", "--last", "2"],
                json!([{"kind":"user","text":"hello"},{"kind":"assistant","text":"OK"}]),
            ),
            (
                vec!["ui", "i", "R", "--answers", "[1]"],
                json!({"requestId":"R","answers":[1]}),
            ),
            (vec!["abort", "i"], json!({"accepted":true})),
            (vec!["shutdown", "i"], json!({"accepted":true})),
        ] {
            let l = UnixListener::bind(run.join("i.sock"))?;
            let raced = args == ["wait", "i", "--timeout", "2"];
            let pending = args == ["wait", "i"] || raced;
            let job = tokio::spawn(async move {
                let (sock, _) = l.accept().await?;
                let mut io = Framed::new(sock, LinesCodec::new());
                while let Some(line) = io.next().await {
                    let r: Value = serde_json::from_str(&line?)?;
                    let mut close = false;
                    let result = match r["method"].as_str() {
                        Some("hello") => {
                            if raced {
                                let events = [
                                    json!({"type":"ui_request_end","requestId":"R"}),
                                    json!({"type":"message_end","item":{"kind":"assistant","text":"first"}}),
                                    json!({"type":"agent_settled"}),
                                    json!({"type":"agent_start"}),
                                ];
                                for (n, e) in events.into_iter().enumerate() {
                                    io.send(json!({"type":"event","instanceId":"i","seq":n+1,"event":e}).to_string()).await?;
                                }
                            }
                            json!({"snapshot":{"protocol":1,"seq":0,"instanceId":"i","status":"running","pendingUI":if pending{json!([{"kind":"confirm","requestId":"R"}])}else{json!([])}}})
                        }
                        Some("prompt") => {
                            let mode = r["params"]["deliverAs"].as_str().unwrap_or("prompt");
                            close = r["params"]["text"] == "bye";
                            if !close {
                                for(seq,e)in [json!({"type":"agent_start"}),json!({"type":"message_end","item":match mode{"prompt"=>json!({"kind":"assistant","text":"OK"}),"steer"=>json!({"kind":"assistant","stopReason":"error","errorMessage":"bad"}),_=>json!({"kind":"user"})}}),json!({"type":"agent_settled","aborted":mode=="followUp"})].into_iter().enumerate(){io.send(json!({"type":"event","seq":seq+1,"instanceId":"i","event":e}).to_string()).await?}
                            }
                            json!({"deliveredAs":mode})
                        }
                        Some("history") => {
                            json!({"items":[{"kind":"assistant","text":"old"},{"kind":"user","text":"hello"},{"kind":"toolResult"},{"kind":"assistant","text":"OK"}]})
                        }
                        Some("ui_respond") => r["params"].clone(),
                        Some("abort" | "shutdown") => json!({"accepted":true}),
                        _ => bail!("unknown call"),
                    };
                    io.send(
                        json!({"id":r["id"],"type":"response","ok":true,"result":result})
                            .to_string(),
                    )
                    .await?;
                    if close {
                        break;
                    }
                }
                Ok::<_, anyhow::Error>(())
            });
            let o = timeout(
                Duration::from_secs(8),
                cli_exe(root, &exe).args(&args).output(),
            )
            .await??;
            ensure!(
                o.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&o.stdout)?,
                expected,
                "{args:?}"
            );
            job.await??;
            std::fs::remove_file(run.join("i.sock"))?;
        }
        let o = cli_exe(root, &exe)
            .args(["prompt", "i", "hi", "--steer", "--follow-up"])
            .output()
            .await?;
        assert_eq!(o.status.code(), Some(1));
        assert!(o.stdout.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn watched_dialog_and_settle_feed_loopback_without_subscribing() -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    use tokio_util::codec::{Framed, LinesCodec};
    let s = Server::start().await?;
    let root = s.dir.path();
    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/", http.local_addr()?);
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let router = axum::Router::new().route(
        "/",
        axum::routing::post(move |axum::Json(v): axum::Json<Value>| {
            let tx = tx.clone();
            async move {
                tx.send(v).await.unwrap();
                axum::http::StatusCode::NO_CONTENT
            }
        }),
    );
    let web = tokio::spawn(async move { axum::serve(http, router).await });
    let p = root.join("config");
    std::fs::write(
        p.join("webhook.json"),
        json!({"url":url,"machine":"test","headers":{}}).to_string(),
    )?;
    let watch = p.join("webhook-watch");
    std::fs::DirBuilder::new().mode(0o700).create(&watch)?;
    std::fs::write(watch.join("pi"), "")?;
    let token = s.paired().await?["token"].clone();
    let mut ws = s.auth(&token).await?;
    send(&mut ws, json!({"type":"watch_alerts","enabled":true})).await?;
    let run = root.join("run");
    let listener = tokio::net::UnixListener::bind(run.join("pi.sock"))?;
    let task = tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut io = Framed::new(sock, LinesCodec::new());
                let l = io.next().await.context("RC EOF")??;
                let req: Value = serde_json::from_str(&l)?;
                let snap = json!({"instanceId":"pi","protocol":1,"seq":10,"status":"waiting_input","session":{"id":"s"},"pendingUI":[{"requestId":"r","kind":"confirm","title":"Proceed?"}]});
                io.send(
                    json!({"id":req["id"],"type":"response","ok":true,"result":{"snapshot":snap}})
                        .to_string(),
                )
                .await?;
                let events = [
                    json!({"type":"ui_request_end","requestId":"r","by":"tui"}),
                    json!({"type":"session_changed","session":{"id":"s2","name":"New title"}}),
                    json!({"type":"agent_start"}),
                    json!({"type":"message_end","item":{"kind":"assistant","stopReason":"stop","text":"token=secret123"}}),
                    json!({"type":"agent_settled","aborted":false}),
                ];
                for (i, e) in events.into_iter().enumerate() {
                    let session = if i == 0 { "s" } else { "s2" };
                    io.send(json!({"type":"event","instanceId":"pi","sessionId":session,"seq":11+i,"event":e}).to_string()).await?;
                }

                let _ = io.next().await;
                Ok::<_, anyhow::Error>(())
            });
        }
    });
    std::fs::write(run.join("pi.json"),json!({"instanceId":"pi","pid":std::process::id(),"origin":"pc","cwd":root,"startedAt":1,"status":"idle","session":null,"protocol":1,"abducoSession":null}).to_string())?;
    let a = until(&mut ws, |m| m["type"] == "alert").await?;
    assert_eq!(
        a["alert"],
        json!({"id":"pi:15","kind":"complete","instanceId":"pi","sessionId":"s2"})
    );
    let mut kinds = vec![];
    for _ in 0..3 {
        let v = timeout(Duration::from_secs(6), rx.recv())
            .await?
            .context("no POST")?;
        match v["event"].as_str() {
            Some("request.opened") => assert_eq!(v["requestTitle"], "Proceed?"),
            Some("request.resolved") => {
                assert!(kinds.contains(&json!("request.opened")));
                assert_eq!(v["by"], "tui")
            }
            Some("pi.finished") => {
                assert_eq!(v["lastReply"], "[redacted]");
                assert_eq!(v["sessionId"], "s2");
                assert_eq!(v["title"], "New title");
            }
            _ => bail!("unexpected {v}"),
        };
        kinds.push(v["event"].clone());
    }
    assert_eq!(
        kinds.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    assert!(p.join("webhook-state.json").exists());
    task.abort();
    web.abort();
    Ok(())
}

// Once discovery publishes the Pi there is no further discovery tick in the first second.
#[tokio::test]
async fn subscribe_starts_monitor_for_already_discovered_pi() -> Result<()> {
    use tokio_util::codec::{Framed, LinesCodec};
    let s = Server::start().await?;
    let dir = s.dir.path().join("run");
    let l = tokio::net::UnixListener::bind(dir.join("known.sock"))?;
    let mut ws = s.auth(&s.paired().await?["token"]).await?;
    std::fs::write(dir.join("known.json"),json!({"instanceId":"known","pid":std::process::id(),"origin":"pc","cwd":s.dir.path(),"startedAt":1,"status":"idle","session":null,"protocol":1,"abducoSession":null}).to_string())?;
    let hello = async || -> Result<_> {
        let (sock, _) = l.accept().await?;
        let mut io = Framed::new(sock, LinesCodec::new());
        let r: Value = serde_json::from_str(&io.next().await.context("hello")??)?;
        io.send(json!({"id":r["id"],"type":"response","ok":true,"result":{"snapshot":{"protocol":1,"instanceId":"known","seq":0,"session":{"id":"s"}}}}).to_string()).await?;
        Ok(io)
    };
    drop(timeout(Duration::from_secs(6), hello()).await??);
    until(&mut ws, |v| {
        v["instances"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await?;
    send(&mut ws, json!({"type":"watch_alerts","enabled":true})).await?;
    let mut io = timeout(Duration::from_secs(1), hello()).await??;
    io.send(json!({"type":"event","instanceId":"known","sessionId":"s","seq":1,"event":{"type":"status","status":"waiting_input"}}).to_string()).await?;
    assert_eq!(
        until(&mut ws, |v| v["type"] == "alert").await?["alert"]["kind"],
        "input"
    );
    Ok(())
}

#[path = "support/core_flow.rs"]
mod core_flow;

#[tokio::test]
async fn keeper_drains_detached_and_resizes_shell() -> Result<()> {
    let tmp = tempfile::tempdir_in("/tmp")?;
    let root = tmp.path();
    let name = "pimate-12345678";
    let socket = root.join("run/t").join(name);
    ensure!(cli(root).arg("attach").status().await?.success());
    let mut keeper = cli(root).arg("__hold").arg(&socket).args(["/bin/sh", "-c", "dd if=/dev/zero bs=65536 count=128 2>/dev/null && touch drained; while read cmd; do [ \"$cmd\" = stop ] && exit 0; stty size; done"]).spawn()?;
    timeout(Duration::from_secs(8), async {
        while !root.join("drained").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    for rows in [32u8, 42] {
        let mut c = tokio::net::UnixStream::connect(&socket).await?;
        c.write_all(b"PMT1").await?;
        let mut magic = [0; 4];
        c.read_exact(&mut magic).await?;
        assert_eq!(&magic, b"PMT1");
        c.write_all(&[b'S', 0, rows, 0, 80]).await?;
        c.write_all(b"I\0\x05size\n").await?;
        let mut c = BufReader::new(c);
        timeout(Duration::from_secs(3), async {
            let mut line = String::new();
            loop {
                line.clear();
                ensure!(c.read_line(&mut line).await? > 0);
                if line.trim() == format!("{rows} 80") {
                    return Ok::<_, anyhow::Error>(());
                }
            }
        })
        .await??;
        if rows == 42 {
            c.get_mut().write_all(b"I\0\x05stop\n").await?;
        }
        drop(c);
        timeout(Duration::from_secs(3), async {
            use std::os::unix::fs::MetadataExt;
            while socket.metadata().is_ok_and(|m| m.mode() & 0o100 != 0) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
    }
    ensure!(
        timeout(Duration::from_secs(3), keeper.wait())
            .await??
            .success()
    );
    ensure!(!socket.exists());
    Ok(())
}
