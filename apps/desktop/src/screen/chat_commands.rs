use super::*;
impl Desktop {
    pub(super) fn commands(
        &self,
        key: &EditorKey,
        page: &state::Page,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let text = self.drafts.get(key)?;
        let prefix = text.strip_prefix('/')?;
        if prefix.chars().any(char::is_whitespace) || self.composing {
            return None;
        }
        let status = w.use_keyed_state(
            SharedString::from(format!("commands:{key:?}")),
            cx,
            |_, _| None::<Result<(), Error>>,
        );
        let pg = page.clone();
        let s = status.clone();
        let _load = w.use_keyed_state("command-request", cx, |_, cx| {
            cx.spawn(async move |_, cx| {
                let r = pg.sheet(state::SheetAction::Commands).await.map(|_| ());
                let _ = s.update(cx, |s, cx| {
                    *s = Some(r);
                    cx.notify()
                });
            })
        });
        let mut row = div()
            .id("command-candidates")
            .overflow_x_scroll()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_1();
        match status.read(cx) {
            None => return Some(row.child(p.caption("コマンド候補を取得中…"))),
            Some(Err(e)) => {
                return Some(row.child(p.caption(format!("候補を取得できません: {e}"))));
            }
            _ => {}
        }
        let names = page.suggestions(text.clone()).unwrap_or_default();
        if names.is_empty() {
            return Some(row.child(p.caption("一致するコマンドはありません")));
        }
        row = row.child(p.caption("候補"));
        for name in names {
            let pg = page.clone();
            let key = key.clone();
            let text = text.clone();
            row = row.child(
                Button::new(SharedString::from(format!("cmd:{name}")))
                    .small()
                    .label(format!("/{name}"))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if pg.visible(this)
                            && this.live_matches(&key, &text, w, cx)
                            && let Some((_, input, _)) = &this.input
                        {
                            input.update(cx, |s, cx| {
                                s.set_value(format!("/{name} "), w, cx);
                                s.focus(w, cx)
                            });
                            cx.notify();
                        }
                    })),
            );
        }
        Some(row)
    }
}
