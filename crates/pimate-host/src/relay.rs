use crate::{
    rc::{self, Rc},
    server::{LIMIT, Out},
};
use anyhow::{Result, ensure};
use pimate_protocol::relay::HostFrame;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::time::{Instant, sleep, sleep_until, timeout};
pub async fn forward(dir: &Path, id: &str, req: Value, out: Out) {
    let rid = req["id"].clone();
    let wait = match req["method"].as_str() {
        Some("auth_wait" | "auth_list") => 32,
        Some("auth_logout") => 62,
        Some("subagents_rpc") => {
            req["params"]["timeoutMs"]
                .as_u64()
                .unwrap_or(15000)
                .min(120000)
                / 1000
                + 2
        }
        Some("subagent_control") => 17,
        _ => 5,
    };
    let op = async {
        let mut c = Rc::open(dir, id).await?;
        c.hello(|_| Ok(())).await?;
        c.send(req, |_| Ok(())).await
    };
    let response = match timeout(Duration::from_secs(wait), op).await {
        Ok(Ok(v)) => v,
        _ => {
            json!({"type":"response","id":rid,"ok":false,"error":{"code":"internal","message":"RC transport failed; outcome unknown, not retried"}})
        }
    };
    out.send(&HostFrame::RcResult {
        instance_id: id,
        response: &response,
    });
}
pub async fn subscribe(dir: &Path, id: &str, out: Out) {
    let mut retry = 250;
    loop {
        let _ = stream(dir, id, &out, &mut retry).await;
        out.send(&json!({"type":"subscribe_error","instanceId":id,"code":"unreachable"}));
        sleep(Duration::from_millis(retry)).await;
        retry = (retry * 2).min(5000)
    }
}
async fn stream(dir: &Path, id: &str, out: &Out, retry: &mut u64) -> Result<()> {
    let mut saved = vec![];
    let mut bytes = 0;
    let mut save = |v: Value| {
        bytes += v.to_string().len();
        if bytes > LIMIT {
            out.overflow();
            anyhow::bail!("resync overflow")
        }
        saved.push(v);
        Ok(())
    };
    let (mut c, s, h) = timeout(Duration::from_secs(10), async {
        let mut c = Rc::open(dir, id).await?;
        let s = c.hello(&mut save).await?;
        let mut req = rc::request("history");
        if !s["session"].is_null() {
            req["expectSessionId"] = s["session"]["id"].clone()
        }
        let r = c.send(req, &mut save).await?;
        ensure!(r["ok"] == true);
        let h = r["result"].clone();
        ensure!(s["session"]["id"] == h["session"]["id"]);
        Ok::<_, anyhow::Error>((c, s, h))
    })
    .await??;
    let mut seq = s["seq"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing seq"))?;
    for v in &saved {
        if v["seq"].as_u64().is_some_and(|n| n <= seq)
            && matches!(
                v["event"]["type"].as_str(),
                Some("notify" | "compact_error" | "compact_done" | "session_op_done")
            )
        {
            out.send(v)
        }
    }
    out.send(&HostFrame::Subscribed {
        instance_id: id,
        snapshot: &s,
        history: &h,
    });
    for v in saved {
        if let Some(n) = v["seq"].as_u64()
            && n > seq
        {
            out.send(&v);
            seq = n
        }
    }
    *retry = 250;
    let mut pending: Option<Value> = None;
    let mut due = Instant::now();
    loop {
        let v = tokio::select! {biased;
            _=sleep_until(due),if pending.is_some()=>{out.send(&pending.take().unwrap());continue},
            v=c.next()=>v,
        };
        let v = match v {
            Ok(v) => v,
            Err(e) => {
                if let Some(p) = pending {
                    out.send(&p)
                }
                return Err(e);
            }
        };
        if v["type"] != "event" || v["instanceId"] != id {
            continue;
        }
        let Some(n) = v["seq"].as_u64().filter(|n| *n > seq) else {
            continue;
        };
        seq = n;
        let delta = matches!(
            v["event"]["type"].as_str(),
            Some("text_delta" | "thinking_delta")
        ) && v["event"]["delta"].is_string();
        let mut v = v;
        if let Some(p) = pending.take() {
            if delta && p["sessionId"] == v["sessionId"] && p["event"]["type"] == v["event"]["type"]
            {
                let text = p["event"]["delta"].as_str().unwrap().to_owned()
                    + v["event"]["delta"].as_str().unwrap();
                if text.len() > LIMIT {
                    out.overflow();
                    anyhow::bail!("delta overflow")
                }
                v["event"]["delta"] = json!(text);
                pending = Some(v);
                continue;
            }
            out.send(&p)
        }
        if delta {
            pending = Some(v);
            due = Instant::now() + Duration::from_millis(50)
        } else {
            out.send(&v)
        }
    }
}
