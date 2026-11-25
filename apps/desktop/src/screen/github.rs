use super::*;
use pimate_protocol::GitHubQuery;
use settings::{group, line};
#[derive(Default)]
pub(super) struct Github {
    loaded: bool,
    data: Option<Value>,
    docs: markdown::Messages,
}
fn check_note(v: &Value) -> Option<&'static str> {
    if v["checksTruncated"] == true {
        Some("CI チェックの一部を省略しています。")
    } else {
        match v["checks"].as_array() {
            None => Some("CI チェックの内訳は未提供です"),
            Some(a) if a.is_empty() => Some("チェックはありません"),
            _ => None,
        }
    }
}
impl Desktop {
    pub(super) fn github_view(
        &mut self,
        n: &Navigation,
        p: &Colors,
        q: &GitHubQuery,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(page) = n.read(cx).page.clone().filter(|p| p.visible(self)) else {
            return div().child("Pi の接続先が変わりました。会話を選び直してください。");
        };
        let mut root = div().flex().flex_col().gap_3();
        if !n.read(cx).github.loaded {
            n.update(cx, |s, _| s.github.loaded = true);
            let query = q.clone();
            self.page_task(n,w,cx,async move|page|{
                let mut reply=page.github(query.clone()).await?;
                let data=reply["data"].take();let item=&data["item"];
                let valid=match query {
                    GitHubQuery::List=>data["issues"].is_array() && data["prs"].is_array(),
                    GitHubQuery::Issue{number}=>item["kind"]=="issue"&&item["number"]==number,
                    GitHubQuery::Pr{number}=>item["kind"]=="pr"&&item["number"]==number,
                };
                valid.then_some(data).ok_or(Error::InvalidMessage)
            },|_,nav,result,_,cx|{nav.update(cx,|s,_|match result{
                Ok(data)=>s.github.data=Some(data),
                Err(_)=>s.note="取得できませんでした。PC の gh のログイン状態・リポジトリ・閲覧権限を確認してください。".into(),
            });});
        }
        let Some(data) = n.read(cx).github.data.as_ref() else {
            return if n.read(cx).note.is_empty() {
                root.child(p.caption("PC から読み込み中…"))
            } else {
                root.child(self.nav_button(
                    n,
                    "github-retry",
                    "再読み込み",
                    Route::Github(q.clone()),
                    cx,
                ))
            };
        };
        if q == &GitHubQuery::List {
            root = root.child(
                div().font_weight(FontWeight::SEMIBOLD).child(
                    data["repo"]["name"]
                        .as_str()
                        .unwrap_or("リポジトリ")
                        .to_string(),
                ),
            );
            for (key, title) in [("issues", "Issue"), ("prs", "Pull request")] {
                let mut g = group(title, p);
                let rows = data[key].as_array().unwrap();
                if rows.is_empty() {
                    g = g.child(p.caption("該当する項目はありません"));
                }
                for row in rows {
                    if let Ok(item) = page.github_item(row) {
                        let number = item.number;
                        let query = if key == "issues" {
                            GitHubQuery::Issue { number }
                        } else {
                            GitHubQuery::Pr { number }
                        };
                        let nav = n.clone();
                        let shown = page.clone();
                        let label = format!(
                            "#{number}　{}　·　{}",
                            item.title,
                            row["state"].as_str().unwrap_or("未確認")
                        );
                        let open = Button::new(SharedString::from(format!("{key}:{number}")))
                            .label(label)
                            .justify_start()
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if shown.visible(app) {
                                    app.navigate(&nav, Route::Github(query.clone()), cx);
                                }
                            }));
                        g = g.child(open);
                    }
                }
                root = root.child(g);
            }
            return root;
        }
        let v = &data["item"];
        let Ok(item) = page.github_item(v) else {
            return root.child("項目を確認できません。");
        };
        let mut heading = group(
            format!(
                "{} #{}　·　{}",
                if v["kind"] == "issue" { "Issue" } else { "PR" },
                item.number,
                v["state"].as_str().unwrap_or("未確認")
            ),
            p,
        )
        .child(
            div()
                .text_size(rems(1.2))
                .font_weight(FontWeight::BOLD)
                .child(item.title.clone()),
        )
        .child(p.caption(v["author"].as_str().unwrap_or("").to_string()));
        let mut body_parts = vec![(
            "本文".to_string(),
            v["body"].as_str().unwrap_or("").to_string(),
            v["bodyTruncated"] == true,
        )];
        for c in v["comments"].as_array().into_iter().flatten() {
            body_parts.push((
                format!("コメント　·　{}", c["author"].as_str().unwrap_or("")),
                c["body"].as_str().unwrap_or("").to_string(),
                c["bodyTruncated"] == true,
            ));
        }
        let mut controls = line();
        if let Some(url) = &item.url {
            let u = url.clone();
            controls = controls.child(
                Button::new("github-open")
                    .label("GitHub で開く ↗")
                    .on_click(move |_, _, cx| cx.open_url(u.as_str())),
            );
        }
        let nav = n.clone();
        controls = controls.child(Button::new("ask").label("Pi の下書きに追加").on_click(
            cx.listener(move |app, _, w, cx| {
                if item.ask_about(app, w, cx).is_ok() {
                    app.navigate(&nav, Route::Home, cx);
                } else {
                    nav.update(cx, |s, _| {
                        s.note = "送信先が変わりました。下書きは変更していません。".into()
                    });
                    cx.notify();
                }
            }),
        ));
        if v["isDraft"] == true {
            heading = heading.child(p.caption("ドラフト PR"));
        }
        if let Some(decision) = v["reviewDecision"].as_str() {
            heading = heading.child(p.caption(format!("レビュー　{decision}")));
        }
        if let (Some(head), Some(base)) = (v["headRefName"].as_str(), v["baseRefName"].as_str()) {
            heading = heading.child(p.caption(format!("{head} → {base}")));
        }
        if let (Some(files), Some(added), Some(deleted)) = (
            v["changedFiles"].as_u64(),
            v["additions"].as_u64(),
            v["deletions"].as_u64(),
        ) {
            heading = heading
                .child(p.caption(format!("変更 {files} ファイル　·　+{added} / −{deleted}")));
        }
        heading = heading.child(controls);
        root = root.child(heading);
        if v["kind"] == "pr" {
            let mut g = group("CI チェック", p);
            for c in v["checks"].as_array().into_iter().flatten() {
                g = g.child(format!(
                    "{}　·　{}",
                    c["name"].as_str().unwrap_or(""),
                    c["state"].as_str().unwrap_or("未確認")
                ));
            }
            if let Some(note) = check_note(v) {
                g = g.child(p.caption(note));
            }
            root = root.child(g);
        }
        let latest = v["commentsTruncated"] == true;
        let empty = v["comments"].as_array().is_some_and(Vec::is_empty);
        n.update(cx, |s, cx| {
            s.github.docs.begin_frame();
            for (i, (title, source, truncated)) in body_parts.into_iter().enumerate() {
                let mut g = group(title, p);
                if source.is_empty() {
                    g = g.child(p.caption("本文はありません"));
                } else {
                    g = g.child(s.github.docs.view(
                        format!("github-{i}"),
                        &source,
                        false,
                        None,
                        w,
                        cx,
                    ));
                }
                if truncated {
                    g = g.child(p.caption("一部を省略しています"));
                }
                root = root.child(g);
            }
            if latest {
                root = root.child(p.caption("最新のコメントのみ表示しています"));
            } else if empty {
                root = root.child(p.caption("コメントはありません"));
            }
            root
        })
    }
}
#[cfg(test)]
mod tests {
    use super::check_note;
    #[test]
    fn missing_checks_are_not_empty() {
        assert_ne!(
            check_note(&serde_json::json!({})),
            check_note(&serde_json::json!({"checks":[]}))
        );
        assert_eq!(
            check_note(&serde_json::json!({"checks":[],"checksTruncated":true})),
            Some("CI チェックの一部を省略しています。")
        );
    }
}
