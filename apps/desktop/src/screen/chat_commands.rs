use super::*;

#[derive(Default)]
struct Request {
    task: Option<Task<()>>,
    result: Option<Result<(), Error>>,
}
fn load(req: &Entity<Request>, page: state::Page, cx: &mut Context<Desktop>) {
    let weak = req.downgrade();
    let task = cx.spawn(async move |app, cx| {
        let result = page.sheet(state::SheetAction::Commands).await.map(|_| ());
        let _ = app.update(cx, |app, cx| {
            if page.visible(app)
                && let Some(req) = weak.upgrade()
            {
                req.update(cx, |r, _| r.result = Some(result));
                cx.notify();
            }
        });
    });
    req.update(cx, |r, _| {
        r.task = Some(task);
        r.result = None;
    });
}
#[derive(Clone)]
pub(super) struct Candidates {
    page: state::Page,
    key: EditorKey,
    text: String,
    names: Vec<String>,
    active: Entity<usize>,
}
impl Candidates {
    fn accept(&self, app: &Desktop, name: &str, w: &mut Window, cx: &mut Context<Desktop>) {
        if self.page.visible(app)
            && app.live_matches(&self.key, &self.text, w, cx)
            && let Some((_, input, _)) = &app.input
        {
            input.update(cx, |s, cx| {
                s.set_value(format!("/{name} "), w, cx);
                s.focus(w, cx)
            });
        }
    }
    pub(super) fn bind(self, div: Div, cx: &Context<Desktop>) -> Div {
        let up = self.active.clone();
        let down = up.clone();
        let n = self.names.len();
        div.key_context("Composer Picker")
            .on_action(cx.listener(move |_, _: &keys::PickUp, _, cx| {
                up.update(cx, |v, _| *v = v.saturating_sub(1));
                cx.notify();
            }))
            .on_action(cx.listener(move |_, _: &keys::PickDown, _, cx| {
                down.update(cx, |v, _| *v = (*v + 1).min(n - 1));
                cx.notify();
            }))
            .on_action(cx.listener(move |app, _: &keys::PickTab, w, cx| {
                if let Some(name) = self.names.get(*self.active.read(cx)) {
                    self.accept(app, name, w, cx);
                }
            }))
    }
}
impl Desktop {
    pub(super) fn commands(
        &self,
        key: &EditorKey,
        page: &state::Page,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Option<Stateful<Div>>, Option<Candidates>) {
        let Some(text) = self.drafts.get(key).filter(|t| {
            t.strip_prefix('/')
                .is_some_and(|s| !s.chars().any(char::is_whitespace))
                && !self.composing
        }) else {
            return (None, None);
        };
        let req = w.use_keyed_state(
            SharedString::from(format!("commands:{key:?}:{}", self.epoch)),
            cx,
            |_, _| Request::default(),
        );
        if req.read(cx).task.is_none() {
            load(&req, page.clone(), cx)
        }
        let mut list = div()
            .id("command-candidates")
            .overflow_y_scroll()
            .max_h(rems(9.))
            .flex()
            .flex_col()
            .px_4()
            .py_1();
        match req.read(cx).result {
            None => return (Some(list.child(p.caption("コマンド候補を取得中…"))), None),
            Some(Err(e)) => {
                let pg = page.clone();
                return (
                    Some(
                        list.child(p.caption(format!("候補を取得できません: {e}")))
                            .child(
                                p.text_button("retry-commands", p.accent_ink, cx)
                                    .label("再試行")
                                    .debug_selector(|| "retry-commands".into())
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        if pg.visible(app) {
                                            load(&req, pg.clone(), cx);
                                            cx.notify();
                                        }
                                    })),
                            ),
                    ),
                    None,
                );
            }
            Some(Ok(())) => {}
        }
        let names = page.suggestions(text.clone()).unwrap_or_default();
        if names.is_empty() {
            return (
                Some(list.child(p.caption("一致するコマンドはありません"))),
                None,
            );
        }
        let active = w.use_keyed_state(
            SharedString::from(format!("candidate:{key:?}:{text}")),
            cx,
            |_, _| 0usize,
        );
        let ix = (*active.read(cx)).min(names.len() - 1);
        let scroller = w
            .use_keyed_state("candidate-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        scroller.scroll_to_item(ix);
        list = list.track_scroll(&scroller);
        let candidates = Candidates {
            page: page.clone(),
            key: key.clone(),
            text: text.clone(),
            names,
            active,
        };
        let shared = std::rc::Rc::new(candidates.clone());
        for (i, name) in candidates.names.iter().enumerate() {
            let shared = shared.clone();
            let value = self.machines[&key.host].rows.get("command", name);
            let description =
                value["description"]
                    .as_str()
                    .unwrap_or_else(|| match name.as_str() {
                        "model" => "モデルを変更",
                        "resume" => "過去のセッションを開く",
                        "tree" => "セッションツリー",
                        "fork" => "会話を分岐",
                        "new" => "新しいセッション",
                        "compact" => "会話を圧縮",
                        "reload" => "拡張機能を読み直す",
                        _ => "この Pi のコマンド",
                    });
            list = list.child(
                p.row(("candidate", i), ix == i, cx)
                    .debug_selector(|| format!("command-choice-{i}"))
                    .label(format!("/{name}　{description}"))
                    .on_click(cx.listener(move |app, _, w, cx| {
                        shared.accept(app, &shared.names[i], w, cx)
                    })),
            );
        }
        (Some(list), Some(candidates))
    }
}
