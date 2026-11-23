use super::{Server, cli};
use anyhow::{Context, Result, ensure};
use futures_util::{SinkExt, StreamExt};
use pimate_core::{Change, Core, Error, Host, Listener, Operation, SecretStore, Target};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream, UnixListener},
    sync::{broadcast, mpsc},
    time::timeout,
};
use tokio_util::codec::{Framed, LinesCodec};
#[derive(Default)]
struct Storage(Mutex<Option<(Host, String)>>);
impl SecretStore for Storage {
    fn hosts(&self) -> Result<Vec<Host>, Error> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|(h, _)| h.clone())
            .collect())
    }
    fn save(&self, h: Host, t: String) -> Result<(), Error> {
        *self.0.lock().unwrap() = Some((h, t));
        Ok(())
    }
    fn load(&self, _: String) -> Result<pimate_core::Credential, Error> {
        let (host, token) = self.0.lock().unwrap().clone().ok_or(Error::Storage)?;
        Ok(pimate_core::Credential { host, token })
    }
    fn invalidate(&self, id: String, device_id: String) -> Result<(), Error> {
        if let Some((h, _)) = &mut *self.0.lock().unwrap()
            && h.id == id
            && h.device_id == device_id
        {
            h.needs_pairing = true
        }
        Ok(())
    }
    fn forget(&self, _: String) -> Result<(), Error> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
