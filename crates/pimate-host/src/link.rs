//! One ordered RC stream per Pi. Mailboxes only receive events they subscribe to
//! and responses to their own requests; a slow phone cannot stall the others.
use crate::{
    rc::{self, Rc},
    server::{LIMIT, Out},
};
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    time::timeout,
};
type Packet = (Arc<Value>, OwnedSemaphorePermit);
#[derive(Clone)]
struct Mail {
    tx: mpsc::Sender<Packet>,
    budget: Arc<Semaphore>,
    out: Out,
}
impl Mail {
    fn deliver(&self, v: &Arc<Value>, len: usize) {
        if self.tx.is_closed() {
            return;
        }
        if len <= LIMIT
            && let Ok(p) = self.budget.clone().try_acquire_many_owned(len as u32)
            && self.tx.try_send((v.clone(), p)).is_ok()
        {
            return;
        }
        if !self.tx.is_closed() {
            self.out.overflow()
        }
    }
}
enum Command {
    Watch(Mail),
    Send(Value, Mail),
}
#[derive(Default)]
pub struct Links(Mutex<HashMap<String, mpsc::WeakSender<Command>>>);
pub struct Client {
    tx: mpsc::Sender<Command>,
    mail: Mail,
    rx: mpsc::Receiver<Packet>,
    id: String,
}
impl Links {
    pub fn client(&self, dir: &Path, id: &str, out: &Out, events: bool) -> Result<Client> {
        let tx = {
            let mut map = self.0.lock().unwrap();
            map.retain(|_, s| s.upgrade().is_some_and(|s| !s.is_closed()));
            if let Some(tx) = map.get(id).and_then(|s| s.upgrade()) {
                tx
            } else {
                let (tx, rx) = mpsc::channel(128);
                map.insert(id.into(), tx.downgrade());
                tokio::spawn(run(dir.into(), id.into(), rx));
                tx
            }
        };
        let (send, rx) = mpsc::channel(8192);
        let mail = Mail {
            tx: send,
            budget: Arc::new(Semaphore::new(LIMIT)),
            out: out.clone(),
        };
        if events {
            tx.try_send(Command::Watch(mail.clone()))
                .map_err(|_| anyhow!("RC busy or closed"))?
        }
        Ok(Client {
            tx,
            rx,
            mail,
            id: id.into(),
        })
    }
}
async fn run(dir: PathBuf, id: String, mut cmds: mpsc::Receiver<Command>) -> Result<()> {
    let mut rc = timeout(Duration::from_secs(5), Rc::open(&dir, &id)).await??;
    let mut watchers: Vec<Mail> = vec![];
    let mut verified = false;
    let mut pending = HashMap::<String, Mail>::new();
    loop {
        let cmd = tokio::select! {
            cmd=cmds.recv()=>cmd,
            frame=rc.next()=>{
                let v=Arc::new(frame?);
                if v["type"]=="event" && v["instanceId"]==id {
                    let bytes=v.to_string().len();
                    watchers.retain(|m| !m.tx.is_closed());
                    for m in &watchers { m.deliver(&v,bytes); }
                } else if v["type"]=="response" && let Some(mail)=v["id"].as_str().and_then(|k|pending.remove(k)) {
                    mail.deliver(&v,v.to_string().len());
                }
                continue
            }
        };
        match cmd {
            None => return Ok(()),
            Some(Command::Watch(mail)) => {
                watchers.retain(|m| !m.tx.is_closed());
                watchers.push(mail)
            }
            Some(Command::Send(req, mail)) => {
                if mail.tx.is_closed() {
                    continue;
                }
                if !verified {
                    let snapshot = timeout(
                        Duration::from_secs(5),
                        rc.hello(|v| {
                            let len = v.to_string().len();
                            let v = Arc::new(v);
                            for w in &watchers {
                                w.deliver(&v, len)
                            }
                            Ok(())
                        }),
                    )
                    .await??;
                    ensure!(snapshot["purpose"].is_null(), "reserved RC instance");
                    verified = true;
                    if mail.tx.is_closed() {
                        continue;
                    }
                }
                let key = req["id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing id"))?
                    .to_owned();
                if pending.contains_key(&key) {
                    let err = Arc::new(
                        json!({"type":"response","id":key,"ok":false,"error":{"code":"bad_request","message":"RC request id already in flight"}}),
                    );
                    mail.deliver(&err, err.to_string().len());
                    continue;
                }
                // Reserve timed-out IDs until their replies arrive; reset a wedged link rather than grow forever.
                ensure!(pending.len() < 1024, "too many unanswered RC requests");
                timeout(Duration::from_secs(5), rc.write(req)).await??;
                pending.insert(key, mail);
            }
        }
    }
}
impl Client {
    pub async fn next(&mut self) -> Result<Value> {
        // The actor owns the only inbound route. Its exit also invalidates queued synchronization.
        // Drain already received frames before propagating EOF.
        let packet = tokio::select! {biased;p=self.rx.recv()=>p,_=self.tx.closed()=>None};
        Ok(Arc::unwrap_or_clone(
            packet.ok_or_else(|| anyhow!("RC closed"))?.0,
        ))
    }
    pub fn enqueue(&self, req: Value) -> Result<()> {
        self.tx
            .try_send(Command::Send(req, self.mail.clone()))
            .map_err(|_| anyhow!("RC busy or closed"))
    }
    pub async fn send(
        &mut self,
        req: Value,
        mut event: impl FnMut(Value) -> Result<()>,
    ) -> Result<Value> {
        let id = req["id"].clone();
        self.enqueue(req)?;
        loop {
            let v = self.next().await?;
            if v["type"] == "response" && v["id"] == id {
                return Ok(v);
            }
            if v["type"] == "event" {
                event(v)?
            }
        }
    }
    pub async fn hello(&mut self, event: impl FnMut(Value) -> Result<()>) -> Result<Value> {
        let v = self.send(rc::request("hello"), event).await?;
        let s = &v["result"]["snapshot"];
        ensure!(
            v["ok"] == true
                && s["instanceId"] == self.id
                && s["protocol"] == pimate_protocol::PROTOCOL_VERSION
                && s["purpose"].is_null(),
            "unexpected RC identity or purpose"
        );
        Ok(s.clone())
    }
}
