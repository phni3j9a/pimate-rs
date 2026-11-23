use crate::{
    link::Links,
    rc,
    server::{LIMIT, Out},
};
use anyhow::{Result, ensure};
use pimate_protocol::relay::HostFrame;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::time::{Instant, sleep, sleep_until};
pub fn forward(
    links: &Links,
    dir: &Path,
    id: &str,
    req: Value,
    out: Out,
) -> impl Future<Output = ()> + use<> {
    let id = id.to_owned();
    let rid = req["id"].clone();
    let wait = match req["method"].as_str() {
        Some("auth_wait") => pimate_protocol::AUTH_MAX_WAIT_MS + 2000,
        Some("auth_list") => 32000,
        Some("auth_logout") => 62000,
        Some("subagents_rpc") => req["params"]["timeoutMs"]
            .as_u64()
            .unwrap_or(15000)
            .min(120000)
            .saturating_add(2000)
            .max(5000),
        Some("subagent_control") => 17000,
        _ => 5000,
    };
    // Enqueue before returning the future: the WS reader determines write order, not task scheduling.
    let mut c = links.client(dir, &id, &out, false);
    let written = c.enqueue(req);
    async move {
        let response = match async {
            let sent_at = written?.await?;
            tokio::time::timeout_at(sent_at + Duration::from_millis(wait), c.next()).await?
        }
        .await
        {
            Ok(v) => v,
            _ => {
                json!({"type":"response","id":rid,"ok":false,"error":{"code":"internal","message":"RC transport failed; outcome unknown, not retried"}})
            }
        };
        out.send(&HostFrame::RcResult {
            instance_id: &id,
            response: &response,
        });
    }
}
pub fn subscribe(
    links: Arc<Links>,
    dir: std::path::PathBuf,
    id: String,
    out: Out,
) -> impl Future<Output = ()> {
    let mut c = links.client(&dir, &id, &out, true);
    async move {
        let mut retry = 250;
        let mut last = None;
        loop {
            let _ = stream(c, &id, &out, &mut retry, &mut last).await;
            out.send(&json!({"type":"subscribe_error","instanceId":id,"code":"unreachable"}));
            sleep(Duration::from_millis(retry)).await;
            retry = (retry * 2).min(5000);
            c = links.client(&dir, &id, &out, true);
        }
    }
}
async fn stream(
    mut c: crate::link::Client,
    id: &str,
    out: &Out,
    retry: &mut u64,
    last: &mut Option<u64>,
) -> Result<()> {
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
    let s = c.hello(&mut save).await?;
    let mut req = rc::request("history");
    if !s["session"].is_null() {
        req["expectSessionId"] = s["session"]["id"].clone()
    }
    let r = c.send(req, &mut save).await?;
    ensure!(r["ok"] == true);
    let h = &r["result"];
    ensure!(s["session"]["id"] == h["session"]["id"]);
    let mut seq = s["seq"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing seq"))?;
    for v in &saved {
        if v["seq"]
            .as_u64()
            .is_some_and(|n| n <= seq && last.is_none_or(|p| n > p))
            && matches!(
                v["event"]["type"].as_str(),
                Some("notify" | "compact_error" | "compact_done" | "session_op_done")
            )
        {
            out.send(v);
            *last = v["seq"].as_u64();
        }
    }
    out.send(&HostFrame::Subscribed {
        instance_id: id,
        snapshot: &s,
        history: h,
    });
    for v in saved {
        if let Some(n) = v["seq"].as_u64()
            && n > seq
        {
            out.send(&v);
            seq = n
        }
    }
    *last = Some(seq);
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
        *last = Some(seq);
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
