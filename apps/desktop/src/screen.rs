mod chat_agents;
mod chat_area;
mod chat_commands;
pub(crate) mod chat_media;
mod chat_question;
mod chat_review;
mod conversation;
mod first_step;
mod github;
mod machine;
mod new_pi;
mod providers;
mod settings;
mod sheet_chat;
mod sheet_tree;
mod theme;
use super::*;
use gpui_component::{
    Disableable, Selectable, Sizable, TitleBar,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{Enter, Input},
    scroll::ScrollableElement,
};
use serde_json::Value;
use settings::{Navigation, Route};
pub(crate) use theme::Assets;
use theme::{Colors, Layout, icon};

pub fn transcript() -> ListState {
    ListState::new(0, ListAlignment::Top, px(100.))
}
fn pending_panel(id: SharedString, handle: &ScrollHandle) -> Stateful<Div> {
    div()
        .id(id)
        .min_h_0()
        .max_h(rems(6.))
        .overflow_y_scroll()
        .track_scroll(handle)
        .px_4()
}
fn connection(h: &Host, rows: Option<&Rows>) -> &'static str {
    if h.needs_pairing {
        return "再登録が必要";
    }
    match rows.and_then(|r| r.get("host", "connection").as_str()) {
        Some("connected") => "接続中",
        Some("offline") => "オフライン",
        Some("revoked" | "invalid_token" | "auth_failed") => "再登録が必要",
        Some("protocol_mismatch") => "バージョン不一致",
        _ => "接続しています…",
    }
}
fn pi_title<'a>(v: &'a Value, id: &'a str) -> &'a str {
    v["session"]["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| v["cwd"].as_str()?.rsplit('/').find(|s| !s.is_empty()))
        .unwrap_or(id)
}
fn github_available(rows: &Rows, host: &str, pi: &str) -> bool {
    rows.get("host", "info")["github"] == true && rows.editor(host, pi).is_some()
}
/// 一覧の行の右端に出す、最後の動きからの時間。
fn ago(now_ms: u64, then_ms: u64) -> String {
    match now_ms.saturating_sub(then_ms) / 60_000 {
        0 => "今".into(),
        m @ 1..60 => format!("{m}分"),
        m @ 60..1440 => format!("{}時間", m / 60),
        1440..2880 => "昨日".into(),
        m => format!("{}日", m / 1440),
    }
}
/// 「<セッション名> — PiMate」。要対応があれば件数を先頭に出す（OS の通知の代わり）。
fn window_title(name: Option<&str>, waiting: usize) -> String {
    let title = name.map_or_else(|| "PiMate".into(), |name| format!("{name} — PiMate"));
    if waiting == 0 {
        title
    } else {
        format!("({waiting}) {title}")
    }
}
#[derive(Clone)]
struct DragSidebar;
impl Render for DragSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}
impl Desktop {
    fn update_spinner(&mut self, cx: &mut Context<Self>) {
        let working = self.machines.values().any(|m| {
            m.rows
                .group("instance")
                .iter()
                .any(|(_, row)| row["working"] == true)
        });
        if !working {
            self.spinner_tick = None;
            self.spinner_phase = 0;
        } else if self.spinner_tick.is_none() {
            // All rows share one tick; the rest of the window needn't redraw between phases.
            self.spinner_tick = Some(cx.spawn(async |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.spinner_tick = None;
                    this.spinner_phase = (this.spinner_phase + 1) % 4;
                    cx.notify();
                });
            }));
        }
    }
    fn working_spinner(&self, color: Hsla) -> Div {
        let ring = div()
            .size(px(11.))
            .flex_none()
            .rounded_full()
            .border_2()
            .border_color(color);
        match self.spinner_phase {
            0 => ring.border_t_0(),
            1 => ring.border_r_0(),
            2 => ring.border_b_0(),
            _ => ring.border_l_0(),
        }
    }
    fn waiting_count(&self) -> usize {
        self.machines
            .values()
            .flat_map(|m| m.rows.group("instance"))
            .filter(|(_, v)| v["section"] == "waiting")
            .count()
    }
    fn sync_title(&mut self, w: &mut Window) {
        let name = self.selected.as_ref().and_then(|(h, i)| {
            let v = self.machines.get(h)?.rows.get("instance", i);
            (!v.is_null()).then(|| pi_title(v, i))
        });
        let title = window_title(name, self.waiting_count());
        if title != self.title {
            w.set_window_title(&title);
            self.title = title;
        }
    }
    fn pi_row(
        &self,
        h: &Host,
        id: &str,
        v: &Value,
        nav: &Navigation,
        p: &Colors,
        cx: &Context<Self>,
    ) -> Button {
        let (host, pi) = (h.id.clone(), id.to_owned());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let waiting = v["section"] == "waiting";
        let nav = nav.clone();
        let working = v["working"] == true;
        let failure = v["failureText"].as_str().filter(|s| !s.is_empty());
        let cwd = v["cwd"].as_str().unwrap_or(id);
        let (detail, color) = if let Some(text) = failure {
            (format!("失敗: {text}"), p.fail_ink)
        } else if waiting {
            ("入力待ち".to_owned(), p.waiting)
        } else {
            let mut detail = cwd.to_owned();
            if let Some(n) = v["subagents"]["active"].as_u64().filter(|n| *n > 0) {
                detail += &format!(" · サブエージェント {n} 実行中");
            }
            match v["origin"].as_str() {
                Some("mobile") => detail += " · スマホで起動",
                Some("grok-bot") => detail += " · Grok Bot",
                _ => {}
            }
            (detail, p.secondary)
        };
        let selected = self
            .selected
            .as_ref()
            .is_some_and(|(a, b)| a == &host && b == &pi);
        let mark = if working {
            self.working_spinner(p.secondary).mt(px(4.))
        } else {
            p.dot(if failure.is_some() {
                p.fail
            } else if waiting {
                p.wait
            } else {
                p.secondary.opacity(0.55)
            })
            .mt(px(7.))
        };
        p.row(SharedString::from(format!("pi-{host}-{pi}")), selected, cx)
            .h(px(theme::ROW))
            .px_3()
            .tooltip(format!("{} · {cwd}", self.host_label(h)))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    // Button は中身を幅いっぱいに広げないので、行の内側の幅を決める
                    // （サイドバーの左右 8px と行の左右 12px を引く）。
                    .w(px(self.sidebar_width() - 40.))
                    .child(mark)
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
                                    .child(pi_title(v, id).to_owned()),
                            )
                            .child(p.caption(detail).text_color(color).truncate()),
                    )
                    .child(
                        p.caption(if working {
                            "作業中".to_owned()
                        } else {
                            v["updatedAt"]
                                .as_u64()
                                .map_or_else(String::new, |t| ago(now, t))
                        })
                        .flex_none()
                        .text_size(rems(0.69)),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if nav.read(cx).route != Route::Home {
                    this.navigate(&nav, Route::Home, cx);
                }
                this.select(host.clone(), pi.clone(), cx)
            }))
    }
    /// サイドバー: 検索欄・要対応・マシンごと・最下段（設定／＋新しい Pi）。
    fn sidebar(
        &self,
        n: &Navigation,
        p: &Colors,
        search: &Entity<InputState>,
        cx: &Context<Self>,
    ) -> Div {
        let query = search.read(cx).value().trim().to_lowercase();
        let shown = |id: &str, v: &Value| {
            query.is_empty()
                || format!("{} {}", pi_title(v, id), v["cwd"].as_str().unwrap_or(""))
                    .to_lowercase()
                    .contains(&query)
        };
        let group = || {
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .px_3()
                .pt_3()
                .pb_1()
                .text_size(rems(0.75))
                .text_color(p.secondary)
        };
        let mut rows = div()
            .id("machine-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        let waiting: Vec<_> = self
            .hosts
            .iter()
            .filter_map(|h| Some((h, self.machines.get(&h.id)?)))
            .flat_map(|(h, m)| {
                m.rows
                    .group("instance")
                    .iter()
                    .map(move |(id, v)| (h, id, v))
            })
            .filter(|(_, id, v)| v["section"] == "waiting" && shown(id, v))
            .collect();
        if !waiting.is_empty() {
            rows = rows.child(
                group()
                    .text_color(p.waiting)
                    .child(icon("bell", 13., p.waiting))
                    .child(format!("要対応　{}", waiting.len()))
                    .debug_selector(|| "sidebar-waiting".into()),
            );
            for (h, id, v) in waiting {
                rows = rows.child(self.pi_row(h, id, v, n, p, cx));
            }
        }
        for h in &self.hosts {
            let data = self.machines.get(&h.id).map(|m| &m.rows);
            let state = connection(h, data);
            rows = rows.child(
                group()
                    .child(icon("laptop", 14., p.secondary))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(self.host_label(h)),
                    )
                    .child(
                        p.dot(if state == "接続中" { p.done } else { p.wait })
                            .size(px(6.)),
                    )
                    .child(div().flex_none().text_size(rems(0.69)).child(state)),
            );
            if let Some(error) = self.state.registration.connection_errors.get(&h.id) {
                rows = rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl_3()
                        .child(
                            p.caption(format!("接続できません: {error}"))
                                .text_color(p.fail_ink)
                                .flex_1()
                                .min_w_0(),
                        )
                        .child(
                            self.reload_button(
                                SharedString::from(format!("retry-{}", h.id)),
                                "つなぎ直す",
                                p,
                                cx,
                            )
                            .flex_none(),
                        ),
                );
            }
            if let Some(data) = data {
                let entries = data.group("instance");
                if entries.is_empty() {
                    rows = rows.child(
                        p.caption(if data.get("host", "listKnown") == true {
                            "起動中の Pi はありません"
                        } else {
                            "一覧を取得しています…"
                        })
                        .px_3()
                        .py_2(),
                    );
                }
                for (id, v) in entries
                    .iter()
                    .filter(|(id, v)| v["section"] != "waiting" && shown(id, v))
                {
                    rows = rows.child(self.pi_row(h, id, v, n, p, cx));
                }
            }
        }
        let go = |id: &'static str, route: Route, button: Button| {
            let n = n.clone();
            button
                .debug_selector(move || format!("nav-{id}"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.sidebar_open = false;
                    this.navigate(&n, route.clone(), cx)
                }))
        };
        div()
            .debug_selector(|| "sidebar".into())
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .bg(p.surface)
            .px_2()
            .pt_3()
            .pb_2()
            .child(
                div()
                    .mx_1()
                    .mb_1()
                    .h(px(theme::CONTROL))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl_3()
                    .rounded(px(theme::CONTROL / 2.))
                    .bg(p.bg)
                    .child(icon("search", 16., p.secondary))
                    .child(Input::new(search).appearance(false).cleanable(true)),
            )
            .child(rows)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .pt_2()
                    .child(go(
                        "settings",
                        Route::Settings,
                        p.text_button("settings", p.secondary, cx)
                            .child(icon("gear", 16., p.secondary))
                            .child("設定"),
                    ))
                    .child(go(
                        "new-pi",
                        self.new_route(),
                        p.primary("new-pi", cx)
                            .child(icon("plus", 16., p.on_accent))
                            .child("新しい Pi"),
                    )),
            )
    }
    fn sidebar_width(&self) -> f32 {
        let (min, max) = theme::SIDEBAR_RANGE;
        self.sidebar_width.unwrap_or(theme::SIDEBAR).clamp(min, max)
    }
    fn sidebar_toggle(&self, p: &Colors, cx: &Context<Self>) -> Button {
        p.icon_button("sidebar-toggle", "side", "Pi の一覧", cx)
            .debug_selector(|| "sidebar-toggle".into())
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify()
            }))
    }
    /// 登録の進み具合・結果・失敗。読み出せないときは、ここから読み直せる。
    fn registration_notice(&self, p: &Colors, cx: &Context<Self>) -> Option<Div> {
        let r = &self.state.registration;
        let message = r.message();
        if message.is_empty() && !self.pairing {
            return None;
        }
        let warning = r.error.is_some() || r.save_unknown;
        // Keep the store diagnostic legible and separate from the Japanese recovery text.
        let message = match r.error {
            Some(e) => message.replace(&format!(": {e}"), ""),
            None => message,
        };
        Some(
            p.card()
                .flex()
                .flex_col()
                .items_start()
                .gap_1()
                .debug_selector(|| "registration-notice".into())
                // 保存の応答は待ち続けることがある。その間も画面は操作できるよう、幕は出さない。
                .when(self.pairing, |card| card.child("登録・保存の結果を確認中…"))
                .when(!message.is_empty(), |card| {
                    card.child(
                        div()
                            .text_color(if warning { p.fail_ink } else { p.ink })
                            .child(message),
                    )
                })
                .children(r.error.map(|e| {
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_family("monospace")
                        .child(format!("Error: {e}"))
                }))
                .when(warning, |card| {
                    card.child(self.reload_button("reload", "登録を読み直す", p, cx))
                }),
        )
    }
    /// 登録を読み直し、つながっていないマシンへつなぎ直す。
    fn reload_button(
        &self,
        id: impl Into<ElementId>,
        label: &'static str,
        p: &Colors,
        cx: &Context<Self>,
    ) -> Button {
        p.text_button(id, p.accent_ink, cx)
            .label(label)
            .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
    }
    /// 貼り付けたリンクの接続先を見せてから登録する。確定したあとの進み具合は知らせに出す。
    fn pair_dialog(&self, p: &Colors, cx: &Context<Self>) -> Option<Div> {
        let (_, target) = self.confirmation.as_ref()?;
        let card = div()
            .w(px(380.))
            .max_w(relative(0.9))
            .p_5()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(px(theme::R_CARD))
            .bg(p.bg)
            .shadow_lg()
            .debug_selector(|| "pair-dialog".into());
        let (name, address) = target.rsplit_once(" · ").unwrap_or((target, ""));
        let card = card
            .child(p.caption("接続先を確認"))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name.to_owned()),
            )
            .child(p.caption(address.to_owned()).font_family("monospace"))
            .child(
                div()
                    .mt_3()
                    .flex()
                    .justify_end()
                    .items_center()
                    .gap_2()
                    .child(
                        p.text_button("cancel", p.secondary, cx)
                            .label("キャンセル")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirmation = None;
                                cx.notify()
                            })),
                    )
                    .child(
                        p.primary("pair", cx)
                            .label("この接続先を登録")
                            .disabled(self.pairing)
                            .on_click(cx.listener(|this, _, _, cx| this.pair(cx))),
                    ),
            );
        Some(p.scrim().items_center().justify_center().child(card))
    }
    /// 3ペインの入れ物。幅に応じてサイドバーとインスペクターを並べるか重ねる。
    fn panes(
        &self,
        layout: Layout,
        side: Option<Div>,
        main: Div,
        inspector: Option<Div>,
        p: &Colors,
        cx: &Context<Self>,
    ) -> Div {
        let (min, max) = theme::SIDEBAR_RANGE;
        let width = self.sidebar_width();
        let side = side.map(|s| s.w(px(width)));
        let mut panes = div().relative().flex().flex_1().min_h_0();
        let mut overlay = None;
        match side {
            Some(side) if layout != Layout::Narrow => {
                panes = panes
                    .on_drag_move(cx.listener(
                        move |this, e: &DragMoveEvent<DragSidebar>, _, cx| {
                            let width = f32::from(e.event.position.x - e.bounds.left());
                            this.sidebar_width = Some(width.clamp(min, max));
                            cx.notify()
                        },
                    ))
                    .on_drop(cx.listener(|this, _: &DragSidebar, _, _| {
                        // 幅は次回も覚える。書けなくても、この起動中は効く。設定のファイルには触れない。
                        let _ = state::layout_path()
                            .and_then(|p| state::save_sidebar_width(&p, this.sidebar_width()));
                    }))
                    .child(side)
                    .child(
                        div()
                            .id("sidebar-handle")
                            .absolute()
                            .top_0()
                            .left(px(width - 3.))
                            .w(px(6.))
                            .h_full()
                            .cursor_col_resize()
                            .on_drag(DragSidebar, |drag, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| drag.clone())
                            }),
                    )
            }
            side => overlay = side.filter(|_| self.sidebar_open),
        }
        panes = panes.child(main);
        // 変更・サブエージェント・GitHub。広ければ並べ、狭ければ会話の上に重ねる。
        if let Some(inspector) = inspector {
            panes = panes.child(if layout == Layout::Wide {
                inspector.flex_none().border_l_1().border_color(p.fill2)
            } else {
                inspector
                    .absolute()
                    .top_0()
                    .right_0()
                    .h_full()
                    .bg(p.bg)
                    .shadow_lg()
            });
        }
        if let Some(side) = overlay {
            panes = panes.child(
                p.scrim()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.sidebar_open = false;
                            cx.notify()
                        }),
                    )
                    .child(
                        side.shadow_lg()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                    ),
            );
        }
        panes
    }
}
impl Render for Desktop {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_spinner(cx);
        self.bind_editor(w, cx);
        let nav = self.init_navigation(w, cx);
        if let Some(host) = self.state.registration.paired_host.take() {
            self.navigate(&nav, Route::First(host), cx);
        }
        let dark = self.palette(w, cx);
        w.set_rem_size(px(self.size));
        let p = Colors::new(dark);
        self.sync_title(w);
        let layout = Layout::of(w.viewport_size().width);
        if self.search.is_none() {
            let input = cx.new(|cx| InputState::new(w, cx).placeholder("Pi・フォルダを検索"));
            let changed = cx.subscribe(&input, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify()
                }
            });
            self.search = Some((input, changed));
        }
        let search = self.search.as_ref().unwrap().0.clone();
        // マシンが1台もないうちは、並べる Pi がないのでサイドバーを出さない。
        let side = (!self.hosts.is_empty()).then(|| self.sidebar(&nav, &p, &search, cx));
        let toggle =
            (side.is_some() && layout == Layout::Narrow).then(|| self.sidebar_toggle(&p, cx));
        // The answer belongs to the pending question, not to the visible workspace panel.
        let editor = self.editor();
        let question = editor.as_ref().and_then(|key| {
            let rows = &self.machines[&key.host].rows;
            rows.group("question").first().map(|(id, q)| {
                (
                    id.clone(),
                    chat_question::pending_form(key, &rows.view_life, id, q, w, cx),
                )
            })
        });
        let home = nav.read(cx).route == Route::Home;
        let first = home && self.hosts.is_empty();
        let mut main = div().flex().flex_col().flex_1().min_w_0().h_full();
        if !first && let Some(notice) = self.registration_notice(&p, cx) {
            main = main.child(notice.mx_4().mt_3());
        }
        if !self.notice.is_empty() {
            main = main.child(
                p.caption(self.notice.clone())
                    .px_4()
                    .py_2()
                    .debug_selector(|| "chat-notice".into()),
            );
        }
        // 会話のヘッダーがない画面では、狭いときの一覧のボタンだけを上に置く。
        let bar = |toggle: Option<Button>| {
            toggle.map(|t| {
                div()
                    .h(px(theme::HEADER))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .child(t)
            })
        };
        if let Some(panel) = self.other_panel(&nav, &p, w, cx) {
            main = main
                .children(bar(toggle))
                .child(div().flex().flex_1().min_h_0().child(panel));
        } else if first {
            main = main.child(self.first_screen(&nav, &p, cx));
        } else if let Some(key) = editor {
            main = self.chat_area(main, key, question, &p, w, cx);
        } else if let Some(hint) = self.reconnect_hint() {
            main = main.children(bar(toggle)).child(hint);
        } else {
            let description = self
                .selected
                .as_ref()
                .and_then(|(host, pi)| self.machines.get(host).map(|m| m.rows.subscription(pi)))
                .unwrap_or_else(|| "左の一覧から Pi を選ぶと、ここに会話が出ます。".into());
            main = main.children(bar(toggle)).child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .p_4()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(rems(1.06))
                            .child("会話を開く"),
                    )
                    .child(div().text_color(p.secondary).child(description)),
            );
        }
        // Linux で OS がタイトルバーを描かない（GNOME）ときだけ、アプリが描く。
        let titlebar = (cfg!(target_os = "linux")
            && matches!(w.window_decorations(), Decorations::Client { .. }))
        .then(|| {
            TitleBar::new().bg(p.fill2).border_color(p.fill2).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_center()
                    .text_size(rems(0.75))
                    .text_color(p.secondary)
                    .child(self.title.clone()),
            )
        });
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(p.bg)
            .text_color(p.ink)
            .text_size(rems(0.875))
            .children(titlebar)
            .child(self.panes(layout, side, main, None, &p, cx))
            .children(self.pair_dialog(&p, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::pending_panel;
    use gpui::{Context, ScrollHandle, Window, div, point, prelude::*, px, rems, size};
    pub(super) fn test_host() -> (
        std::net::TcpListener,
        pimate_core::Host,
        std::sync::Arc<pimate_core::Core>,
    ) {
        use pimate_core::{Core, Credential, Error, Host, SecretStore};
        use std::sync::Arc;
        struct Store(Host);
        impl SecretStore for Store {
            fn hosts(&self) -> Result<Vec<Host>, Error> {
                Ok(vec![])
            }
            fn save(&self, _: Host, _: String) -> Result<(), Error> {
                unreachable!()
            }
            fn load(&self, _: String) -> Result<Credential, Error> {
                Ok(Credential {
                    host: self.0.clone(),
                    token: "fixture".into(),
                })
            }
            fn invalidate(&self, _: String, _: String) -> Result<(), Error> {
                unreachable!()
            }
            fn forget(&self, _: String) -> Result<(), Error> {
                unreachable!()
            }
        }
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let h = Host {
            id: "fixture".into(),
            name: "fixture".into(),
            address: socket.local_addr().unwrap().to_string(),
            device_id: "test".into(),
            needs_pairing: false,
        };
        let core = Arc::new(Core::new(Arc::new(Store(h.clone()))));
        (socket, h, core)
    }
    #[gpui::test]
    fn spinner_ticks_only_at_phase_changes_while_working(cx: &mut gpui::TestAppContext) {
        use super::{Desktop, Machine, Rows};
        use pimate_core::{Item, Snapshot};
        use std::{sync::Arc, time::Duration};

        let (_socket, h, core) = test_host();
        let (tx, _rx) = crate::Delivery::channel();
        let conn = core.connect(h.id.clone(), Arc::new(tx)).unwrap();
        cx.update(gpui_component::init);
        let (desktop, w) = cx.add_window_view(|w, cx| Desktop::new(core, w, cx));
        w.run_until_parked();
        desktop.update(w, |d, cx| {
            d.hosts = vec![h.clone()];
            d.machines.insert(
                h.id.clone(),
                Machine {
                    conn,
                    registration: h,
                    rows: Rows::snapshot(Snapshot {
                        revision: 1,
                        items: ["a", "b"]
                            .map(|id| Item {
                                collection: "instance".into(),
                                id: id.into(),
                                json: r#"{"working":true}"#.into(),
                                position: 0,
                            })
                            .into(),
                    }),
                },
            );
            cx.notify();
        });
        w.run_until_parked();
        for (ms, phase) in [(199, 0), (1, 1), (200, 2)] {
            w.executor().advance_clock(Duration::from_millis(ms));
            assert_eq!(desktop.read_with(w, |d, _| d.spinner_phase), phase);
        }
        desktop.update(w, |d, cx| {
            d.machines.clear();
            cx.notify();
        });
        w.run_until_parked();
        w.executor().advance_clock(Duration::from_millis(400));
        desktop.read_with(w, |d, _| {
            assert!(d.spinner_tick.is_none());
            assert_eq!(d.spinner_phase, 0);
        });
    }
    /// 1台のマシンと、質問待ちの会話を開いた状態。
    fn open_chat(
        cx: &mut gpui::TestAppContext,
    ) -> (
        gpui::Entity<super::Desktop>,
        &mut gpui::VisualTestContext,
        std::net::TcpListener,
    ) {
        use super::{Desktop, Machine, Rows};
        use gpui_component::Root;
        use pimate_core::{Item, Snapshot};
        use serde_json::json;
        use std::sync::Arc;
        let (socket, h, core) = test_host();
        let (tx, _rx) = crate::Delivery::channel();
        let conn = core.connect(h.id.clone(), Arc::new(tx)).unwrap();
        cx.update(gpui_component::init);
        let mut desktop = None;
        let (_, w) = cx.add_window_view(|w, cx| {
            let app = cx.new(|cx| Desktop::new(core, w, cx));
            desktop = Some(app.clone());
            Root::new(app, w, cx)
        });
        let desktop = desktop.unwrap();
        desktop.update(w, |d, cx| {
            let rows = Rows::snapshot(Snapshot {
                revision: 1,
                items: [
                    ("host", "connection", json!("connected")),
                    ("host", "info", json!({"github":true})),
                    ("view", "phase", json!("ready")),
                    ("view", "instanceId", json!("pi")),
                    ("question", "q", json!({"kind":"input","title":"Question"})),
                    (
                        "instance",
                        "pi",
                        json!({"cwd":"/home/u/a-rather-long-folder-name-for-the-header","section":"waiting"}),
                    ),
                ]
                .map(|(c, id, v)| Item {
                    collection: c.into(),
                    id: id.into(),
                    json: v.to_string(),
                    position: 0,
                })
                .into(),
            });
            d.selected = Some((h.id.clone(), "pi".into()));
            d.hosts = vec![h.clone()];
            d.machines.insert(
                h.id.clone(),
                Machine {
                    conn,
                    registration: h,
                    rows,
                },
            );
            cx.notify();
        });
        w.run_until_parked();
        (desktop, w, socket)
    }
    #[test]
    fn row_time_and_window_title_follow_the_design() {
        let minute = 60_000;
        for (age, text) in [
            (0, "今"),
            (59_999, "今"),
            (5 * minute, "5分"),
            (179 * minute, "2時間"),
            (30 * 60 * minute, "昨日"),
            (72 * 60 * minute, "3日"),
        ] {
            assert_eq!(
                super::ago(100 * 60 * 60 * minute, 100 * 60 * 60 * minute - age),
                text
            );
        }
        assert_eq!(super::ago(0, 5), "今", "a clock behind the host");
        assert_eq!(super::window_title(None, 0), "PiMate");
        assert_eq!(super::window_title(Some("demo"), 0), "demo — PiMate");
        assert_eq!(super::window_title(Some("demo"), 2), "(2) demo — PiMate");
        assert_eq!(super::window_title(None, 1), "(1) PiMate");
    }
    #[gpui::test]
    fn panes_follow_the_window_width(cx: &mut gpui::TestAppContext) {
        let (desktop, w, _socket) = open_chat(cx);
        // debug_bounds keeps selectors from earlier frames, so judge by where the header starts.
        for (width, docked) in [(1280., true), (900., true), (620., false), (480., false)] {
            w.simulate_resize(size(px(width), px(800.)));
            w.run_until_parked();
            // The header stays one line: fixed height, nothing wrapped below or cut off.
            let header = w.debug_bounds("chat-header").unwrap();
            assert_eq!(header.size.height, px(52.), "{width}");
            assert_eq!(header.left(), px(if docked { 264. } else { 0. }), "{width}");
            assert_eq!(header.right(), px(width), "{width}");
            let mut parts = vec!["chat-title", "nav-github"];
            if docked {
                let side = w.debug_bounds("sidebar").unwrap();
                assert_eq!((side.left(), side.right()), (px(0.), header.left()));
            } else {
                parts.push("sidebar-toggle");
            }
            for part in parts {
                let b = w.debug_bounds(part).unwrap();
                assert!(
                    b.top() >= header.top()
                        && b.bottom() <= header.bottom()
                        && b.left() >= header.left()
                        && b.right() <= header.right()
                        && b.size.width > px(0.),
                    "{part} at {width}: {b:?}"
                );
            }
            // The conversation keeps a 720px column (plus its 16px gutters) in the middle.
            let column = w.debug_bounds("chat-column").unwrap();
            assert_eq!(
                column.size.width,
                header.size.width.min(px(752.)),
                "{width}"
            );
            assert_eq!(
                column.left() - header.left(),
                header.right() - column.right(),
                "{width}"
            );
        }
        // Below 760px the list overlays the conversation; a click outside puts it away.
        let toggle = w.debug_bounds("sidebar-toggle").unwrap().center();
        w.simulate_click(toggle, Default::default());
        desktop.read_with(w, |d, _| assert!(d.sidebar_open));
        assert_eq!(w.debug_bounds("chat-header").unwrap().left(), px(0.));
        w.simulate_click(point(px(400.), px(400.)), Default::default());
        desktop.read_with(w, |d, _| {
            assert!(!d.sidebar_open);
            assert_eq!(
                d.title,
                "(1) a-rather-long-folder-name-for-the-header — PiMate"
            );
        });
    }
    #[gpui::test]
    fn dragged_sidebar_width_is_used_within_its_range(cx: &mut gpui::TestAppContext) {
        // Desktop::new never reads the saved width, so these tests don't depend on the user's file.
        let (desktop, w, _socket) = open_chat(cx);
        w.simulate_resize(size(px(1280.), px(800.)));
        for (saved, shown) in [(None, 264.), (Some(300.), 300.), (Some(1000.), 360.)] {
            desktop.update(w, |d, cx| {
                d.sidebar_width = saved;
                cx.notify()
            });
            w.run_until_parked();
            assert_eq!(
                w.debug_bounds("chat-header").unwrap().left(),
                px(shown),
                "{saved:?}"
            );
        }
    }
    #[gpui::test]
    fn registrations_reload_from_settings_even_while_a_pairing_is_saving(
        cx: &mut gpui::TestAppContext,
    ) {
        let (desktop, w, _socket) = open_chat(cx);
        // A save that never answers shows its progress without covering the window.
        desktop.update(w, |d, cx| {
            d.pairing = true;
            cx.notify()
        });
        w.run_until_parked();
        assert!(w.debug_bounds("registration-notice").is_some());
        assert!(w.debug_bounds("pair-dialog").is_none());
        for id in ["nav-settings", "settings-reload"] {
            let pos = w.debug_bounds(id).unwrap().center();
            w.simulate_click(pos, Default::default());
            w.run_until_parked();
        }
        // The fixture's store lists no machines, so a reload that really ran forgets the injected one.
        desktop.read_with(w, |d, _| {
            assert!(d.hosts.is_empty() && d.machines.is_empty())
        });
    }
    #[gpui::test]
    fn first_screen_has_no_sidebar_until_a_machine_is_registered(cx: &mut gpui::TestAppContext) {
        use gpui::ClipboardItem;
        let (_socket, _, core) = test_host();
        cx.update(gpui_component::init);
        let (desktop, w) = cx.add_window_view(|w, cx| super::Desktop::new(core, w, cx));
        w.run_until_parked();
        assert!(w.debug_bounds("first-screen").is_some());
        assert!(w.debug_bounds("sidebar").is_none());
        assert!(w.debug_bounds("pair-dialog").is_none());
        // A valid link asks for confirmation in a dialog before anything is registered.
        w.update(|_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                "pimate://pair?h=127.0.0.1%3A47811&c=ABCDEFGH-JKLMNPQR&n=Desk&id=desk".into(),
            ))
        });
        desktop.update(w, |d, cx| d.paste(cx));
        w.run_until_parked();
        assert!(w.debug_bounds("pair-dialog").is_some());
        desktop.read_with(w, |d, _| {
            assert_eq!(d.confirmation.as_ref().unwrap().1, "Desk · 127.0.0.1:47811")
        });
    }
    #[gpui::test]
    fn unanswered_input_survives_sidebar_routes(cx: &mut gpui::TestAppContext) {
        use gpui::{ClipboardItem, VisualTestContext};
        use pimate_core::Item;
        use serde_json::json;
        let (desktop, w, _socket) = open_chat(cx);
        let click = |w: &mut VisualTestContext, id| {
            let pos = w.debug_bounds(id).unwrap().center();
            w.simulate_click(pos, Default::default());
        };
        click(w, "question-card");
        w.simulate_keystrokes("s t a y");
        for (dest, replaced) in [
            ("nav-settings", false),
            ("nav-github", false),
            ("nav-settings", true),
        ] {
            click(w, dest);
            if replaced {
                desktop.update(w, |d, cx| {
                    let rows = &mut d.machines.values_mut().next().unwrap().rows;
                    rows.apply(
                        (
                            2,
                            vec![pimate_core::Change::Upsert(Item {
                                collection: "question".into(),
                                id: "q".into(),
                                json: json!({"kind":"input","title":"Replacement"}).to_string(),
                                position: 0,
                            })],
                        ),
                        || unreachable!(),
                    );
                    cx.notify();
                });
                w.run_until_parked();
            }
            click(w, "nav-back");
            click(w, "question-card");
            w.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into())));
            w.simulate_keystrokes(if cfg!(target_os = "macos") {
                "cmd-a cmd-c"
            } else {
                "ctrl-a ctrl-c"
            });
            assert_eq!(
                w.read_from_clipboard().and_then(|c| c.text()).as_deref(),
                Some(if replaced { "sentinel" } else { "stay" }),
                "{dest}"
            );
        }
    }
    #[test]
    fn github_requires_both_a_ready_editor_and_host_query_support() {
        use pimate_core::{Item, Snapshot};
        use serde_json::json;
        for ready in [false, true] {
            for github in [false, true] {
                let rows = super::Rows::snapshot(Snapshot {
                    revision: 1,
                    items: [
                        ("host", "info", json!({"github":github,"githubStatus":true})),
                        (
                            "view",
                            "phase",
                            json!(if ready { "ready" } else { "loading" }),
                        ),
                        ("view", "instanceId", json!("pi")),
                    ]
                    .map(|(c, id, v)| Item {
                        collection: c.into(),
                        id: id.into(),
                        json: v.to_string(),
                        position: 0,
                    })
                    .into(),
                });
                assert_eq!(
                    super::github_available(&rows, "host", "pi"),
                    ready && github
                );
                assert!(!super::github_available(&rows, "host", "other"));
            }
        }
    }
    #[gpui::test]
    fn long_pending_draft_scrolls_without_displacing_editor(cx: &mut gpui::TestAppContext) {
        struct Probe(ScrollHandle);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(div().flex_1().min_h_0().debug_selector(|| "chat".into()))
                    .child(
                        div().relative().flex_shrink_0().child(
                            pending_panel("test-drafts".into(), &self.0)
                                .child("long pending draft\n".repeat(200))
                                .child(div().h(px(32.)).child("入力欄へ戻す")),
                        ),
                    )
                    .child(div().h(rems(6.)).debug_selector(|| "editor".into()))
            }
        }
        let (view, w) = cx.add_window_view(|_, _| Probe(ScrollHandle::new()));
        w.simulate_resize(size(px(600.), px(400.)));
        w.run_until_parked();
        let scroller = view.read_with(w, |v, _| v.0.clone());
        let area = scroller.bounds();
        let editor = w.debug_bounds("editor").unwrap();
        assert!(area.size.height > px(0.) && area.size.height <= px(120.));
        assert!(
            w.debug_bounds("chat").unwrap().size.height > px(0.) && editor.bottom() <= px(400.)
        );
        assert!(scroller.max_offset().height > px(1000.));
        scroller.set_offset(point(px(0.), -scroller.max_offset().height));
        view.update(w, |_, cx| cx.notify());
        w.run_until_parked();
        let restore = scroller.bounds_for_item(1).unwrap();
        assert!(
            restore.top() + scroller.offset().y >= area.top()
                && restore.bottom() + scroller.offset().y <= area.bottom() + px(1.)
        );
        assert_eq!(editor, w.debug_bounds("editor").unwrap());
    }
}
