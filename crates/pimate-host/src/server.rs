use crate::{
    config::{self, Config},
    rc, relay,
};
use anyhow::{Result, bail, ensure};
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
    q: mpsc::Sender<(String, OwnedSemaphorePermit)>,
    budget: Arc<Semaphore>,
    slow: CancellationToken,
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
            && self.q.try_send((s, p)).is_ok()
        {
            return;
        }
        self.overflow()
    }
}
struct App {
    config: Config,
    dir: PathBuf,
    instances: watch::Receiver<Vec<Value>>,
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
                        | "subagents"
                )
            });
            v
        })
        .collect()
}
pub async fn serve(config: Config, ips: Vec<IpAddr>, port: u16) -> Result<()> {
    let dir = config::run_dir();
    let (tx, instances) = watch::channel(summaries(rc::list(&dir).await?));
    let app = Arc::new(App {
        config,
        dir: dir.clone(),
        instances,
    });
    let router = Router::new()
        .route("/pair", post(pair))
        .route("/ws", get(upgrade))
        .route("/health", get(health))
        .with_state(app);
    let mut tasks = JoinSet::new();
    for ip in ips {
        let listener = TcpListener::bind((ip, port)).await?;
        println!("Listening http://{}", listener.local_addr()?);
        let router = router.clone();
        tasks.spawn(async move { axum::serve(listener, router).await });
    }
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(1500)).await;
            if let Ok(v) = rc::list(&dir).await {
                let next = summaries(v);
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
        json!({"ok":true,"protocol":VERSION,"rcProtocol":pimate_protocol::PROTOCOL_VERSION,"hostId":a.config.host.host_id,"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION")}),
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
async fn peer(mut ws: WebSocket, a: Arc<App>) {
    let m = match timeout(Duration::from_secs(5), read(&mut ws)).await {
        Ok(Ok(Some(v))) => v,
        Ok(Ok(None)) => return,
        Ok(Err(_)) => {
            close(&mut ws, Code::Protocol as u16).await;
            return;
        }
        Err(_) => {
            close(&mut ws, Code::AuthTimeout as u16).await;
            return;
        }
    };
    let auth = if m["type"] != "auth"
        || !m["token"].is_string()
        || !m["app"]["version"].is_string()
        || !m["app"]["platform"].is_string()
    {
        Err(("bad_request", Code::Protocol))
    } else if m["protocol"] != VERSION {
        Err(("protocol_mismatch", Code::Protocol))
    } else {
        a.config
            .authenticate(m["token"].as_str().unwrap())
            .ok()
            .flatten()
            .ok_or(("invalid_token", Code::AuthFailed))
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
    };
    let c = &a.config;
    out.send(&json!({"type":"auth_ok","deviceId":device,"host":{"hostId":c.host.host_id,"name":c.host.name,"platform":if cfg!(target_os="macos"){"darwin"}else{"linux"},"protocol":VERSION,"version":env!("CARGO_PKG_VERSION"),"pairedDevices":true}}));
    let mut list = a.instances.clone();
    let mut current = list.borrow_and_update().clone();
    out.send(&json!({"type":"instances","instances":current}));
    let mut tick = interval(Duration::from_millis(1500));
    let mut subs: HashMap<String, AbortOnDropHandle<()>> = HashMap::new();
    let mut ops = JoinSet::new();
    let mut inflight = std::collections::HashSet::new();
    let end = loop {
        let m = tokio::select! {
            _=out.slow.cancelled()=>break 1009,
            _=tick.tick()=>{if !c.active(&device).unwrap_or(false){break Code::Revoked as u16}continue},
            Some((text,_permit))=rx.recv()=>{match timeout(Duration::from_secs(5),ws.send(Message::Text(text.into()))).await{Ok(Ok(()))=>continue,Ok(Err(_))=>return,Err(_)=>break 1009}},
            Ok(())=list.changed()=>{
                current=list.borrow_and_update().clone();
                subs.retain(|id,_|{
                    let keep=current.iter().any(|v|v["instanceId"].as_str()==Some(id.as_str()));
                    if !keep {out.send(&json!({"type":"instance_gone","instanceId":id}))}
                    keep
                });
                out.send(&json!({"type":"instances","instances":current}));continue
            },
            Some(Ok(key))=ops.join_next()=>{inflight.remove(&key);continue},
            m=read(&mut ws)=>m,
        };
        let m = match m {
            Ok(Some(m)) => m,
            Ok(None) => return,
            Err(_) => break Code::Protocol as u16,
        };
        let kind = m["type"].as_str().unwrap_or("");
        match kind {
            "ping" if m["t"].is_number() => out.send(&json!({"type":"pong","t":m["t"]})),
            "devices" | "device_revoke" => {
                let Ok(reply) = settings(c, &device, &m) else {
                    break Code::Protocol as u16;
                };
                if kind == "device_revoke" && m["deviceId"] == device && reply["ok"] == true {
                    let _ = timeout(
                        Duration::from_secs(2),
                        ws.send(Message::Text(reply.to_string().into())),
                    )
                    .await;
                    break Code::Revoked as u16;
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
                        let (dir, id, out) = (a.dir.clone(), id.to_owned(), out.clone());
                        subs.insert(
                            id.clone(),
                            AbortOnDropHandle::new(tokio::spawn(async move {
                                relay::subscribe(&dir, &id, out).await
                            })),
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
                        if !exists || inflight.contains(&key) || inflight.len() >= 64 {
                            out.send(&json!({"type":"rc_result","instanceId":id,"response":{"type":"response","id":r["id"],"ok":false,"error":{"code":if exists{"bad_request"}else{"not_found"}}}}));
                            continue;
                        }
                        inflight.insert(key.clone());
                        let (dir, r, out) = (a.dir.clone(), r.clone(), out.clone());
                        ops.spawn(async move {
                            relay::forward(&dir, &key.0, r, out).await;
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
fn settings(c: &Config, device: &str, m: &Value) -> Result<Value> {
    let revoke = m["type"] == "device_revoke";
    ensure!(short(&m["requestId"]) && m.as_object().unwrap().len() == if revoke { 3 } else { 2 });
    if revoke {
        let id = m["deviceId"].as_str().unwrap_or("");
        let u = Uuid::parse_str(id)?;
        ensure!(
            u.get_version_num() == 4
                && u.get_variant() == uuid::Variant::RFC4122
                && u.to_string().eq_ignore_ascii_case(id)
        );
        let ok = c.revoke(id).is_ok();
        let mut r = json!({"type":"device_revoke_result","requestId":m["requestId"],"ok":ok});
        if !ok {
            r["code"] = json!("failed")
        }
        Ok(r)
    } else {
        let ds = c
            .devices()?
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
