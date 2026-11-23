use crate::process::output;
use crate::{operations::home, terminal::executable};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;
use tokio::process::Command;
const SUMMARY: &str = "number,title,state,author,labels,updatedAt,url";
const PR: &str = "isDraft,reviewDecision,statusCheckRollup";
fn safe(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n <= 9007199254740991)
}
pub fn valid(q: &Value) -> bool {
    q.as_object().is_some_and(|o| match q["kind"].as_str() {
        Some("list") => o.len() == 1,
        Some("issue" | "pr") => o.len() == 2 && safe(&q["number"]).is_some_and(|n| n > 0),
        _ => false,
    })
}
fn text(v: &Value, n: usize) -> String {
    String::from_utf16_lossy(
        &v.as_str()
            .unwrap_or_default()
            .encode_utf16()
            .take(n)
            .collect::<Vec<_>>(),
    )
}
fn author(v: &Value) -> String {
    let s = text(&v["login"], 100);
    if s.is_empty() {
        "（削除済み）".into()
    } else {
        s
    }
}
fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}
fn item(v: &Value, k: &str, detail: bool) -> Result<Value> {
    ensure!(
        safe(&v["number"]).is_some_and(|n| n > 0) && v["title"].is_string(),
        "Invalid gh JSON item"
    );
    let labels: Vec<_> = arr(&v["labels"])
        .iter()
        .take(100)
        .map(|l| text(&l["name"], 100))
        .collect();
    let mut r = json!({
        "kind":k,"number":v["number"],"title":text(&v["title"],1000),"state":text(&v["state"],30),
        "author":author(&v["author"]),"url":text(&v["url"],2048),"updatedAt":text(&v["updatedAt"],50),"labels":labels,
    });
    if detail {
        let comments = arr(&v["comments"]);
        let recent = &comments[comments.len().saturating_sub(30)..];
        r["comments"]=recent.iter().map(|c|json!({
            "author":author(&c["author"]),"createdAt":text(&c["createdAt"],50),
            "body":text(&c["body"],8192),"bodyTruncated":c["body"].as_str().is_some_and(|s|s.encode_utf16().count()>8192)
        })).collect();
        r["commentsTruncated"] = (comments.len() > 30).into();
        r["body"] = text(&v["body"], 32768).into();
        r["bodyTruncated"] = v["body"]
            .as_str()
            .is_some_and(|s| s.encode_utf16().count() > 32768)
            .into();
        r["createdAt"] = text(&v["createdAt"], 50).into()
    }
    if k == "pr" {
        r["isDraft"] = (v["isDraft"] == true).into();
        r["reviewDecision"] = text(&v["reviewDecision"], 50).into();
        let mut ci = json!({"passed":0,"failed":0,"pending":0});
        let mut checks = vec![];
        let roll = arr(&v["statusCheckRollup"]);
        for c in roll {
            let status = c["state"].as_str().unwrap_or_else(|| {
                c[if c["status"] == "COMPLETED" {
                    "conclusion"
                } else {
                    "status"
                }]
                .as_str()
                .unwrap_or_default()
            });
            let state = match status.to_ascii_uppercase().as_str() {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => "passed",
                "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STALE"
                | "STARTUP_FAILURE" => "failed",
                _ => "pending",
            };
            ci[state] = (ci[state].as_u64().unwrap() + 1).into();
            if detail && checks.len() < 100 {
                let name = c["name"]
                    .as_str()
                    .or(c["context"].as_str())
                    .unwrap_or("チェック");
                checks.push(json!({"name":text(&json!(name),1000),"state":state}))
            }
        }
        r["ci"] = ci;
        if detail {
            r["checks"] = checks.into();
            r["checksTruncated"] = (roll.len() > 100).into();
            for k in ["additions", "deletions", "changedFiles"] {
                r[k] = safe(&v[k]).unwrap_or_default().into()
            }
            for k in ["headRefName", "baseRefName"] {
                r[k] = text(&v[k], 1000).into()
            }
        }
    }
    Ok(r)
}
async fn gh(cwd: &Path, args: &[&str]) -> Result<String> {
    let mut c = Command::new(executable(Path::new("gh")).context("gh_missing")?);
    c.current_dir(cwd).args(args).envs([
        ("GH_REPO", ""),
        ("GH_PROMPT_DISABLED", "1"),
        ("NO_COLOR", "1"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
    ]);
    let (code, out, err) = output(c, 15, 4 << 20).await?;
    if code != Some(0) {
        let message = if err.trim().is_empty() {
            "GitHub query failed"
        } else {
            err.trim()
        };
        let e = anyhow::anyhow!("{message}");
        return Err(if code == Some(4) {
            e.context("gh_auth")
        } else {
            e
        });
    }
    Ok(out)
}
fn auth(e: &str) -> bool {
    ["gh_auth", "gh auth login", "not logged in"]
        .iter()
        .any(|s| e.contains(s))
}
pub async fn status() -> Value {
    let r = async {
        gh(
            &home()?,
            &["api", "--hostname", "github.com", "user", "--jq", ".login"],
        )
        .await
    }
    .await;
    match r {
        Ok(s) => {
            let s = s.trim();
            if (1..=39).contains(&s.len())
                && !s.starts_with('-')
                && !s.ends_with('-')
                && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return json!({"ok":true,"status":{"installed":true,"loggedIn":true,"login":s}});
            }
        }
        Err(e) => {
            let s = e.to_string().to_lowercase();
            if s == "gh_missing" || auth(&s) || s.contains("http 401") {
                return json!({"ok":true,"status":{"installed":s!="gh_missing","loggedIn":false}});
            }
        }
    };
    json!({"ok":false,"code":"failed"})
}
async fn data(cwd: &Path, q: &Value) -> Result<Value> {
    async fn get(d: &Path, a: &[&str]) -> Result<Value> {
        Ok(serde_json::from_str(&gh(d, a).await?)?)
    }
    let kind = q["kind"].as_str().context("invalid query")?;
    if kind == "list" {
        let repo = get(cwd, &["repo", "view", "--json", "nameWithOwner,url"]).await?;
        ensure!(repo["nameWithOwner"].is_string());
        let mut v = json!({"kind":"list","repo":{"name":text(&repo["nameWithOwner"],1000),"url":text(&repo["url"],2048)}});
        for k in ["issue", "pr"] {
            let fields = if k == "pr" {
                format!("{SUMMARY},{PR}")
            } else {
                SUMMARY.into()
            };
            let rows = get(
                cwd,
                &[
                    k, "list", "--state", "open", "--limit", "30", "--json", &fields,
                ],
            )
            .await?;
            v[format!("{k}s")] = json!(
                rows.as_array()
                    .context("invalid gh list")?
                    .iter()
                    .take(30)
                    .map(|x| item(x, k, false))
                    .collect::<Result<Vec<_>>>()?
            )
        }
        Ok(v)
    } else {
        let mut fields = format!("{SUMMARY},body,comments,createdAt");
        if kind == "pr" {
            fields += &format!(",{PR},headRefName,baseRefName,additions,deletions,changedFiles")
        }
        let v = get(
            cwd,
            &[kind, "view", &q["number"].to_string(), "--json", &fields],
        )
        .await?;
        Ok(json!({"kind":"detail","item":item(&v,kind,true)?}))
    }
}
pub async fn query(cwd: Option<String>, q: &Value) -> Value {
    let Some(dir) = cwd else {
        return json!({"ok":false,"code":"not_found","message":"Pi instance not found"});
    };
    match data(Path::new(&dir), q).await {
        Ok(v) => json!({"ok":true,"data":v}),
        Err(e) => {
            let s = e.to_string().to_lowercase();
            let code = if s == "gh_missing" {
                "gh_missing"
            } else if auth(&s) {
                "gh_auth"
            } else if [
                "not a git repository",
                "no git remotes",
                "none of the git remotes",
                "not a github",
                "not on github",
                "could not determine",
            ]
            .iter()
            .any(|v| s.contains(v))
            {
                "not_github"
            } else if [
                "not found",
                "could not resolve to",
                "could not find",
                "no pull requests found",
            ]
            .iter()
            .any(|v| s.contains(v))
            {
                "not_found"
            } else {
                "failed"
            };
            let message = match code {
                "gh_missing" => "GitHub CLI is not installed",
                "gh_auth" => "GitHub login required",
                "not_github" => "Not a GitHub repository",
                "not_found" => "Issue or pull request not found",
                _ => "GitHub query failed",
            };
            json!({"ok":false,"code":code,"message":message})
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_query_and_limits() -> Result<()> {
        assert!(!valid(&json!({"kind":"list","repo":"a/b"})));
        assert!(!valid(&json!({"kind":"pr","number":9007199254740992u64})));
        assert!(valid(&json!({"kind":"issue","number":1})));
        let item = item(
            &json!({"number":1,"title":"a","body":"a".repeat(33000),"comments":vec![json!({"body":"漢"});31],"statusCheckRollup":[{"state":"SUCCESS"},{"status":"COMPLETED","conclusion":"FAILURE"},{"status":"QUEUED"}]}),
            "pr",
            true,
        )?;
        assert_eq!(item["bodyTruncated"], true);
        assert_eq!(item["body"].as_str().unwrap().len(), 32768);
        assert_eq!(item["comments"].as_array().unwrap().len(), 30);
        assert_eq!(item["ci"], json!({"passed":1,"failed":1,"pending":1}));
        Ok(())
    }
}
