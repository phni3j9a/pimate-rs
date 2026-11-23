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
    c.current_dir(dir)
        .env("PIMATE_CONFIG_DIR", dir.join("config"))
        .env("PIMATE_RUN_DIR", dir.join("run"))
        .kill_on_drop(true);
    c
}
struct Server {
    _child: Child,
    addr: String,
    dir: tempfile::TempDir,
}
impl Server {
    async fn start() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let mut child = cli(dir.path())
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
    huge.send(Message::Text("x".repeat((1 << 20) + 1).into()))
        .await?;
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
