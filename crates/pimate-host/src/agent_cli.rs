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
// A busy prompt may settle before its acknowledgement; an idle prompt must see its own start.
#[derive(Default)]
struct Turn {
    seq: u64,
    last: Value,
    pending: Vec<Value>,
    started: bool,
    prompt_in_flight: bool,
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
                if self.prompt_in_flight {
                    self.started = true;
                    self.held = None;
                }
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
    if method == "ui_respond" && v["ok"] != true {
        anyhow::bail!(if v["error"]["code"] == "no_such_request" {
            "no_such_request"
        } else {
            "UI answer rejected"
        });
    }
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
// Match display labels on the PC; never include raw options in CLI output or errors.
fn match_option<'a>(
    value: &mut Value,
    options: impl Iterator<Item = &'a str>,
    home: &str,
) -> Result<()> {
    let pick = value.as_str().context("Invalid selection")?;
    let mut matches = options.filter(|o| crate::webhook::display_text(o, home) == pick);
    let original = matches.next().context("Unknown option")?;
    ensure!(matches.next().is_none(), "Ambiguous option");
    *value = original.into();
    Ok(())
}
fn match_ui_options(params: &mut Value, pending: &[Value], home: &str) -> Result<()> {
    let req = pending
        .iter()
        .find(|r| r["requestId"] == params["requestId"])
        .context("no_such_request")?;
    if req["kind"] == "select"
        && let Some(value) = params.get_mut("value")
    {
        match_option(
            value,
            req["options"]
                .as_array()
                .context("Missing options")?
                .iter()
                .filter_map(Value::as_str),
            home,
        )?;
    } else if req["kind"] == "questionnaire"
        && let Some(answers) = params.get_mut("answers").and_then(Value::as_array_mut)
    {
        let questions = req["questions"].as_array().context("Missing questions")?;
        ensure!(answers.len() == questions.len(), "Answer count mismatch");
        for (a, q) in answers.iter_mut().zip(questions) {
            for pick in a
                .get_mut("selected")
                .and_then(Value::as_array_mut)
                .context("Invalid selection")?
            {
                match_option(
                    pick,
                    q["options"]
                        .as_array()
                        .context("Missing options")?
                        .iter()
                        .filter_map(|o| o["label"].as_str()),
                    home,
                )?;
            }
        }
    } else {
        anyhow::bail!("Card label does not match dialog kind");
    }
    Ok(())
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
        let (id, name, isolation) = Terminal
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
    let mut redact = false;
    let mut secs = 300;
    let mut last = None;
    let mut method = cmd;
    let mut params = json!({});
    let mut card_labels = false;
    match cmd {
        "prompt" | "wait" => {
            secs = option(&mut args, "--timeout")?.unwrap_or(300);
            if cmd == "prompt" {
                wait = flag(&mut args, "--wait");
                let steer = flag(&mut args, "--steer");
                let follow = flag(&mut args, "--follow-up");
                ensure!(!(steer && follow));
                // A lone "--redact" is the prompt text, not an option.
                redact = wait && args.len() > 1 && flag(&mut args, "--redact");
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
                redact = flag(&mut args, "--redact");
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
                "--label" | "--answer-labels" if args.len() == 2 => {
                    params["value"] = args[1].clone().into()
                }
                "--label" => {
                    ensure!(args.len() == 3, "--label requires one label");
                    card_labels = true;
                    params["value"] = args[2].clone().into();
                }
                "--answers" | "--answer-labels" => {
                    card_labels = args[1] == "--answer-labels";
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
        prompt_in_flight: cmd == "prompt",
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
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let stream = async {
            while let Ok(v) = c.next().await {
                if v["type"] == "event" && v["instanceId"] == id {
                    print(&v)?
                }
            }
            std::future::pending::<Result<()>>().await
        };
        return tokio::select! {r=stream=>r,_=term.recv()=>Ok(()),r=tokio::signal::ctrl_c()=>Ok(r?)};
    }
    let mut reply = json!({});
    if cmd != "wait" {
        let marker = if cmd == "ui" {
            if card_labels {
                match_ui_options(&mut params, &t.pending, &crate::webhook::display_home())?;
            }
            crate::webhook::mark(&id, &params["requestId"]).unwrap_or_else(|_| {
                eprintln!("webhook answer marker failed");
                None
            })
        } else {
            None
        };
        reply = call(&mut c, method, params, &mut t)
            .await
            .inspect_err(|_| {
                if let Some(m) = marker {
                    m.undo()
                }
            })?;
        if cmd == "ui" {
            return print(&json!({}));
        }
        if let Some(n) = last {
            let mut rows = vec![];
            for v in items(&reply)? {
                if v["kind"] == "user"
                    || v["kind"] == "assistant"
                        && (nonempty(&v["text"]) || nonempty(&v["errorMessage"]))
                {
                    let mut row = json!({"kind":v["kind"],"text":v["text"]});
                    if let Some(id) = v["entryId"].as_str() {
                        row["entryId"] = id.into();
                    }
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
        t.prompt_in_flight = false;
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
    if redact {
        crate::webhook::redact(&mut result);
    }
    reply
        .as_object_mut()
        .context("invalid RC result")?
        .extend(result.as_object().unwrap().clone());
    print(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_only_the_target_and_never_reports_raw_options() {
        let pending = [
            json!({"requestId":"other","kind":"editor","value":"private"}),
            json!({"requestId":"R","kind":"select","options":["/home/test/one", "/home/test/x", "~/x", "sk-aaaaaaaaaaaaaaaaaaaaaaaaa", "sk-bbbbbbbbbbbbbbbbbbbbbbbbb"]}),
        ];
        for (pick, expected) in [
            ("~/one", Ok("/home/test/one")),
            ("/home/test/one", Err("Unknown option")),
            ("~/x", Err("Ambiguous option")),
            ("[redacted]", Err("Ambiguous option")),
            ("other", Err("Unknown option")),
        ] {
            let mut params = json!({"requestId":"R","value":pick});
            match expected {
                Ok(original) => {
                    match_ui_options(&mut params, &pending, "/home/test").unwrap();
                    assert_eq!(params["value"], original);
                }
                Err(error) => assert_eq!(
                    match_ui_options(&mut params, &pending, "/home/test")
                        .unwrap_err()
                        .to_string(),
                    error
                ),
            }
        }
        let pending = [
            json!({"requestId":"R","kind":"questionnaire","questions":[{"options":[{"label":"/home/test/x"}]}]}),
        ];
        let mut params = json!({"requestId":"R","answers":[{"selected":["~/x"],"custom":"~/x"}]});
        match_ui_options(&mut params, &pending, "/home/test").unwrap();
        assert_eq!(
            params["answers"][0],
            json!({"selected":["/home/test/x"],"custom":"~/x"})
        );
    }
}
