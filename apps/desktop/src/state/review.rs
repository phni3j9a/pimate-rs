use super::*;
use std::collections::HashMap;

pub struct Line<'a> {
    pub old: Option<u64>,
    pub new: Option<u64>,
    pub text: &'a str,
    pub no_newline: bool,
}
pub struct Hunk<'a> {
    pub header: &'a str,
    pub lines: Vec<Line<'a>>,
}
pub struct Patch<'a> {
    pub path: Option<&'a str>,
    pub hunks: Vec<Hunk<'a>>,
    pub added: usize,
    pub removed: usize,
}
fn range(s: &str, prefix: char) -> Option<(u64, u64)> {
    let s = s.strip_prefix(prefix)?;
    let (n, k) = s.split_once(',').unwrap_or((s, "1"));
    let n: u64 = n.parse().ok()?;
    let k = k.parse().ok()?;
    n.checked_add(k)?;
    Some((n, k))
}
impl<'a> Patch<'a> {
    pub fn parse(s: &'a str) -> Self {
        let mut p = Self {
            path: None,
            hunks: vec![],
            added: 0,
            removed: 0,
        };
        let mut lines = s.split('\n').peekable();
        let mut git = false;
        while let Some(l) = lines.next() {
            if l.starts_with("diff --git ") {
                git = true;
            }
            if let Some(old) = l.strip_prefix("--- ")
                && let Some(new) = lines.peek().and_then(|s| s.strip_prefix("+++ "))
            {
                let new = new.split('\t').next().unwrap();
                let old = old.split('\t').next().unwrap();
                let (path, pre) = if new == "/dev/null" {
                    (old, "a/")
                } else {
                    (new, "b/")
                };
                p.path.get_or_insert(if git {
                    path.strip_prefix(pre).unwrap_or(path)
                } else {
                    path
                });
                lines.next();
                continue;
            }
            let Some(spec) = l
                .strip_prefix("@@ ")
                .and_then(|s| s.split_once(" @@"))
                .map(|v| v.0)
            else {
                continue;
            };
            let Some((a, b)) = spec.split_once(' ') else {
                continue;
            };
            let (Some((mut old, mut left)), Some((mut new, mut right))) =
                (range(a, '-'), range(b, '+'))
            else {
                continue;
            };
            let mut h = Hunk {
                header: l,
                lines: vec![],
            };
            while let Some(&raw) = lines.peek() {
                if raw.starts_with('\\') {
                    if let Some(line) = h.lines.last_mut() {
                        line.no_newline = true;
                    }
                    lines.next();
                    continue;
                }
                let text = raw.get(1..).unwrap_or("");
                let (o, n) = match raw.as_bytes().first() {
                    Some(b'+') if right > 0 => {
                        p.added += 1;
                        (None, Some(new))
                    }
                    Some(b'-') if left > 0 => {
                        p.removed += 1;
                        (Some(old), None)
                    }
                    Some(b' ') | None if left > 0 && right > 0 => (Some(old), Some(new)),
                    _ => break,
                };
                if o.is_some() {
                    old += 1;
                    left -= 1
                }
                if n.is_some() {
                    new += 1;
                    right -= 1
                }
                h.lines.push(Line {
                    old: o,
                    new: n,
                    text,
                    no_newline: false,
                });
                lines.next();
            }
            p.hunks.push(h);
        }
        p
    }
}
pub struct Piece {
    pub call: String,
    pub created: bool,
    pub patch: Option<String>,
    pub content: Option<String>,
    pub omitted: Option<String>,
}
pub struct ChangedFile {
    pub path: String,
    pub pieces: Vec<Piece>,
}
pub struct Review {
    page: Page,
    pub turn: Option<String>,
    pub latest_changed_turn: Option<String>,
    pub files: Vec<ChangedFile>,
}
fn normalized(path: &str, cwd: &str) -> String {
    if path == "~" || path.starts_with("~/") {
        return path.into();
    }
    let full = if !cwd.is_empty() && !path.starts_with('/') {
        format!("{}/{path}", cwd.trim_end_matches('/'))
    } else {
        path.into()
    };
    let mut parts = vec![];
    for p in full.split('/') {
        match p {
            "" | "." => (),
            ".." if parts.last().is_some_and(|s| *s != "..") => {
                parts.pop();
            }
            ".." if full.starts_with('/') => (),
            _ => parts.push(p),
        }
    }
    format!(
        "{}{}",
        if full.starts_with('/') { "/" } else { "./" },
        parts.join("/")
    )
}
impl Page {
    /// Run on a background executor; parse only locally observed history.
    pub fn review(&self, turn: Option<String>) -> Result<Review, Error> {
        let r = self.current()?;
        let k = self.editor()?;
        let result = self.review_rows(&r, k, turn);
        self.live()?;
        Ok(result)
    }
    fn review_rows(&self, r: &Rows, k: &EditorKey, turn: Option<String>) -> Review {
        let cwd = r.get("instance", &k.instance)["cwd"]
            .as_str()
            .unwrap_or_default();
        let mut files: Vec<ChangedFile> = vec![];
        let mut indices = HashMap::<String, usize>::new();
        let entries = r.group("entry");
        let results: HashMap<_, _> = entries
            .iter()
            .filter(|(_, v)| v["kind"] == "tool_result")
            .filter_map(|(_, v)| Some((v["toolCallId"].as_str()?, v)))
            .collect();
        let mut current = None;
        let mut latest = None;
        for (id, e) in entries {
            if e["kind"] == "user" {
                current = Some(e["entryId"].as_str().unwrap_or(id));
            }
            if e["kind"] != "assistant" || turn.is_some() && turn.as_deref() != current {
                continue;
            }
            for call in e["toolCalls"].as_array().into_iter().flatten() {
                let Some(cid) = call["id"].as_str() else {
                    continue;
                };
                let Some(result) = results.get(cid) else {
                    continue;
                };
                if result["isError"] == true
                    || !matches!(call["name"].as_str(), Some("write" | "edit"))
                {
                    continue;
                }
                latest = current.map(str::to_owned);
                let args = &call["args"];
                let details = &result["details"];
                let w = &details["pimate"];
                let write = if call["name"] == "write" && w["kind"] == "write_diff" {
                    w
                } else {
                    &Value::Null
                };
                let patch = (if call["name"] == "edit" {
                    details
                } else {
                    write
                })["patch"]
                    .as_str();
                let path = args["path"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| args["file_path"].as_str().filter(|s| !s.is_empty()))
                    .or_else(|| Patch::parse(patch?).path)
                    .unwrap_or("（不明なファイル）");
                let path = path.strip_prefix('@').unwrap_or(path);
                let i = *indices.entry(normalized(path, cwd)).or_insert_with(|| {
                    let i = files.len();
                    files.push(ChangedFile {
                        path: path.into(),
                        pieces: vec![],
                    });
                    i
                });
                let reason = if result["detailsOmitted"] == true {
                    Some("too_large")
                } else {
                    write["omitted"].as_str()
                };
                let content = (call["name"] == "write"
                    && patch.is_none()
                    && !matches!(reason, Some("too_large" | "binary")))
                .then(|| args["content"].as_str())
                .flatten();
                files[i].pieces.push(Piece {
                    call: cid.into(),
                    created: write["existed"] == false && reason != Some("concurrent"),
                    patch: patch.map(str::to_owned),
                    content: content.map(str::to_owned),
                    omitted: patch.is_none().then(|| {
                        reason
                            .unwrap_or(if content.is_some() {
                                "unknown_before"
                            } else {
                                "no_diff"
                            })
                            .into()
                    }),
                });
            }
        }
        Review {
            page: self.clone(),
            turn,
            files,
            latest_changed_turn: latest,
        }
    }
    pub fn append_reference(
        &self,
        app: &mut Desktop,
        text: &str,
        w: &mut gpui::Window,
        cx: &mut gpui::Context<Desktop>,
    ) -> Result<(), Error> {
        let k = self.editor()?;
        if !self.visible(app)
            || app.composing
            || (app.input.is_some()
                && !app.live_matches(
                    k,
                    app.drafts.get(k).map(String::as_str).unwrap_or_default(),
                    w,
                    cx,
                ))
        {
            return Err(Error::StaleTarget);
        }
        self.current()?;
        let draft = app.drafts.entry(k.clone()).or_default();
        if !draft.is_empty() && !draft.ends_with('\n') {
            draft.push('\n')
        }
        draft.push_str(text);
        app.input = None;
        Ok(())
    }
}
impl Review {
    pub fn ask_about(
        &self,
        app: &mut Desktop,
        w: &mut gpui::Window,
        cx: &mut gpui::Context<Desktop>,
    ) -> Result<(), Error> {
        let f = self.files.first().ok_or(Error::BadRequest)?;
        let scope = if self.turn.is_some() {
            "ターン"
        } else {
            "セッション"
        };
        let others = if self.files.len() > 1 {
            format!(" ほか {} ファイル", self.files.len() - 1)
        } else {
            String::new()
        };
        self.page.append_reference(
            app,
            &format!("この{scope}の変更（{}{others}）について：", f.path),
            w,
            cx,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn patch_counts_only_hunk_lines() {
        let p = Patch::parse(
            "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -2,2 +3,2 @@ text\n same\n-old\n+new\n\\ No newline at end of file\n",
        );
        assert_eq!(p.path, Some("f"));
        assert_eq!((p.added, p.removed), (1, 1));
        let lines = &p.hunks[0].lines;
        assert_eq!(lines[1].old, Some(3));
        assert_eq!(lines[2].new, Some(4));
        assert!(lines[2].no_newline);
    }
    #[test]
    fn review_keeps_call_order_and_unicode_identity() {
        let page = super::super::tests::dummy_page();
        let key = page.editor().unwrap();
        let items=[("u",json!({"kind":"user"})),("a",json!({"kind":"assistant","toolCalls":[{"id":"1","name":"write","args":{"path":"./é","content":"body"}},{"id":"2","name":"edit","args":{"path":"sub/../é"}},{"id":"3","name":"write","args":{"path":"e\u{301}"}}]})),("r1",json!({"kind":"tool_result","toolCallId":"1","details":{"pimate":{"kind":"write_diff","existed":false}}})),("r2",json!({"kind":"tool_result","toolCallId":"2","details":{"patch":"patch"}})),("r3",json!({"kind":"tool_result","toolCallId":"3","detailsOmitted":true}))].into_iter().enumerate().map(|(i,(id,v))|pimate_core::Item{collection:"entry".into(),id:id.into(),position:i as u64,json:v.to_string()}).collect();
        let r = Rows::snapshot(pimate_core::Snapshot { revision: 1, items });
        let v = page.review_rows(&r, key, Some("u".into()));
        assert_eq!(v.files.len(), 2);
        let p = &v.files[0].pieces;
        assert!(p[0].created);
        assert_eq!(p[0].content.as_deref(), Some("body"));
        assert_eq!(p[1].patch.as_deref(), Some("patch"));
        assert_eq!(v.files[1].pieces[0].omitted.as_deref(), Some("too_large"));
        assert!(
            page.review_rows(&r, key, Some("other".into()))
                .files
                .is_empty()
        );
    }
}
