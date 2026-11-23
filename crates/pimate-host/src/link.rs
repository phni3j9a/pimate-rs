//! One ordered RC stream per Pi. Mailboxes only receive events they subscribe to
//! and responses to their own requests; a slow phone cannot stall the others.
use crate::{
    rc::{self, Rc},
    server::{LIMIT, Out},
};
use anyhow::{Result, anyhow, ensure};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    time::{Instant, timeout, timeout_at},
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
struct Command {
    req: Value,
    mail: Mail,
    written: oneshot::Sender<Instant>,
}
struct Link {
    tx: mpsc::Sender<Command>,
    watchers: Arc<Mutex<Vec<Mail>>>,
}
#[derive(Default)]
pub struct Links(Mutex<HashMap<String, Weak<Link>>>);
pub struct Client {
    link: Arc<Link>,
    mail: Mail,
    rx: mpsc::Receiver<Packet>,
    id: String,
}
impl Links {
    pub fn client(&self, dir: &Path, id: &str, out: &Out, events: bool) -> Client {
        let (send, rx) = mpsc::channel(8192);
        let mail = Mail {
            tx: send,
            budget: Arc::new(Semaphore::new(LIMIT)),
            out: out.clone(),
        };
        let mut map = self.0.lock().unwrap();
        map.retain(|_, weak| weak.upgrade().is_some_and(|l| !l.tx.is_closed()));
        let mut start = None;
        let link = map.get(id).and_then(Weak::upgrade).unwrap_or_else(|| {
            let (tx, cmds) = mpsc::channel(128);
            let link = Arc::new(Link {
                tx,
                watchers: Default::default(),
            });
            map.insert(id.into(), Arc::downgrade(&link));
            start = Some(cmds);
            link
        });
        if events {
            let mut watchers = link.watchers.lock().unwrap();
            watchers.retain(|m| !m.tx.is_closed());
            watchers.push(mail.clone())
        }
        if let Some(cmds) = start {
            tokio::spawn(run(dir.into(), id.into(), cmds, link.watchers.clone()));
        }
        Client {
            link,
            rx,
            mail,
            id: id.into(),
        }
    }
}
async fn run(
    dir: PathBuf,
    id: String,
    mut cmds: mpsc::Receiver<Command>,
    watchers: Arc<Mutex<Vec<Mail>>>,
) -> Result<()> {
    let fanout = |frame: Value| {
        let len = frame.to_string().len();
        let frame = Arc::new(frame);
        let mut watchers = watchers.lock().unwrap();
        watchers.retain(|m| !m.tx.is_closed());
        for m in watchers.iter() {
            m.deliver(&frame, len)
        }
        Ok(())
    };
    let mut rc = timeout(Duration::from_secs(5), Rc::open(&dir, &id)).await??;
    let snapshot = timeout(Duration::from_secs(5), rc.hello(&fanout)).await??;
    ensure!(snapshot["purpose"].is_null(), "reserved RC instance");
    let (mut sink, mut stream) = rc.io.split();
    let pending = Mutex::new(HashMap::<String, Mail>::new());
    let read = async {
        while let Some(line) = stream.next().await {
            let frame = pimate_protocol::decode_json(&line?)?;
            if frame["type"] == "event" && frame["instanceId"] == id {
                fanout(frame)?
            } else if frame["type"] == "response"
                && let Some(mail) = frame["id"]
                    .as_str()
                    .and_then(|k| pending.lock().unwrap().remove(k))
            {
                let len = frame.to_string().len();
                mail.deliver(&Arc::new(frame), len)
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    let write = async {
        while let Some(Command { req, mail, written }) = cmds.recv().await {
            if mail.tx.is_closed() {
                continue;
            }
            let key = req["id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing id"))?
                .to_owned();
            {
                let mut pending = pending.lock().unwrap();
                if pending.contains_key(&key) {
                    let err = Arc::new(
                        json!({"type":"response","id":key,"ok":false,"error":{"code":"bad_request","message":"RC request id already in flight"}}),
                    );
                    mail.deliver(&err, err.to_string().len());
                    let _ = written.send(Instant::now());
                    continue;
                }
                ensure!(pending.len() < 1024, "too many unanswered RC requests");
                pending.insert(key, mail);
            }
            timeout(
                Duration::from_secs(5),
                sink.send(rc::request_line(req, &id)?),
            )
            .await??;
            let _ = written.send(Instant::now());
        }
        Ok(())
    };
    tokio::select! {r=read=>r,r=write=>r}
}
impl Client {
    pub async fn next(&mut self) -> Result<Value> {
        // The actor owns the only inbound route. Its exit also invalidates queued synchronization.
        // Drain already received frames before propagating EOF.
        let packet = tokio::select! {biased;p=self.rx.recv()=>p,_=self.link.tx.closed()=>None};
        Ok(Arc::unwrap_or_clone(
            packet.ok_or_else(|| anyhow!("RC closed"))?.0,
        ))
    }
    pub fn enqueue(&self, req: Value) -> Result<oneshot::Receiver<Instant>> {
        let (written, rx) = oneshot::channel();
        self.link
            .tx
            .try_send(Command {
                req,
                mail: self.mail.clone(),
                written,
            })
            .map_err(|_| anyhow!("RC busy or closed"))?;
        Ok(rx)
    }
    pub async fn send(
        &mut self,
        req: Value,
        mut event: impl FnMut(Value) -> Result<()>,
    ) -> Result<Value> {
        let id = req["id"].clone();
        let deadline = self.enqueue(req)?.await? + Duration::from_secs(5);
        loop {
            let v = timeout_at(deadline, self.next()).await??;
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
