use super::*;
use pimate_protocol::{ModelRef, SetDefaultModelParams};
use state::{Page, SheetAction as A, SheetPage as S};

pub(super) fn copy_attach(rows: &Rows) -> Option<Button> {
    let cmd = state::attach_command(rows.get("view", "abducoSession").as_str()?)?;
    Some(
        Button::new("copy-attach")
            .label(format!("{cmd}　· コピー"))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(cmd.clone()))
            }),
    )
}
pub(super) fn preview(s: &str) -> String {
    s.lines()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("本文なし")
        .chars()
        .take(80)
        .collect()
}
fn label(kind: S) -> &'static str {
    match kind {
        S::Model => "モデル",
        S::Usage => "使用状況",
        S::Session => "セッション",
        S::Resume => "過去のセッション",
        S::Fork => "分岐元を選択",
        S::Tree => "セッションツリー",
        S::Commands => "コマンド",
    }
}
fn load(kind: S) -> Option<A> {
    match kind {
        S::Model => Some(A::Models),
        S::Resume => Some(A::Sessions),
        S::Tree => Some(A::Tree),
        _ => None,
    }
}
// Favorites replaces the PC's whole list. Never build that write from cached display rows.
fn scoped(reply: Option<&Value>) -> Option<Vec<ModelRef>> {
    serde_json::from_value(reply?.get("scoped")?.clone()).ok()
}
fn toggle_favorite(reply: Option<&Value>, model: &ModelRef, was_favorite: bool) -> Option<A> {
    let mut current = scoped(reply)?;
    if current.contains(model) != was_favorite {
        return None;
    }
    if was_favorite {
        current.retain(|m| m != model)
    } else {
        current.push(model.clone())
    }
    Some(A::Favorites(current))
}
fn nav(kind: S, page: &Page, cx: &Context<Desktop>) -> Button {
    let pg = page.clone();
    Button::new(label(kind))
        .small()
        .label(label(kind))
        .on_click(cx.listener(move |this, _, _, cx| {
            if pg.visible(this) {
                this.show_sheet(kind, cx)
            }
        }))
}
fn section(title: &str, p: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .bg(p.surface)
        .rounded_lg()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
}
pub(super) fn action(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    disabled: bool,
    page: &Page,
    make: impl Fn() -> A + 'static,
    cx: &Context<Desktop>,
) -> Button {
    let pg = page.clone();
    Button::new(id.into())
        .label(title)
        .disabled(disabled)
        .on_click(cx.listener(move |this, _, _, cx| {
            if pg.visible(this) {
                this.sheet_action(make(), cx)
            }
        }))
}
impl Desktop {
    pub(super) fn sheet_nav(&self, page: &Page, cx: &Context<Self>) -> Div {
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children([S::Model, S::Usage, S::Session].map(|k| nav(k, page, cx)))
    }
    pub(super) fn show_sheet(&mut self, kind: S, cx: &mut Context<Self>) {
        if self.open_sheet(kind).is_ok() {
            if let Some(a) = load(kind) {
                self.sheet_action(a, cx)
            }
            cx.notify()
        }
    }
    pub(super) fn sheet_action(&mut self, action: A, cx: &mut Context<Self>) {
        let Some(sheet) = self.state.sheet.as_mut() else {
            return;
        };
        let reload = match action {
            A::Label(_) => Some(A::Tree),
            A::Select(_) | A::Thinking(_) | A::Default(_) => Some(A::Models),
            _ => None,
        };
        let page = sheet.page.clone();
        let Ok(req) = sheet.begin(action) else { return };
        cx.spawn(async move |this, cx| {
            let reply = req.run().await;
            let _ = this.update(cx, |this, cx| {
                reply.finish(this);
                if let Some(refresh) = reload
                    && page.visible(this)
                    && this.active_sheet().is_some_and(|s| s.error.is_none())
                {
                    this.sheet_action(refresh, cx)
                }
                cx.notify()
            });
        })
        .detach();
        cx.notify()
    }
    pub(super) fn sheet_panel(
        &self,
        key: &EditorKey,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let sheet = self.active_sheet().unwrap();
        let pg = &sheet.page;
        let kind = sheet.kind;
        let rows = &self.machines[&key.host].rows;
        let view = |k| rows.get("view", k);
        let busy = sheet.loading() || !rows.group("busy").is_empty();
        let idle = view("status") == "idle" && view("agentActive") != true;
        let mut body = div()
            .id("sheet-scroll")
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_3();
        if sheet.loading() {
            body = body.child(p.caption("読み込み中…"))
        } else if let Some(err) = sheet.error {
            body = body.child(p.caption(format!("応答を確認できません: {err}")));
            if load(kind).is_some() {
                body = body.child(action(
                    "retry",
                    "再取得",
                    false,
                    pg,
                    move || load(kind).unwrap(),
                    cx,
                ))
            }
        } else if sheet.reply.is_some() {
            body = body.child(p.caption("Pi が要求を受け付けました"))
        }
        if let Some(r) = rows
            .receipts
            .shown(&key.instance, view("startedAt").as_u64())
        {
            body = body.child(p.caption(format!(
                "直近の操作: {}",
                match r.outcome() {
                    state::receipts::Outcome::Done => "完了",
                    state::receipts::Outcome::Failed => "失敗",
                    state::receipts::Outcome::Cancelled => "取消",
                    _ => "結果不明",
                }
            )))
        }
        match kind {
            S::Session => {
                body = body.child(
                    section("会話", p)
                        .child(p.caption(view("cwd").as_str().unwrap_or("").to_owned()))
                        .child(action(
                            "new",
                            "新しいセッション",
                            busy || !idle,
                            pg,
                            || A::New,
                            cx,
                        ))
                        .child(nav(S::Resume, pg, cx))
                        .child(nav(S::Fork, pg, cx))
                        .when(view("sessionTree") == true, |b| {
                            b.child(nav(S::Tree, pg, cx))
                        })
                        .child(action(
                            "compact",
                            "コンテキストを圧縮",
                            busy,
                            pg,
                            || A::Compact,
                            cx,
                        )),
                );
                let page = pg.clone();
                let title = pi_title(rows.get("instance", &key.instance), &key.instance).to_owned();
                let saved = view("session")["file"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty());
                body=body.child(section("この Pi",p).children(copy_attach(rows)).child(Button::new("shutdown").danger().label("この Pi を終了…").disabled(busy).on_click(cx.listener(move|this,_,w,cx|{if !page.visible(this){return}let Ok(stop)=page.shutdown_confirmation() else{return}; let pg=page.clone();let detail=if saved{"処理が止まります。保存済みの履歴は一覧から開き直せます。"}else{"処理が止まります。保存先が不明なため履歴が失われる可能性があります。"};let confirm=w.prompt(PromptLevel::Critical,&format!("{title} を終了しますか？"),Some(detail),&[PromptButton::cancel("キャンセル"),PromptButton::new("終了する")],cx);cx.spawn(async move|this,cx|{if confirm.await==Ok(1){let r=stop.confirm().await;let _=this.update(cx,|this,cx|if pg.visible(this){this.finish_send_notice(r.map(|_|String::new()));cx.notify()});}}).detach();}))))
            }
            S::Tree => body = body.child(self.tree(key, pg, p, w, cx)),
            S::Resume | S::Fork => {
                let list = match kind {
                    S::Resume => rows.group("session"),
                    S::Fork => rows.group("entry"),
                    _ => unreachable!(),
                };
                let mut card = section(label(kind), p);
                let mut n = 0;
                for (id, v) in list {
                    let (title, a, disabled): (String, Box<dyn Fn() -> A>, bool) = match kind {
                        S::Resume => {
                            let Some(path) = v["path"].as_str().map(str::to_owned) else {
                                continue;
                            };
                            (
                                preview(
                                    v["name"]
                                        .as_str()
                                        .or(v["firstMessage"].as_str())
                                        .unwrap_or("空のセッション"),
                                ),
                                Box::new(move || A::Resume(path.clone())),
                                v["current"] == true,
                            )
                        }
                        S::Fork if v["kind"] == "user" => {
                            let id = id.clone();
                            (
                                preview(v["text"].as_str().unwrap_or("画像のみ")),
                                Box::new(move || A::Fork(id.clone())),
                                false,
                            )
                        }
                        _ => continue,
                    };
                    n += 1;
                    card = card.child(action(
                        id.clone(),
                        title,
                        busy || !idle || disabled || sheet.error.is_some(),
                        pg,
                        a,
                        cx,
                    ));
                }
                if n == 0 && !busy && sheet.error.is_none() {
                    card = card.child(p.caption("対象のメッセージ・セッションはありません"))
                }
                body = body.child(card)
            }
            S::Model => {
                let models = rows.group("model");
                let current = serde_json::from_value::<ModelRef>(view("model").clone()).ok();
                let favorites = scoped(sheet.reply.as_ref());
                let mut list = section("この Pi のモデル", p)
                    .child(p.caption("選択とお気に入りは PC の Pi にも反映されます。"));
                let only = w.use_keyed_state("favorite-filter", cx, |_, _| false);
                let filter = *only.read(cx);
                let search = w.use_keyed_state("model-search", cx, |w, cx| {
                    InputState::new(w, cx).placeholder("モデルを検索")
                });
                let query = search.read(cx).value().to_lowercase();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("favorites-filter")
                                .small()
                                .label(if filter {
                                    "★ お気に入りのみ"
                                } else {
                                    "すべてのモデル"
                                })
                                .on_click(move |_, _, cx| {
                                    only.update(cx, |s, cx| {
                                        *s = !*s;
                                        cx.notify()
                                    })
                                }),
                        )
                        .child(div().flex_1().child(Input::new(&search))),
                );
                let mut matched = false;
                let mut thinking = div().flex().flex_wrap().gap_1();
                let mut active = false;
                let mut level = None;
                for (id, v) in models {
                    let Ok(m) = serde_json::from_value::<ModelRef>(v.clone()) else {
                        continue;
                    };
                    let selected = current.as_ref() == Some(&m);
                    let fav = favorites
                        .as_ref()
                        .map_or(v["favorite"] == true, |f| f.contains(&m));
                    let favorite_model = m.clone();
                    let page = pg.clone();
                    let name = v["name"].as_str().unwrap_or(&m.id);
                    if (!filter || fav)
                        && [name, &m.id, &m.provider]
                            .iter()
                            .any(|s| s.to_lowercase().contains(&query))
                    {
                        matched = true;
                        list = list.child(
                            div()
                                .flex()
                                .gap_2()
                                .items_center()
                                .child(action(
                                    id.clone(),
                                    format!(
                                        "{}{} · {}",
                                        if selected { "✓ " } else { "" },
                                        name,
                                        m.provider
                                    ),
                                    busy || selected,
                                    pg,
                                    move || A::Select(m.clone()),
                                    cx,
                                ))
                                .child(
                                    Button::new(SharedString::from(format!("fav:{id}")))
                                        .label(if fav { "★ 解除" } else { "☆ 追加" })
                                        .disabled(busy || favorites.is_none())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if page.visible(this)
                                                && let Some(a) = toggle_favorite(
                                                    this.active_sheet()
                                                        .and_then(|s| s.reply.as_ref()),
                                                    &favorite_model,
                                                    fav,
                                                )
                                            {
                                                this.sheet_action(a, cx)
                                            }
                                        })),
                                ),
                        );
                    }
                    if selected {
                        active = true;
                        for val in v["thinkingLevels"].as_array().into_iter().flatten() {
                            if let Ok(l) = serde_json::from_value(val.clone()) {
                                let sel = view("thinking") == val;
                                if sel {
                                    level = Some(l)
                                }
                                thinking = thinking.child(action(
                                    format!("level:{l:?}"),
                                    format!(
                                        "{}{}",
                                        if sel { "✓ " } else { "" },
                                        val.as_str().unwrap_or("")
                                    ),
                                    busy || sel,
                                    pg,
                                    move || A::Thinking(l),
                                    cx,
                                ))
                            }
                        }
                    }
                }
                if !matched && !busy && sheet.error.is_none() {
                    list = list.child(p.caption("該当するモデルはありません"))
                }
                body = body
                    .child(list)
                    .child(section("Thinking", p).child(thinking));
                if let Some(m) = current.filter(|_| active && !view("defaultModel").is_null()) {
                    body = body.child(
                        section("新しい Pi の既定", p)
                            .child(
                                p.caption(
                                    view("defaultModel")["model"]["id"]
                                        .as_str()
                                        .unwrap_or("未設定")
                                        .to_owned(),
                                ),
                            )
                            .child(action(
                                "save-default",
                                "現在のモデルと Thinking を既定にする",
                                busy,
                                pg,
                                move || {
                                    A::Default(SetDefaultModelParams {
                                        model: m.clone(),
                                        thinking: level,
                                    })
                                },
                                cx,
                            )),
                    )
                }
            }
            S::Usage => {
                let ctx = view("context");
                let val = |k: &str| {
                    ctx[k]
                        .as_u64()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "不明".into())
                };
                let mut card = section("コンテキスト", p)
                    .child(format!(
                        "{} / {} トークン",
                        val("tokens"),
                        val("contextWindow")
                    ))
                    .child(
                        p.caption(
                            ctx["percent"]
                                .as_f64()
                                .map(|v| format!("{v:.0}% 使用"))
                                .unwrap_or_else(|| "使用率は不明".into()),
                        ),
                    )
                    .child(action(
                        "compact",
                        "コンテキストを圧縮",
                        busy,
                        pg,
                        || A::Compact,
                        cx,
                    ));
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                match pg.usage(now) {
                    Ok((stats, limits)) => {
                        card = card.child(p.caption(format!(
                            "このセッション · {} メッセージ · 記録済み ${:.2}",
                            stats.messages, stats.cost
                        )));
                        body = body.child(card);
                        for l in limits {
                            let mut c = section(
                                if l.source == "claude" {
                                    "Claude"
                                } else {
                                    "Codex"
                                },
                                p,
                            );
                            if l.windows.is_empty() {
                                c = c.child(p.caption("この PC の利用枠は未確認です"))
                            }
                            for window in l.windows {
                                c = c.child(format!(
                                    "{}時間　{}",
                                    window.minutes as f64 / 60.,
                                    window
                                        .used_percent
                                        .map(|v| format!("{v:.0}% 使用"))
                                        .unwrap_or_else(|| "不明 · 次の応答で更新".into())
                                ))
                            }
                            body = body.child(c)
                        }
                    }
                    Err(e) => body = body.child(card).child(format!("取得できません: {e}")),
                }
            }
            S::Commands => {}
        }
        let close = pg.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(p.border)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .child(p.caption("会話の操作"))
                            .child(div().font_weight(FontWeight::BOLD).child(label(kind))),
                    )
                    .child(
                        Button::new("close-sheet")
                            .label("会話に戻る")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if close.visible(this) {
                                    this.state.close_page();
                                    cx.notify()
                                }
                            })),
                    ),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{A, ModelRef, toggle_favorite};
    #[test]
    fn favorite_requires_a_current_scoped_reply_and_preserves_unlisted_models() {
        use serde_json::json;
        let m = ModelRef {
            provider: "p".into(),
            id: "new".into(),
        };
        for reply in [None, Some(&json!({"model":m})), Some(&json!({"models":[]}))] {
            assert!(toggle_favorite(reply, &m, false).is_none())
        }
        let reply = json!({"scoped":[{"provider":"p","id":"old"}]});
        assert!(toggle_favorite(Some(&reply), &m, true).is_none());
        let Some(A::Favorites(next)) = toggle_favorite(Some(&reply), &m, false) else {
            panic!()
        };
        assert_eq!(next[0].id, "old");
        assert_eq!(next[1], m);
    }
}
