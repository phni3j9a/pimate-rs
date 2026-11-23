use crate::{
    rc::{self, Rc},
    terminal::Terminal,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tokio::time::{Instant, sleep, timeout};
pub fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .context("HOME missing")
}
fn expand(s: &str) -> Option<PathBuf> {
    if s == "~" {
        home().ok()
    } else if let Some(v) = s.strip_prefix("~/") {
        Some(home().ok()?.join(v.trim_start_matches('/')))
    } else {
        Some(PathBuf::from(s)).filter(|p| p.is_absolute())
    }
}
pub fn dirs(path: &str) -> Vec<String> {
    let Some(files) = expand(path).and_then(|p| fs::read_dir(p).ok()) else {
        return vec![];
    };
    let mut names: Vec<_> = files
        .take(5000)
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|key| env::var(key).ok().filter(|s| !s.is_empty()))
        .and_then(|s| {
            s.split(['.', '@'])
                .next()?
                .replace('_', "-")
                .parse::<icu_locale_core::Locale>()
                .ok()
        })
        .unwrap_or(icu_locale_core::locale!("en-US"));
    let collator = icu_collator::Collator::try_new(locale.into(), Default::default())
        .expect("compiled ICU collation data");
    names.sort_by(|a, b| collator.compare(a, b));
    names.truncate(1000);
    names
}
fn dated(dir: &Path, directories: bool) -> Vec<(u64, PathBuf)> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let m = e.metadata().ok()?;
            if !(directories && m.is_dir()
                || !directories && m.is_file() && p.extension()? == "jsonl")
            {
                return None;
            }
            Some((
                m.modified()
                    .ok()?
                    .duration_since(UNIX_EPOCH)
                    .ok()?
                    .as_millis() as u64,
                p,
            ))
        })
        .collect();
    entries.sort_unstable_by(|a, b| b.cmp(a));
    entries
}
pub fn folders(rows: &[Value]) -> Vec<Value> {
    let mut map = BTreeMap::new();
    let mut add = |p: &str, t: u64, n: u32| {
        let p = Path::new(p);
        if p.is_absolute()
            && p.is_dir()
            && let Ok(p) = p.canonicalize()
        {
            let v = map.entry(p).or_insert((0, 0));
            v.0 = t.max(v.0);
            v.1 += n;
        }
    };
    for row in rows {
        if let Some(p) = row["cwd"].as_str() {
            add(p, row["updatedAt"].as_u64().unwrap_or(0), 1)
        }
    }
    let agent = env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .or_else(|| Some(home().ok()?.join(".pi/agent")));
    let mut trust = Value::Null;
    if let Some(a) = agent {
        for (_, d) in dated(&a.join("sessions"), true).into_iter().take(200) {
            if let Some((time, file)) = dated(&d, false).first()
                && let Ok(f) = fs::File::open(file)
                && let Some(Ok(line)) = BufReader::new(f.take(16384)).lines().next()
                && let Ok(h) = serde_json::from_str::<Value>(&line)
                && h["type"] == "session"
                && let Some(s) = h["cwd"].as_str()
            {
                add(s, *time, 0)
            }
        }
        let p = a.join("trust.json");
        if fs::metadata(&p).is_ok_and(|m| m.is_file() && m.len() < 1 << 20)
            && let Ok(s) = fs::read_to_string(p)
        {
            trust = serde_json::from_str(s.trim_start_matches('\u{feff}')).unwrap_or_default()
        }
    }
    let mut out: Vec<_> = map
        .into_iter()
        .map(|(p, (t, n))| {
            let mut v = json!({"path":p,"running":n});
            if t > 0 {
                v["lastUsed"] = t.into()
            }
            if let Some(b) = p
                .ancestors()
                .find_map(|d| trust.get(d.to_str()?)?.as_bool())
            {
                v["trusted"] = b.into()
            }
            v
        })
        .collect();
    out.sort_by_key(|v| std::cmp::Reverse(v["lastUsed"].as_u64()));
    out.truncate(200);
    out
}
pub async fn ready(dir: &Path, id: &str) -> Result<(Rc, Value)> {
    timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(mut rc) = Rc::open(dir, id).await
                && let Ok(Ok(s)) = timeout(Duration::from_secs(2), rc.hello(|_| Ok(()))).await
            {
                return (rc, s);
            }
            sleep(Duration::from_millis(250)).await
        }
    })
    .await
    .context("RC readiness timed out")
}
pub async fn spawn(
    term: &Terminal,
    dir: &Path,
    params: &Value,
    last: &tokio::sync::Mutex<Option<Instant>>,
) -> Result<Value> {
    let p: pimate_protocol::SpawnParams =
        serde_json::from_value(params.clone()).context("bad_request")?;
    ensure!(
        !p.cwd.contains('\0')
            && p.message
                .as_ref()
                .is_none_or(|s| !s.trim().is_empty() && s.encode_utf16().count() <= 128 * 1024),
        "bad_request"
    );
    let cwd = expand(&p.cwd)
        .context("bad_request")?
        .canonicalize()
        .context("not_found")?;
    ensure!(cwd.is_dir(), "not_directory");
    let mut args = vec![];
    if let Some(m) = p.model {
        ensure!(
            [&m.provider, &m.id]
                .iter()
                .all(|s| !s.is_empty() && !s.contains(|c: char| c.is_whitespace() || c == '\0'))
                && !m.provider.contains('/'),
            "bad_request"
        );
        args.extend(["--model".into(), format!("{}/{}", m.provider, m.id)])
    }
    if let Some(v) = p.thinking {
        args.extend([
            "--thinking".into(),
            serde_json::to_value(v)?.as_str().unwrap().into(),
        ])
    }
    if let Some(v) = p.ask_tool {
        args.extend([
            "--tools".into(),
            if v { "+pimate_ask" } else { "-pimate_ask" }.into(),
        ])
    }
    let mut guard = last.try_lock().context("Another spawn is active")?;
    ensure!(
        guard.is_none_or(|t| t.elapsed() >= Duration::from_secs(3)),
        "Another spawn is too recent"
    );
    *guard = Some(Instant::now());
    let (id, name, _) = term
        .launch(dir, &cwd, p.trust == Some(true), "mobile", &args)
        .await?;
    let mut result = json!({"ok":true,"instanceId":id,"abducoSession":name});
    if let Some(text) = p.message
        && let Err(e) = first(dir, &id, text).await
    {
        result["messageError"] = format!("{e}; not resent").into()
    }
    Ok(result)
}
async fn first(dir: &Path, id: &str, text: String) -> Result<()> {
    let (mut rc, s) = ready(dir, id).await?;
    let mut req = rc::request("prompt");
    req["params"] = json!({"text":text,"expandTemplates":false});
    if s["session"]["id"].is_string() {
        req["expectSessionId"] = s["session"]["id"].clone()
    }
    ensure!(
        timeout(Duration::from_secs(5), rc.send(req, |_| Ok(()))).await??["ok"] == true,
        "prompt failed"
    );
    Ok(())
}
