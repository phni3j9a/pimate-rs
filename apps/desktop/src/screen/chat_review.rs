use super::*;
use state::{Page, review::Review};
#[derive(Default)]
pub(crate) struct Inspector {
    pub review: Option<Arc<Review>>,
    pub(super) tab: Option<Tab>,
    pub(super) github: Option<Navigation>,
    read: Option<Task<()>>,
    error: Option<Error>,
    pub child: Option<state::Child>,
    pub run: Option<String>,
}
impl Inspector {
    pub(super) fn load(
        &mut self,
        pg: Page,
        turn: Option<state::review::Turn>,
        latest: bool,
        cx: &mut Context<Self>,
    ) {
        self.tab = Some(Tab::Review);
        self.review = None;
        self.error = None;
        self.read = Some(cx.spawn(async move |this, cx| {
            let r = cx
                .background_executor()
                .spawn(async move {
                    let r = pg.review(turn)?;
                    if latest && let Some(turn) = r.latest_changed_turn {
                        return pg.review(Some(turn));
                    }
                    Ok(r)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => this.review = Some(Arc::new(r)),
                    Err(e) => this.error = Some(e),
                }
                cx.notify()
            });
        }));
        cx.notify()
    }
}
impl Desktop {
    pub(super) fn review_button(
        &self,
        ins: &Entity<Inspector>,
        pg: &Page,
        p: &Colors,
        cx: &Context<Self>,
    ) -> Button {
        let pg = pg.clone();
        let ins = ins.clone();
        p.icon_button("review", "diff", "変更をレビュー", cx)
            .on_click(cx.listener(move |app, _, w, cx| {
                if pg.visible(app) {
                    ins.update(cx, |s, cx| {
                        if s.tab == Some(Tab::Review) {
                            s.tab = None;
                            cx.notify()
                        } else {
                            s.load(pg.clone(), None, true, cx)
                        }
                    });
                    app.restore_input(w, cx)
                }
            }))
    }
    pub(super) fn review_panel(
        &self,
        key: &EditorKey,
        pg: &Page,
        ins: &Entity<Inspector>,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut body = div()
            .id("review-files")
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .p_4()
            .flex()
            .flex_col()
            .gap_3();
        let review = ins.read(cx).review.clone();
        let error = ins.read(cx).error;
        let mut controls = div().flex().gap_1().items_center();
        let pg1 = pg.clone();
        let ins1 = ins.clone();
        controls = controls.child(
            Button::new("all-changes")
                .small()
                .label("セッション全体")
                .on_click(move |_, _, cx| {
                    ins1.update(cx, |s, cx| s.load(pg1.clone(), None, false, cx))
                }),
        );
        let turns = self.machines[&key.host]
            .rows
            .group("entry")
            .iter()
            .filter(|(_, e)| e["kind"] == "user");
        for (i, (id, v)) in turns.enumerate() {
            let turn = state::review::Turn::User(v["entryId"].as_str().unwrap_or(id).to_owned());
            let pg = pg.clone();
            let ins = ins.clone();
            controls = controls.child(
                Button::new(("turn", i))
                    .small()
                    .label(format!("ターン {}", i + 1))
                    .selected(
                        review
                            .as_ref()
                            .is_some_and(|r| r.turn.as_ref() == Some(&turn)),
                    )
                    .on_click(move |_, _, cx| {
                        ins.update(cx, |s, cx| {
                            s.load(pg.clone(), Some(turn.clone()), false, cx)
                        })
                    }),
            );
        }
        if let Some(r) = &review {
            if r.files.is_empty() {
                body = body.child(p.caption("この範囲に読み込み済みの変更はありません"))
            }
            for (fi, f) in r.files.iter().enumerate() {
                let collapsed = w.use_keyed_state(
                    SharedString::from(format!("collapsed:{}", f.path)),
                    cx,
                    |_, _| false,
                );
                let closed = *collapsed.read(cx);
                let mut card = div()
                    .rounded_md()
                    .border_1()
                    .border_color(p.border)
                    .p_3()
                    .child(
                        Button::new(("toggle-file", fi))
                            .ghost()
                            .label(format!("{} {}", if closed { "▸" } else { "▾" }, f.path))
                            .on_click(move |_, _, cx| {
                                collapsed.update(cx, |v, cx| {
                                    *v = !*v;
                                    cx.notify()
                                })
                            }),
                    );
                if closed {
                    body = body.child(card);
                    continue;
                }
                for (i, piece) in f.pieces.iter().enumerate() {
                    if let Some(omitted) = &piece.omitted {
                        card = card.child(p.caption(omitted.clone()))
                    }
                    if let Some(text) = piece.patch.as_ref().or(piece.content.as_ref()) {
                        let limit = w.use_keyed_state(
                            SharedString::from(format!("diff:{fi}:{i}:{}", f.path)),
                            cx,
                            |_, _| 120usize,
                        );
                        let n = *limit.read(cx);
                        card = card.child(p.caption(if piece.created {
                            "新しいファイル"
                        } else {
                            "変更内容"
                        }));
                        let mut lines = text.lines();
                        for (li, l) in lines.by_ref().take(n).enumerate() {
                            let page = pg.clone();
                            let quote = format!("{}\n> {l}", f.path);
                            card = card.child(
                                Button::new(SharedString::from(format!("quote:{fi}:{i}:{li}")))
                                    .ghost()
                                    .small()
                                    .justify_start()
                                    .px_1()
                                    .tooltip("この行を下書きに引用")
                                    .child(
                                        div()
                                            .w_full()
                                            .font_family("monospace")
                                            .bg(if l.starts_with('+') {
                                                p.add_bg
                                            } else if l.starts_with('-') {
                                                p.del_bg
                                            } else {
                                                p.bg
                                            })
                                            .child(l.to_owned()),
                                    )
                                    .on_click(cx.listener(move |app, _, w, cx| {
                                        if let Err(e) = page.append_reference(app, &quote, w, cx) {
                                            app.notice = format!("入力欄に追加できません: {e}")
                                        };
                                        cx.notify()
                                    })),
                            );
                        }
                        if lines.next().is_some() {
                            card = card.child(
                                Button::new(SharedString::from(format!("more:{fi}:{i}")))
                                    .small()
                                    .label("次の120行")
                                    .on_click(move |_, _, cx| {
                                        limit.update(cx, |n, cx| {
                                            *n += 120;
                                            cx.notify()
                                        })
                                    }),
                            );
                        }
                    }
                }
                body = body.child(card)
            }
        } else if let Some(e) = error {
            body = body.child(format!("差分を取得できません: {e}"))
        } else {
            body = body.child("差分を集計中…")
        }
        let ask = ins.clone();
        let page = pg.clone();
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
                    .child("変更をレビュー"),
            )
            .child(
                div()
                    .id("review-turns")
                    .overflow_x_scroll()
                    .p_2()
                    .child(controls),
            )
            .child(body)
            .child(
                div().p_3().child(
                    p.primary("ask-changes", cx)
                        .label("この変更について依頼")
                        .disabled(review.as_ref().is_none_or(|r| r.files.is_empty()))
                        .on_click(cx.listener(move |this, _, w, cx| {
                            if !page.visible(this) {
                                return;
                            }
                            let outcome = ask
                                .read(cx)
                                .review
                                .clone()
                                .as_ref()
                                .map(|r| r.ask_about(this, w, cx));
                            match outcome {
                                Some(Ok(())) => ask.update(cx, |s, cx| {
                                    *s = Inspector::default();
                                    cx.notify()
                                }),
                                Some(Err(e)) => {
                                    this.notice = format!("入力欄に追加できません: {e}");
                                    cx.notify()
                                }
                                _ => {}
                            }
                        })),
                ),
            )
    }
}
