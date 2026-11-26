use super::sheet_chat::copy_attach;
use super::*;
impl Desktop {
    pub(super) fn reconnect_hint(&self) -> Option<Div> {
        let (h, pi) = self.selected.as_ref()?;
        let m = self.machines.get(h)?;
        if !matches!(
            m.rows.get("host", "connection").as_str(),
            Some("offline" | "connecting" | "reconnecting")
        ) {
            return None;
        }
        let count = m
            .rows
            .group("draft")
            .iter()
            .filter(|(_, d)| d["instanceId"] == *pi && d["status"] == "unconfirmed")
            .count();
        Some(div().p_4().child(format!("{} に再接続中です",m.registration.name)).child(if count==0{"接続が戻るまで送信できません。下書きは保持しています。".to_owned()}else{format!("{count} 件の送信が未確認です。接続が戻ったら履歴を確認してください。自動では再送しません。") }))
    }

    pub(super) fn chat_area(
        &mut self,
        mut main: Div,
        key: EditorKey,
        question: Option<(String, Entity<super::chat_question::Form>)>,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // 760px 未満ではサイドバーを隠すので、ヘッダーの左に一覧を出すボタンを置く。
        let toggle = (Layout::of(w.viewport_size().width) == Layout::Narrow)
            .then(|| self.sidebar_toggle(p, cx));
        let nav = self.init_navigation(w, cx);
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
        let named = session["name"].as_str().filter(|s| !s.is_empty());
        let machine = &self.machines[&key.host].registration.name;
        let mut meta = format!("{} · {machine}", info["cwd"].as_str().unwrap_or_default());
        if named.is_none() {
            // 名前のないセッションは見出しがフォルダ名になるので、送り先を添える。
            meta += &format!(
                " · 送信先: {}",
                key.session.as_deref().unwrap_or("新規セッション")
            );
        }
        let github = github_available(rows, &key.host, &key.instance).then(|| {
            let nav = nav.clone();
            p.icon_button("github", "github", "GitHub — Issue / PR", cx)
                .debug_selector(|| "nav-github".into())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.navigate(&nav, Route::Github(pimate_protocol::GitHubQuery::List), cx)
                }))
        });
        // 760px 未満では状態を印だけにする（文字サイズ「大」の 480px でも右のボタンが切れないように）。
        let narrow = toggle.is_some();
        main = main.child(
            div()
                .debug_selector(|| "chat-header".into())
                .h(px(theme::HEADER))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .pl_5()
                .pr_3()
                .when(toggle.is_some(), |d| d.pl_3())
                .children(toggle)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(rems(0.94))
                                .debug_selector(|| "chat-title".into())
                                .child(pi_title(info, &key.instance).to_owned()),
                        )
                        .child(p.caption(meta).text_size(rems(0.69)).truncate()),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_1p5()
                        .text_size(rems(0.75))
                        .text_color(if blocked { p.waiting } else { p.secondary })
                        .child(if running && !blocked {
                            self.working_spinner(p.secondary)
                        } else {
                            p.dot(if blocked {
                                p.wait
                            } else {
                                p.secondary.opacity(0.55)
                            })
                        })
                        .when(!narrow, |d| d.child(status)),
                )
                .child(self.review_button(&inspector, &page, p, cx))
                .child(self.sheet_nav(&page, cx))
                .children(github),
        );
        if let Some(rev) = movement {
            let shown = key.clone();
            main = main.child(theme::column(
                div()
                    .px_4()
                    .pb_2()
                    .flex()
                    .items_center()
                    .gap_2()
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
                    ),
            ));
        }
        main = main.child(self.conversation(&key, p, media, inspector.clone(), w, cx));
        // 会話の下の部品も、会話と同じ列にそろえる。
        let outer = main;
        let mut main = div().flex().flex_col().min_w_0();
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
        if let Some((id, form)) = question {
            main = main.child(self.question_card(&page, &id, form, p, w, cx));
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
            let has_photos = !self.state.attachments.images(&key).is_empty();
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
                    .mx_4()
                    .mb_3()
                    .p_3()
                    .rounded(px(theme::R_COMPOSER))
                    .bg(p.surface)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(has_photos, |b| {
                        b.child(self.attachments(&key, &page, w, cx))
                    })
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
                            .child(Input::new(input).appearance(false).h(rems(4.))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .when(!has_photos, |b| {
                                b.child(self.attachments(&key, &page, w, cx))
                            })
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
        outer.child(theme::column(main).flex_shrink_0())
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
