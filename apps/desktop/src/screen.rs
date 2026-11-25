use super::*;
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{Enter, Input},
    scroll::ScrollableElement,
};
use serde_json::Value;

pub fn transcript() -> ListState {
    ListState::new(0, ListAlignment::Top, px(100.))
}
struct Colors {
    bg: Hsla,
    surface: Hsla,
    ink: Hsla,
    secondary: Hsla,
    border: Hsla,
    accent: Hsla,
    bubble: Hsla,
    waiting: Hsla,
}
impl Colors {
    fn new(dark: bool) -> Self {
        let c = |light, d| rgb(if dark { d } else { light }).into();
        Self {
            bg: c(0xffffff, 0x131618),
            surface: c(0xf1f3f4, 0x1e2326),
            ink: c(0x1f2326, 0xe4e8ea),
            secondary: c(0x5e686d, 0xa8b2b8),
            border: c(0xd6dde1, 0x353d41),
            accent: c(0x1b6b91, 0x88c8e6),
            bubble: c(0xdfe7ec, 0x293a43),
            waiting: c(0x936000, 0xf1bd68),
        }
    }
    fn primary(&self, id: &'static str, cx: &App) -> Button {
        Button::new(id).custom(
            ButtonCustomVariant::new(cx)
                .color(self.accent)
                .foreground(self.bg)
                .hover(self.accent.opacity(0.9))
                .active(self.accent.opacity(0.8)),
        )
    }
    fn caption(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(rems(0.78))
            .text_color(self.secondary)
            .child(text.into())
    }
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
impl Desktop {
    fn pi_row(
        &self,
        h: &Host,
        id: &str,
        v: &Value,
        waiting: bool,
        p: &Colors,
        cx: &Context<Self>,
    ) -> Button {
        let (host, pi) = (h.id.clone(), id.to_owned());
        let failed = v["failureText"].as_str().is_some_and(|s| !s.is_empty());
        let state = if failed {
            "失敗"
        } else if waiting {
            "入力待ち"
        } else if v["status"] == "running" {
            "作業中"
        } else {
            "待機"
        };
        let origin = match v["origin"].as_str() {
            Some("mobile") => "スマホ",
            Some("grok-bot") => "Grok Bot",
            _ => "PC",
        };
        let detail = if waiting {
            format!("{state} · {} · {origin}", h.name)
        } else {
            format!("{state} · {origin} で起動")
        };
        let selected = self
            .selected
            .as_ref()
            .is_some_and(|(a, b)| a == &host && b == &pi);
        Button::new(SharedString::from(format!("pi-{host}-{pi}")))
            .ghost()
            .selected(selected)
            .w_full()
            .h(rems(3.2))
            .tooltip(format!("{} · {detail}", v["cwd"].as_str().unwrap_or(id)))
            .justify_start()
            .px_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .max_w(rems(13.))
                    .child(div().size(px(7.)).rounded_full().bg(if waiting {
                        p.waiting
                    } else if selected || v["status"] == "running" {
                        p.accent
                    } else {
                        p.secondary
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(pi_title(v, id).to_owned()),
                            )
                            .child(p.caption(detail).truncate()),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select(host.clone(), pi.clone(), cx)))
    }
    fn sidebar(&self, p: &Colors, cx: &Context<Self>) -> Div {
        let mut rows = div()
            .id("machine-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2();
        let mut any_waiting = false;
        for h in &self.hosts {
            if let Some(m) = self.machines.get(&h.id) {
                for (id, v) in m
                    .rows
                    .group("instance")
                    .iter()
                    .filter(|(_, v)| v["section"] == "waiting")
                {
                    if !any_waiting {
                        rows = rows.child(
                            p.caption("要対応")
                                .px_2()
                                .py_1()
                                .font_weight(FontWeight::SEMIBOLD),
                        );
                        any_waiting = true;
                    }
                    rows = rows.child(self.pi_row(h, id, v, true, p, cx));
                }
            }
        }
        for h in &self.hosts {
            let data = self.machines.get(&h.id).map(|m| &m.rows);
            rows = rows.child(
                div()
                    .mt_3()
                    .px_2()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::BOLD).child(h.name.clone()))
                    .child(p.caption(connection(h, data))),
            );
            if let Some(data) = data {
                let entries = data.group("instance");
                if entries.is_empty() {
                    rows = rows.child(
                        p.caption(if data.get("host", "listKnown") == true {
                            "起動中の Pi はありません"
                        } else {
                            "一覧を取得しています…"
                        })
                        .p_2(),
                    );
                }
                for (id, v) in entries.iter().filter(|(_, v)| v["section"] != "waiting") {
                    rows = rows.child(self.pi_row(h, id, v, false, p, cx));
                }
            }
        }
        let mut side = div()
            .flex()
            .flex_col()
            .w(rems(15.5))
            .max_w(relative(0.4))
            .bg(p.surface)
            .border_r_1()
            .border_color(p.border)
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .child(
                        p.caption("PiMate")
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.accent),
                    )
                    .child(
                        div()
                            .text_size(rems(1.22))
                            .font_weight(FontWeight::BOLD)
                            .child("マシンと Pi"),
                    ),
            )
            .child(
                div().p_3().child(
                    Button::new("paste")
                        .w_full()
                        .label("リンクを貼り付ける")
                        .disabled(self.pairing)
                        .on_click(cx.listener(|this, _, _, cx| this.paste(cx))),
                ),
            );
        if let Some((_, name)) = &self.confirmation {
            side = side.child(
                div()
                    .mx_3()
                    .mb_2()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .bg(p.bg)
                    .rounded_lg()
                    .child(p.caption("接続先を確認"))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                    .child(
                        p.primary("pair", cx)
                            .label("この接続先を登録")
                            .on_click(cx.listener(|this, _, _, cx| this.pair(cx))),
                    )
                    .child(Button::new("cancel").ghost().label("キャンセル").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.confirmation = None;
                            cx.notify()
                        }),
                    )),
            );
        }
        let msg = if self.pairing {
            "登録・保存の結果を確認中…"
        } else {
            &self.notice
        };
        if !msg.is_empty() {
            side = side.child(p.caption(msg.to_owned()).px_3().pb_2());
        }
        side.child(rows).child(
            div()
                .flex()
                .gap_1()
                .p_2()
                .border_t_1()
                .border_color(p.border)
                .child(
                    Button::new("reload")
                        .small()
                        .ghost()
                        .label("登録を読み直す")
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                )
                .child(
                    Button::new("size")
                        .small()
                        .ghost()
                        .label(format!("文字 {}", self.size))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.size = match this.size as u8 {
                                16 => 19.,
                                19 => 14.,
                                _ => 16.,
                            };
                            this.scroll.reset(this.scroll.item_count());
                            cx.notify()
                        })),
                ),
        )
    }
}
impl Render for Desktop {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.bind_editor(w, cx);
        w.set_rem_size(px(self.size));
        let dark = matches!(
            w.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let p = Colors::new(dark);
        let side = self.sidebar(&p, cx);
        let mut main = div().flex().flex_col().flex_1().min_w_0();
        if let Some(key) = self.editor() {
            let rows = &self.machines[&key.host].rows;
            let session = rows.get("view", "session");
            let info = rows.get("instance", &key.instance);
            let running = rows.get("view", "status") == "running";
            let blocked =
                !rows.group("question").is_empty() || !rows.get("view", "pcPrompt").is_null();
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
                    .child(p.caption(status).text_color(if blocked {
                        p.waiting
                    } else {
                        p.accent
                    })),
            );
            if let Some(rev) = rows.pending_move(&key, &self.drafts) {
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
            let entries = rows.group("entry");
            let n = entries.len()
                + usize::from(
                    rows.get("stream", "text")
                        .as_str()
                        .is_some_and(|s| !s.is_empty()),
                );
            // Top-align short answers; follow growing content only while the reader is at the end.
            let reader = w.use_state(cx, |_, cx| cx.focus_handle()).read(cx).clone();
            let at_end = self.scroll.max_offset_for_scrollbar().height
                + self.scroll.scroll_px_offset_for_scrollbar().y
                < px(2.);
            let old = self.scroll.item_count();
            if old != n {
                self.scroll.splice(old.min(n)..old, n.saturating_sub(old));
            }
            if at_end && !reader.contains_focused(w, cx) {
                self.scroll.scroll_to(ListOffset {
                    item_ix: n,
                    offset_in_item: px(0.),
                });
            }
            if n == 0 {
                main = main.child(
                    div()
                        .flex_1()
                        .p_4()
                        .child(p.caption("まだメッセージはありません")),
                );
            } else {
                let desktop = cx.entity();
                let k = key.clone();
                let messages = w.use_keyed_state(
                    SharedString::from(format!("documents:{k:?}")),
                    cx,
                    |_, _| markdown::Messages::default(),
                );
                let list = list(self.scroll.clone(), move |i, w, cx| {
                    desktop.update(cx, |this, cx| {
                        let data = &this.machines[&k.host].rows;
                        let entries = data.group("entry");
                        let (id, text, role, stream) = if let Some((id, v)) = entries.get(i) {
                            (
                                format!("{k:?}/entry/{id}"),
                                v["text"].as_str().unwrap_or(""),
                                match v["kind"].as_str() {
                                    Some("user") => "あなた",
                                    Some("assistant") => "Pi",
                                    Some("tool_result") => "ツールの結果",
                                    _ => "作業",
                                },
                                false,
                            )
                        } else {
                            (
                                format!("{k:?}/stream/{:?}", entries.last().map(|e| &e.0)),
                                data.get("stream", "text").as_str().unwrap_or(""),
                                "Pi · 応答中",
                                true,
                            )
                        };
                        let p = Colors::new(dark);
                        let user = role == "あなた";
                        div()
                            .px_4()
                            .py_2()
                            .flex()
                            .flex_col()
                            .when(user, |v| v.items_end())
                            .child(p.caption(role).mb_1().font_weight(FontWeight::SEMIBOLD))
                            .child(
                                div()
                                    .when(user, |v| {
                                        v.max_w(relative(0.9))
                                            .rounded_lg()
                                            .bg(p.bubble)
                                            .px_3()
                                            .py_2()
                                    })
                                    .child(messages.update(cx, |cache, cx| {
                                        cache.view(id, text, stream, w, cx)
                                    })),
                            )
                            .into_any_element()
                    })
                })
                .size_full();
                main = main.child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .track_focus(&reader)
                        .child(list)
                        .vertical_scrollbar(&self.scroll),
                );
            }
            let mut chat = div().px_4();
            for (id, d) in rows.group("draft") {
                if d["instanceId"] == key.instance
                    && d["sessionId"].as_str() == key.session.as_deref()
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
                        let shown = key.clone();
                        let id = id.clone();
                        chat = chat.child(
                            Button::new(SharedString::from(format!("restore-{id}")))
                                .label("入力欄へ戻す")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.composing
                                        || this.sending
                                        || this.editor().as_ref() != Some(&shown)
                                        || this.drafts.get(&shown).is_some_and(|s| !s.is_empty())
                                    {
                                        return;
                                    }
                                    if let Ok(d) = this.machines[&shown.host]
                                        .conn
                                        .take_unconfirmed(shown.target(), id.clone())
                                    {
                                        this.drafts.insert(shown.clone(), d.text);
                                        this.input = None;
                                        cx.notify();
                                    }
                                })),
                        );
                    }
                }
            }
            main = main.child(chat);
            if let Some((_, input, _)) = &self.input {
                let movement = rows.pending_move(&key, &self.drafts).is_some();
                let disabled = blocked
                    || movement
                    || self.sending
                    || self.composing
                    || self.drafts.get(&key).is_none_or(|s| s.trim().is_empty());
                let shown = key.clone();
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
                                            this.send(shown.clone(), w, cx);
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
                                            this.send(key.clone(), w, cx)
                                        })),
                                ),
                        ),
                );
            }
        } else {
            let title = if self.hosts.is_empty() {
                "PC の Pi とつなぐ"
            } else {
                "会話を開く"
            };
            let description = self.selected.as_ref().and_then(|(host,pi)| self.machines.get(host).map(|m|m.rows.subscription(pi))).unwrap_or_else(|| {
                if self.hosts.is_empty() {"PC の Pi で /pimate setup を実行してリンクをコピーします。左の「リンクを貼り付ける」で接続先を確認してください。"} else {"左の一覧から Pi を選ぶと、この場所に会話が表示されます。"}.into()
            });
            main = main.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .child(
                        div()
                            .max_w(rems(24.))
                            .child(
                                p.caption("PiMate Desktop")
                                    .text_color(p.accent)
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .mb_2()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(rems(1.6))
                                    .child(title),
                            )
                            .child(div().text_color(p.secondary).child(description)),
                    ),
            );
        }
        div()
            .flex()
            .size_full()
            .bg(p.bg)
            .text_color(p.ink)
            .text_size(rems(0.94))
            .child(side)
            .child(main)
    }
}
