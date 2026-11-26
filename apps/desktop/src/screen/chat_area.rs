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

        if self.receive_editor_text(w, cx) {
            self.bind_editor(w, cx);
        }
        let held = w.use_keyed_state("chat-page", cx, |_, _| None::<state::Page>);
        let background = self
            .navigation
            .as_ref()
            .is_some_and(|n| matches!(n.read(cx).route, Route::New(_)));
        let page = if let Some(page) = held.read(cx).clone().filter(|p| p.visible(self)) {
            page
        } else {
            let Some(page) = (if background || self.active_sheet().is_some() {
                self.background_chat()
            } else {
                self.open_chat().ok()
            }) else {
                return main;
            };
            held.update(cx, |h, _| *h = Some(page.clone()));
            page
        };
        let send_page = page.clone();
        let stop_page = page.clone();
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
        if !background {
            self.chat_keys = Some((page.clone(), inspector.clone()));
        }
        if let Some((host, pi, start, shutdown)) = self.row_action.take()
            && key.host == host
            && key.instance == pi
            && rows.get("view", "startedAt").as_u64() == Some(start)
        {
            if shutdown {
                self.confirm_shutdown(&page, w, cx)
            } else {
                inspector.update(cx, |s, cx| s.load(page.clone(), None, true, cx));
            }
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
            self.media_keys = Some(media.viewer.clone());
            return main.child(viewer);
        }
        let movement = rows.pending_move(&key, &self.drafts).or_else(|| {
            rows.pending_move_with(&key, |k| !self.state.attachments.images(k).is_empty())
        });
        let session = rows.get("view", "session");
        let info = rows.get("instance", &key.instance);
        let running = rows.get("view", "status") == "running";
        let can_stop = rows.can_stop();
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
            p.icon_button("github", "github", "GitHub — Issue / PR", cx)
                .debug_selector(|| "nav-github".into())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_github(pimate_protocol::GitHubQuery::List, cx)
                }))
        });
        // 760px 未満では状態の文字を置く幅がない（文字サイズ「大」の 480px で右のボタンが切れる）。
        // 色だけに頼らないよう、印の形を変え（入力待ち＝ベル・作業中＝輪・待機＝丸）、状態名を
        // ツールチップに出す。
        let narrow = toggle.is_some();
        let mark = if running && !blocked {
            self.working_spinner(p.secondary).into_any_element()
        } else if blocked && narrow {
            icon("bell", 13., p.waiting).into_any_element()
        } else {
            p.dot(if blocked {
                p.wait
            } else {
                p.secondary.opacity(0.55)
            })
            .into_any_element()
        };
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
                .when(narrow, |d| d.pl_3().gap_1())
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
                        // 状態ごとの id: 開いたツールチップは作り直されないので、状態が変わったら閉じる。
                        .id(status)
                        .debug_selector(|| "chat-status".into())
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .gap_1p5()
                        .text_size(rems(0.75))
                        .text_color(if blocked { p.waiting } else { p.secondary })
                        .child(mark)
                        .map(|d| {
                            if narrow {
                                d.size(px(20.)).tooltip(move |w, cx| {
                                    gpui_component::tooltip::Tooltip::new(status).build(w, cx)
                                })
                            } else {
                                d.child(status)
                            }
                        }),
                )
                .child(self.review_button(&inspector, &page, p, cx))
                .child(self.session_menu(&page, rows, cx))
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
                        Some("queued") => "送信待ち（受理済み・個別取消不可）",
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
                            .debug_selector(|| "restore-draft".into())
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
        let (suggestions, candidates) = if background {
            (None, None)
        } else {
            self.commands(&key, &page, p, w, cx)
        };
        self.slash_visible = suggestions.is_some();
        main = main.children(suggestions);
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
            let now = chrono::Utc::now().timestamp_millis() as u64;
            let allowances = self.machines[&key.host].conn.allowances(now);
            let gauges = std::iter::once((
                "コンテキスト",
                rows.get("view", "context")["percent"].as_f64(),
            ))
            .chain(allowances.iter().map(|a| {
                (
                    if a.source == "claude" {
                        "Claude"
                    } else {
                        "Codex"
                    },
                    a.gauge,
                )
            }))
            .collect::<Vec<_>>();
            let usage_hint = gauges
                .iter()
                .map(|(label, v)| {
                    format!(
                        "{label}: {}",
                        v.map_or_else(|| "未確認".into(), |n| format!("{n:.0}%"))
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ");
            let model = rows.get("view", "model")["id"]
                .as_str()
                .unwrap_or("モデル")
                .to_owned();
            let pg = page.clone();
            let usage_page = page.clone();
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
                        super::context::editor(
                            div().debug_selector(|| "composer-input".into()),
                            input.clone(),
                            false,
                        )
                        .key_context("Composer")
                        .when_some(candidates, |div, c| c.bind(div, cx))
                        .on_action(cx.listener(|this, _: &keys::Newline, w, cx| {
                            if !this.composing
                                && let Some((_, input, _)) = &this.input
                            {
                                input.update(cx, |s, cx| s.replace("\n", w, cx));
                            }
                        }))
                        .capture_action(cx.listener(move |this, e: &Enter, w, cx| {
                            if e.secondary == this.ctrl_enter {
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
                            .gap_1()
                            .when(!has_photos, |b| {
                                b.child(self.attachments(&key, &page, w, cx))
                            })
                            .child(
                                p.text_button("composer-model", p.secondary, cx)
                                    .max_w(rems(10.))
                                    .child(div().truncate().child(model))
                                    .on_click(cx.listener(move |app, e: &ClickEvent, w, cx| {
                                        if pg.visible(app)
                                            && (e.is_keyboard() || !app.pointer_down_on_overlay)
                                            && !app.overlay_open(w, cx)
                                        {
                                            app.show_sheet(state::SheetPage::Model, cx)
                                        }
                                    })),
                            )
                            .child(
                                p.text_button("composer-usage", p.secondary, cx)
                                    .tooltip(usage_hint)
                                    .children(gauges.into_iter().map(|(_, v)| {
                                        let used = v.unwrap_or_default().clamp(0., 100.) as f32;
                                        let data = [(used, p.accent), (100. - used, p.track)];
                                        div().size(px(15.)).child(
                                            gpui_component::chart::PieChart::new(data)
                                                .value(|d| d.0)
                                                .color(|d| d.1)
                                                .inner_radius(4.5)
                                                .outer_radius(7.),
                                        )
                                    }))
                                    .on_click(cx.listener(move |app, e: &ClickEvent, w, cx| {
                                        if usage_page.visible(app)
                                            && (e.is_keyboard() || !app.pointer_down_on_overlay)
                                            && !app.overlay_open(w, cx)
                                        {
                                            app.show_sheet(state::SheetPage::Usage, cx)
                                        }
                                    })),
                            )
                            .child(div().flex_1())
                            .when(can_stop, |row| {
                                row.child(
                                    p.text_button("abort", p.fail_ink, cx)
                                        .tab_stop(false)
                                        .label("停止")
                                        .debug_selector(|| "abort-turn".into())
                                        .disabled(self.stopping)
                                        .on_click(cx.listener(move |this, e, w, cx| {
                                            if this.background_click(e) {
                                                return;
                                            }
                                            this.stop_chat(&stop_page, w, cx)
                                        })),
                                )
                            })
                            .child(
                                p.primary("send", cx)
                                    .tab_stop(false)
                                    .debug_selector(|| "send-message".into())
                                    .label("送信")
                                    .disabled(disabled)
                                    .on_click(cx.listener(move |this, e, w, cx| {
                                        if this.background_click(e) {
                                            return;
                                        }
                                        this.submit_chat(&send_page, w, cx)
                                    })),
                            ),
                    ),
            );
        }
        let chat = outer.child(theme::column(main).flex_shrink_0());
        let chat = self.with_inspector(chat, &page, &inspector, p, w, cx);
        if self.active_sheet().is_some_and(|s| s.kind.is_popover()) {
            chat.child(self.sheet_overlay(&key, p, w, cx))
        } else {
            chat
        }
    }
    pub(super) fn sheet_overlay(
        &self,
        key: &EditorKey,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let kind = self.active_sheet().unwrap().kind;
        let pop = kind.is_popover();
        let focus = self.modal_focus.clone();
        if !focus.contains_focused(w, cx)
            && !self.sidebar_open
            && !self.palette_open
            && !self.dialog_focus.borrow().is_some()
            && self.context_menu.is_none()
        {
            w.focus(&focus);
        }
        let scrim = div()
            .absolute()
            .inset_0()
            .occlude()
            .when(!pop, |d| d.bg(p.scrim))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|app, _, w, cx| {
                    app.state.sheet = None;
                    app.restore_input(w, cx);
                    cx.notify()
                }),
            );
        let panel = self.sheet_panel(key, p, w, cx);
        div().absolute().inset_0().child(scrim).child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .when(pop, |d| d.items_end().justify_start().p_3().pb(rems(3.2)))
                .child(
                    div()
                        .id("chat-overlay")
                        .occlude()
                        .track_focus(&focus)
                        .rounded(px(theme::R_CARD))
                        .bg(p.surface)
                        .shadow_lg()
                        .max_w_full()
                        .w(px(if pop { 390. } else { 540. }))
                        .h(relative(0.72))
                        .child(panel),
                ),
        )
    }
    fn submit_chat(&mut self, page: &state::Page, w: &mut Window, cx: &mut Context<Self>) {
        if self.overlay_open(w, cx) || !page.visible(self) {
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
        self.run_submission(page, t, true, w, cx);
    }
    pub(super) fn run_submission(
        &mut self,
        page: &state::Page,
        t: state::Submission,
        from_input: bool,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sending = true;
        self.notice.clear();
        let page = page.clone();
        cx.spawn_in(w, async move |this, cx| {
            let result = t.submit().await;
            let _ = this.update_in(cx, |this, w, cx| {
                this.sending = false;
                match result {
                    Ok(ack) => {
                        let open = if from_input {
                            ack.finish(this, w, cx)
                        } else if page.visible(this) {
                            ack.opened_sheet()
                        } else {
                            None
                        };
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
