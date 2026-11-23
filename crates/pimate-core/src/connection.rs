use crate::{Error, Host, Item, Listener, Operation, runtime, state::State};
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
        drop(s);
        if !changes.is_empty() {
            self.listener.changed(changes)
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
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.task.abort()
    }
}
impl Connection {
    pub(crate) fn start(host: Host, token: String, listener: Arc<dyn Listener>) -> Arc<Self> {
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
                    st.auth_targets.clear();
                    st.apply(&json!({"type":"instances","instances":[]}));
                    for id in st.drafts.iter().map(|d| d.id.clone()).collect::<Vec<_>>() {
                        let mut v = st.get("draft", &id).clone();
                        if v["status"] == "sending" {
                            v["status"] = json!("unconfirmed");
                            st.put("draft", &id, v)
                        }
                    }
                });
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
        })
    }
    pub fn snapshot(&self) -> Vec<Item> {
        self.shared.state.lock().unwrap().snapshot()
    }
    pub fn subscribe(&self, instance: Option<String>) {
        self.selection.send_replace(instance);
    }
    pub async fn perform(&self, operation: Operation) -> Result<String, Error> {
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
                let msg: pimate_protocol::HostMessage =
                    serde_json::from_value(raw).map_err(|_| Error::InvalidMessage)?;
                if matches!(msg, pimate_protocol::HostMessage::Unknown)
                    || matches!(&msg,pimate_protocol::HostMessage::Event{event,..} if matches!(**event,pimate_protocol::InstanceEvent::Unknown))
                {
                    continue;
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
        tokio::select! {
         _=selection.changed()=>{let next=selection.borrow_and_update().clone();if next!=selected{if let Some(id)=selected.take(){send(&mut ws,json!({"type":"unsubscribe","instanceId":id})).await?}selected=next;s.edit(|st|{st.reset();st.selected=selected.clone()});if let Some(id)=&selected{send(&mut ws,json!({"type":"subscribe","instanceId":id})).await?}}},
         Some(Command(op,reply))=commands.recv()=>{if reply.is_closed(){continue}let id=uuid::Uuid::new_v4().to_string();let m=match s.edit(|st|st.prepare(&id,op)){Ok(v)=>v,Err(e)=>{let _=reply.send(Err(e));continue}};let kind=m["type"].as_str().unwrap().to_owned();let hold=m["holdId"].as_str().map(str::to_owned);let instance=m["instanceId"].clone();send(&mut ws,m).await?;if kind=="auth_session_end"{let _=reply.send(Ok("{}".into()));}else{pending.insert(id,Pending{reply,deadline:Instant::now()+Duration::from_secs(60),kind,instance,hold});}},
         frame=read(&mut ws)=>{let m=frame?;if m["type"]=="pong"{waiting=false;ping=Instant::now();continue}let rc=m["type"]=="rc_result";let result=if rc{&m["response"]}else{&m};if let Some(id)=(if rc{&result["id"]}else{&m["requestId"]}).as_str(){if let Some(p)=pending.remove(id){if m["type"]!=format!("{}_result",p.kind)||(rc&&m["instanceId"]!=p.instance){return Err(Error::InvalidMessage)}s.edit(|st|{if let Some(hold)=p.hold&&let Some(instance)=result["instanceId"].as_str(){st.auth_targets.insert(hold,instance.into());}if !st.get("draft",id).is_null(){if result["error"]["code"]=="outcome_unknown"{unconfirmed(st,id)}else if result["ok"]==false{let mut v=st.get("draft",id).clone();v["status"]=json!("rejected");st.put("draft",id,v)}else{st.forget_draft(id)}}});let _=p.reply.send(Ok(result.to_string()));}continue}if s.edit(|st|st.apply(&m))&&let Some(id)=&selected{send(&mut ws,json!({"type":"subscribe","instanceId":id})).await?}},
         _=tick.tick()=>{for id in pending.iter().filter(|(_,p)|p.deadline<=Instant::now()).map(|(id,_)|id.clone()).collect::<Vec<_>>(){let p=pending.remove(&id).unwrap();s.edit(|st|unconfirmed(st,&id));let _=p.reply.send(Err(Error::OutcomeUnknown));}if waiting&&ping.elapsed()>Duration::from_secs(10){return Err(Error::Timeout)}if !waiting&&ping.elapsed()>Duration::from_secs(20){send(&mut ws,json!({"type":"ping","t":0})).await?;waiting=true;ping=Instant::now()}}
        }
    }
}
fn unconfirmed(st: &mut State, id: &str) {
    let mut v = st.get("draft", id).clone();
    if !v.is_null() {
        v["status"] = json!("unconfirmed");
        st.put("draft", id, v)
    }
}