struct Changes(mpsc::UnboundedSender<Change>);
impl Listener for Changes {
    fn changed(&self, _revision: u64, changes: Vec<Change>) {
        for c in changes {
            let _ = self.0.send(c);
        }
    }
}
async fn until(
    rx: &mut mpsc::UnboundedReceiver<Change>,
    collection: &str,
    id: &str,
    expected: Option<&str>,
) -> Result<()> {
    timeout(Duration::from_secs(10), async {
        while let Some(c) = rx.recv().await {
            if let Change::Upsert(i) = c
                && i.collection == collection
                && i.id == id
                && expected
                    .is_none_or(|v| serde_json::from_str::<Value>(&i.json).is_ok_and(|j| j == v))
            {
                return Ok(());
            }
        }
        anyhow::bail!("listener closed")
    })
    .await
    .with_context(|| format!("waiting for {collection}/{id}"))?
}
#[tokio::test]
async fn paired_core_recovers_without_replaying_prompt() -> Result<()> {
    let s = Server::start().await?;
    let proxy = TcpListener::bind("127.0.0.1:0").await?;
    let address = proxy.local_addr()?.to_string();
    let host = s.addr.clone();
    let (cut, _) = broadcast::channel::<()>(2);
    let cutting = cut.clone();
    tokio::spawn(async move {
        while let Ok((mut client, _)) = proxy.accept().await {
            let mut stop = cutting.subscribe();
            let host = host.clone();
            tokio::spawn(async move {
                let mut backend = TcpStream::connect(host).await?;
                tokio::select! {_ =stop.recv()=>{},_=tokio::io::copy_bidirectional(&mut client,&mut backend)=>{}};
                Ok::<_, std::io::Error>(())
            });
        }
    });
    let run = s.dir.path().join("run");
    let meta = json!({"protocol":1,"instanceId":"stub","pid":std::process::id(),"cwd":"/test","origin":"pc","abducoSession":null,"session":{"id":"s"},"startedAt":1,"updatedAt":1,"status":"idle"});
    std::fs::write(run.join("stub.json"), meta.to_string())?;
    let socket = UnixListener::bind(run.join("stub.sock"))?;
    let sends = Arc::new(AtomicUsize::new(0));
    let count = sends.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = socket.accept().await {
            let mut snap = meta.clone();
            snap.as_object_mut().unwrap().extend(
                json!({"model":null,"thinking":null,"seq":1,"pendingUI":[]})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            let count = count.clone();
            let cut = cut.clone();
            tokio::spawn(async move {
                let mut io = Framed::new(sock, LinesCodec::new());
                while let Some(Ok(line)) = io.next().await {
                    let r: Value = serde_json::from_str(&line)?;
                    let recovered = count.load(Ordering::SeqCst) != 0;
                    let result = match r["method"].as_str() {
                        Some("hello") => json!({"snapshot":snap}),
                        Some("history") => {
                            json!({"session":{"id":"s"},"items":if recovered{json!([{"kind":"user","entryId":"u","text":"hey"}])}else{json!([])}})
                        }
                        Some("prompt") => {
                            ensure!(
                                r["params"]["deliverAs"] == "steer" && r["expectSessionId"] == "s"
                            );
                            count.fetch_add(1, Ordering::SeqCst);
                            cut.send(())?;
                            break;
                        }
                        Some("compact") => json!({}),
                        Some("ui_respond") => {
                            ensure!(r["params"]["confirmed"] == true);
                            json!({})
                        }
                        _ => anyhow::bail!("unexpected method"),
                    };
                    io.send(
                        json!({"type":"response","id":r["id"],"ok":true,"result":result})
                            .to_string(),
                    )
                    .await?;
                    if r["method"] == "compact" {
                        // Force a new snapshot after acceptance, then deliberately omit compact_done.
                        io.send(json!({"type":"event","instanceId":"stub","sessionId":"s","seq":3,"event":{"type":"session_changed","reason":"reload","session":{"id":"s"}}}).to_string()).await?;
                    }
                    if r["method"] == "history" {
                        let e = if recovered {
                            json!({"type":"ui_request","request":{"kind":"confirm","requestId":"q","title":"ok?"}})
                        } else {
                            json!({"type":"text_delta","delta":"hello"})
                        };
                        io.send(json!({"type":"event","instanceId":"stub","sessionId":"s","seq":2,"event":e}).to_string()).await?;
                    }
                }
                Ok::<_, anyhow::Error>(())
            });
        }
    });
    let o = cli(s.dir.path())
        .args(["pair", "--address", &address])
        .output()
        .await?;
    ensure!(o.status.success());
    let link = std::str::from_utf8(&o.stdout)?
        .lines()
        .next()
        .unwrap()
        .to_owned();
    let core = Core::new(Arc::new(Storage::default()));
    let h = core.pair(link.clone(), "test".into()).await?;
    assert_eq!(core.hosts()?.as_slice(), std::slice::from_ref(&h));
    assert_eq!(
        core.pair(link, "test".into()).await,
        Err(Error::InvalidCode)
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let c = core.connect(h.id, Arc::new(Changes(tx)))?;
    until(&mut rx, "instance", "stub", None).await?;
    c.subscribe(Some("stub".into()))?;
    for bad in ["", "../stub", "invalid/target", &"x".repeat(101)] {
        assert_eq!(c.subscribe(Some(bad.into())), Err(Error::BadRequest));
    }
    until(&mut rx, "stream", "text", Some("hello")).await?;
    let target = Target {
        instance: "stub".into(),
        session: Some("s".into()),
    };
    assert_eq!(
        c.perform(Operation::Send {
            target: target.clone(),
            text: "hey".into(),
            images: vec![]
        })
        .await,
        Err(Error::OutcomeUnknown)
    );
    until(&mut rx, "host", "connection", Some("offline")).await?;
    assert!(
        c.snapshot()
            .items
            .iter()
            .any(|i| i.collection == "draft" && i.json.contains("unconfirmed"))
    );
    until(&mut rx, "question", "q", None).await?;
    assert!(!c.snapshot().items.iter().any(|i| i.collection == "draft"));
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    let result = c
        .perform(Operation::Answer {
            target: target.clone(),
            params: json!({"requestId":"q","confirmed":true}).to_string(),
        })
        .await?;
    ensure!(serde_json::from_str::<Value>(&result)? == json!({}));
    let compact = Operation::Rc {
        target,
        method: pimate_protocol::RcMethod::Compact,
        params: "{}".into(),
    };
    c.perform(compact.clone()).await?;
    until(&mut rx, "operationResult", "latest", None).await?;
    let snapshot = c.snapshot();
    ensure!(
        snapshot
            .items
            .iter()
            .any(|i| i.collection == "operationResult"
                && serde_json::from_str::<Value>(&i.json).is_ok_and(|v| v["outcome"] == "unknown"))
    );
    c.check(compact)?;
    c.perform(Operation::Revoke {
        device_id: h.device_id,
    })
    .await?;
    until(&mut rx, "host", "connection", Some("revoked")).await?;
    assert_eq!(c.perform(Operation::Devices).await, Err(Error::Offline));
    assert!(core.hosts()?[0].needs_pairing);
    Ok(())
}
