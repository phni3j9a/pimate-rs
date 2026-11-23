use crate::{config, operations::home, rc, service::write};
use anyhow::{Context, Result, bail, ensure};
use chrono::{SecondsFormat, Utc};
use reqwest::{
    Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::error::Error as _;
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{Duration, SystemTime},
};
const WEEK: Duration = Duration::from_secs(7 * 86400);
pub fn key(v: Value) -> String {
    format!("{:x}", Sha256::digest(v.to_string()))[..16].into()
}
fn url(s: &str) -> Result<Url> {
    let u = Url::parse(s).map_err(|_| anyhow::anyhow!("Invalid webhook URL"))?;
    ensure!(
        u.username().is_empty()
            && u.password().is_none()
            && (u.scheme() == "https"
                || u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "localhost"))),
        "Invalid webhook URL"
    );
    Ok(u)
}
fn headers(v: &Value) -> Result<HeaderMap> {
    let mut h = HeaderMap::new();
    for (k, v) in v.as_object().context("Invalid headers")? {
        h.insert(
            HeaderName::from_bytes(k.as_bytes()).context("Invalid header")?,
            HeaderValue::from_str(v.as_str().context("Invalid header")?)
                .map_err(|_| anyhow::anyhow!("Invalid header"))?,
        );
    }
    Ok(h)
}
pub fn saved(dir: &Path) -> Option<Value> {
    let c: Value = serde_json::from_slice(&fs::read(dir.join("webhook.json")).ok()?).ok()?;
    url(c["url"].as_str()?).ok()?;
    headers(&c["headers"]).ok()?;
    c["machine"].as_str()?;
    Some(c)
}
pub fn watches(dir: &Path) -> Vec<String> {
    let p = dir.join("webhook-watch");
    if !p.exists() || config::private(&p).is_err() {
        return vec![];
    }
    let mut v: Vec<_> = fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|s| rc::valid_id(s))
        .collect();
    v.sort();
    v
}
fn remove(p: &Path) -> Result<()> {
    if let Err(e) = fs::remove_file(p)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(e.into());
    }
    Ok(())
}
fn create(p: &Path, s: &str) -> Result<bool> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)
    {
        Ok(mut f) => {
            f.write_all(s.as_bytes())?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub struct Marker(PathBuf, String);
impl Marker {
    pub fn undo(self) {
        if fs::read_to_string(&self.0).ok().as_ref() == Some(&self.1) {
            let _ = remove(&self.0);
        }
    }
}
pub fn mark(id: &str, r: &Value) -> Result<Option<Marker>> {
    let dir = config::config_dir()?;
    config::private(&dir)?;
    let d = dir.join("answered-here");
    config::private(&d)?;
    for e in fs::read_dir(&d)?.flatten() {
        let n = e.file_name();
        if n.len() == 16
            && n.as_encoded_bytes().iter().all(u8::is_ascii_hexdigit)
            && e.path().symlink_metadata().is_ok_and(|m| {
                m.is_file()
                    && m.modified()
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|a| a > WEEK)
            })
        {
            remove(&e.path())?
        }
    }
    let path = d.join(key(json!([id, r])));
    let token = json!(uuid::Uuid::new_v4().to_string()).to_string();
    Ok(create(&path, &token)?.then_some(Marker(path, token)))
}
pub fn answered(dir: &Path, k: &str) -> bool {
    let d = dir.join("answered-here");
    d.exists()
        && config::private(&d).is_ok()
        && d.join(k).symlink_metadata().is_ok_and(|m| {
            m.is_file()
                && m.modified()
                    .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() <= WEEK)
        })
}
// Secret patterns adapted from T3 Tots t3listen (MIT), as in the upstream Host.
static RED: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r###"(?i)(sk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}|(?:sk|rk)_[a-z]+_[A-Za-z0-9_]{16,}|whsec_[A-Za-z0-9+/=_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[abposr]-[A-Za-z0-9-]{10,}|xapp-[A-Za-z0-9-]{10,}|npm_[A-Za-z0-9]{30,}|glpat-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|-----BEGIN [A-Z ]*PRIVATE KEY-----|https://hooks\.slack\.com/services/[A-Za-z0-9/]+|https://(?:ptb\.|canary\.)?discord(?:app)?\.com/api/webhooks/\d+/[\w-]+|eyJ[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_.-]+|\b(?:(?:api|secret|access|private|client)[_-]?(?:key|secret)s?|tokens?|secrets?|passwds?|passwords?)["']?\s*[:=]\s*(?:"(?:[^"\\\n]|\\.){4,}"|'(?:[^'\\\n]|\\.){4,}'|\S{6,})|\bauthorization\s*[:=]\s*(?:(?:bearer|basic|token)\s+)?[A-Za-z0-9._~+/=-]{12,}|\bbearer\s+[A-Za-z0-9._~+/=-]{16,})"###).expect("redaction pattern")
});
fn clean(v: &mut Value, k: &str, home: &str) {
    match v {
        Value::String(s) if k != "id" && !k.ends_with("Id") => {
            *s = RED.replace_all(s, "[redacted]").into_owned();
            if !home.is_empty() {
                *s = s.replace(home, "~")
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|v| clean(v, k, home)),
        Value::Object(m) => m.iter_mut().for_each(|(k, v)| clean(v, k, home)),
        _ => (),
    }
}
pub fn payload(
    c: &Value,
    event: &str,
    row: &Value,
    session: &Value,
    reference: &str,
    mut d: Value,
) -> Value {
    let cwd = row["cwd"].as_str().unwrap_or("");
    let title = row["session"]["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            Path::new(cwd)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
        });
    let id = row["instanceId"].as_str().unwrap_or("");
    let machine = c["machine"].as_str().unwrap_or("");
    let place = format!("\"{title}\" on {machine}");
    let text = match event {
        "test" => format!("Pi webhook test on {machine}."),
        "request.resolved" => format!(
            "Pi request in {place} was resolved by {}.",
            d["by"].as_str().unwrap_or("")
        ),
        "request.opened" => {
            let qs = d["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|q| {
                    let s = q["question"].as_str().unwrap_or("");
                    let opts = q["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|o| o["label"].as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    if opts.is_empty() {
                        s.into()
                    } else {
                        format!("{s} ({opts})")
                    }
                })
                .collect::<Vec<_>>()
                .join("; ");
            let opts = d["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let title = d["title"].as_str().filter(|_| {
                !d["questions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|q| q["question"] == d["title"])
            });
            format!(
                "Pi needs input in {place}: {}",
                [title, d["message"].as_str(), Some(&qs), Some(&opts)]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        }
        _ => {
            let mut s = format!(
                "Pi {} in {place}.",
                match event {
                    "pi.finished" => "finished",
                    "pi.failed" => "failed",
                    _ => d["kind"].as_str().unwrap_or(""),
                }
            );
            for (k, label) in [("lastReply", "Last reply"), ("error", "Error")] {
                if let Some(t) = d[k].as_str() {
                    s += &format!(" {label}: {t}")
                }
            }
            s
        }
    };
    d.as_object_mut().unwrap().extend(json!({"event":event,"eventId":key(json!([event,id,reference])),"machine":machine,"instanceId":id,"sessionId":session,"title":title,"cwd":cwd,"sentAt":Utc::now().to_rfc3339_opts(SecondsFormat::Millis,true),"text":text}).as_object().unwrap().clone());
    clean(
        &mut d,
        "",
        home()
            .ok()
            .and_then(|p| p.into_os_string().into_string().ok())
            .as_deref()
            .unwrap_or(""),
    );
    d
}
async fn post(c: &Value, v: &Value) -> Result<(u16, bool)> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut r = client
        .post(url(c["url"].as_str().context("Invalid webhook URL")?)?)
        .header("user-agent", concat!("pimate/", env!("CARGO_PKG_VERSION")))
        .json(v)
        .headers(headers(&c["headers"])?)
        .send()
        .await
        .map_err(|e| {
            let mut reason = e.source();
            let mut code = if e.is_timeout() {
                "TimeoutError"
            } else {
                "ERROR"
            };
            while let Some(e) = reason {
                if let Some(io) = e.downcast_ref::<std::io::Error>() {
                    code = match io.kind() {
                        std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
                        std::io::ErrorKind::ConnectionReset => "ECONNRESET",
                        std::io::ErrorKind::TimedOut => "ETIMEDOUT",
                        _ => code,
                    };
                }
                reason = e.source()
            }
            anyhow::anyhow!("WEBHOOK_UNREACHABLE {code}")
        })?;
    let code = r.status();
    let mut body = vec![];
    if !code.is_success() {
        while let Some(b) = r.chunk().await.unwrap_or_default() {
            body.extend_from_slice(&b[..b.len().min(4096 - body.len())]);
            if body.len() == 4096 {
                break;
            }
        }
    }
    Ok((code.as_u16(), rate_limited(&body)))
}
fn rate_limited(body: &[u8]) -> bool {
    static RATE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new("(?i)rate.?limit|too many|try again").unwrap());
    RATE.is_match(&String::from_utf8_lossy(body))
}
// shortcut: retries live in memory only; durable delivery would need an outbox.
pub async fn deliver(
    dir: &Path,
    v: Value,
    initial: tokio::sync::OwnedSemaphorePermit,
    retries: &tokio::sync::Semaphore,
) -> bool {
    let mut initial = Some(initial);
    let mut retry_slot = None;
    let start = tokio::time::Instant::now();
    let mut rounds = 0;
    loop {
        let Some(c) = saved(dir) else { return false };
        if !watches(dir).iter().any(|id| v["instanceId"] == *id) {
            return false;
        }
        let (status, rate) = post(&c, &v).await.unwrap_or((0, false));
        eprintln!(
            "webhook {} {} HTTP {status}",
            v["event"].as_str().unwrap_or(""),
            v["eventId"].as_str().unwrap_or("")
        );
        if (200..300).contains(&status) {
            return true;
        }
        drop(initial.take());
        if retry_slot.is_none() {
            let Ok(slot) = retries.try_acquire() else {
                eprintln!("webhook retry queue full");
                return false;
            };
            retry_slot = Some(slot)
        }
        let Some(delay) = retry_delay(status, rate, rounds, start.elapsed()) else {
            return false;
        };
        rounds += 1;
        tokio::time::sleep(Duration::from_secs(delay)).await
    }
}
fn retry_delay(status: u16, rate: bool, rounds: u32, age: Duration) -> Option<u64> {
    let retry = status == 0 || matches!(status, 408 | 429) || status >= 500 || rate;
    if age >= Duration::from_secs(86400) || !retry && rounds >= 10 {
        return None;
    }
    let delay = if retry && rounds < 2 {
        (if rate { 20 } else { 2 }) * (if rounds == 1 { 3 } else { 1 })
    } else {
        60
    };
    Some(delay)
}
pub async fn cli(args: Vec<String>) -> Result<()> {
    let dir = config::config_dir()?;
    config::private(&dir)?;
    let Some((action, rest)) = args.split_first() else {
        bail!("missing webhook action")
    };
    let p = dir.join("webhook.json");
    let v = match action.as_str() {
        "setup" => {
            let machine = match rest {
                [] => nix::unistd::gethostname()?.to_string_lossy().into_owned(),
                [k, v] if k == "--machine" => v.clone(),
                _ => bail!("webhook setup [--machine NAME]"),
            };
            ensure!(!machine.trim().is_empty());
            use std::io::IsTerminal;
            let input = std::io::stdin();
            if input.is_terminal() {
                eprintln!(
                    "Line 1: Webhook URL starting https://\nLine 2: Authorization header starting Authorization:"
                )
            }
            let mut lines = input.lock().lines();
            let u = lines.next().context("missing URL")??;
            url(u.trim())?;
            let mut h = json!({});
            for line in lines {
                let line = line?;
                if line.trim().is_empty() {
                    break;
                }
                let (k, v) = line.split_once(':').context("Invalid header")?;
                h[k] = v.trim_start().into();
                if k.eq_ignore_ascii_case("authorization") {
                    break;
                }
            }
            headers(&h)?;
            write(
                &p,
                json!({"url":u.trim(),"headers":h,"machine":machine})
                    .to_string()
                    .as_bytes(),
            )?;
            println!("saved webhook.json");
            return Ok(());
        }
        "status" if rest.is_empty() => {
            let c = saved(&dir);
            json!({"configured":c.is_some(),"machine":c.as_ref().map(|s|&s["machine"]),"watched":watches(&dir)})
        }
        "off" if rest.is_empty() => {
            remove(&p)?;
            let w = dir.join("webhook-watch");
            if w.exists() {
                config::private(&w)?;
                fs::remove_dir_all(w)?
            }
            println!("webhook off");
            return Ok(());
        }
        "watch" | "unwatch" if rest.len() == 1 => {
            let id = &rest[0];
            ensure!(rc::valid_id(id));
            let existing: HashSet<_> = rc::list(&config::run_dir())
                .await?
                .instances
                .into_iter()
                .filter_map(|v| v["instanceId"].as_str().map(str::to_owned))
                .collect();
            ensure!(action != "watch" || existing.contains(id), "NOT_FOUND");
            let d = dir.join("webhook-watch");
            config::private(&d)?;
            if action == "watch" {
                create(&d.join(id), "")?;
            } else {
                remove(&d.join(id))?
            }
            json!({"watched":watches(&dir).into_iter().filter(|id|existing.contains(id)).collect::<Vec<_>>()})
        }
        "test" if rest.is_empty() => {
            let c = saved(&dir).context("WEBHOOK_NOT_CONFIGURED")?;
            let (status, _) = post(
                &c,
                &payload(
                    &c,
                    "test",
                    &Value::Null,
                    &Value::Null,
                    &Utc::now().timestamp_millis().to_string(),
                    json!({}),
                ),
            )
            .await?;
            ensure!(
                (200..300).contains(&status),
                "WEBHOOK_REJECTED HTTP {status}{}",
                if matches!(status, 401 | 403) {
                    " (key changed, run setup again)"
                } else {
                    ""
                }
            );
            json!({"ok":true,"status":status})
        }
        _ => bail!("invalid webhook command"),
    };
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rate_limit_uses_transient_window() {
        let limited = rate_limited(br#"{"error":"RATE_LIMITED"}"#);
        assert_eq!(
            retry_delay(403, limited, 11, Duration::from_secs(1500)),
            Some(60)
        );
        assert_eq!(
            retry_delay(403, limited, 11, Duration::from_secs(86400)),
            None
        );
        assert_eq!(retry_delay(403, false, 10, Duration::from_secs(600)), None);
    }
    #[tokio::test]
    async fn fresh_event_posts_with_retry_queue_full() -> Result<()> {
        use std::sync::Arc;
        use tokio::sync::Semaphore;
        let dir = tempfile::tempdir()?;
        let d = dir.path();
        config::private(&d.join("webhook-watch"))?;
        fs::write(d.join("webhook-watch/i"), "")?;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        fs::write(
            d.join("webhook.json"),
            json!({"machine":"test","headers":{},"url":format!("http://{}/",l.local_addr()?)})
                .to_string(),
        )?;
        let req = axum::Router::new().route(
            "/",
            axum::routing::post(|axum::Json(v): axum::Json<Value>| async move {
                if v["eventId"] == "1" {
                    axum::http::StatusCode::FORBIDDEN
                } else {
                    axum::http::StatusCode::OK
                }
            }),
        );
        let srv = tokio::spawn(async move { axum::serve(l, req).await });
        let initial = Arc::new(Semaphore::new(1));
        let retries = Semaphore::new(1);
        let first = deliver(
            d,
            json!({"instanceId":"i","eventId":"1"}),
            initial.clone().try_acquire_owned()?,
            &retries,
        );
        tokio::pin!(first);
        let second = async {
            let slot = initial.acquire_owned().await?;
            assert_eq!(retries.available_permits(), 0);
            assert!(deliver(d, json!({"instanceId":"i","eventId":"2"}), slot, &retries).await);
            Ok::<_, anyhow::Error>(())
        };
        tokio::select! {_=&mut first=>panic!("expected pending retry"),r=tokio::time::timeout(Duration::from_secs(3),second)=>r??};
        srv.abort();
        Ok(())
    }
    #[test]
    fn redaction_preserves_ids_and_refuses_unsafe_urls() {
        let mut v = json!({"id":"token=secret123","requestId":"/home/u/r","value":["/home/u/hello token=secret123","Authorization: Bearer secretsecretsecret"]});
        clean(&mut v, "", "/home/u");
        assert_eq!(
            v,
            json!({"id":"token=secret123","requestId":"/home/u/r","value":["~/hello [redacted]","[redacted]"]})
        );
        for s in [
            "http://example.org/",
            "file:///tmp/1",
            "https://user:pw@localhost/",
        ] {
            assert!(url(s).is_err())
        }
        assert!(url("http://127.0.0.1/hook").is_ok());
        let p = payload(
            &json!({"machine":"Mac"}),
            "request.opened",
            &json!({"cwd":"/tmp/project","instanceId":"i"}),
            &json!("s"),
            "r",
            json!({"requestId":"r","title":"Which?","questions":[{"question":"Which?","options":[{"label":"A"}]}]}),
        );
        assert_eq!(
            p["text"],
            "Pi needs input in \"project\" on Mac: Which? (A)"
        );
    }
}
