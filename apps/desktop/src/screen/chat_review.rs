use super::*;
use state::{Page, review::Review};
#[derive(Default)]
pub(super) struct Inspector {
    pub review: Option<Review>,
    pub review_open: bool,
    read: Option<Task<()>>,
    error: Option<Error>,
    pub agents: bool,
    pub child: Option<state::Child>,
    pub run: Option<String>,
}
impl Inspector {
    pub(super) fn load(
        &mut self,
        pg: Page,
        turn: Option<String>,
        latest: bool,
        cx: &mut Context<Self>,
    ) {
        self.review_open = true;
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
                    Ok(r) => this.review = Some(r),
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
        cx: &Context<Self>,
    ) -> Button {
        let pg = pg.clone();
        let ins = ins.clone();
        Button::new("review")
            .small()
            .label("変更をレビュー")
            .on_click(cx.listener(move |this, _, _, cx| {
                if pg.visible(this) {
                    ins.update(cx, |s, cx| s.load(pg.clone(), None, true, cx))
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
        let state = ins.read(cx);
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
            let turn = v["entryId"].as_str().unwrap_or(id).to_owned();
            let pg = pg.clone();
            let ins = ins.clone();
            controls = controls.child(
                Button::new(("turn", i))
                    .small()
                    .label(format!("ターン {}", i + 1))
                    .selected(
                        state
                            .review
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
        if let Some(r) = &state.review {
            if r.files.is_empty() {
                body = body.child(p.caption("この範囲に読み込み済みの変更はありません"))
            }
            for (fi, f) in r.files.iter().enumerate() {
                let mut card = div()
                    .rounded_md()
                    .border_1()
                    .border_color(p.border)
                    .p_3()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(f.path.clone()),
                    );
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
                        for l in lines.by_ref().take(n) {
                            card = card.child(
                                div()
                                    .font_family("monospace")
                                    .bg(if l.starts_with('+') {
                                        p.accent.opacity(0.10)
                                    } else if l.starts_with('-') {
                                        Hsla::from(rgb(0xd46055)).opacity(0.12)
                                    } else {
                                        p.bg
                                    })
                                    .child(l.to_owned()),
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
        } else if let Some(e) = state.error {
            body = body.child(format!("差分を取得できません: {e}"))
        } else {
            body = body.child("差分を集計中…")
        }
        let back = ins.clone();
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
                    .child("変更をレビュー")
                    .child(Button::new("review-back").label("会話に戻る").on_click(
                        move |_, _, cx| {
                            back.update(cx, |s, cx| {
                                *s = Inspector::default();
                                cx.notify()
                            })
                        },
                    )),
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
                        .disabled(state.review.as_ref().is_none_or(|r| r.files.is_empty()))
                        .on_click(cx.listener(move |this, _, w, cx| {
                            if !page.visible(this) {
                                return;
                            }
                            let outcome = ask
                                .read(cx)
                                .review
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
