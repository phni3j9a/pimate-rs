use super::sheet_chat::copy_attach;
use super::*;
impl Desktop {
    pub(super) fn chat_area(
        &mut self,
        mut main: Div,
        key: EditorKey,
        p: &Colors,
        dark: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if self.receive_editor_text(w, cx) {
            self.bind_editor(w, cx);
        }
        if self.active_sheet().is_some() {
            return main.child(self.sheet_panel(&key, p, w, cx));
        }
        let held = w.use_keyed_state("chat-page", cx, |_, _| None::<state::Page>);
        let page = if let Some(page) = held.read(cx).clone().filter(|p| p.visible(self)) {
            page
        } else {
            let Ok(page) = self.open_chat() else {
                return main;
            };
            held.update(cx, |h, _| *h = Some(page.clone()));
            page
        };
        let send_page = page.clone();
        let key_page = page.clone();
        let rows = &self.machines[&key.host].rows;
        let inspector = w.use_keyed_state(
            SharedString::from(format!(
                "inspector:{}:{:p}",
                self.epoch,
                Arc::as_ptr(&rows.view_life)
            )),
            cx,
            |_, _| super::chat_review::Inspector::default(),
        );
        if inspector.read(cx).agents {
            return main.child(self.agent_panel(&key, &page, &inspector, p, w, cx));
        }
        if inspector.read(cx).review_open {
            return main.child(self.review_panel(&key, &page, &inspector, p, w, cx));
        }
        let media = super::chat_media::Media {
            page: page.clone(),
            files: rows.get("view", "fileImages") == true,
            viewer: w.use_keyed_state(
                SharedString::from(format!("viewer:{:p}", Arc::as_ptr(&rows.view_life))),
                cx,
                |_, _| Default::default(),
            ),
        };
        if let Some(viewer) = media.panel(p, w, cx) {
            return main.child(viewer);
        }
        let movement = rows.pending_move(&key, &self.drafts).or_else(|| {
            rows.pending_move_with(&key, |k| !self.state.attachments.images(k).is_empty())
        });
        let session = rows.get("view", "session");
        let info = rows.get("instance", &key.instance);
        let running = rows.get("view", "status") == "running";
        let blocked = !rows.group("question").is_empty() || !rows.get("view", "pcPrompt").is_null();
        let status = if blocked {
            "入力待ち"
        } else if running {
            "作業中"
        } else {
            "待機"
        };
        let dest = session["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or(key.session.as_deref())
            .unwrap_or("新規セッション");
        main = main.child(
            div()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(p.border)
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::BOLD)
                                .text_size(rems(1.08))
                                .child(pi_title(info, &key.instance).to_owned()),
                        )
                        .child(
                            p.caption(format!(
                                "{} · 送信先: {dest}",
                                self.machines[&key.host].registration.name
                            ))
                            .truncate(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .items_center()
                        .child(self.review_button(&inspector, &page, cx))
                        .child(self.sheet_nav(&page, cx)),
                )
                .child(
                    p.caption(status)
                        .text_color(if blocked { p.waiting } else { p.accent }),
                ),
        );
        if let Some(rev) = movement {
            let shown = key.clone();
            main = main
                .child("別のセッションに移りました。前の下書きは保持しています。")
                .child(
                    Button::new("confirm-move")
                        .label("現在の送信先を確認")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.editor().as_ref() == Some(&shown) {
                                this.machines
                                    .get_mut(&shown.host)
                                    .unwrap()
                                    .rows
                                    .confirm_move(&shown, rev);
                                cx.notify();
                            }
                        })),
                );
        }
        main = main.child(self.conversation(&key, p, dark, media, inspector.clone(), w, cx));
        let pending_scroll = w
            .use_keyed_state(
                SharedString::from(format!("pending-scroll:{key:?}")),
                cx,
                |_, _| ScrollHandle::new(),
            )
            .read(cx)
            .clone();
        let mut chat = pending_panel("pending-drafts".into(), &pending_scroll);
        for (id, d) in rows.group("draft") {
            if d["instanceId"] == key.instance && d["sessionId"].as_str() == key.session.as_deref()
            {
                chat = chat.child(format!(
                    "{}: {}",
                    match d["status"].as_str() {
                        Some("queued") => "割り込み待ち",
                        Some("sending") => "送信中",
                        _ => "未確認（自動再送しません）",
                    },
                    d["params"]["text"].as_str().unwrap_or_default()
                ));
                if d["status"] == "unconfirmed"
                    && !self.composing
                    && !self.sending
                    && self.drafts.get(&key).is_none_or(String::is_empty)
                {
                    let pg = page.clone();
                    let id = id.clone();
                    chat = chat.child(
                        Button::new(SharedString::from(format!("restore-{id}")))
                            .label("入力欄へ戻す")
                            .on_click(cx.listener(move |this, _, w, cx| {
                                if pg.visible(this) {
                                    if let Err(e) = this.restore_draft(&pg, id.clone(), w, cx) {
                                        this.notice = format!("復元できません: {e}")
                                    };
                                    cx.notify()
                                }
                            })),
                    );
                }
            }
        }
        main = main.child(
            div()
                .relative()
                .flex_shrink_0()
                .child(chat)
                .vertical_scrollbar(&pending_scroll),
        );
        if let Some((id, q)) = rows.group("question").first() {
            main = main.child(self.question_card(&page, id, q, p, w, cx));
        }
        main = main.children(self.agent_summary(rows, &inspector));
        let pc = rows.get("view", "pcPrompt");
        if !pc.is_null() {
            main = main.child(
                div()
                    .mx_4()
                    .my_2()
                    .p_3()
                    .bg(p.surface)
                    .rounded_md()
                    .child("PC のターミナルで操作が必要です")
                    .child(
                        p.caption(
                            pc["title"]
                                .as_str()
                                .unwrap_or("Pi を起動した PC で続けてください。")
                                .to_owned(),
                        ),
                    )
                    .children(copy_attach(rows)),
            );
        }
        if let Some(r) = rows
            .receipts
            .shown(&key.instance, rows.get("view", "startedAt").as_u64())
        {
            main = main.child(
                p.caption(match r.outcome() {
                    state::receipts::Outcome::Done => "操作が完了しました",
                    state::receipts::Outcome::Failed => "操作に失敗しました",
                    state::receipts::Outcome::Cancelled => "操作を取り消しました",
                    _ => "操作の結果は不明です",
                })
                .px_4(),
            );
        }
        main = main.children(self.commands(&key, &page, p, w, cx));
        if let Some((_, input, _)) = &self.input {
            let movement = movement.is_some();
            let disabled = self.state.attachments.busy()
                || blocked
                || movement
                || self.sending
                || self.composing
                || (self.state.attachments.images(&key).is_empty()
                    && self.drafts.get(&key).is_none_or(|s| s.trim().is_empty()));
            let action = if running { "割り込む" } else { "送信" };
            main = main.child(
                div()
                    .border_t_1()
                    .border_color(p.border)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .capture_action(cx.listener(move |this, e: &Enter, w, cx| {
                                if e.secondary {
                                    if !disabled {
                                        this.submit_chat(&key_page, w, cx);
                                    }
                                    cx.stop_propagation();
                                }
                            }))
                            .child(Input::new(input).h(rems(4.))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(self.attachments(&key, &page, w, cx))
                            .child(p.caption(if blocked {
                                "入力待ちのため送信できません"
                            } else if cfg!(target_os = "macos") {
                                "↩ 改行　⌘↩ 送信"
                            } else {
                                "Enter で改行　Ctrl+Enter で送信"
                            }))
                            .child(
                                p.primary("send", cx)
                                    .label(action)
                                    .disabled(disabled)
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        this.submit_chat(&send_page, w, cx)
                                    })),
                            ),
                    ),
            );
        }
        main
    }
    fn submit_chat(&mut self, page: &state::Page, w: &mut Window, cx: &mut Context<Self>) {
        if !page.visible(self) {
            return;
        }
        let t = match self.prepare_submission(page, w, cx) {
            Ok(t) => t,
            Err(e) => {
                self.finish_send_notice(Err(e));
                cx.notify();
                return;
            }
        };
        self.sending = true;
        self.notice.clear();
        let page = page.clone();
        cx.spawn_in(w, async move |this, cx| {
            let result = t.submit().await;
            let _ = this.update_in(cx, |this, w, cx| {
                this.sending = false;
                match result {
                    Ok(ack) => {
                        let open = ack.finish(this, w, cx);
                        if let Some(name) = open {
                            use state::SheetPage::*;
                            this.show_sheet(
                                match name.as_str() {
                                    "model" => Model,
                                    "resume" => Resume,
                                    "fork" => Fork,
                                    _ => Tree,
                                },
                                cx,
                            );
                        } else if page.visible(this) {
                            this.finish_send_notice(Ok(String::new()));
                        }
                    }
                    Err(e) if page.visible(this) => this.finish_send_notice(Err(e)),
                    _ => {}
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
