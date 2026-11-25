use super::*;
use gpui_component::{Disableable, button::Button, input::Input};
impl Render for Desktop {
    fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.bind_editor(w, cx);
        w.set_rem_size(px(self.size));
        let dark = matches!(
            w.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let (bg, surf, ink) = if dark {
            (0x131618, 0x1e2326, 0xe4e8ea)
        } else {
            (0xffffff, 0xf1f3f4, 0x1f2326)
        };
        let mut side = div()
            .id("sidebar")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .w(px(250.))
            .flex_shrink_0()
            .bg(rgb(surf))
            .child("マシンと Pi");
        for h in &self.hosts {
            side = side.child(h.name.clone());
            if let Some(m) = self.machines.get(&h.id) {
                side = side.child(
                    m.rows
                        .get("host", "connection")
                        .as_str()
                        .unwrap_or("接続中")
                        .to_owned(),
                );
                for (id, row) in m.rows.group("instance") {
                    let (host, pi) = (h.id.clone(), id.clone());
                    let label = ["label", "sessionName", "cwd"]
                        .iter()
                        .find_map(|k| row[k].as_str().filter(|s| !s.is_empty()))
                        .unwrap_or(id);
                    side = side.child(
                        Button::new(SharedString::from(format!("pi-{host}-{pi}")))
                            .label(label.to_owned())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select(host.clone(), pi.clone(), cx)
                            })),
                    );
                }
            } else {
                side = side.child(if h.needs_pairing {
                    "再登録が必要"
                } else {
                    "未接続"
                })
            }
        }
        side = side.child(
            Button::new("paste")
                .label("リンクを貼り付ける")
                .disabled(self.pairing || cfg!(not(any(target_os = "macos", target_os = "linux"))))
                .on_click(cx.listener(|this, _, _, cx| this.paste(cx))),
        );
        if let Some((_, name)) = &self.confirmation {
            side = side
                .child(name.clone())
                .child(
                    Button::new("pair")
                        .label("この接続先を登録")
                        .on_click(cx.listener(|this, _, _, cx| this.pair(cx))),
                )
                .child(
                    Button::new("cancel")
                        .label("キャンセル")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirmation = None;
                            cx.notify()
                        })),
                );
        }
        side = side
            .child(
                Button::new("reload")
                    .label("登録を読み直す")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(
                Button::new("size")
                    .label(format!("文字サイズ: {}", self.size))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.size = match this.size as u8 {
                            16 => 19.,
                            19 => 14.,
                            _ => 16.,
                        };
                        cx.notify()
                    })),
            );
        // Follow new output only while the reader is at the end.
        if self.scroll.max_offset().height + self.scroll.offset().y < px(2.) {
            self.scroll.scroll_to_bottom();
        }
        let mut chat = div()
            .id("transcript")
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .p_3();
        let mut main = div().flex().flex_col().gap_2().flex_1().min_w_0().p_3();
        if let Some(key) = self.editor() {
            let rows = &self.machines[&key.host].rows;
            let session = rows.get("view", "session");
            let dest = session["name"]
                .as_str()
                .or(key.session.as_deref())
                .unwrap_or("新規セッション");
            main = main.child(format!("現在の送信先: {dest}"));
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
            for (id, v) in rows.group("entry") {
                if let Some(text) = v["text"].as_str() {
                    chat = chat
                        .child(
                            div()
                                .mt_3()
                                .font_weight(FontWeight::BOLD)
                                .child(v["kind"].as_str().unwrap_or("メッセージ").to_string()),
                        )
                        .child(markdown::view(
                            format!("{key:?}-entry-{id}"),
                            text,
                            false,
                            w,
                            cx,
                        ));
                }
            }
            if let Some(s) = rows.get("stream", "text").as_str() {
                chat = chat.child(markdown::view(format!("{key:?}-stream"), s, true, w, cx));
            }
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
        } else {
            chat = chat.child(
                self.selected
                    .as_ref()
                    .and_then(|(h, i)| self.machines.get(h).map(|m| m.rows.subscription(i)))
                    .unwrap_or_else(|| "接続後、左の一覧から Pi を選択してください。".into()),
            );
        }
        main = main.child(chat).child(self.notice.clone());
        if let Some((key, input, _)) = &self.input {
            let key = key.clone();
            main = main.child(Input::new(input).h(px(86.))).child(
                Button::new("send")
                    .label("送信")
                    .disabled(
                        self.sending
                            || self.machines[&key.host]
                                .rows
                                .pending_move(&key, &self.drafts)
                                .is_some()
                            || self.composing
                            || self.drafts.get(&key).is_none_or(|s| s.trim().is_empty()),
                    )
                    .on_click(cx.listener(move |this, _, w, cx| this.send(key.clone(), w, cx))),
            );
        }
        div()
            .flex()
            .size_full()
            .bg(rgb(bg))
            .text_color(rgb(ink))
            .text_size(rems(0.875))
            .child(side)
            .child(main)
    }
}
