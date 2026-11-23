use crate::{
    config,
    rc::{self, Rc},
    terminal::Terminal,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{io::Read, path::Path, time::Duration};
use tokio::time::timeout;

fn print(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}
fn flag(args: &mut Vec<String>, f: &str) -> bool {
    args.iter().position(|a| a == f).is_some_and(|i| {
        args.remove(i);
        true
    })
}
fn option(args: &mut Vec<String>, f: &str) -> Result<Option<u32>> {
    let Some(i) = args.iter().position(|a| a == f) else {
        return Ok(None);
    };
    args.remove(i);
    ensure!(i < args.len(), "missing {f} value");
    let n = args.remove(i).parse()?;
    ensure!(n > 0, "invalid {f}");
    Ok(Some(n))
}
fn nonempty(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty())
}
#[derive(Default)]
struct Turn {
    seq: u64,
    last: Value,
    pending: Vec<Value>,
    started: bool,
    held: Option<Value>,
}
impl Turn {
    fn result(&self, outcome: &str) -> Value {
        let mut v = json!({"outcome":outcome});
        for (k, key) in [("text", "reply"), ("errorMessage", "error")] {
            if nonempty(&self.last[k]) {
                v[key] = self.last[k].clone()
            }
        }
        if !self.pending.is_empty() {
            v["pending"] = json!(self.pending)
        }
        v
    }
    fn event(&mut self, m: Value) {
        let Some(seq) = m["seq"].as_u64().filter(|n| *n > self.seq) else {
            return;
        };
        self.seq = seq;
        let e = &m["event"];
        match e["type"].as_str() {
            Some("agent_start") => {
                self.started = true;
                self.held = None;
                self.last = Value::Null
            }
            Some("message_end") if e["item"]["kind"] == "assistant" => {
                self.last = e["item"].clone()
            }
            Some("ui_request_end") => self.pending.retain(|r| r["requestId"] != e["requestId"]),
            Some("ui_request") => {
                let r = &e["request"];
                self.pending.retain(|v| v["requestId"] != r["requestId"]);
                self.pending.push(r.clone());
                self.held = Some(self.result("waiting_input"))
            }
            Some("agent_settled") => {
                self.held = Some(self.result(if e["aborted"] == true {
                    "aborted"
                } else if self.last["stopReason"] == "error" {
                    "failed"
                } else {
                    "done"
                }))
            }
            _ => (),
        }
    }
}
async fn call(c: &mut Rc, method: &str, params: Value, t: &mut Turn) -> Result<Value> {
    let mut req = rc::request(method);
    req["params"] = params;
    let v = timeout(
        Duration::from_secs(5),
        c.send(req, |v| {
            t.event(v);
            Ok(())
        }),
    )
    .await??;
    ensure!(
        v["ok"] == true,
        "{}",
        v["error"]["message"].as_str().unwrap_or("RC failed")
    );
    Ok(v["result"].clone())
}
fn items(h: &Value) -> Result<&Vec<Value>> {
    h["items"].as_array().context("invalid history")
}
pub async fn run(cmd: &str, mut args: Vec<String>) -> Result<()> {
    let run = config::run_dir();
    if cmd == "spawn" {
        ensure!(
            args.first().is_some_and(|s| s == "--cwd")
                && args.len() >= 2
                && (args.len() == 2 || args[2] == "--"),
            "spawn --cwd DIR [-- pi args]"
        );
        let cwd = Path::new(&args[1]).canonicalize()?;
        ensure!(cwd.is_dir());
        let (id, name, isolation) = Terminal::resolve()
            .launch(
                &run,
                &cwd,
                false,
                "grok-bot",
                args.get(3..).unwrap_or_default(),
            )
            .await?;
        return print(
            &json!({"instanceId":id,"abducoSession":name,"attachCommand":format!("pimate attach {name}"),"isolation":isolation}),
        );
    }
    ensure!(!args.is_empty(), "missing instanceId");
    let id = args.remove(0);
    let mut wait = cmd == "wait";
    let mut secs = 300;
    let mut last = None;
    let mut method = cmd;
    let mut params = json!({});
    match cmd {
        "prompt" | "wait" => {
            secs = option(&mut args, "--timeout")?.unwrap_or(300);
            if cmd == "prompt" {
                wait = flag(&mut args, "--wait");
                let steer = flag(&mut args, "--steer");
                let follow = flag(&mut args, "--follow-up");
                ensure!(!(steer && follow));
                let mut text = args.join(" ");
                if text == "-" {
                    text.clear();
                    std::io::stdin().read_to_string(&mut text)?;
                    text.truncate(text.trim_end().len())
                }
                ensure!(!text.is_empty());
                params["text"] = text.into();
                if steer || follow {
                    params["deliverAs"] = if steer { "steer" } else { "followUp" }.into()
                }
            } else {
                ensure!(args.is_empty())
            }
        }
        "history" => {
            last = option(&mut args, "--last")?;
            ensure!(args.is_empty())
        }
        "ui" => {
            ensure!(args.len() >= 2);
            method = "ui_respond";
            params["requestId"] = args[0].clone().into();
            match args[1].as_str() {
                "--confirm" => params["confirmed"] = true.into(),
                "--cancel" => params["cancelled"] = true.into(),
                "--answers" => {
                    ensure!(args.len() == 3);
                    params["answers"] = serde_json::from_str(&args[2])?;
                    ensure!(params["answers"].is_array())
                }
                _ => params["value"] = args[1..].join(" ").into(),
            }
        }
        _ => (),
    }
    let mut c = Rc::open(&run, &id).await?;
    let mut early = vec![];
    let mut bytes = 0;
    let snapshot = timeout(
        Duration::from_secs(5),
        c.hello(|v| {
            bytes += v.to_string().len();
            ensure!(bytes <= crate::server::LIMIT);
            early.push(v);
            Ok(())
        }),
    )
    .await??;
    ensure!(snapshot["purpose"].is_null(), "reserved RC instance");
    let mut t = Turn {
        seq: snapshot["seq"].as_u64().unwrap_or(0),
        pending: snapshot["pendingUI"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        ..Default::default()
    };
    if cmd == "wait" && !t.pending.is_empty() {
        t.held = Some(t.result("waiting_input"))
    }
    for v in early {
        t.event(v)
    }
    if cmd == "watch" {
        while let Ok(v) = c.next().await {
            if v["type"] == "event" && v["instanceId"] == id {
                print(&v)?
            }
        }
        return Ok(());
    }
    let mut reply = json!({});
    if cmd != "wait" {
        reply = call(&mut c, method, params, &mut t).await?;
        if let Some(n) = last {
            let mut rows = vec![];
            for v in items(&reply)? {
                if v["kind"] == "user"
                    || v["kind"] == "assistant"
                        && (nonempty(&v["text"]) || nonempty(&v["errorMessage"]))
                {
                    let mut row = json!({"kind":v["kind"],"text":v["text"]});
                    if nonempty(&v["errorMessage"]) {
                        row["error"] = v["errorMessage"].clone()
                    }
                    rows.push(row)
                }
            }
            return print(&json!(&rows[rows.len().saturating_sub(n as usize)..]));
        }
        if !wait || reply["deliveredAs"] == "command" {
            return print(&reply);
        }
        if reply["deliveredAs"] == "prompt" && !t.started {
            t.held = None
        }
        if t.held.is_none() && !t.pending.is_empty() {
            t.held = Some(t.result("waiting_input"))
        }
    }
    let mut result = if let Some(v) = t.held.take() {
        v
    } else if cmd == "wait" && snapshot["status"] == "idle" {
        t.result("idle")
    } else {
        timeout(Duration::from_secs(secs.into()), async {
            loop {
                let Ok(v) = c.next().await else {
                    break t.result("closed");
                };
                if v["type"] == "event" && v["instanceId"] == id {
                    t.event(v);
                    if let Some(r) = t.held.take() {
                        break r;
                    }
                }
            }
        })
        .await
        .unwrap_or_else(|_| t.result("timeout"))
    };
    if result["outcome"] == "idle"
        || result["outcome"] == "done" && result["reply"].is_null() && result["error"].is_null()
    {
        let h = call(&mut c, "history", json!({}), &mut t).await?;
        t.last = items(&h)?
            .iter()
            .rev()
            .find(|i| i["kind"] == "assistant")
            .cloned()
            .unwrap_or_default();
        result = t.result(if result["outcome"] == "idle" {
            "idle"
        } else if t.last["stopReason"] == "error" {
            "failed"
        } else {
            "done"
        })
    }
    reply
        .as_object_mut()
        .context("invalid RC result")?
        .extend(result.as_object().unwrap().clone());
    print(&reply)
}
