use super::*;
use pimate_protocol::GitHubQuery;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Review,
    Agents,
    Github,
}

impl Desktop {
    pub(super) fn select_github(&mut self, query: GitHubQuery, cx: &mut Context<Self>) {
        let Some((page, ins)) = &self.chat_keys else {
            return;
        };
        if !page.visible(self) {
            return;
        }
        // 各リクエストを独立した Nav に束縛し、古い応答が次の項目を上書きしないようにする。
        let n = cx.new(|_| settings::Nav {
            page: Some(page.clone()),
            github: github::Github {
                query: Some(query),
                ..Default::default()
            },
            ..Default::default()
        });
        ins.update(cx, |s, cx| {
            s.tab = Some(Tab::Github);
            s.github = Some(n);
            cx.notify();
        });
        cx.notify();
    }
    pub(super) fn with_inspector(
        &mut self,
        chat: Div,
        key: &EditorKey,
        page: &state::Page,
        ins: &Entity<Inspector>,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let row = div()
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(chat);
        let Some(tab) = ins.read(cx).tab else {
            return row;
        };
        let mut tabs = div().flex().items_center().gap_1().p_2();
        for (t, label) in [
            (Tab::Review, "変更"),
            (Tab::Agents, "サブエージェント"),
            (Tab::Github, "GitHub"),
        ] {
            let ins = ins.clone();
            let pg = page.clone();
            tabs = tabs.child(
                Button::new(label)
                    .small()
                    .label(label)
                    .selected(tab == t)
                    .disabled(
                        t == Tab::Github
                            && !github_available(
                                &self.machines[&key.host].rows,
                                &key.host,
                                &key.instance,
                            ),
                    )
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if !pg.visible(app) {
                            return;
                        }
                        if t == Tab::Github {
                            app.select_github(GitHubQuery::List, cx)
                        } else {
                            ins.update(cx, |s, cx| {
                                if t == Tab::Review {
                                    s.load(pg.clone(), None, true, cx)
                                } else {
                                    s.tab = Some(t);
                                    cx.notify()
                                }
                            })
                        }
                    })),
            );
        }
        let close = ins.clone();
        tabs = tabs.child(div().flex_1()).child(
            p.icon_button("close-inspector", "x", "閉じる", cx)
                .on_click(cx.listener(move |app, _, w, cx| {
                    close.update(cx, |s, cx| {
                        s.tab = None;
                        cx.notify()
                    });
                    app.restore_input(w, cx)
                })),
        );
        let body = match tab {
            Tab::Review => self.review_panel(key, page, ins, p, w, cx),
            Tab::Agents => self.agent_panel(key, page, ins, p, w, cx),
            Tab::Github => {
                let n = ins.read(cx).github.clone().unwrap();
                let q = n.read(cx).github.query.clone().unwrap();
                if let Some(panel) = n
                    .read(cx)
                    .github
                    .viewer
                    .clone()
                    .as_ref()
                    .and_then(|v| chat_media::Viewer::panel(v, None, p, w, cx))
                {
                    div().child(panel)
                } else {
                    let mut body = div().flex().flex_col().min_h_0().flex_1();
                    if q != GitHubQuery::List {
                        body = body.child(Button::new("github-list").label("一覧に戻る").on_click(
                            cx.listener(|app, _, _, cx| app.select_github(GitHubQuery::List, cx)),
                        ))
                    };
                    body.child(
                        div()
                            .id("github-inspector-body")
                            .overflow_y_scroll()
                            .child(self.github_view(&n, p, &q, w, cx)),
                    )
                }
            }
        };
        row.child(
            div()
                .id("inspector")
                .occlude()
                .flex()
                .flex_col()
                .flex_none()
                .w(px(400.))
                .max_w_full()
                .h_full()
                .bg(p.bg)
                .border_l_1()
                .border_color(p.border)
                .when(Layout::of(w.viewport_size().width) != Layout::Wide, |d| {
                    d.absolute().right_0().top_0()
                })
                .child(tabs)
                .child(div().flex().flex_1().min_h_0().child(body)),
        )
    }
}
