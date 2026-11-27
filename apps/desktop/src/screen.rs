mod chat_agents;
mod chat_area;
mod chat_commands;
pub(crate) mod chat_media;
mod chat_question;
mod chat_review;
pub(crate) mod context;
mod conversation;
mod first_step;
mod github;
mod inspector;
use inspector::Tab;
pub(crate) mod keys;
mod machine;
mod new_pi;
mod palette;
mod providers;
mod settings;
mod sheet_chat;
mod sheet_tree;
mod theme;
use super::*;
pub(crate) use chat_review::Inspector;
pub(crate) use conversation::History;
use gpui_component::{
    Disableable, Selectable, Sizable, TitleBar, WindowExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{Enter, Input},
    scroll::ScrollableElement,
};
use serde_json::Value;
pub(crate) use settings::Navigation;
use settings::Route;
pub(crate) struct Assets;
macro_rules! mobile_mark {
    ($name:literal) => {
        include_bytes!(concat!(
            "../../ios/App/Assets.xcassets/",
            $name,
            ".imageset/",
            $name,
            ".svg"
        ))
    };
}
// 同名のテーマ資産を差し替え、ブランドとウィンドウ装飾はそのまま使う。
const MARKS: &[(&str, &[u8])] = &[
    (
        "icons/search.svg",
        include_bytes!("screen/lucide/search.svg"),
    ),
    ("icons/bell.svg", include_bytes!("screen/lucide/bell.svg")),
    ("icons/side.svg", include_bytes!("screen/lucide/side.svg")),
    ("icons/copy.svg", include_bytes!("screen/lucide/copy.svg")),
    ("icons/diff.svg", include_bytes!("screen/lucide/diff.svg")),
    ("icons/x.svg", include_bytes!("screen/lucide/x.svg")),
    (
        "icons/circle-x.svg",
        include_bytes!("screen/lucide/circle-x.svg"),
    ),
    ("icons/grok-bot.svg", mobile_mark!("grok-bot")),
    ("icons/laptop.svg", mobile_mark!("lucide-monitor")),
    ("icons/gear.svg", mobile_mark!("lucide-settings")),
    ("icons/plus.svg", mobile_mark!("lucide-plus")),
];
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = MARKS.iter().find(|m| m.0 == path) {
            return Ok(Some((*bytes).into()));
        }
        theme::Assets.load(path)
    }
    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut paths = theme::Assets.list(path)?;
        paths.push("icons/grok-bot.svg".into());
        Ok(paths)
    }
}
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
    ) -> Div {
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
            match v["origin"].as_str() {
                Some("mobile") => detail += " · スマホで起動",
                Some("grok-bot") => detail += " · Grok Bot",
                _ => {}
            }
            (detail, p.secondary)
        };
        let pending = v["subagents"]["waiting"].as_u64().unwrap_or(0);
        let running = v["subagents"]["active"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(pending);
        let counts = [(running, "実行中"), (pending, "入力待ち")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, state)| format!("{state} {n}"))
            .collect::<Vec<_>>()
            .join(" · ");
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
        let row_host = host.clone();
        let row_pi = pi.clone();
        let start = v["startedAt"].as_u64();
        let new_nav = nav.clone();
        let open = cx.listener(move |this, _: &ClickEvent, _, cx| {
            if nav.read(cx).route != Route::Home && !this.navigate(&nav, Route::Home, cx) {
                return;
            }
            this.select(host.clone(), pi.clone(), cx);
        });
        let open = std::rc::Rc::new(open);
        let click = open.clone();
        let folder = v["cwd"].as_str().map(str::to_owned);
        let attach = v["abducoSession"].as_str().and_then(state::attach_command);
        let row = p
            .row(
                SharedString::from(format!("pi-{}-{id}", h.id)),
                selected,
                cx,
            )
            .h(px(theme::ROW + if counts.is_empty() { 0. } else { 18. }))
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
                    .when(v["origin"] == "grok-bot", |row| {
                        row.child(icon("grok-bot", 14., p.ink))
                    })
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
                            .when(!counts.is_empty(), |row| {
                                row.child(
                                    p.caption(format!("子: {counts}"))
                                        .text_color(if pending > 0 {
                                            p.waiting
                                        } else {
                                            p.secondary
                                        }),
                                )
                            })
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
            .on_click(move |e, w, cx| click(e, w, cx));
        let app = cx.weak_entity();
        context::on_right_click(
            div()
                .debug_selector(|| format!("context-pi-{id}"))
                .child(row),
            move |m| {
                let open = open.clone();
                let owner = app.clone();
                let host = row_host.clone();
                let n = new_nav.clone();
                let on_row = |label, shutdown| {
                    let owner = app.clone();
                    let host = row_host.clone();
                    let pi = row_pi.clone();
                    let nav = new_nav.clone();
                    gpui_component::menu::PopupMenuItem::new(label)
                        .disabled(start.is_none())
                        .on_click(move |_, w, cx| {
                            let _ = owner.update(cx, |app, cx| {
                                app.prepare_dialog(w, cx);
                                let Some(start) = start else { return };
                                if app.machines.get(&host).is_none_or(|m| {
                                    m.rows.get("instance", &pi)["startedAt"].as_u64() != Some(start)
                                }) {
                                    return;
                                }
                                if nav.read(cx).route != Route::Home
                                    && !app.navigate(&nav, Route::Home, cx)
                                {
                                    return;
                                }
                                if app.selected.as_ref() != Some(&(host.clone(), pi.clone())) {
                                    app.select(host.clone(), pi.clone(), cx);
                                }
                                if app.selected.as_ref() == Some(&(host.clone(), pi.clone())) {
                                    app.state.sheet = None;
                                    app.row_action =
                                        Some((host.clone(), pi.clone(), start, shutdown));
                                    cx.notify();
                                }
                            });
                        })
                };
                m.item(
                    gpui_component::menu::PopupMenuItem::new("開く")
                        .on_click(move |e, w, cx| open(e, w, cx)),
                )
                .when_some(folder.clone(), |m, path| {
                    m.item(context::copy("フォルダのパスをコピー", path))
                })
                .when_some(attach.clone(), |m, cmd| {
                    m.item(context::copy("pimate attach をコピー", cmd))
                })
                .when_some(folder.clone(), |m, path| {
                    m.item(
                        gpui_component::menu::PopupMenuItem::new("同じフォルダで新しい Pi…")
                            .on_click(move |_, w, cx| {
                                let _ = owner.update(cx, |d, cx| {
                                    d.prepare_dialog(w, cx);
                                    if d.navigate(&n, Route::New(host.clone()), cx) {
                                        n.update(cx, |s, _| s.initial_folder = Some(path.clone()));
                                    }
                                });
                            }),
                    )
                })
                .item(on_row("変更をレビュー", false))
                .item(on_row("この Pi を終了…", true))
            },
        )
    }
    /// サイドバー: 検索欄・要対応・マシンごと・最下段（設定／＋新しい Pi）。
    fn sidebar(&self, n: &Navigation, p: &Colors, cx: &Context<Self>) -> Div {
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
            .filter(|(_, _, v)| v["section"] == "waiting")
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
                    .filter(|(_, v)| v["section"] != "waiting" && v["section"] != "bot")
                {
                    rows = rows.child(self.pi_row(h, id, v, n, p, cx));
                }
                let mut bots = entries
                    .iter()
                    .filter(|(_, v)| v["section"] == "bot")
                    .peekable();
                if bots.peek().is_some() {
                    rows = rows.child(
                        group()
                            .debug_selector(|| "sidebar-bots".into())
                            .child(icon("grok-bot", 14., p.ink))
                            .child("Grok Bot"),
                    );
                    for (id, v) in bots {
                        rows = rows.child(self.pi_row(h, id, v, n, p, cx));
                    }
                }
            }
        }
        let go = |id: &'static str, route: Route, button: Button| {
            let n = n.clone();
            button
                .debug_selector(move || format!("nav-{id}"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.sidebar_open = false;
                    this.navigate(&n, route.clone(), cx);
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
                p.row("sidebar-search", false, cx)
                    .h(px(theme::CONTROL))
                    .child(icon("search", 16., p.secondary))
                    .label("Pi・コマンドを検索")
                    .on_click(cx.listener(|app, _, w, cx| app.open_palette(false, w, cx))),
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
            .on_click(cx.listener(|this, _, w, cx| this.command(keys::Command::Sidebar, w, cx)))
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
    fn pair_dialog(&self, p: &Colors, w: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        let (_, target) = self.confirmation.as_ref()?;
        let (cancel, accept) = w
            .use_keyed_state("pair-dialog-focus", cx, |_, cx| {
                (cx.focus_handle(), cx.focus_handle())
            })
            .read(cx)
            .clone();
        if !cancel.is_focused(w) && !accept.is_focused(w) {
            w.focus(&cancel);
        }
        let choices = (cancel.clone(), accept.clone());
        let confirm = accept.clone();
        let ring = |focus: &FocusHandle| {
            div().track_focus(focus).when(self.keyboard_focus, |d| {
                d.focus(|d| d.border_2().border_color(p.accent))
            })
        };
        let card = div()
            .key_context("PairDialog")
            .on_action(move |_: &keys::PairTab, w, cx| {
                w.focus(if choices.0.is_focused(w) {
                    &choices.1
                } else {
                    &choices.0
                });
                cx.stop_propagation();
            })
            .on_action(cx.listener(move |d, _: &keys::PairEnter, w, cx| {
                if confirm.is_focused(w) {
                    d.pair(cx);
                    d.restore_input(w, cx);
                } else {
                    d.confirmation = None;
                    d.restore_input(w, cx);
                    cx.notify()
                }
            }))
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
                        ring(&cancel).child(
                            p.text_button("cancel", p.secondary, cx)
                                .label("キャンセル")
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.confirmation = None;
                                    this.restore_input(w, cx);
                                    cx.notify()
                                })),
                        ),
                    )
                    .child(
                        ring(&accept).child(
                            p.primary("pair", cx)
                                .label("この接続先を登録")
                                .disabled(self.pairing)
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.pair(cx);
                                    this.restore_input(w, cx);
                                })),
                        ),
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
        p: &Colors,
        cx: &Context<Self>,
    ) -> (Div, Option<Div>) {
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
        let overlay = overlay.map(|side| {
            p.scrim()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, w, cx| {
                        this.sidebar_open = false;
                        this.restore_input(w, cx);
                        cx.notify()
                    }),
                )
                .child(
                    side.shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
                )
        });
        (panes, overlay)
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
        let p = Colors {
            keyboard_focus: self.keyboard_focus,
            ..Colors::new(dark)
        };
        self.sync_title(w);
        let layout = Layout::of(w.viewport_size().width);
        if self.search.is_none() {
            let input = cx.new(|cx| InputState::new(w, cx).placeholder("Pi・フォルダを検索"));
            let changed = cx.subscribe(&input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.palette_index = 0;
                    cx.notify()
                }
            });
            self.search = Some((input, changed));
        }
        self.navigation = Some(nav.clone());
        self.chat_keys = None;
        self.slash_visible = false;
        self.media_keys = None;
        // マシンが1台もないうちは、並べる Pi がないのでサイドバーを出さない。
        let side = (!self.hosts.is_empty() && (layout == Layout::Narrow || !self.sidebar_hidden))
            .then(|| {
                self.sidebar(&nav, &p, cx)
                    .track_focus(&self.side_focus)
                    .when(self.keyboard_focus, |d| {
                        d.focus(|d| d.border_2().border_color(p.accent))
                    })
            });
        let toggle =
            (side.is_some() && layout == Layout::Narrow).then(|| self.sidebar_toggle(&p, cx));
        // The answer belongs to the pending question, not to the visible workspace panel.
        let editor = self.editor();
        if editor.as_ref() != self.history.as_ref().map(|h| &h.key) {
            self.history = editor.clone().map(|k| History::new(k, cx));
        }
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
        let mut main = div()
            .track_focus(&self.main_focus)
            .when(self.keyboard_focus, |d| {
                d.focus(|d| d.border_2().border_color(p.accent))
            })
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full();
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
        let is_new = matches!(nav.read(cx).route, Route::New(_));
        let panel = self.other_panel(&nav, &p, w, cx);
        let (new_pi, panel) = if is_new { (panel, None) } else { (None, panel) };
        if let Some(panel) = panel {
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
        if w.focused(cx).is_none() {
            w.focus(&self.root_focus);
        }
        let root = div().track_focus(&self.root_focus).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.keyboard_focus {
                    this.keyboard_focus = false;
                    cx.notify();
                }
            }),
        );
        let (panes, mut sidebar) = self.panes(layout, side, main, &p, cx);
        let mut new_pi = new_pi.map(|panel| {
            p.scrim().items_center().justify_center().child(
                div()
                    .id("new-pi-dialog")
                    .track_focus(&self.modal_focus)
                    .occlude()
                    .flex()
                    .flex_col()
                    .w(px(560.))
                    .max_w_full()
                    .h(relative(0.86))
                    .bg(p.surface)
                    .rounded(px(theme::R_CARD))
                    .shadow_lg()
                    .overflow_hidden()
                    .child(panel),
            )
        });
        let stack = self.overlay_stack(w, cx).clone();
        let mut root = keys::listeners(root, cx)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(p.bg)
            .text_color(p.ink)
            .text_size(rems(0.875))
            .children(titlebar)
            .child(panes);
        for layer in stack {
            use keys::Overlay::*;
            let panel = match layer {
                Sheet => self
                    .editor()
                    .map(|k| self.sheet_overlay(&k, &p, w, cx).into_any_element()),
                Sidebar => sidebar.take().map(IntoElement::into_any_element),
                Route => new_pi.take().map(IntoElement::into_any_element),
                Palette => self
                    .command_palette(&p, cx)
                    .map(IntoElement::into_any_element),
                Pair => self
                    .pair_dialog(&p, w, cx)
                    .map(IntoElement::into_any_element),
                Menu => self.context_menu.as_ref().map(context::OpenMenu::layer),
                Prompt => Root::render_dialog_layer(w, cx).map(IntoElement::into_any_element),
                // Conversation-local controls are in panes; tooltip is painted by Window.
                Inspector | Media | Slash | Tooltip => None,
            };
            root = root.children(panel);
        }
        root
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
    pub(super) fn open_chat(
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
        use crate::state::FontSize;
        let (desktop, w, _socket) = open_chat(cx);
        // Desktop::new reads no settings file; every text size the user can choose is set here.
        // Test text is not measured with the real fonts: whether the labels fit is checked on screen.
        // debug_bounds keeps selectors from earlier frames, so judge by where the header starts.
        for (font, (width, docked)) in ["small", "standard", "large"].into_iter().flat_map(|font| {
            [(1280., true), (900., true), (620., false), (480., false)].map(|at| (font, at))
        }) {
            desktop.update(w, |d, cx| {
                d.state.settings.font_size = match font {
                    "small" => FontSize::Small,
                    "large" => FontSize::Large,
                    _ => FontSize::Standard,
                };
                cx.notify()
            });
            w.simulate_resize(size(px(width), px(800.)));
            w.run_until_parked();
            let at = format!("{font} {width}");
            // The header stays one line: fixed height, nothing wrapped below or cut off.
            let header = w.debug_bounds("chat-header").unwrap();
            assert_eq!(header.size.height, px(52.), "{at}");
            assert_eq!(header.left(), px(if docked { 264. } else { 0. }), "{at}");
            assert_eq!(header.right(), px(width), "{at}");
            let mut parts = vec!["chat-title", "nav-github"];
            if docked {
                let side = w.debug_bounds("sidebar").unwrap();
                assert_eq!((side.left(), side.right()), (px(0.), header.left()));
            } else {
                // Without room for its text, the state keeps a mark with a tooltip to hover.
                parts.extend(["sidebar-toggle", "chat-status"]);
            }
            for part in parts {
                let b = w.debug_bounds(part).unwrap();
                assert!(
                    b.top() >= header.top()
                        && b.bottom() <= header.bottom()
                        && b.left() >= header.left()
                        && b.right() <= header.right()
                        && b.size.width > px(0.),
                    "{part} at {at}: {b:?}"
                );
            }
            // The conversation keeps a 720px column (plus its 16px gutters) in the middle.
            let column = w.debug_bounds("chat-column").unwrap();
            assert_eq!(column.size.width, header.size.width.min(px(752.)), "{at}");
            assert_eq!(
                column.left() - header.left(),
                header.right() - column.right(),
                "{at}"
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
    fn opening_a_chat_keeps_the_reason_a_registration_failed(cx: &mut gpui::TestAppContext) {
        let (desktop, w, _socket) = open_chat(cx);
        for (message, kept) in [
            ("登録できません: expired", true),
            ("有効なペアリングリンクをコピーしてください", true),
            (crate::PAIRED, false),
        ] {
            desktop.update(w, |d, cx| {
                d.state.registration.pair_message = message.into();
                let host = d.hosts[0].id.clone();
                d.select(host, "pi".into(), cx);
                assert_eq!(
                    !d.state.registration.message().is_empty(),
                    kept,
                    "{message}"
                );
            });
        }
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

#[cfg(test)]
mod key_tests {
    use super::{Route, keys, state, tests};
    fn release(w: &mut gpui::VisualTestContext, key: &str) {
        w.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
        });
        w.run_until_parked();
    }
    fn press(w: &mut gpui::VisualTestContext, keys: &str) {
        for key in keys.split_whitespace() {
            w.simulate_keystrokes(key);
            release(w, key);
        }
    }
    fn wait(w: &mut gpui::VisualTestContext, selector: &'static str) -> gpui::Point<gpui::Pixels> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            w.run_until_parked();
            if let Some(b) = w.debug_bounds(selector) {
                break b.center();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{selector} did not appear"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[gpui::test]
    fn stage5_external_image_and_image_paste_both_attach(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, mut wire, _server) = attach_active(&app, w, "key-menu");
        let path = std::env::temp_dir().join(format!("{}.png", uuid::Uuid::new_v4()));
        image::RgbImage::new(3, 4).save(&path).unwrap();
        w.update(|w, cx| {
            app.update(cx, |d, cx| {
                let pg = d.chat_keys.as_ref().unwrap().0.clone();
                d.add_images(
                    &pg,
                    super::chat_media::ImageInput::Files(vec![path.clone()]),
                    w,
                    cx,
                );
            })
        });
        wait(w, "remove-image-0");
        w.update(|_, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                std::fs::read(&path).unwrap(),
            )))
        });
        w.update(|w, cx| {
            app.update(cx, |d, cx| {
                let pg = d.chat_keys.as_ref().unwrap().0.clone();
                d.add_images(
                    &pg,
                    super::chat_media::ImageInput::Clipboard(Box::new(|_| {
                        Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into())
                    })),
                    w,
                    cx,
                );
            })
        });
        w.run_until_parked();
        assert!(app.read_with(w, |d, _| !d.state.attachments.busy()));
        std::fs::remove_file(path).unwrap();
        assert!(w.update(|_, cx| cx.clipboard_image_reader().unwrap()(8).is_err()));
        press(w, "secondary-l secondary-v");
        wait(w, "remove-image-1");
        let pos = wait(w, "composer-input");
        w.simulate_mouse_down(pos, gpui::MouseButton::Right, Default::default());
        w.run_until_parked();
        assert!(app.read_with(w, |d, _| d.context_menu.is_some()));
        press(w, "up up enter");
        assert!(app.read_with(w, |d, _| d.context_menu.is_none()));
        wait(w, "remove-image-2");
        press(w, "secondary-l secondary-v");
        wait(w, "remove-image-3");
        w.update(|w, cx| {
            app.update(cx, |d, cx| {
                let pg = d.chat_keys.as_ref().unwrap().0.clone();
                d.add_images(
                    &pg,
                    super::chat_media::ImageInput::Clipboard(Box::new(|_| {
                        panic!("full composer must not read bytes")
                    })),
                    w,
                    cx,
                );
                assert!(!d.state.attachments.busy());
            })
        });
        assert!(wire.try_recv().is_err());
    }
    #[gpui::test]
    fn stage5_bot_group_excludes_waiting_and_answer_keeps_parent_draft(
        cx: &mut gpui::TestAppContext,
    ) {
        use pimate_core::{Change, Item};
        use serde_json::json;
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, mut wire, _server) = attach_active(&app, w, "key-menu");
        w.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(800.)));
        press(w, "secondary-l d");
        app.update(w,|d,cx|{let m=d.machines.get_mut("key-menu").unwrap();m.rows.apply((m.conn.snapshot().revision+1,[
            ("instance","bot",json!({"origin":"grok-bot","section":"bot"})),("instance","ask",json!({"origin":"grok-bot","section":"waiting"})),("run","run",json!({"children":[{"id":"unit","agent":"reader","state":"waiting","question":"OK?"}]}))
        ].map(|(c,id,v)|Change::Upsert(Item{collection:c.into(),id:id.into(),position:0,json:v.to_string()})).into()),||panic!());
        d.chat_keys.as_ref().unwrap().1.update(cx,|i,cx|{i.tab=Some(super::Tab::Agents);i.child=Some(state::Child{run:"run".into(),child:"unit".into()});cx.notify()});cx.notify();});
        let waiting = wait(w, "context-pi-ask");
        let group = wait(w, "sidebar-bots");
        let bot = wait(w, "context-pi-bot");
        assert!(waiting.y < group.y && group.y < bot.y);
        let pos = wait(w, "answer-child");
        w.simulate_click(pos, Default::default());
        w.run_until_parked();
        press(w, "x");
        assert_eq!(
            app.read_with(w, |d, cx| d.input.as_ref().unwrap().1.read(cx).value()),
            "reader（reader）の質問への回答: dx"
        );
        assert!(wire.try_recv().is_err());
    }
    #[gpui::test]
    fn x11_enter_repeat_cannot_submit_completed_command(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        w.simulate_keystrokes("secondary-l / m o d");
        wait(w, "command-choice-0");
        w.simulate_keystrokes("enter enter enter");
        app.read_with(w, |d, cx| {
            assert!(!d.sending && d.active_sheet().is_none());
            assert_eq!(
                d.input.as_ref().unwrap().1.read(cx).value().as_str(),
                "/model "
            );
        });
        release(w, "enter");
        press(w, "enter");
        assert!(app.read_with(w, |d, _| {
            d.active_sheet()
                .is_some_and(|s| s.kind == state::SheetPage::Model)
        }));
    }
    #[gpui::test]
    fn palette_preferences_over_narrow_sidebar(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        w.simulate_resize(gpui::size(gpui::px(640.), gpui::px(800.)));
        press(w, "secondary-b secondary-,");
        assert!(app.read_with(w, |d, cx| d.sidebar_open
            && d.navigation.as_ref().unwrap().read(cx).route == Route::Home));
        press(w, "secondary-k");
        w.simulate_input("設定");
        press(w, "enter");
        assert!(app.read_with(w, |d, cx| !d.sidebar_open
            && !d.palette_open
            && d.navigation.as_ref().unwrap().read(cx).route == Route::Settings));
    }
    #[gpui::test]
    fn palette_selects_on_keydown_and_discards_release(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        w.simulate_keystrokes("secondary-l d");
        let before = app.read_with(w, |d, _| d.size);
        w.simulate_keystrokes("secondary-k");
        w.update(|w, cx| {
            app.read(cx)
                .search
                .as_ref()
                .unwrap()
                .0
                .clone()
                .update(cx, |s, cx| s.set_value("文字を大きく", w, cx));
        });
        w.run_until_parked();
        w.simulate_keystrokes("enter");
        assert!(!app.read_with(w, |d, _| d.palette_open));
        assert!(app.read_with(w, |d, _| d.size > before
            && d.consumed_keys.contains_key("enter")));
        press(w, "down");
        release(w, "enter");
        assert!(!app.read_with(w, |d, _| d.consumed_keys.contains_key("enter")));
        app.read_with(w, |d, cx| {
            assert!(!d.palette_open && d.size > before && !d.sending && d.notice.is_empty());
            assert_eq!(d.input.as_ref().unwrap().1.read(cx).value().as_str(), "d");
        });
    }
    #[gpui::test]
    fn close_palette_with_space_held_returns_to_composer(cx: &mut gpui::TestAppContext) {
        use gpui::Focusable;
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        w.simulate_keystrokes("secondary-l d f6 secondary-k space escape");
        assert!(w.update(|w, cx| {
            app.read(cx)
                .input
                .as_ref()
                .unwrap()
                .1
                .focus_handle(cx)
                .is_focused(w)
        }));
        w.simulate_keystrokes("space");
        release(w, "space");
        app.read_with(w, |d, cx| {
            assert!(
                !d.sending
                    && d.notice.is_empty()
                    && d.input.as_ref().unwrap().1.read(cx).value().as_str() == "d"
            )
        });
    }
    #[gpui::test]
    fn palette_model_over_slash_picker_opens_model_sheet(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        w.simulate_keystrokes("secondary-l / secondary-k / m o d e l");
        assert!(app.read_with(w, |d, _| d.palette_open && d.slash_visible));
        press(w, "enter");
        assert!(app.read_with(w, |d, _| {
            d.active_sheet()
                .is_some_and(|s| s.kind == state::SheetPage::Model)
        }));
    }
    #[gpui::test]
    fn shutdown_dialog_blocks_background_clicks(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| {
            keys::init(cx);
        });
        let (_rt, mut wire, _server) = attach_active(&app, w, "key-menu");
        w.simulate_keystrokes("secondary-l d");
        let buttons = ["send-message", "abort-turn"].map(|s| w.debug_bounds(s).unwrap().center());
        w.update(|w, cx| {
            app.update(cx, |d, cx| {
                d.confirm_shutdown(&d.chat_keys.as_ref().unwrap().0.clone(), w, cx)
            })
        });
        w.run_until_parked();
        for button in buttons {
            assert!(w.update(|w, cx| app.read(cx).front(w, cx) == Some(keys::Overlay::Prompt)));
            w.simulate_click(button, Default::default());
        }
        assert!(w.update(gpui_component::WindowExt::has_active_dialog));
        press(w, "escape");
        assert!(!w.update(gpui_component::WindowExt::has_active_dialog));
        assert!(wire.try_recv().is_err());
        app.read_with(w, |d, cx| {
            assert!(
                !d.stopping
                    && !d.sending
                    && d.notice.is_empty()
                    && d.input.as_ref().unwrap().1.read(cx).value().as_str() == "d"
            )
        });
    }
    fn fixture_pi_row(app: &gpui::Entity<super::Desktop>, w: &mut gpui::VisualTestContext) {
        // この RC fixture は会話行だけを送る。実際のサイドバーと同じ instance 行も置く。
        app.update(w, |d, cx| {
            let m = d.machines.get_mut("key-menu").unwrap();
            m.rows.apply(
                (
                    m.conn.snapshot().revision + 1,
                    vec![pimate_core::Change::Upsert(pimate_core::Item {
                        collection: "instance".into(),
                        id: "pi".into(),
                        position: 0,
                        json: serde_json::json!({"startedAt":1,"cwd":"/fixture"}).to_string(),
                    })],
                ),
                || panic!(),
            );
            cx.notify()
        });
    }
    #[gpui::test]
    fn sidebar_menu_new_pi_tabs_only_in_dialog(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _, _server) = attach_active(&app, w, "key-menu");
        fixture_pi_row(&app, w);
        w.simulate_resize(gpui::size(gpui::px(620.), gpui::px(800.)));
        let button = wait(w, "sidebar-toggle");
        w.simulate_click(button, Default::default());
        let row = wait(w, "context-pi-pi");
        w.simulate_mouse_down(row, gpui::MouseButton::Right, Default::default());
        // Open, copy path, new Pi (attach is not present on this fixture).
        press(w, "down down down enter tab");
        assert!(w.update(|w, cx| {
            let d = app.read(cx);
            !d.sidebar_open
                && d.context_menu.is_none()
                && d.front(w, cx) == Some(keys::Overlay::Route)
                && d.modal_focus.contains_focused(w, cx)
        }));
        press(w, "escape");
        assert!(w.update(|w, cx| app.read(cx).front(w, cx).is_none()));
    }
    fn focus_explicit(w: &mut gpui::VisualTestContext) {
        // Tab order is window-scoped and the destructive action needs explicit focus.
        for _ in 0..2 {
            if w.update(|w, _| {
                w.context_stack()
                    .iter()
                    .any(|c| c.contains("ExplicitConfirmation"))
            }) {
                break;
            }
            press(w, "tab");
        }
        assert!(w.update(|w, _| {
            w.context_stack()
                .iter()
                .any(|c| c.contains("ExplicitConfirmation"))
        }));
    }
    #[gpui::test]
    fn stop_confirmation_does_not_follow_the_next_turn(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, mut wire, _server) = attach_active(&app, w, "abort-test");
        press(w, "escape");
        wait(w, "confirm-destructive");
        app.update(w, |d, cx| {
            let m = d.machines.get_mut("abort-test").unwrap();
            let changes = [false, true].map(|b| {
                pimate_core::Change::Upsert(pimate_core::Item {
                    collection: "view".into(),
                    id: "agentActive".into(),
                    position: 0,
                    json: b.to_string(),
                })
            });
            m.rows.apply(
                (m.conn.snapshot().revision + 1, changes.into()),
                || unreachable!(),
            );
            cx.notify();
        });
        focus_explicit(w);
        press(w, "enter");
        assert!(app.read_with(w, |d, _| !d.stopping && d.dialog_focus.borrow().is_none()));
        assert!(wire.try_recv().is_err());
    }
    #[gpui::test]
    fn release_in_shutdown_dialog_rearms_enter_and_default_is_cancel(
        cx: &mut gpui::TestAppContext,
    ) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (rt, mut wire, _server) = attach_active(&app, w, "key-menu");
        fixture_pi_row(&app, w);
        w.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(800.)));
        w.run_until_parked();
        for close in ["escape", "enter"] {
            let row = w.debug_bounds("context-pi-pi").unwrap().center();
            w.simulate_mouse_down(row, gpui::MouseButton::Right, Default::default());
            w.run_until_parked();
            assert!(app.read_with(w, |d, _| d.context_menu.is_some()));
            w.simulate_keystrokes("up enter");
            w.run_until_parked();
            assert!(app.read_with(w, |d, _| d.dialog_focus.borrow().is_some()
                && d.consumed_keys.contains_key("enter")));
            w.simulate_keystrokes("enter");
            assert!(app.read_with(w, |d, _| d.dialog_focus.borrow().is_some()));
            release(w, "enter");
            assert!(!app.read_with(w, |d, _| d.consumed_keys.contains_key("enter")));
            press(w, close);
            assert!(!app.read_with(w, |d, _| d.dialog_focus.borrow().is_some()));
            assert!(wire.try_recv().is_err());
        }
        // The session menu closes before the dialog records its return focus.
        let more = wait(w, "session-menu");
        w.simulate_click(more, Default::default());
        press(w, "up enter");
        wait(w, "confirm-destructive");
        w.simulate_keystrokes("space");
        focus_explicit(w);
        release(w, "space");
        assert!(w.update(gpui_component::WindowExt::has_active_dialog));
        assert!(wire.try_recv().is_err());
        press(w, "escape");
        assert!(w.update(|w, cx| {
            use gpui::Focusable;
            app.read(cx)
                .input
                .as_ref()
                .unwrap()
                .1
                .focus_handle(cx)
                .is_focused(w)
        }));
        w.update(|w, cx| {
            app.update(cx, |d, cx| {
                d.confirm_shutdown(&d.chat_keys.as_ref().unwrap().0.clone(), w, cx)
            })
        });
        w.run_until_parked();
        focus_explicit(w);
        press(w, "enter");
        assert_eq!(
            rt.block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(2), wire.recv()).await
            })
            .unwrap()
            .as_deref(),
            Some("shutdown")
        );
        assert!(wire.try_recv().is_err());
    }
    #[gpui::test]
    fn inspector_keeps_composer_and_marked_text(cx: &mut gpui::TestAppContext) {
        use gpui::EntityInputHandler;
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        w.simulate_keystrokes("secondary-l d");
        let input = app.read_with(w, |d, _| d.input.as_ref().unwrap().1.clone());
        w.update(|w, cx| {
            input.update(cx, |i, cx| {
                i.replace_and_mark_text_in_range(Some(1..1), "に", None, w, cx)
            })
        });
        for key in ["secondary-shift-d", "escape"] {
            press(w, key);
            assert!(w.debug_bounds("composer-input").is_some());
            assert!(app.read_with(w, |d, _| d.input.as_ref().unwrap().1 == input
                && d.drafts.values().any(|s| s == "d")));
            w.update(|w, cx| {
                input.update(cx, |i, cx| assert!(i.marked_text_range(w, cx).is_some()))
            });
        }
    }
    #[gpui::test]
    fn inspector_docks_only_at_1080(cx: &mut gpui::TestAppContext) {
        let (_, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        press(w, "secondary-shift-d");
        for width in [1080., 1079., 620.] {
            w.simulate_resize(gpui::size(gpui::px(width), gpui::px(800.)));
            w.run_until_parked();
            let pane = w.debug_bounds("inspector").unwrap();
            let header = w.debug_bounds("chat-header").unwrap();
            assert_eq!(pane.right(), gpui::px(width));
            assert_eq!(
                header.right(),
                if width >= 1080. {
                    pane.left()
                } else {
                    pane.right()
                }
            );
        }
    }
    #[gpui::test]
    fn narrow_sidebar_owns_focus_and_enter(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        w.simulate_resize(gpui::size(gpui::px(640.), gpui::px(800.)));
        w.simulate_keystrokes("secondary-l d secondary-b");
        press(w, "enter");
        let mut stops = Vec::new();
        for _ in 0..64 {
            press(w, "tab");
            let f = w.update(|w, cx| {
                assert!(app.read(cx).side_focus.contains_focused(w, cx));
                w.focused(cx).unwrap()
            });
            if stops.first() == Some(&f) {
                break;
            }
            stops.push(f);
        }
        assert!((2..64).contains(&stops.len()));
        press(w, "shift-tab");
        assert!(w.update(|w, _| stops.last().unwrap().is_focused(w)));
        app.read_with(w, |d, cx| {
            assert!(
                d.sidebar_open
                    && !d.sending
                    && d.notice.is_empty()
                    && d.input.as_ref().unwrap().1.read(cx).value().as_str() == "d"
            )
        });
        press(w, "escape");
        assert!(!app.read_with(w, |d, _| d.sidebar_open));
    }
    #[gpui::test]
    fn tooltip_owns_escape_before_turn_stop(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (rt, mut wire, server) = attach_active(&app, w, "key-menu");
        w.simulate_resize(gpui::size(gpui::px(640.), gpui::px(800.)));
        w.simulate_keystrokes("secondary-l");
        let pos = w.debug_bounds("chat-status").unwrap().center();
        w.simulate_mouse_move(pos, None, Default::default());
        w.run_until_parked();
        w.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        w.run_until_parked();
        assert!(w.update(|w, _| w.tooltip_visible()));
        w.simulate_keystrokes("a space space");
        release(w, "space");
        press(w, "b");
        assert!(w.update(|w, _| w.tooltip_visible()));
        assert_eq!(
            app.read_with(w, |d, cx| d
                .input
                .as_ref()
                .unwrap()
                .1
                .read(cx)
                .value()
                .to_string()),
            "a  b"
        );
        w.simulate_keystrokes("escape");
        for is_held in [false, true] {
            w.simulate_event(gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse("escape").unwrap(),
                is_held,
            });
        }
        release(w, "escape");
        assert!(!w.update(|w, _| w.tooltip_visible()));
        assert!(wire.try_recv().is_err());
        assert!(!app.read_with(w, |d, _| d.stopping));
        press(w, "escape");
        let yes = wait(w, "confirm-destructive");
        w.simulate_click(yes, Default::default());
        assert_eq!(
            rt.block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(3), wire.recv()).await
            })
            .unwrap()
            .as_deref(),
            Some("abort")
        );
        server.abort();
    }
    #[gpui::test]
    fn shortcuts_dispatch_and_palette_stacks_over_inspector(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let seen = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let record = seen.clone();
        w.update(|_, cx| {
            cx.observe_keystrokes(move |e, _, _| {
                *record.borrow_mut() = e
                    .action
                    .as_ref()
                    .map(|a| a.name().to_string())
                    .unwrap_or_default();
            })
            .detach()
        });
        for &(command, key, _) in keys::SHORTCUTS {
            w.update(|w, cx| {
                app.update(cx, |d, cx| {
                    let nav = d.navigation.clone().unwrap();
                    d.navigate(&nav, Route::Home, cx);
                    d.palette_open = false;
                    w.focus(&d.root_focus);
                })
            });
            w.run_until_parked();
            press(w, key);
            assert_eq!(*seen.borrow(), format!("pimate::{command:?}"), "{key}");
        }
        app.update(w, |d, cx| {
            d.selected = None;
            cx.notify()
        });
        w.simulate_keystrokes("secondary-1");
        assert_eq!(
            app.read_with(w, |d, _| d.selected.clone()),
            Some(("fixture".into(), "pi".into()))
        );
        w.simulate_keystrokes("f6");
        assert!(w.update(|w, cx| app.read(cx).side_focus.is_focused(w)));
        w.simulate_keystrokes("f6");
        assert!(w.update(|w, cx| app.read(cx).main_focus.is_focused(w)));
        w.simulate_keystrokes("secondary-shift-d secondary-k");
        assert!(app.read_with(w, |d, _| d.palette_open));
        w.simulate_keystrokes("down");
        assert_eq!(app.read_with(w, |d, _| d.palette_index), 1);
        press(w, "escape");
        assert!(app.read_with(w, |d, cx| !d.palette_open
            && d.chat_keys.as_ref().unwrap().1.read(cx).tab == Some(super::Tab::Review)));
        press(w, "escape");
        assert!(!app.read_with(w, |d, cx| {
            d.chat_keys.as_ref().unwrap().1.read(cx).tab == Some(super::Tab::Review)
        }));
    }
    #[gpui::test]
    fn focused_composer_replaced_by_review_still_receives_pane_keys(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        w.simulate_keystrokes("secondary-l secondary-shift-d f6");
        assert!(w.update(|w, cx| app.read(cx).side_focus.is_focused(w)));
        press(w, "escape");
        assert!(!app.read_with(w, |d, cx| {
            d.chat_keys.as_ref().unwrap().1.read(cx).tab == Some(super::Tab::Review)
        }));
    }
    #[gpui::test]
    fn right_click_other_pi_waits_for_subscription(cx: &mut gpui::TestAppContext) {
        use pimate_core::{Change, Item};
        use serde_json::json;
        let row = |c: &str, id: &str, v: serde_json::Value| {
            Change::Upsert(Item {
                collection: c.into(),
                id: id.into(),
                position: 1,
                json: v.to_string(),
            })
        };
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        app.update(w, |d, cx| {
            d.machines.get_mut("fixture").unwrap().rows.apply(
                (2, vec![row("instance", "second", json!({"startedAt":2}))]),
                || panic!(),
            );
            cx.notify();
        });
        w.run_until_parked();
        w.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(800.)));
        w.run_until_parked();
        let pos = w.debug_bounds("context-pi-second").unwrap().center();
        w.simulate_mouse_down(pos, gpui::MouseButton::Right, gpui::Modifiers::none());
        w.run_until_parked();
        press(w, "down down enter");
        assert_eq!(
            app.read_with(w, |d, _| d.selected.clone()).unwrap().1,
            "second"
        );
        app.update(w, |d, cx| {
            assert!(d.editor().is_none());
            d.machines.get_mut("fixture").unwrap().rows.apply(
                (
                    3,
                    vec![
                        row("view", "instanceId", json!("second")),
                        row("view", "startedAt", json!(2)),
                    ],
                ),
                || panic!(),
            );
            d.epoch += 1;
            cx.notify();
        });
        w.run_until_parked();
        assert!(app.read_with(w, |d, cx| {
            d.chat_keys.as_ref().unwrap().1.read(cx).tab == Some(super::Tab::Review)
        }));
    }
    fn fixture_message(
        app: &gpui::Entity<super::Desktop>,
        w: &mut gpui::VisualTestContext,
        raw: &str,
    ) {
        app.update(w, |d, cx| {
            d.machines.get_mut("fixture").unwrap().rows.apply(
                (
                    2,
                    vec![pimate_core::Change::Upsert(pimate_core::Item {
                        collection: "entry".into(),
                        id: "msg".into(),
                        position: 0,
                        json: serde_json::json!({"kind":"assistant","text":raw}).to_string(),
                    })],
                ),
                || panic!(),
            );
            cx.notify();
        });
        w.run_until_parked();
    }
    #[gpui::test]
    fn model_and_inspector_shortcuts_cannot_cover_a_table_viewer(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        fixture_message(&app, w, "| A | B |\n|---|---|\n| 1 | 2 |");
        let table = wait(w, "open-markdown-table");
        w.simulate_click(table, Default::default());
        wait(w, "close-media");
        press(
            w,
            "secondary-shift-m secondary-shift-d secondary-shift-s secondary-shift-g",
        );
        assert!(w.update(|w, cx| {
            let d = app.read(cx);
            d.chat_keys.is_none()
                && d.front(w, cx) == Some(keys::Overlay::Media)
                && d.active_sheet().is_none()
        }));
        press(w, "escape");
        assert!(w.update(|w, cx| app.read(cx).front(w, cx).is_none()));
    }
    fn open_github_table_detail(
        app: &gpui::Entity<super::Desktop>,
        w: &mut gpui::VisualTestContext,
    ) -> super::Navigation {
        app.update(w, |d, cx| {
            d.select_github(pimate_protocol::GitHubQuery::Pr { number: 93 }, cx);
            let github = d.chat_keys.as_ref().unwrap().1.read(cx).github.clone().unwrap();
            github.update(cx, |n, _| {
                n.github.loaded = true;
                n.github.data = Some(serde_json::json!({"item": {"kind":"pr", "number":93, "title":"detail", "body":"| A | B |\n|---|---|\n| 1 | 2 |"}}));
            });
            github
        })
    }
    #[gpui::test]
    fn sidebar_review_replaces_open_github_viewer_without_invalidating_its_page(
        cx: &mut gpui::TestAppContext,
    ) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        fixture_pi_row(&app, w);
        w.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(800.)));
        let github = open_github_table_detail(&app, w);
        let table = wait(w, "open-markdown-table");
        w.simulate_click(table, Default::default());
        wait(w, "close-media");
        let row = wait(w, "context-pi-pi");
        w.simulate_mouse_down(row, gpui::MouseButton::Right, Default::default());
        press(w, "up up enter"); // The review entry precedes shutdown.
        assert!(w.update(|w, cx| {
            let d = app.read(cx);
            d.front(w, cx) == Some(keys::Overlay::Inspector)
                && d.chat_keys
                    .as_ref()
                    .is_some_and(|(_, ins)| ins.read(cx).tab == Some(super::Tab::Review))
        }));
        app.read_with(w, |d, cx| {
            let n = github.read(cx);
            assert!(n.page.as_ref().unwrap().visible(d));
            assert!(n.github.viewer.as_ref().unwrap().read(cx).table.is_none());
        });
    }
    #[gpui::test]
    fn new_pi_keeps_github_detail_and_review_target(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        let github = open_github_table_detail(&app, w);
        let (page, ins) = app.read_with(w, |d, _| d.chat_keys.as_ref().unwrap().clone());
        press(w, "secondary-n");
        let modal = app.read_with(w, |d, cx| {
            d.navigation
                .as_ref()
                .unwrap()
                .read(cx)
                .page
                .clone()
                .unwrap()
        });
        press(w, "escape");
        app.read_with(w, |d, cx| {
            assert!(github.read(cx).page.as_ref().unwrap().visible(d) && !modal.visible(d));
            assert!(ins.read(cx).github.as_ref() == Some(&github));
        });
        let table = wait(w, "open-markdown-table");
        w.simulate_click(table, Default::default());
        wait(w, "close-media");
        press(w, "secondary-shift-d escape");
        assert!(w.update(
            |w, cx| app.read(cx).front(w, cx) == Some(keys::Overlay::Inspector)
                && ins.read(cx).tab == Some(super::Tab::Github)
        ));
        let mut review = page.review(None).unwrap();
        review.files.push(state::review::ChangedFile {
            path: "review.txt".into(),
            pieces: vec![],
        });
        ins.update(w, |i, cx| {
            i.tab = Some(super::Tab::Review);
            i.review = Some(std::sync::Arc::new(review));
            cx.notify();
        });
        press(w, "secondary-n escape");
        let ask = wait(w, "ask-changes");
        w.simulate_click(ask, Default::default());
        w.run_until_parked();
        assert!(app.read_with(w, |d, cx| {
            d.input
                .as_ref()
                .unwrap()
                .1
                .read(cx)
                .value()
                .contains("review.txt")
        }));
    }
    #[gpui::test]
    fn pending_spawn_rejects_palette_pi_without_selecting(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        press(w, "secondary-n");
        let epoch = app.update(w, |d, cx| {
            d.navigation
                .as_ref()
                .unwrap()
                .update(cx, |n, _| n.spawn.as_mut().unwrap().submitting = true);
            d.epoch
        });
        press(w, "secondary-k enter");
        assert!(app.read_with(w, |d, cx| {
            let n = d.navigation.as_ref().unwrap().read(cx);
            !d.palette_open
                && d.epoch == epoch
                && n.spawn.as_ref().unwrap().submitting
                && n.page.as_ref().unwrap().visible(d)
        }));
    }
    #[gpui::test]
    fn disconnected_spawn_host_can_tab_to_a_connected_host(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        app.update(w, |d, cx| {
            let mut missing = d.hosts[0].clone();
            missing.id = "offline".into();
            d.hosts.push(missing);
            let nav = d.navigation.clone().unwrap();
            d.navigate(&nav, Route::New("offline".into()), cx);
        });
        w.run_until_parked();
        press(w, "tab tab");
        app.update(w, |_, cx| cx.notify());
        w.run_until_parked();
        assert!(w.update(|w, cx| {
            let f = &app.read(cx).modal_focus;
            f.contains_focused(w, cx) && !f.is_focused(w)
        }));
        press(w, "enter");
        assert!(
            app.read_with(w, |d, cx| d.navigation.as_ref().unwrap().read(cx).route
                == Route::New("fixture".into()))
        );
    }
    #[gpui::test]
    fn right_click_copies_raw_message_not_rendered_selection(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let raw = "**太字** と [リンク](https://example.org)\n\n2 行目";
        fixture_message(&app, w, raw);
        let bounds = w.debug_bounds("message-entry:msg").unwrap();
        w.simulate_mouse_down(
            bounds.center(),
            gpui::MouseButton::Right,
            gpui::Modifiers::none(),
        );
        w.run_until_parked();
        assert!(app.read_with(w, |app, _| app.context_menu.is_some()));
        w.simulate_keystrokes("down space");
        assert!(app.read_with(w, |app, _| app.context_menu.is_none()));
        release(w, "space");
        assert_eq!(
            w.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()))
                .as_deref(),
            Some(raw)
        );
    }
    fn attach_active(
        app: &gpui::Entity<super::Desktop>,
        w: &mut gpui::VisualTestContext,
        id: &'static str,
    ) -> (
        tokio::runtime::Runtime,
        tokio::sync::mpsc::UnboundedReceiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (mut m, rx, wire, server) = rt.block_on(state::fixture(id));
        m.conn.subscribe(Some("pi".into())).unwrap();
        rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !m.rows.can_stop() {
                    m.rows.apply(rx.recv().await.unwrap(), || m.conn.snapshot());
                }
            })
            .await
            .unwrap();
        });
        app.update(w, |d, cx| {
            d.hosts = vec![m.registration.clone()];
            d.selected = Some((m.registration.id.clone(), "pi".into()));
            d.machines.insert(m.registration.id.clone(), m);
            cx.notify();
        });
        w.run_until_parked();
        (rt, wire, server)
    }
    #[gpui::test]
    fn review_return_keeps_the_readers_row_and_offset(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        app.update(w, |d, cx| {
            let rows = (0..100)
                .map(|i| {
                    pimate_core::Change::Upsert(pimate_core::Item {
                        collection: "entry".into(),
                        id: format!("u{i}"),
                        position: i,
                        json: serde_json::json!({"kind":"user","text":format!("質問 {i}")})
                            .to_string(),
                    })
                })
                .collect();
            d.machines
                .get_mut("fixture")
                .unwrap()
                .rows
                .apply((2, rows), || unreachable!());
            cx.notify();
        });
        w.run_until_parked();
        let list = app.read_with(w, |d, _| d.history.as_ref().unwrap().scroll.clone());
        list.scroll_to(gpui::ListOffset {
            item_ix: 23,
            offset_in_item: gpui::px(5.),
        });
        app.update(w, |_, cx| cx.notify());
        w.run_until_parked();
        let before = list.logical_scroll_top();
        assert_eq!(before.item_ix, 23);
        press(w, "secondary-shift-d");
        assert!(app.read_with(w, |d, cx| {
            d.chat_keys.as_ref().unwrap().1.read(cx).tab == Some(super::Tab::Review)
        }));
        press(w, "escape");
        assert!(w.debug_bounds("composer-input").is_some());
        let after = app.read_with(w, |d, _| {
            d.history.as_ref().unwrap().scroll.logical_scroll_top()
        });
        assert_eq!(
            (after.item_ix, after.offset_in_item),
            (before.item_ix, before.offset_in_item)
        );
    }
    #[gpui::test]
    fn composer_sheet_buttons_accept_focused_enter_and_space(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, _wire, _server) = attach_active(&app, w, "key-menu");
        for (kind, tabs) in [(state::SheetPage::Model, 2), (state::SheetPage::Usage, 3)] {
            for key in ["enter", "space"] {
                press(w, "secondary-l");
                // The dismissed palette still owns this release, even after focus moves.
                w.simulate_keystrokes("secondary-k space");
                press(w, "escape");
                assert!(app.read_with(w, |d, _| !d.palette_open
                    && d.consumed_keys.contains_key("space")));
                for _ in 0..tabs {
                    w.update(|w, _| w.focus_next());
                    w.run_until_parked();
                }
                release(w, "space");
                assert!(app.read_with(w, |d, _| d.active_sheet().is_none()));
                press(w, key);
                assert!(
                    app.read_with(w, |d, _| d.active_sheet().map(|s| s.kind)) == Some(kind),
                    "{key}"
                );
                press(w, "escape");
            }
        }
    }
    #[gpui::test]
    fn multi_choice_tab_stops_on_group_and_space_toggles_only_cursor(
        cx: &mut gpui::TestAppContext,
    ) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let question = serde_json::json!({"kind":"questionnaire","questions":[{"multiSelect":true,"options":["A","B"]}]});
        app.update(w, |d, cx| {
            let row = pimate_core::Item {
                collection: "question".into(),
                id: "q".into(),
                position: 0,
                json: question.to_string(),
            };
            d.machines.get_mut("fixture").unwrap().rows.apply(
                (2, vec![pimate_core::Change::Upsert(row)]),
                || unreachable!(),
            );
            cx.notify();
        });
        w.run_until_parked();
        let pos = w.debug_bounds("choice-0-0-false").unwrap().center();
        w.simulate_click(pos, Default::default());
        press(w, "tab");
        assert!(w.update(|w, _| w.context_stack().iter().any(|c| c.contains("Input"))));
        press(w, "shift-tab down space");
        assert!(w.debug_bounds("choice-0-0-true").is_some());
        assert!(w.debug_bounds("choice-0-1-true").is_some());
        press(w, "space");
        assert!(w.debug_bounds("choice-0-0-true").is_some());
        assert!(w.debug_bounds("choice-0-1-false").is_some());
    }
    #[gpui::test]
    fn question_keys_send_the_selected_option_once(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (rt, mut wire, server) = attach_active(&app, w, "question-keys");
        let pos = w.debug_bounds("choice-0-0-false").unwrap().center();
        w.simulate_click(pos, Default::default());
        press(w, "3 up down 2");
        assert!(w.debug_bounds("choice-0-1-true").is_some());
        w.simulate_keystrokes("secondary-enter secondary-enter");
        w.run_until_parked();
        assert_eq!(
            rt.block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(2), wire.recv())
                    .await
                    .unwrap()
                    .unwrap()
            }),
            "ui_respond"
        );
        assert!(wire.try_recv().is_err());
        server.abort();
    }
    #[gpui::test]
    fn queued_steer_has_no_undo_and_only_unconfirmed_text_returns_to_input(
        cx: &mut gpui::TestAppContext,
    ) {
        use pimate_core::Operation;
        let (app, w, _) = tests::open_chat(cx);
        let (rt, mut wire, server) = attach_active(&app, w, "key-menu");
        let (conn, target) = app.read_with(w, |app, _| {
            (
                app.machines["key-menu"].conn.clone(),
                app.editor().unwrap().target(),
            )
        });
        let refresh = |app: &mut super::Desktop, cx: &mut gpui::Context<super::Desktop>| {
            app.machines.get_mut("key-menu").unwrap().rows = super::Rows::snapshot(conn.snapshot());
            cx.notify()
        };
        rt.block_on(conn.perform(Operation::Send {
            target: target.clone(),
            text: "保持した指示".into(),
            images: vec![],
        }))
        .unwrap();
        app.update(w, refresh);
        w.run_until_parked();
        assert_eq!(wire.try_recv().unwrap(), "prompt");
        assert_eq!(
            app.read_with(w, |app, _| app.machines["key-menu"].rows.group("draft")[0]
                .1["status"]
                .clone()),
            "queued"
        );
        assert!(w.debug_bounds("restore-draft").is_none());
        // Simulate a separately requested stop, not a cancel button on the draft.
        rt.block_on(conn.perform(Operation::Abort { target }))
            .unwrap();
        app.update(w, refresh);
        w.run_until_parked();
        assert_eq!(wire.try_recv().unwrap(), "abort");
        let pos = w.debug_bounds("restore-draft").unwrap().center();
        w.simulate_click(pos, Default::default());
        w.run_until_parked();
        assert_eq!(
            app.read_with(w, |app, cx| app.input.as_ref().unwrap().1.read(cx).value()),
            "保持した指示"
        );
        assert!(wire.try_recv().is_err());
        server.abort();
    }
    #[gpui::test]
    fn pairing_confirmation_keeps_enter_off_the_composer(cx: &mut gpui::TestAppContext) {
        let (app, w, _) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        w.simulate_keystrokes("secondary-l d");
        app.update(w, |d, cx| {
            d.confirmation = Some(("unused".into(), "demo".into()));
            cx.notify();
        });
        w.run_until_parked();
        press(w, "tab shift-tab enter");
        assert!(app.read_with(w, |d, cx| d.confirmation.is_none()
            && !d.pairing
            && d.input.as_ref().unwrap().1.read(cx).value().as_str() == "d"));
    }
    #[gpui::test]
    fn composer_edit_menu_consumes_enter_and_escape(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (_rt, mut wire, server) = attach_active(&app, w, "key-menu");
        w.simulate_keystrokes("secondary-l d r a f t");
        let pos = w.debug_bounds("composer-input").unwrap().center();
        w.simulate_mouse_down(pos, gpui::MouseButton::Right, gpui::Modifiers::none());
        w.run_until_parked();
        assert!(app.read_with(w, |d, _| d.context_menu.is_some()));
        let menu = w.update(|w, cx| w.focused(cx).unwrap());
        for key in ["shift-tab", "tab", "shift-tab"] {
            press(w, key);
            assert!(w.update(|w, _| menu.is_focused(w)));
        }
        press(w, "enter escape");
        assert!(app.read_with(w, |d, cx| d.context_menu.is_none()
            && d.notice.is_empty()
            && d.input.as_ref().unwrap().1.read(cx).value().as_str() == "draft"));
        assert!(wire.try_recv().is_err());
        server.abort();
    }
    #[gpui::test]
    fn escape_dismisses_slash_only(cx: &mut gpui::TestAppContext) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (rt, mut wire, server) = attach_active(&app, w, "key-menu");
        w.simulate_keystrokes("secondary-l /");
        assert!(app.read_with(w, |d, _| d.slash_visible));
        press(w, "escape");
        assert!(app.read_with(w, |d, cx| !d.slash_visible
            && d.input.as_ref().unwrap().1.read(cx).value().as_str() == "/"));
        press(w, "escape");
        let yes = wait(w, "confirm-destructive");
        w.simulate_click(yes, Default::default());
        rt.block_on(async {
            for method in ["commands", "abort"] {
                assert_eq!(
                    tokio::time::timeout(std::time::Duration::from_secs(3), wire.recv())
                        .await
                        .unwrap()
                        .as_deref(),
                    Some(method)
                );
            }
        });
        assert!(wire.try_recv().is_err());
        server.abort();
    }
    #[gpui::test]
    fn active_turn_stops_once_and_commands_retry_into_keyboard_picker(
        cx: &mut gpui::TestAppContext,
    ) {
        let (app, w, _socket) = tests::open_chat(cx);
        w.update(|_, cx| keys::init(cx));
        let (rt, mut wire, server) = attach_active(&app, w, "abort-test");
        press(w, "escape");
        let yes = wait(w, "confirm-destructive");
        w.simulate_click(yes, Default::default());
        let first = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(3), wire.recv())
                .await
                .unwrap()
        });
        assert_eq!(first.as_deref(), Some("abort"));
        w.run_until_parked();
        assert!(wire.try_recv().is_err());
        // The button and Esc use the same guard and must each generate one request.
        let stop = wait(w, "abort-turn");
        w.simulate_click(stop, gpui::Modifiers::none());
        let yes = wait(w, "confirm-destructive");
        w.simulate_click(yes, Default::default());
        let second = rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(3), wire.recv())
                .await
                .unwrap()
        });
        assert_eq!(second.as_deref(), Some("abort"));
        w.simulate_keystrokes("secondary-l shift-enter");
        assert_eq!(
            app.read_with(w, |d, cx| d
                .input
                .as_ref()
                .unwrap()
                .1
                .read(cx)
                .value()
                .to_string()),
            "\n"
        );
        w.simulate_keystrokes("backspace /");
        let retry = wait(w, "retry-commands");
        w.simulate_click(retry, gpui::Modifiers::none());
        wait(w, "command-choice-0");
        w.simulate_keystrokes("secondary-l down up down tab");
        let before = app.read_with(w, |d, cx| {
            d.input.as_ref().unwrap().1.read(cx).value().to_string()
        });
        assert!(before.starts_with('/') && before.ends_with(' ') && before.trim().len() > 1);
        w.simulate_keystrokes("a r g");
        assert_eq!(
            app.read_with(w, |d, cx| d
                .input
                .as_ref()
                .unwrap()
                .1
                .read(cx)
                .value()
                .to_string()),
            format!("{before}arg")
        );
        assert_eq!(wire.try_recv().as_deref(), Ok("commands"));
        assert_eq!(wire.try_recv().as_deref(), Ok("commands"));
        assert!(wire.try_recv().is_err());
        server.abort();
    }
}
