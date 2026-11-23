use crate::{
    config::{self, Config},
    rc, relay,
};
use anyhow::{Result, bail};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{
        Request, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use pimate_protocol::{HOST_PROTOCOL_VERSION as VERSION, HostCloseCode as Code, decode_json};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashMap, net::IpAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch},
    task::JoinSet,
    time::{interval, sleep, timeout},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use uuid::Uuid;
pub const LIMIT: usize = 8 << 20;
#[derive(Clone)]
pub struct Out {
    q: mpsc::Sender<(String, OwnedSemaphorePermit, CancellationToken)>,
    budget: Arc<Semaphore>,
    slow: CancellationToken,
    generation: CancellationToken,
}
impl Out {
    pub fn overflow(&self) {
        self.slow.cancel()
    }
    pub fn send(&self, v: &impl Serialize) {
        let Ok(s) = serde_json::to_string(v) else {
            return;
        };
        if s.len() > LIMIT {
            self.overflow();
            return;
        }
        if let Ok(p) = self.budget.clone().try_acquire_many_owned(s.len() as u32)
            && self.q.try_send((s, p, self.generation.clone())).is_ok()
        {
            return;
        }
        self.overflow()
    }
}
struct App {
    monitor: std::sync::Mutex<crate::alerts::Monitor>,
    package: crate::update::Package,
    terminal: crate::terminal::Terminal,
    login: Arc<crate::login::Login>,
    folder_cache: tokio::sync::Mutex<Option<(tokio::time::Instant, Vec<Value>)>>,
    last_spawn: tokio::sync::Mutex<Option<tokio::time::Instant>>,
    addresses: Vec<Value>,
    incompatible: watch::Receiver<serde_json::Map<String, Value>>,
    revoked: tokio::sync::broadcast::Sender<String>,
    links: Arc<crate::link::Links>,
    config: Config,
    dir: PathBuf,
    instances: watch::Receiver<Vec<Value>>,
}
impl App {
    async fn operation(&self, m: Value, hold: Option<CancellationToken>) -> Value {
        let kind = m["type"].as_str().unwrap();
        let mut reply = match kind {
            "github_status" => crate::github::status().await,
            "github" => {
                let cwd = self
                    .instances
                    .borrow()
                    .iter()
                    .find(|r| r["instanceId"] == m["instanceId"])
                    .and_then(|r| r["cwd"].as_str())
                    .map(String::from);
                crate::github::query(cwd, &m["query"]).await
            }
            "update" | "update_check" => self.package.run(kind == "update_check").await,
            "host_restart" => crate::update::restart(crate::service::managed()).await,
            "dirs" => {
                let path = m["path"].as_str().unwrap().to_owned();
                let dirs = tokio::task::spawn_blocking(move || crate::operations::dirs(&path))
                    .await
                    .unwrap_or_default();
                json!({"dirs":dirs})
            }
            "folders" => {
                let mut cache = self.folder_cache.lock().await;
                if cache
                    .as_ref()
                    .is_none_or(|(t, _)| t.elapsed() >= Duration::from_secs(5))
                {
                    let rows = self.instances.borrow().clone();
                    let folders =
                        tokio::task::spawn_blocking(move || crate::operations::folders(&rows))
                            .await
                            .unwrap_or_default();
                    *cache = Some((tokio::time::Instant::now(), folders));
                }
                json!({"home":crate::operations::home().ok(),"folders":cache.as_ref().unwrap().1})
            }
            _ => {
                let result = if kind == "auth_session" {
                    self.login
                        .acquire(&self.terminal, &self.dir, hold.as_ref().expect("auth hold"))
                        .await
                        .map(|id| json!({"ok":true,"instanceId":id}))
                } else {
                    crate::operations::spawn(
                        &self.terminal,
                        &self.dir,
                        &m["params"],
                        &self.last_spawn,
                    )
                    .await
                };
                result.unwrap_or_else(|e| {
                    let msg = e.to_string();
                    let code = match msg.as_str() {
                        "not_found" | "not_directory" | "bad_request" => msg.as_str(),
                        _ => "spawn_failed",
                    };
                    json!({"ok":false,"code":code,"message":format!("{msg}; not retried")})
                })
            }
        };
        reply["type"] = format!("{kind}_result").into();
        reply["requestId"] = m["requestId"].clone();
        reply
    }
}
fn summaries(list: Vec<Value>) -> Vec<Value> {
    list.into_iter()
        .map(|mut v| {
            v.as_object_mut().unwrap().retain(|k, _| {
                matches!(
                    k.as_str(),
                    "instanceId"
                        | "cwd"
                        | "origin"
                        | "abducoSession"
                        | "pcAttached"
                        | "session"
                        | "status"
                        | "startedAt"
                        | "updatedAt"
                        | "questionTitle"
                        | "failureText"
                        | "subagents"
                )
            });
            v
        })
        .collect()
}
pub async fn serve(config: Config, ips: Vec<IpAddr>, port: u16) -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    // Stop even during discovery/bind. Runtime shutdown waits for synchronous DB updates and lock guards.
    let login = Arc::new(crate::login::Login::default());
    let result = tokio::select! {biased;_=terminate.recv()=>Ok(()),_=interrupt.recv()=>Ok(()),result=listen(config,ips,port,login.clone())=>result};
    login.close(&config::run_dir()).await;
    result
}
async fn listen(
    config: Config,
    ips: Vec<IpAddr>,
    port: u16,
    login: Arc<crate::login::Login>,
) -> Result<()> {
    let dir = config::run_dir();
    let mut discovery = rc::list(&dir).await?;
    let terminal = crate::terminal::Terminal::resolve();
    terminal.refresh(&dir, &mut discovery.instances).await;
    let (tx, instances) = watch::channel(summaries(discovery.instances));
    let (versions, incompatible) = watch::channel(discovery.incompatible);
    let mut listeners = vec![];
    let mut addresses = vec![];
    for ip in ips {
        let l = TcpListener::bind((ip, port)).await?;
        let addr = l.local_addr()?;
        addresses.push(
            json!({"address":ip,"port":addr.port(),"family":if ip.is_ipv4(){"IPv4"}else{"IPv6"}}),
        );
        listeners.push(l);
    }
    let mut monitor = crate::alerts::Monitor::new()?;
    monitor.refresh(&dir, &instances.borrow());
    let app = Arc::new(App {
        monitor: std::sync::Mutex::new(monitor),
        package: crate::update::Package::running(),
        terminal,
        login,
        last_spawn: Default::default(),
        folder_cache: Default::default(),
        addresses,
        incompatible,
        revoked: tokio::sync::broadcast::channel(128).0,
        links: Default::default(),
        config,
        dir: dir.clone(),
        instances,
    });
    let router = Router::new()
        .route("/pair", post(pair))
        .route("/ws", get(upgrade))
        .route("/health", get(health))
        .with_state(app.clone());
    let mut tasks = JoinSet::new();
    for listener in listeners {
        println!("Listening http://{}", listener.local_addr()?);
        let router = router.clone();
        tasks.spawn(async move { axum::serve(listener, router).await });
    }
    tokio::spawn(async move {
        let mut failures = Default::default();
        loop {
            sleep(Duration::from_millis(1500)).await;
            if let Ok(mut v) =
                rc::discover(&dir, (tx.receiver_count() > 1).then_some(&mut failures)).await
            {
                app.terminal.refresh(&dir, &mut v.instances).await;
                app.login.observe(&dir, v.login);
                versions.send_replace(v.incompatible);
                let next = summaries(v.instances);
                let mut monitor = app.monitor.lock().unwrap();
                monitor.refresh(&dir, &next);
                tx.send_if_modified(|prev| {
                    if *prev == next {
                        return false;
                    }
                    *prev = next;
                    true
                });
            }
        }
    });
    tasks.join_next().await.unwrap()??;
    Ok(())
}
async fn health(State(a): State<Arc<App>>) -> Json<Value> {
    Json(
        json!({"ok":true,"protocol":VERSION,"rcProtocol":pimate_protocol::PROTOCOL_VERSION,"addresses":a.addresses,"abduco":a.terminal.executable,"incompatible":*a.incompatible.borrow(),"hostId":a.config.host.host_id,"pid":std::process::id(),"version":a.package.current()}),
    )
}
async fn pair(State(a): State<Arc<App>>, req: Request) -> (StatusCode, Json<Value>) {
    let bad = (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":"bad_request"})),
    );
    if req.headers().contains_key("origin")
        || req
            .headers()
            .get("content-type")
            .and_then(|s| s.to_str().ok())
            .and_then(|s| s.split(';').next())
            .map(str::trim)
            != Some("application/json")
    {
        return bad;
    }
    let Ok(b) = to_bytes(req.into_body(), 4096).await else {
        return bad;
    };
    let Ok(v) = serde_json::from_slice::<Value>(&b) else {
        return bad;
    };
    let (Some(c), Some(n)) = (v["code"].as_str(), v["deviceName"].as_str()) else {
        return bad;
    };
    if !(16..=128).contains(&c.len())
        || !c
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        || n.trim().is_empty()
        || n.encode_utf16().count() > 200
    {
        return bad;
    }
    match a.config.pair(c, n.trim()) {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => match e.to_string().as_str() {
            "expired_code" => (StatusCode::GONE, Json(json!({"error":"expired_code"}))),
            "invalid_code" => (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"invalid_code"})),
            ),
            _ => bad,
        },
    }
}
async fn upgrade(State(a): State<Arc<App>>, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(1 << 20)
        .max_frame_size(1 << 20)
        .on_upgrade(move |ws| peer(ws, a))
}
async fn close(ws: &mut WebSocket, code: u16) {
    let _ = timeout(
        Duration::from_secs(1),
        ws.send(Message::Close(Some(CloseFrame {
            code,
            reason: "".into(),
        }))),
    )
    .await;
}
async fn read(ws: &mut WebSocket) -> Result<Option<Value>> {
    loop {
        match ws.recv().await.transpose()? {
            Some(Message::Text(t)) => return Ok(Some(decode_json(&t)?)),
            Some(Message::Ping(_) | Message::Pong(_)) => continue,
            None | Some(Message::Close(_)) => return Ok(None),
            _ => bail!("text required"),
        }
    }
}
fn read_close(e: &anyhow::Error) -> u16 {
    if e.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<tungstenite::Error>(),
            Some(tungstenite::Error::Capacity(_))
        )
    }) {
        1009
    } else {
        Code::Protocol as u16
    }
}
async fn peer(mut ws: WebSocket, a: Arc<App>) {
    let m = match timeout(Duration::from_secs(5), read(&mut ws)).await {
        Ok(Ok(Some(v))) => v,
        Ok(Ok(None)) => return,
        Ok(Err(e)) => {
            close(&mut ws, read_close(&e)).await;
            return;
        }
        Err(_) => {
            close(&mut ws, Code::AuthTimeout as u16).await;
            return;
        }
    };
    let mut revoked = a.revoked.subscribe();
    let auth = if m["type"] != "auth"
        || !m["token"].is_string()
        || !m["app"]["version"].is_string()
        || !m["app"]["platform"].is_string()
    {
        Err(("bad_request", Code::Protocol))
    } else if m["protocol"] != VERSION {
        Err(("protocol_mismatch", Code::Protocol))
    } else {
        match a.config.authenticate(m["token"].as_str().unwrap()) {
            Ok(id) => id.ok_or(("invalid_token", Code::AuthFailed)),
            Err(_) => {
                close(&mut ws, 1011).await;
                return;
            }
        }
    };
    let device = match auth {
        Ok(id) => id,
        Err((error, code)) => {
            let _ = ws
                .send(Message::Text(
                    json!({"type":"auth_error","code":error}).to_string().into(),
                ))
                .await;
            close(&mut ws, code as u16).await;
            return;
        }
    };
    let (q, mut rx) = mpsc::channel(8192);
    let out = Out {
        q,
        budget: Arc::new(Semaphore::new(LIMIT)),
        slow: CancellationToken::new(),
        generation: CancellationToken::new(),
    };
    let c = &a.config;
    out.send(&json!({"type":"auth_ok","deviceId":device,"host":{"hostId":c.host.host_id,"name":c.host.name,"platform":if cfg!(target_os="macos"){"darwin"}else{"linux"},"protocol":VERSION,"version":a.package.current(),"pairedDevices":true,"authSession":true,"providerAuth":true,"dirs":true,"alerts":true,"githubStatus":true,"github":true,"packageUpdate":true,"hostRestart":crate::service::managed()}}));
    let mut list = a.instances.clone();
    let mut current = list.borrow_and_update().clone();
    out.send(&json!({"type":"instances","instances":current}));
    let mut tick = interval(Duration::from_millis(1500));
    let mut subs: HashMap<String, (tokio_util::sync::DropGuard, AbortOnDropHandle<()>)> =
        HashMap::new();
    let mut ops = JoinSet::new();
    let mut inflight = std::collections::HashSet::new();
    let mut holds: HashMap<String, (Uuid, crate::login::Hold)> = HashMap::new();
    let mut admin = JoinSet::new();
    let mut alerts: Option<tokio::sync::broadcast::Receiver<Value>> = None;
    let end = loop {
        let m = tokio::select! {biased;
            alert=async{alerts.as_mut().unwrap().recv().await},if alerts.is_some()=>{match alert{Ok(alert)=>out.send(&json!({"type":"alert","alert":alert})),Err(_)=>break 1009};continue},
            notice=revoked.recv()=>{match notice{Ok(id) if id==device=>break Code::Revoked as u16,Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>tick.reset_immediately(),_=>()}continue},
            _=out.slow.cancelled()=>break 1009,
            _=tick.tick()=>{match c.active(&device){Ok(false)=>break Code::Revoked as u16,Err(_)=>break 1011,Ok(true)=>continue}},
            Some((text,_permit,generation))=rx.recv()=>{if generation.is_cancelled(){continue}match timeout(Duration::from_secs(5),ws.send(Message::Text(text.into()))).await{Ok(Ok(()))=>continue,Ok(Err(_))=>return,Err(_)=>break 1009}},
            Ok(())=list.changed()=>{
                current=list.borrow_and_update().clone();
                subs.retain(|id,_|{
                    let keep=current.iter().any(|v|v["instanceId"].as_str()==Some(id.as_str()));
                    if !keep {out.send(&json!({"type":"instance_gone","instanceId":id}))}
                    keep
                });
                out.send(&json!({"type":"instances","instances":current}));continue
            },
            Some(result)=admin.join_next()=>{if let Ok(Some((key,generation)))=result && holds.get(&key).is_some_and(|h|h.0==generation){holds.remove(&key);}continue},
            Some(Ok(key))=ops.join_next()=>{inflight.remove(&key);continue},
            m=read(&mut ws)=>m,
        };
        let m = match m {
            Ok(Some(m)) => m,
            Ok(None) => return,
            Err(e) => break read_close(&e),
        };
        let kind = m["type"].as_str().unwrap_or("");
        match kind {
            "watch_alerts" if m["enabled"].is_boolean() => {
                if m["enabled"] == true && alerts.is_none() {
                    alerts = Some(
                        a.monitor
                            .lock()
                            .unwrap()
                            .subscribe(&a.dir, &a.instances.borrow()),
                    );
                } else if m["enabled"] == false {
                    alerts = None;
                }
            }
            "ping" if m["t"].is_number() => out.send(&json!({"type":"pong","t":m["t"]})),
            "auth_session_end" | "auth_session" | "spawn" | "dirs" | "folders"
            | "github_status" | "github" | "update" | "update_check" | "host_restart" => {
                let keys = m.as_object().unwrap().len();
                if (matches!(
                    kind,
                    "github_status" | "update" | "update_check" | "host_restart"
                ) && keys != 2)
                    || (kind == "github"
                        && (keys != 4
                            || !crate::github::valid(&m["query"])
                            || !m["instanceId"].as_str().is_some_and(rc::valid_id)))
                {
                    break Code::Protocol as u16;
                }
                if kind != "auth_session_end" && !short(&m["requestId"]) {
                    break Code::Protocol as u16;
                }
                let mut release_on_error = None;
                let mut hold = None;
                if kind.starts_with("auth_session") {
                    if !short(&m["holdId"]) {
                        break Code::Protocol as u16;
                    }
                    let h = m["holdId"].as_str().unwrap();
                    if kind == "auth_session_end" {
                        holds.remove(h);
                        continue;
                    }
                    if !holds.contains_key(h) && holds.len() >= 16 {
                        break Code::Protocol as u16;
                    }
                    let generation = Uuid::new_v4();
                    let (latest, guard) = holds
                        .entry(h.to_string())
                        .or_insert_with(|| (generation, a.login.hold()));
                    *latest = generation;
                    hold = Some(guard.released.clone());
                    release_on_error = Some((h.to_owned(), generation));
                }
                if kind == "dirs"
                    && m["path"]
                        .as_str()
                        .is_none_or(|s| s.encode_utf16().count() > 4096)
                {
                    break Code::Protocol as u16;
                }
                if admin.len() >= 16 {
                    break Code::Protocol as u16;
                }
                let a = a.clone();
                let out = out.clone();
                // Accept launch operations independently of the reply connection. A
                // dropped JoinHandle detaches the operation; the join set bounds waiters.
                let host_owned =
                    matches!(kind, "spawn" | "auth_session" | "update" | "host_restart");
                let operation = async move { a.operation(m, hold).await };
                let launch = if host_owned {
                    Ok(tokio::spawn(operation))
                } else {
                    Err(operation)
                };
                admin.spawn(async move {
                    let reply = match launch {
                        Ok(handle) => handle.await.ok()?,
                        Err(operation) => operation.await,
                    };
                    out.send(&reply);
                    release_on_error.filter(|_| reply["ok"] == false)
                });
            }
            "devices" | "device_revoke" => {
                let reply = match settings(c, &device, &m) {
                    Ok(reply) => reply,
                    Err(code) => break code,
                };
                if kind == "device_revoke" && reply["ok"] == true {
                    let target = m["deviceId"].as_str().unwrap();
                    let sent = notify_and_reply(
                        &a.revoked,
                        target,
                        timeout(
                            Duration::from_secs(2),
                            ws.send(Message::Text(reply.to_string().into())),
                        ),
                    )
                    .await;
                    if target == device {
                        break Code::Revoked as u16;
                    }
                    if !matches!(sent, Ok(Ok(()))) {
                        return;
                    }
                    continue;
                }
                out.send(&reply)
            }
            "subscribe" | "unsubscribe" | "rc" => {
                let Some(id) = m["instanceId"].as_str().filter(|s| rc::valid_id(s)) else {
                    break Code::Protocol as u16;
                };
                let exists = current.iter().any(|v| v["instanceId"] == id);
                match kind {
                    "unsubscribe" => {
                        subs.remove(id);
                    }
                    "subscribe" if exists => {
                        let links = a.links.clone();
                        let (dir, id) = (a.dir.clone(), id.to_owned());
                        let generation = CancellationToken::new();
                        let guard = generation.clone().drop_guard();
                        let out = Out {
                            generation,
                            ..out.clone()
                        };
                        subs.insert(
                            id.clone(),
                            (
                                guard,
                                AbortOnDropHandle::new(tokio::spawn(relay::subscribe(
                                    links, dir, id, out,
                                ))),
                            ),
                        );
                    }
                    "subscribe" => out.send(
                        &json!({"type":"subscribe_error","instanceId":id,"code":"not_found"}),
                    ),
                    _ => {
                        let r = &m["request"];
                        if !valid_req(r) {
                            break Code::Protocol as u16;
                        }
                        let key = (id.to_owned(), r["id"].as_str().unwrap().to_owned());
                        let login = a.login.owns(id);
                        let denied = login && !crate::login::allowed(r["method"].as_str().unwrap());
                        if (!exists && !login)
                            || denied
                            || inflight.contains(&key)
                            || inflight.len() >= 64
                        {
                            out.send(&json!({"type":"rc_result","instanceId":id,"response":{"type":"response","id":r["id"],"ok":false,"error":{"code":if denied{"unsupported"}else if exists||login{"bad_request"}else{"not_found"}}}}));
                            continue;
                        }
                        inflight.insert(key.clone());
                        let response = relay::forward(&a.links, &a.dir, id, r.clone(), out.clone());
                        ops.spawn(async move {
                            response.await;
                            key
                        });
                    }
                }
            }
            _ => break Code::Protocol as u16,
        }
    };
    close(&mut ws, end).await;
}
async fn notify_and_reply<F: Future>(
    revoked: &tokio::sync::broadcast::Sender<String>,
    target: &str,
    reply: F,
) -> F::Output {
    let _ = revoked.send(target.into());
    reply.await
}
fn short(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.encode_utf16().count() <= 200)
}
fn valid_req(r: &Value) -> bool {
    short(&r["id"])
        && r["method"].is_string()
        && r.get("params").is_none_or(Value::is_object)
        && ["expectInstanceId", "expectSessionId"]
            .iter()
            .all(|k| r.get(k).is_none_or(Value::is_string))
}
fn settings(c: &Config, device: &str, m: &Value) -> Result<Value, u16> {
    let revoke = m["type"] == "device_revoke";
    let invalid = Code::Protocol as u16;
    if !short(&m["requestId"]) || m.as_object().unwrap().len() != if revoke { 3 } else { 2 } {
        return Err(invalid);
    }
    if revoke {
        let id = m["deviceId"].as_str().unwrap_or("");
        let u = Uuid::parse_str(id).map_err(|_| invalid)?;
        if u.get_version_num() != 4
            || u.get_variant() != uuid::Variant::RFC4122
            || !u.to_string().eq_ignore_ascii_case(id)
        {
            return Err(invalid);
        }
        let ok = c.revoke(id).is_ok();
        let mut r = json!({"type":"device_revoke_result","requestId":m["requestId"],"ok":ok});
        if !ok {
            r["code"] = json!("failed")
        }
        Ok(r)
    } else {
        let ds = c
            .devices()
            .map_err(|_| 1011u16)?
            .into_iter()
            .map(|d| {
                let mut v = d.public();
                v["current"] = json!(d.device_id == device);
                v
            })
            .collect::<Vec<_>>();
        Ok(json!({"type":"devices_result","requestId":m["requestId"],"devices":ds}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn revoke_notifies_while_reply_is_blocked() {
        let (tx, mut peer) = tokio::sync::broadcast::channel(1);
        let (release, blocked) = tokio::sync::oneshot::channel::<()>();
        let sender = tokio::spawn(async move { notify_and_reply(&tx, "target", blocked).await });
        assert_eq!(
            timeout(Duration::from_secs(1), peer.recv())
                .await
                .unwrap()
                .unwrap(),
            "target"
        );
        assert!(!sender.is_finished());
        release.send(()).unwrap();
        sender.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn delayed_hello_keeps_new_subscriber_events_and_full_operation_deadline() -> Result<()> {
        use futures_util::{SinkExt, StreamExt};
        use tokio::net::UnixListener;
        use tokio_util::codec::{Framed, LinesCodec};
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("run");
        config::private(&dir)?;
        let listener = UnixListener::bind(dir.join("test.sock"))?;
        let links = Arc::new(crate::link::Links::default());
        let (q, mut rx) = mpsc::channel(8);
        let out = Out {
            q,
            budget: Arc::new(Semaphore::new(LIMIT)),
            slow: CancellationToken::new(),
            generation: CancellationToken::new(),
        };
        let op = tokio::spawn(relay::forward(
            &links,
            &dir,
            "test",
            json!({"id":"op","method":"abort"}),
            out.clone(),
        ));
        let mut io = Framed::new(
            timeout(Duration::from_secs(1), listener.accept()).await??.0,
            LinesCodec::new(),
        );
        let response = |id: &Value, result: Value| {
            json!({"id":id,"type":"response","ok":true,"result":result}).to_string()
        };
        let snapshot =
            json!({"snapshot":{"protocol":1,"instanceId":"test","session":null,"seq":1}});
        let first: Value = serde_json::from_str(&io.next().await.unwrap()?)?;
        assert_eq!(first["method"], "hello");
        let sub = AbortOnDropHandle::new(tokio::spawn(relay::subscribe(
            links.clone(),
            dir.clone(),
            "test".into(),
            out.clone(),
        )));
        sleep(Duration::from_secs(3)).await;
        let notify = json!({"type":"event","instanceId":"test","seq":1,"event":{"type":"notify"}});
        io.send(notify.to_string()).await?;
        io.send(response(&first["id"], snapshot.clone())).await?;
        // Hold the operation response but let the concurrent subscriber complete its hello/history.
        for method in ["abort", "hello", "history"] {
            let line = timeout(Duration::from_secs(2), io.next()).await?.unwrap()?;
            let req: Value = serde_json::from_str(&line)?;
            assert_eq!(req["method"], method);
            match method {
                "hello" => io.send(response(&req["id"], snapshot.clone())).await?,
                "history" => {
                    io.send(response(&req["id"], json!({"session":null,"items":[]})))
                        .await?
                }
                _ => (),
            }
        }
        for expected in ["event", "subscribed"] {
            let v: Value = serde_json::from_str(
                &timeout(Duration::from_secs(1), rx.recv()).await?.unwrap().0,
            )?;
            assert_eq!(v["type"], expected);
            if expected == "event" {
                assert_eq!(v, notify)
            }
        }
        sleep(Duration::from_secs(3)).await;
        io.send(response(&json!("op"), json!({"done":true})))
            .await?;
        let v: Value =
            serde_json::from_str(&timeout(Duration::from_secs(1), rx.recv()).await?.unwrap().0)?;
        assert_eq!(v["response"]["result"]["done"], true);
        // Stop reading on the RC side: inbound events must still reach the subscriber.
        let client = links.client(&dir, "test", &out, false);
        let mut written = Vec::new();
        for n in 0..8 {
            written.push(client.enqueue(
                json!({"id":n.to_string(),"method":"test","params":{"text":"x".repeat(900_000)}}),
            )?);
        }
        timeout(Duration::from_secs(1), io.get_ref().readable()).await??;
        assert!(matches!(
            written.last_mut().unwrap().try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        let later = json!({"type":"event","instanceId":"test","seq":2,"event":{"type":"notify"}});
        io.send(later.to_string()).await?;
        assert_eq!(
            serde_json::from_str::<Value>(
                &timeout(Duration::from_secs(2), rx.recv()).await?.unwrap().0
            )?,
            later
        );
        drop(sub);
        op.await?;
        Ok(())
    }
    #[test]
    fn slow_consumer_budget_includes_inflight_frames() {
        let (q, mut rx) = mpsc::channel(2);
        let out = Out {
            q,
            budget: Arc::new(Semaphore::new(10)),
            slow: CancellationToken::new(),
            generation: CancellationToken::new(),
        };
        out.send(&"12345");
        let _sending = rx.try_recv().unwrap();
        out.send(&"12345");
        assert!(out.slow.is_cancelled());
    }
}
