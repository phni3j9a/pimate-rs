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
    sync::{broadcast, mpsc, oneshot, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Reply = oneshot::Sender<Result<String, Error>>;
struct Command(Operation, Reply);
struct Shared {
    state: Mutex<State>,
    notifications: broadcast::Sender<(u64, Vec<crate::Change>)>,
    online: Mutex<Option<mpsc::Sender<Command>>>,
    // Release capacity is reserved on the acquiring socket, never a later connection.
    holds: Mutex<HashMap<String, mpsc::OwnedPermit<Command>>>,
}
impl Shared {
    fn edit<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let mut s = self.state.lock().unwrap();
        let out = f(&mut s);
        let changes = std::mem::take(&mut s.changes);
        if !changes.is_empty() {
            s.revision += 1;
            let revision = s.revision;
            let _ = self.notifications.send((revision, changes));
        }
        out
    }
    fn status(&self, s: &str) {
        self.edit(|st| st.put("host", "connection", json!(s)))
    }
}
/// Never wait for foreign code on a runtime worker. A bounded mailbox reports lag even
/// when the producer goes idle immediately after filling it.
fn delivery(listener: Arc<dyn Listener>) -> broadcast::Sender<(u64, Vec<crate::Change>)> {
    let (tx, mut rx) = broadcast::channel(8);
    std::thread::spawn(move || {
        let mut lagged = false;
        loop {
            match rx.blocking_recv() {
                Ok((rev, changes)) => listener.changed(
                    rev,
                    if std::mem::take(&mut lagged) {
                        vec![crate::Change::ResyncRequired]
                    } else {
                        changes
                    },
                ),
                Err(broadcast::error::RecvError::Lagged(_)) => lagged = true,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    tx
}
pub struct Connection {
    pub(crate) registration: (String, String),
    retry: Arc<tokio::sync::Notify>,
    shared: Arc<Shared>,
    selection: watch::Sender<Option<String>>,
    task: tokio::task::JoinHandle<()>,
    images: Arc<tokio::sync::Mutex<indexmap::IndexMap<String, String>>>,
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
            notifications: delivery(listener),
            online: Mutex::default(),
            holds: Default::default(),
        });
        let (selection, mut rx) = watch::channel(None);
        let registration = (host.id.clone(), host.device_id.clone());
        let retry = Arc::new(tokio::sync::Notify::new());
        let wake = retry.clone();
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
                s.holds.lock().unwrap().clear();
                s.edit(|st| {
                    for id in st.drafts.iter().map(|d| d.id.clone()).collect::<Vec<_>>() {
                        if matches!(
                            st.get("draft", &id)["status"].as_str(),
                            Some("sending" | "queued")
                        ) {
                            unconfirmed(st, &id)
                        }
                    }
                    st.reset();
                    st.selected = None;
                    st.put("host", "selection", Value::Null);
                    st.auth_targets.clear();
                    st.auth_waits.clear();
                    for c in ["auth", "provider", "loginCapability"] {
                        st.clear(c)
                    }
                    st.clear("instance");
                    st.put("host", "listKnown", json!(false));
                });
                if matches!(error, Error::Revoked | Error::InvalidToken) {
                    let (store, id, device) =
                        (store.clone(), host.id.clone(), host.device_id.clone());
                    if !matches!(
                        tokio::task::spawn_blocking(move || store.invalidate(id, device)).await,
                        Ok(Ok(()))
                    ) {
                        s.edit(|st| st.put("host", "storageError", json!(true)))
                    }
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
                tokio::select! { _ = tokio::time::sleep(Duration::from_secs(delay)) => {}, _ = wake.notified() => {} }
                delay = (delay * 2).min(30)
            }
        });
        Arc::new(Self {
            registration,
            retry,
            shared,
            selection,
            task,
            images: Default::default(),
        })
    }
    pub fn retry_now(&self) {
        if self.shared.state.lock().unwrap().get("host", "connection") == "offline" {
            self.retry.notify_one();
        }
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
    pub fn take_unconfirmed(
        &self,
        target: crate::Target,
        id: String,
    ) -> Result<crate::RestoredDraft, Error> {
        self.shared.edit(|st| st.take_unconfirmed(&target, &id))
    }
    pub fn snapshot(&self) -> crate::Snapshot {
        let st = self.shared.state.lock().unwrap();
        crate::Snapshot {
            revision: st.revision,
            items: st.snapshot(),
        }
    }
    pub fn subscribe(&self, instance: Option<String>) -> Result<(), Error> {
        if instance
            .as_deref()
            .is_some_and(|id| !pimate_protocol::valid_instance_id(id))
        {
            return Err(Error::BadRequest);
        }
        self.selection.send_replace(instance);
        Ok(())
    }
    pub async fn perform(&self, operation: Operation) -> Result<String, Error> {
        if let Operation::Rc {
            target,
            method:
                method @ (pimate_protocol::RcMethod::EntryImage | pimate_protocol::RcMethod::FileImage),
            params,
        } = &operation
        {
            // Serialize large replies; immutable transcript entries are shared within one connection.
            let mut cache = self.images.clone().lock_owned().await;
            self.check(operation.clone())?;
            let key = json!([target.instance, target.session, method, params]).to_string();
            if let Some(v) = cache.shift_remove(&key) {
                cache.insert(key, v.clone());
                return Ok(v);
            }
            // Files can change in place. Only immutable history entries are cached.
            let cacheable =
                target.session.is_some() && *method == pimate_protocol::RcMethod::EntryImage;
            let rx = self.enqueue(operation)?;
            // The actor keeps processing even if a UI cancels its await. Keep the image
            // permit until the receipt or disconnect, not just until the UI goes away.
            return runtime()
                .spawn(async move {
                    let v = rx.await.unwrap_or(Err(Error::OutcomeUnknown))?;
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
                    Ok(v)
                })
                .await
                .unwrap_or(Err(Error::OutcomeUnknown));
        }
        self.enqueue(operation)?
            .await
            .unwrap_or(Err(Error::OutcomeUnknown))
    }
    fn enqueue(
        &self,
        operation: Operation,
    ) -> Result<oneshot::Receiver<Result<String, Error>>, Error> {
        if let Operation::EndAuthSession { hold_id } = &operation
            && let Some(rx) = self.release_hold(hold_id)
        {
            return Ok(rx);
        }
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
        Ok(rx)
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
                        ..
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
fn request_timeout(m: &Value) -> Duration {
    if m["type"] == "update" {
        Duration::from_secs(630)
    } else if m["request"]["method"] == "subagents_rpc" {
        Duration::from_millis(
            m["request"]["params"]["timeoutMs"]
                .as_u64()
                .unwrap_or(15000)
                .min(120000)
                + 32000,
        )
    } else {
        Duration::from_secs(90)
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
    host_models: bool,
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
        let deadline = s.state.lock().unwrap().result_deadline;
        let event = tokio::select! {
            _=async { tokio::time::sleep_until(deadline.expect("guarded deadline")).await }, if deadline.is_some()=>Event::Tick,
            _=selection.changed()=>Event::Select,
            Some(cmd)=commands.recv()=>Event::Command(cmd),
            frame=read(&mut ws)=>Event::Frame(frame),
            _=tick.tick()=>Event::Tick,
        };
        s.edit(|st| st.expire_result(Instant::now()));
        match event {
            Event::Select => {
                let next = selection.borrow_and_update().clone();
                // A failed subscription keeps its selection; choosing the same Pi again retries it.
                let retry =
                    next.is_some() && next == selected && s.edit(|st| st.subscription_failed());
                if next != selected || retry {
                    if let Some(id) = selected.take().filter(|_| !retry) {
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
                if reply.is_closed() && !matches!(op, Operation::EndAuthSession { .. }) {
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
                let host_models = matches!(&op, Operation::HostModels { .. });
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
                let deadline = Instant::now() + request_timeout(&m);
                send(&mut ws, m).await?;
                if kind == "auth_session_end" {
                    // A late acquisition reply must not re-register an already released target.
                    pending.retain(|_, p| p.hold != hold);
                    let _ = reply.send(Ok("{}".into()));
                } else {
                    pending.insert(
                        id,
                        Pending {
                            reply,
                            deadline,
                            kind,
                            instance,
                            hold,
                            method,
                            session,
                            host_models,
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
                    // An image may still arrive on this generation. Close it before
                    // allowing another large response to share its outbound budget.
                    if matches!(p.method.as_deref(), Some("entry_image" | "file_image")) {
                        return Err(Error::Timeout);
                    }
                    s.edit(|st| {
                        st.release_unknown(&id);
                        if p.method.as_deref() == Some("abort") {
                            st.settle_drafts(&p.instance, &p.session, true)
                        }
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
    if let Some(hold) = &p.hold
        && let Some(instance) = result["instanceId"].as_str()
    {
        st.auth_targets.insert(hold.clone(), instance.into());
    }
    // GitHub readers render the Host’s typed failure envelope (e.g. not_github vs not_found).
    if result["ok"] == false && p.kind != "github" {
        if result["error"]["code"] == "no_such_request"
            && st.get("busy", "answer")["id"] == id
            && let Some(q) = st.get("busy", "answer")["requestId"]
                .as_str()
                .map(str::to_owned)
        {
            st.remove("question", &q)
        }
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
            st.release_unknown(id);
            if p.method.as_deref() == Some("abort") {
                st.settle_drafts(&p.instance, &p.session, true)
            }
            unconfirmed(st, id)
        } else {
            st.release(id);
            st.draft_status(id, "rejected");
        }
        return Err(err);
    }
    if !matches!(p.method.as_deref(), Some("session_op" | "compact")) {
        st.release(id);
    }
    let Some(method) = &p.method else {
        return Ok(result.to_string());
    };
    let mut body = crate::operation::rc_result(method, &result["result"]).inspect_err(|_| {
        st.release_unknown(id);
        unconfirmed(st, id)
    })?;
    if method == "abort" {
        st.settle_drafts(&p.instance, &p.session, true)
    }
    if matches!(method.as_str(), "session_op" | "compact") {
        st.accept_op(id);
        return Ok(body.to_string());
    }
    if method == "prompt" {
        st.prompt_ack(id, &body["deliveredAs"]);
        // This validated receipt only acknowledges the original send, even after a view switch.
        return Ok(body.to_string());
    }
    if p.host_models {
        // The spawn picker must not overwrite the selected chat’s model catalog.
        return Ok(body.to_string());
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
    st.draft_status(id, "unconfirmed");
}
impl Connection {
    /// The caller must close the ID in finally/defer, even if this await is cancelled.
    pub async fn open_login(&self, hold_id: String) -> Result<crate::Target, Error> {
        if hold_id.is_empty() || hold_id.encode_utf16().count() > 200 {
            return Err(Error::BadRequest);
        }
        let rx = {
            let mut holds = self.shared.holds.lock().unwrap();
            if holds.contains_key(&hold_id) {
                return Err(Error::Blocked);
            }
            let tx = self
                .shared
                .online
                .lock()
                .unwrap()
                .clone()
                .ok_or(Error::Offline)?;
            let slot = tx.clone().try_reserve_owned().map_err(|_| Error::Offline)?;
            let (reply, rx) = oneshot::channel();
            tx.try_send(Command(
                Operation::AuthSession {
                    hold_id: hold_id.clone(),
                },
                reply,
            ))
            .map_err(|_| Error::Offline)?;
            holds.insert(hold_id, slot);
            rx
        };
        let receipt = rx.await.unwrap_or(Err(Error::OutcomeUnknown))?;
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
    pub fn close_login(&self, hold_id: String) {
        self.release_hold(&hold_id);
    }
    fn release_hold(&self, hold_id: &str) -> Option<oneshot::Receiver<Result<String, Error>>> {
        let slot = self.shared.holds.lock().unwrap().remove(hold_id)?;
        let (reply, rx) = oneshot::channel();
        slot.send(Command(
            Operation::EndAuthSession {
                hold_id: hold_id.into(),
            },
            reply,
        ));
        Some(rx)
    }
    pub async fn follow_login(&self, target: crate::Target) -> Result<(), Error> {
        while self.follow_login_once(target.clone()).await? {}
        Ok(())
    }
    /// One bounded long poll, so Swift can observe Task cancellation between requests.
    pub async fn follow_login_once(&self, target: crate::Target) -> Result<bool, Error> {
        let flow = self
            .shared
            .state
            .lock()
            .unwrap()
            .get("auth", &target.instance)
            .clone();
        if !matches!(
            flow["status"].as_str(),
            Some("working" | "input" | "saving")
        ) {
            return Ok(false);
        }
        self.perform(Operation::Rc {
            target: target.clone(),
            method: pimate_protocol::RcMethod::AuthWait,
            params: json!({"flowId":flow["id"],"afterRevision":flow["revision"],"waitMs":25000})
                .to_string(),
        })
        .await?;
        Ok(true)
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
#[cfg(test)]
mod regressions {
    use super::*;
    use std::sync::mpsc as channel;
    // Drive the command boundary directly so cancellation is deterministic at each await.
    fn connected() -> (Connection, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel(4);
        (
            Connection {
                registration: Default::default(),
                retry: Default::default(),
                shared: Arc::new(Shared {
                    state: Default::default(),
                    notifications: broadcast::channel(8).0,
                    online: Mutex::new(Some(tx)),
                    holds: Default::default(),
                }),
                selection: watch::channel(None).0,
                task: tokio::spawn(std::future::pending()),
                images: Default::default(),
            },
            rx,
        )
    }
    #[tokio::test]
    async fn close_login_releases_after_each_cancellation_point_or_success() {
        for completed in 0..=3 {
            let (c, mut commands) = connected();
            let mut open = Box::pin(c.open_login("h".into()));
            for response in [r#"{"instanceId":"dedicated"}"#, "{}", "{}"]
                .iter()
                .take(completed)
            {
                assert!(futures_util::poll!(&mut open).is_pending());
                commands
                    .recv()
                    .await
                    .unwrap()
                    .1
                    .send(Ok(response.to_string()))
                    .unwrap();
            }
            if completed == 3 {
                assert_eq!(open.await.unwrap().instance, "dedicated");
            } else {
                assert!(futures_util::poll!(&mut open).is_pending());
                let _in_flight = commands.recv().await.unwrap();
                // Fill the normal command queue while an acquisition is in flight.
                while c.enqueue(Operation::Devices).is_ok() {}
                drop(open);
            }
            c.close_login("unknown".into());
            c.close_login("h".into());
            c.close_login("h".into());
            loop {
                match commands.try_recv().expect("reserved release command").0 {
                    Operation::EndAuthSession { hold_id } => {
                        assert_eq!(hold_id, "h");
                        break;
                    }
                    Operation::Devices => (),
                    _ => panic!("unexpected command"),
                }
            }
            assert!(commands.try_recv().is_err());
        }
    }
    #[tokio::test]
    async fn follow_login_once_sends_one_wait_even_while_input_is_shown() {
        let (c, mut commands) = connected();
        let target = crate::Target {
            instance: "login".into(),
            session: None,
        };
        assert!(!c.follow_login_once(target.clone()).await.unwrap());
        c.shared.edit(|s| {
            s.put(
                "auth",
                "login",
                json!({"status":"input","id":"flow","revision":1}),
            )
        });
        let mut wait = Box::pin(c.follow_login_once(target));
        assert!(futures_util::poll!(&mut wait).is_pending());
        let Command(op, reply) = commands.recv().await.unwrap();
        assert!(matches!(
            op,
            Operation::Rc {
                method: pimate_protocol::RcMethod::AuthWait,
                ..
            }
        ));
        reply.send(Ok("{}".into())).unwrap();
        assert!(wait.await.unwrap());
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn github_failure_envelope_is_preserved_only_for_github_queries() {
        let mut s = State::default();
        let mut p = Pending {
            reply: oneshot::channel().0,
            deadline: Instant::now(),
            kind: "github".into(),
            instance: json!("pi"),
            hold: None,
            method: None,
            session: Value::Null,
            host_models: false,
            thumbnail: false,
        };
        for code in ["gh_missing", "gh_auth", "not_github", "not_found", "failed"] {
            let reply = json!({"ok":false,"code":code,"message":"failed"});
            assert_eq!(resolve(&mut s, &p, "r", &reply), Ok(reply.to_string()));
        }
        p.kind = "github_status".into();
        assert_eq!(
            resolve(&mut s, &p, "r", &json!({"ok":false,"code":"failed"})),
            Err(Error::Rejected)
        );
    }
    #[test]
    fn host_models_reply_does_not_change_the_chat_catalog() {
        let mut s = State::default();
        s.put("model", "existing", json!({"favorite":true}));
        let pending = Pending {
            reply: oneshot::channel().0,
            deadline: Instant::now(),
            kind: "rc".into(),
            instance: json!("other"),
            hold: None,
            method: Some("models".into()),
            session: Value::Null,
            host_models: true,
            thumbnail: false,
        };
        let body = json!({"models":[],"current":null,"default":{"model":null,"thinking":null}});
        let result = resolve(&mut s, &pending, "r", &json!({"ok":true,"result":body})).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&result).unwrap(), body);
        assert_eq!(s.get("model", "existing")["favorite"], true);
    }
    #[test]
    fn validated_prompt_ack_belongs_to_sender_after_view_switch() {
        let mut s = State::default();
        s.selected = Some("other".into());
        let p = Pending {
            reply: oneshot::channel().0,
            deadline: Instant::now(),
            kind: "rc".into(),
            instance: json!("sender"),
            hold: None,
            method: Some("prompt".into()),
            session: json!("s"),
            host_models: false,
            thumbnail: false,
        };
        let valid = json!({"deliveredAs":"prompt"});
        assert_eq!(
            resolve(&mut s, &p, "r", &json!({"ok":true,"result":valid})),
            Ok(valid.to_string())
        );
        assert!(
            resolve(
                &mut s,
                &p,
                "r",
                &json!({"ok":true,"result":{"deliveredAs":"unknown"}})
            )
            .is_err()
        );
    }
    #[test]
    fn deadlines_outlast_host_processing() {
        assert!(request_timeout(&json!({"type":"update"})) > Duration::from_secs(600));
        assert!(
            request_timeout(
                &json!({"type":"rc","request":{"method":"subagents_rpc","params":{"timeoutMs":120000}}})
            ) > Duration::from_secs(122)
        );
    }
    #[test]
    fn abort_success_or_unknown_retains_queued_drafts_but_rejection_does_not() {
        for (result, status) in [
            (json!({"ok":true,"result":{}}), "unconfirmed"),
            (
                json!({"ok":false,"error":{"code":"timeout"}}),
                "unconfirmed",
            ),
            (
                json!({"ok":false,"error":{"code":"session_changed"}}),
                "queued",
            ),
        ] {
            let mut s = State::default();
            s.selected = Some("i".into());
            s.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":1,"session":{"id":"s"}},"history":{"items":[]}}));
            s.prepare(
                "prompt",
                Operation::Send {
                    target: crate::Target {
                        instance: "i".into(),
                        session: Some("s".into()),
                    },
                    text: "first".into(),
                    images: vec![],
                },
                true,
            )
            .unwrap();
            s.prompt_ack("prompt", &json!("steer"));
            let p = Pending {
                reply: oneshot::channel().0,
                deadline: Instant::now(),
                kind: "rc".into(),
                instance: json!("i"),
                hold: None,
                method: Some("abort".into()),
                session: json!("s"),
                host_models: false,
                thumbnail: false,
            };
            let _ = resolve(&mut s, &p, "stop", &result);
            assert_eq!(s.get("draft", "prompt")["status"], status);
        }
    }
    #[test]
    fn unacknowledged_compaction_only_leaves_marks_for_uncertain_outcomes() {
        for failure in ["disconnect", "internal", "timeout", "busy"] {
            let mut s = State::default();
            s.selected = Some("i".into());
            let snapshot = json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":1,"startedAt":100,"session":{"id":"s"}},"history":{"items":[]}});
            s.apply(&snapshot);
            let compact = || Operation::Rc {
                target: crate::Target {
                    instance: "i".into(),
                    session: Some("s".into()),
                },
                method: pimate_protocol::RcMethod::Compact,
                params: "{}".into(),
            };
            s.prepare("A", compact(), true).unwrap();
            if failure == "disconnect" {
                s.reset();
                s.apply(&snapshot);
            } else {
                let pending = Pending {
                    reply: oneshot::channel().0,
                    deadline: Instant::now(),
                    kind: "rc".into(),
                    instance: json!("i"),
                    hold: None,
                    method: Some("compact".into()),
                    session: json!("s"),
                    host_models: false,
                    thumbnail: false,
                };
                assert_eq!(
                    resolve(
                        &mut s,
                        &pending,
                        "A",
                        &json!({"ok":false,"error":{"code":failure}})
                    ),
                    Err(if failure == "busy" {
                        Error::Blocked
                    } else {
                        Error::OutcomeUnknown
                    })
                );
            }
            assert!(s.get("busy", "operation").is_null());
            if failure != "busy" {
                assert_eq!(s.get("operationResult", "latest")["outcome"], "unknown");
            }
            s.prepare("B", compact(), true).unwrap();
            s.apply(&json!({"type":"event","instanceId":"i","sessionId":"s","seq":2,"event":{"type":"compact_done"}}));
            s.accept_op("B");
            let latest = s.get("operationResult", "latest");
            assert_eq!(latest["id"], "B");
            if failure == "busy" {
                assert_eq!(latest["event"]["type"], "compact_done");
            } else {
                assert_eq!(latest, &json!({"id":"B","outcome":"unknown"}));
            }
            assert!(s.get("busy", "operation").is_null());
        }
    }
    struct Blocking {
        events: channel::Sender<(u64, Vec<crate::Change>)>,
        resume: Mutex<channel::Receiver<()>>,
    }
    impl Listener for Blocking {
        fn changed(&self, revision: u64, changes: Vec<crate::Change>) {
            self.events.send((revision, changes)).unwrap();
            if revision == 1 {
                self.resume.lock().unwrap().recv().unwrap();
            }
        }
    }
    #[test]
    fn blocked_listener_cannot_block_producer_and_gets_resync_after_overflow() {
        let (tx, rx) = channel::channel();
        let (resume, wait) = channel::channel();
        let shared = Shared {
            state: Mutex::default(),
            online: Mutex::default(),
            holds: Default::default(),
            notifications: delivery(Arc::new(Blocking {
                events: tx,
                resume: Mutex::new(wait),
            })),
        };
        shared.status("connected");
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().0, 1);
        for n in 2..=40 {
            shared.status(&n.to_string())
        }
        assert_eq!(shared.state.lock().unwrap().revision, 40);
        resume.send(()).unwrap();
        let (r, c) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(c, [crate::Change::ResyncRequired]);
        assert!(r > 1);
        for _ in r..40 {
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
    }
    #[tokio::test]
    async fn entry_and_file_images_share_a_serial_network_queue() {
        let (tx, mut rx) = mpsc::channel(3);
        let mut st = State::default();
        st.selected = Some("i".into());
        st.apply(&json!({"type":"subscribed","instanceId":"i","snapshot":{"seq":1,"session":{"id":"s"},"entryImages":true,"fileImages":true},"history":{"items":[]}}));
        let c = Connection {
            registration: Default::default(),
            retry: Default::default(),
            shared: Arc::new(Shared {
                state: Mutex::new(st),
                notifications: broadcast::channel(1).0,
                online: Mutex::new(Some(tx)),
                holds: Default::default(),
            }),
            selection: watch::channel(None).0,
            task: tokio::spawn(async {}),
            images: Default::default(),
        };
        let request = |method, params: Value| Operation::Rc {
            method,
            params: params.to_string(),
            target: crate::Target {
                instance: "i".into(),
                session: Some("s".into()),
            },
        };
        use pimate_protocol::RcMethod::*;
        let mut a = Box::pin(c.perform(request(FileImage, json!({"path":"one"}))));
        let mut b = Box::pin(c.perform(request(EntryImage, json!({"entryId":"u","index":0}))));
        let mut d = Box::pin(c.perform(request(FileImage, json!({"path":"two"}))));
        assert!(futures_util::poll!(&mut a).is_pending());
        assert!(futures_util::poll!(&mut b).is_pending());
        assert!(futures_util::poll!(&mut d).is_pending());
        let cmd = rx.try_recv().unwrap();
        assert!(rx.try_recv().is_err());
        drop(a); // UI closed after send; the transport still owns this slot.
        assert!(futures_util::poll!(&mut b).is_pending());
        assert!(rx.try_recv().is_err());
        cmd.1.send(Ok("image1".into())).unwrap();
        tokio::select! {_=&mut b=>panic!("premature receipt"),cmd=rx.recv()=>cmd.unwrap().1.send(Ok("image2".into())).unwrap()};
        b.await.unwrap();
        tokio::select! {_=&mut d=>panic!("premature receipt"),cmd=rx.recv()=>cmd.unwrap().1.send(Ok("image3".into())).unwrap()};
        d.await.unwrap();
    }
}
