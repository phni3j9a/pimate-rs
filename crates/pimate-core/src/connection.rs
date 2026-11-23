use crate::{Error, Host, Listener, Operation, runtime, state::State};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Reply = oneshot::Sender<Result<String, Error>>;
struct Command(Operation, Reply);
struct Shared {
    state: Mutex<State>,
    listener: Arc<dyn Listener>,
    online: Mutex<Option<mpsc::Sender<Command>>>,
}
impl Shared {
    fn edit<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let mut s = self.state.lock().unwrap();
        let out = f(&mut s);
        let changes = std::mem::take(&mut s.changes);
        if !changes.is_empty() {
            s.revision += 1;
            let revision = s.revision;
            drop(s);
            self.listener.changed(revision, changes)
        }
        out
    }
    fn status(&self, s: &str) {
        self.edit(|st| st.put("host", "connection", json!(s)))
    }
}
pub struct Connection {
    shared: Arc<Shared>,
    selection: watch::Sender<Option<String>>,
    task: tokio::task::JoinHandle<()>,
    images: tokio::sync::Mutex<indexmap::IndexMap<String, String>>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.task.abort()
    }
}
impl Connection {
    pub(crate) fn start(
        host: Host,
        token: String,
        listener: Arc<dyn Listener>,
        store: Arc<dyn crate::SecretStore>,
    ) -> Arc<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            listener,
            online: Mutex::default(),
        });
        let (selection, mut rx) = watch::channel(None);
        let s = shared.clone();
        let task = runtime().spawn(async move {
            let mut delay = 1;
            loop {
                s.status("connecting");
                let error = attempt(&host, &token, &s, &mut rx, &mut delay)
                    .await
                    .err()
                    .unwrap_or(Error::Offline);
                *s.online.lock().unwrap() = None;
                s.edit(|st| {
                    st.reset();
                    st.selected = None;
                    st.put("host", "selection", Value::Null);
                    st.auth_targets.clear();
                    for c in ["auth", "provider", "loginCapability"] {
                        st.clear(c)
                    }
                    st.clear("instance");
                    st.put("host", "listKnown", json!(false));
                    for id in st.drafts.iter().map(|d| d.id.clone()).collect::<Vec<_>>() {
                        let mut v = st.get("draft", &id).clone();
                        if matches!(v["status"].as_str(), Some("sending" | "queued")) {
                            v["status"] = json!("unconfirmed");
                            st.put("draft", &id, v)
                        }
                    }
                });
                if matches!(error, Error::Revoked | Error::InvalidToken)
                    && store
                        .invalidate(host.id.clone(), host.device_id.clone())
                        .is_err()
                {
                    s.edit(|st| st.put("host", "storageError", json!(true)))
                }
                match error {
                    Error::Revoked => {
                        s.status("revoked");
                        break;
                    }
                    Error::InvalidToken => {
                        s.status("invalid_token");
                        break;
                    }
                    Error::ProtocolMismatch => {
                        s.status("protocol_mismatch");
                        break;
                    }
                    Error::AuthFailed | Error::HostMismatch => {
                        s.status("auth_failed");
                        break;
                    }
                    _ => s.status("offline"),
                }
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(30)
            }
        });
        Arc::new(Self {
            shared,
            selection,
            task,
            images: Default::default(),
        })
    }
    /// Preflight is advisory: the actor checks the exact target again before sending.
    pub fn check(&self, operation: Operation) -> Result<(), Error> {
        if self.shared.online.lock().unwrap().is_none() {
            return Err(Error::Offline);
        }
        let mut st = self.shared.state.lock().unwrap();
        if let Operation::DismissDraft { id } = operation {
            return if st.get("draft", &id)["status"] == "sending" {
                Err(Error::Blocked)
            } else {
                Ok(())
            };
        }
        st.prepare("", operation, false).map(|_| ())
    }
    pub fn snapshot(&self) -> crate::Snapshot {
        let st = self.shared.state.lock().unwrap();
        crate::Snapshot {
            revision: st.revision,
            items: st.snapshot(),
        }
    }
    pub fn subscribe(&self, instance: Option<String>) {
        self.selection.send_replace(instance);
    }
    pub async fn perform(&self, operation: Operation) -> Result<String, Error> {
        if let Operation::Rc {
            target,
            method: pimate_protocol::RcMethod::EntryImage,
            params,
        } = &operation
        {
            // Serialize large replies; immutable transcript entries are shared within one connection.
            let mut cache = self.images.lock().await;
            self.check(operation.clone())?;
            let key = json!([target.instance, target.session, params]).to_string();
            if let Some(v) = cache.shift_remove(&key) {
                cache.insert(key, v.clone());
                return Ok(v);
            }
            let cacheable = target.session.is_some();
            let v = self.dispatch(operation).await?;
            if cacheable {
                const MAX: usize = 24 * 1024 * 1024;
                let mut bytes = cache.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
                    + key.len()
                    + v.len();
                while bytes > MAX && !cache.is_empty() {
                    let (k, v) = cache.shift_remove_index(0).unwrap();
                    bytes -= k.len() + v.len()
                }
                if bytes <= MAX {
                    cache.insert(key, v.clone());
                }
            }
            return Ok(v);
        }
        self.dispatch(operation).await
    }
    async fn dispatch(&self, operation: Operation) -> Result<String, Error> {
        let tx = self
            .shared
            .online
            .lock()
            .unwrap()
            .clone()
            .ok_or(Error::Offline)?;
        let (reply, rx) = oneshot::channel();
        tx.try_send(Command(operation, reply))
            .map_err(|_| Error::Offline)?;
        rx.await.unwrap_or(Err(Error::OutcomeUnknown))
    }
}
async fn send(ws: &mut Ws, v: Value) -> Result<(), Error> {
    timeout(
        Duration::from_secs(10),
        ws.send(Message::Text(v.to_string().into())),
    )
    .await
    .map_err(|_| Error::Timeout)?
    .map_err(|_| Error::Offline)
}
async fn read(ws: &mut Ws) -> Result<Value, Error> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => {
                let raw = pimate_protocol::decode_json(&t).map_err(|_| Error::InvalidMessage)?;
                let mut msg: pimate_protocol::HostMessage =
                    serde_json::from_value(raw).map_err(|_| Error::InvalidMessage)?;
                if let pimate_protocol::HostMessage::Subscribed { history, .. } = &mut msg {
                    history
                        .items
                        .retain(|i| !matches!(i, pimate_protocol::TranscriptItem::Unknown));
                }
                use pimate_protocol::{HostMessage, InstanceEvent, TranscriptItem};
                match &msg {
                    HostMessage::Unknown => continue,
                    HostMessage::Event {
                        instance_id,
                        session_id,
                        seq,
                        event,
                    } if matches!(
                        **event,
                        InstanceEvent::Unknown
                            | InstanceEvent::MessageEnd {
                                item: TranscriptItem::Unknown
                            }
                    ) =>
                    {
                        return Ok(
                            json!({"type":"event","instanceId":instance_id,"sessionId":session_id,"seq":seq,"event":{"type":"unknown"}}),
                        );
                    }
                    _ => {}
                }
                return serde_json::to_value(msg).map_err(|_| Error::InvalidMessage);
            }
            Some(Ok(Message::Close(Some(c)))) => {
                return Err(match u16::from(c.code) {
                    4403 => Error::Revoked,
                    4401 => Error::InvalidToken,
                    _ => Error::Offline,
                });
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => return Err(Error::Offline),
        }
    }
}
struct Pending {
    reply: Reply,
    deadline: Instant,
    kind: String,
    instance: Value,
    hold: Option<String>,
    method: Option<String>,
    session: Value,
    thumbnail: bool,
}
async fn attempt(
    h: &Host,
    token: &str,
    s: &Shared,
    selection: &mut watch::Receiver<Option<String>>,
    delay: &mut u64,
) -> Result<(), Error> {
    let mut ws = timeout(
        Duration::from_secs(10),
        connect_async(format!("ws://{}/ws", h.address)),
    )
    .await
    .map_err(|_| Error::Timeout)?
    .map_err(|_| Error::Offline)?
    .0;
    send(&mut ws,json!({"type":"auth","protocol":1,"token":token,"app":{"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS}})).await?;
    let auth = timeout(Duration::from_secs(10), read(&mut ws))
        .await
        .map_err(|_| Error::Timeout)??;
    if auth["type"] != "auth_ok" {
        return Err(match auth["code"].as_str() {
            Some("invalid_token") => Error::InvalidToken,
            Some("protocol_mismatch") => Error::ProtocolMismatch,
            _ => Error::AuthFailed,
        });
    }
    if auth["host"]["hostId"] != h.id || auth["deviceId"] != h.device_id {
        return Err(Error::HostMismatch);
    }
    *delay = 1;
    let (tx, mut commands) = mpsc::channel(32);
    *s.online.lock().unwrap() = Some(tx);
    s.edit(|st| {
        st.caps = auth["host"].clone();
        st.put("host", "info", st.caps.clone())
    });
    s.status("connected");
    selection.mark_changed();
    let mut selected: Option<String> = None;
    let mut pending: HashMap<String, Pending> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut ping = Instant::now();
    let mut waiting = false;
    loop {
        enum Event {
            Select,
            Command(Command),
            Frame(Result<Value, Error>),
            Tick,
        }
        let event = tokio::select! {
            _=selection.changed()=>Event::Select,
            Some(cmd)=commands.recv()=>Event::Command(cmd),
            frame=read(&mut ws)=>Event::Frame(frame),
            _=tick.tick()=>Event::Tick,
        };
        match event {
            Event::Select => {
                let next = selection.borrow_and_update().clone();
                if next != selected {
                    if let Some(id) = selected.take() {
                        send(&mut ws, json!({"type":"unsubscribe","instanceId":id})).await?
                    }
                    selected = next;
                    s.edit(|st| {
                        st.reset();
                        st.selected = selected.clone();
                        st.put("host", "selection", json!(selected))
                    });
                    if let Some(id) = &selected {
                        send(&mut ws, json!({"type":"subscribe","instanceId":id})).await?
                    }
                }
            }
            Event::Command(Command(op, reply)) => {
                if reply.is_closed() {
                    continue;
                }
                if let Operation::DismissDraft { id } = op {
                    let result = s.edit(|st| {
                        if st.get("draft", &id)["status"] == "sending" {
                            Err(Error::Blocked)
                        } else {
                            st.forget_draft(&id);
                            Ok("{}".into())
                        }
                    });
                    let _ = reply.send(result);
                    continue;
                }
                let id = uuid::Uuid::new_v4().to_string();
                let m = match s.edit(|st| st.prepare(&id, op, true)) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = reply.send(Err(e));
                        continue;
                    }
                };
                let kind = m["type"].as_str().unwrap().to_owned();
                let hold = m["holdId"].as_str().map(str::to_owned);
                let instance = m["instanceId"].clone();
                let method = m["request"]["method"].as_str().map(str::to_owned);
                let session = m["request"]["expectSessionId"].clone();
                let thumbnail = m["request"]["params"]["thumbnail"] == true;
                send(&mut ws, m).await?;
                if kind == "auth_session_end" {
                    let _ = reply.send(Ok("{}".into()));
                } else {
                    pending.insert(
                        id,
                        Pending {
                            reply,
                            deadline: Instant::now() + Duration::from_secs(90),
                            kind,
                            instance,
                            hold,
                            method,
                            session,
                            thumbnail,
                        },
                    );
                }
            }
            Event::Frame(frame) => {
                let m = frame?;
                if m["type"] == "pong" {
                    waiting = false;
                    ping = Instant::now();
                    continue;
                }
                let rc = m["type"] == "rc_result";
                let result = if rc { &m["response"] } else { &m };
                if let Some(id) = (if rc { &result["id"] } else { &m["requestId"] }).as_str() {
                    if let Some(p) = pending.remove(id) {
                        if m["type"] != format!("{}_result", p.kind)
                            || (rc && m["instanceId"] != p.instance)
                        {
                            return Err(Error::InvalidMessage);
                        }
                        let outcome = s.edit(|st| resolve(st, &p, id, result));
                        let _ = p.reply.send(outcome);
                    }
                    continue;
                }
                if s.edit(|st| st.apply(&m))
                    && let Some(id) = &selected
                {
                    send(&mut ws, json!({"type":"subscribe","instanceId":id})).await?
                }
            }
            Event::Tick => {
                for id in pending
                    .iter()
                    .filter(|(_, p)| p.deadline <= Instant::now())
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
                {
                    let p = pending.remove(&id).unwrap();
                    s.edit(|st| {
                        st.release(&id);
                        unconfirmed(st, &id)
                    });
                    let _ = p.reply.send(Err(Error::OutcomeUnknown));
                }
                if waiting && ping.elapsed() > Duration::from_secs(10) {
                    return Err(Error::Timeout);
                }
                if !waiting && ping.elapsed() > Duration::from_secs(20) {
                    send(&mut ws, json!({"type":"ping","t":0})).await?;
                    waiting = true;
                    ping = Instant::now()
                }
            }
        }
    }
}
fn resolve(st: &mut State, p: &Pending, id: &str, result: &Value) -> Result<String, Error> {
    if result["ok"] == false || !matches!(p.method.as_deref(), Some("compact" | "session_op")) {
        if result["error"]["code"] == "no_such_request"
            && st.get("busy", "answer")["id"] == id
            && let Some(q) = st.get("busy", "answer")["requestId"]
                .as_str()
                .map(str::to_owned)
        {
            st.remove("question", &q)
        }
        st.release(id)
    }
    if let Some(hold) = &p.hold
        && let Some(instance) = result["instanceId"].as_str()
    {
        st.auth_targets.insert(hold.clone(), instance.into());
    }
    if result["ok"] == false {
        let code = if p.method.is_some() {
            &result["error"]["code"]
        } else {
            &result["code"]
        };
        let err = match code.as_str() {
            Some("internal" | "timeout" | "outcome_unknown") => Error::OutcomeUnknown,
            Some("instance_mismatch" | "session_changed" | "no_such_request") => Error::StaleTarget,
            Some("unsupported" | "unknown_method") => Error::Unsupported,
            Some("busy" | "not_ready") => Error::Blocked,
            Some("bad_request") => Error::BadRequest,
            _ => Error::Rejected,
        };
        if err == Error::OutcomeUnknown {
            unconfirmed(st, id)
        } else if !st.get("draft", id).is_null() {
            let mut v = st.get("draft", id).clone();
            v["status"] = json!("rejected");
            st.put("draft", id, v)
        }
        return Err(err);
    }
    let Some(method) = &p.method else {
        return Ok(result.to_string());
    };
    let mut body = crate::operation::rc_result(method, &result["result"])
        .inspect_err(|_| unconfirmed(st, id))?;
    if method == "prompt" {
        if matches!(body["deliveredAs"].as_str(), Some("steer" | "followUp")) {
            st.draft_status(id, "queued")
        } else {
            st.forget_draft(id)
        }
    }
    let dedicated = st.auth_targets.values().any(|i| p.instance == *i);
    if !(dedicated
        || st.ready()
            && p.instance == json!(st.selected)
            && p.session == st.get("view", "session")["id"])
    {
        return Err(Error::StaleTarget);
    }
    if p.thumbnail
        && body["image"]["data"]
            .as_str()
            .is_some_and(|s| s.len() > pimate_protocol::MAX_PROMPT_IMAGE_BYTES.div_ceil(3) * 4)
    {
        return Err(Error::InvalidMessage);
    }
    st.receipt(method, &p.instance, &mut body)?;
    Ok(body.to_string())
}
fn unconfirmed(st: &mut State, id: &str) {
    let mut v = st.get("draft", id).clone();
    if !v.is_null() {
        v["status"] = json!("unconfirmed");
        st.put("draft", id, v)
    }
}
impl Connection {
    /// Acquire a login-only Pi and check both Host and RC capabilities. End the hold when the screen closes.
    pub async fn open_login(&self, hold_id: String) -> Result<crate::Target, Error> {
        let receipt = self
            .perform(Operation::AuthSession {
                hold_id: hold_id.clone(),
            })
            .await?;
        let result = async {
            let r: Value = serde_json::from_str(&receipt).map_err(|_| Error::InvalidMessage)?;
            let target = crate::Target {
                instance: r["instanceId"]
                    .as_str()
                    .ok_or(Error::InvalidMessage)?
                    .into(),
                session: None,
            };
            for method in [
                pimate_protocol::RcMethod::Hello,
                pimate_protocol::RcMethod::AuthList,
            ] {
                self.perform(Operation::Rc {
                    target: target.clone(),
                    method,
                    params: "{}".into(),
                })
                .await?;
            }
            Ok(target)
        }
        .await;
        if result.is_err() {
            let _ = self.perform(Operation::EndAuthSession { hold_id }).await;
        }
        result
    }
    /// Follow read-only revisions until input or a terminal status (never repeat begin/respond).
    pub async fn follow_login(&self, target: crate::Target) -> Result<(), Error> {
        loop {
            let flow = self
                .shared
                .state
                .lock()
                .unwrap()
                .get("auth", &target.instance)
                .clone();
            if !matches!(flow["status"].as_str(), Some("working" | "saving")) {
                return Ok(());
            }
            self.perform(Operation::Rc {
                target: target.clone(),
                method: pimate_protocol::RcMethod::AuthWait,
                params:
                    json!({"flowId":flow["id"],"afterRevision":flow["revision"],"waitMs":25000})
                        .to_string(),
            })
            .await?;
        }
    }
}
impl Connection {
    pub fn session_stats(&self) -> crate::SessionStats {
        self.shared.state.lock().unwrap().stats()
    }
    pub fn allowances(&self, now_ms: u64) -> Vec<crate::Allowance> {
        self.shared.state.lock().unwrap().allowances(now_ms)
    }
    pub fn command_suggestions(&self, text: String) -> Vec<String> {
        self.shared.state.lock().unwrap().suggestions(&text)
    }
}
