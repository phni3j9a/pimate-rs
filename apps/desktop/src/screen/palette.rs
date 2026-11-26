use super::*;
use keys::{Command, SHORTCUTS};
#[derive(Clone)]
enum Choice {
    Pi(String, String),
    Slash(state::Page, String),
    App(Command),
}
impl Desktop {
    pub(super) fn open_palette(&mut self, shortcuts: bool, w: &mut Window, cx: &mut Context<Self>) {
        self.palette_return = w.focused(cx);
        self.palette_open = true;
        self.palette_index = 0;
        if let Some((s, _)) = &self.search {
            s.update(cx, |s, cx| {
                s.set_value(if shortcuts { ">" } else { "" }, w, cx);
                s.focus(w, cx)
            });
        }
        if let Some((page, _)) = &self.chat_keys
            && page.visible(self)
        {
            let pg = page.clone();
            cx.spawn(async move |app, cx| {
                let result = pg.sheet(state::SheetAction::Commands).await;
                let _ = app.update(cx, |app, cx| {
                    if app.palette_open && pg.visible(app) {
                        if let Err(e) = result {
                            app.notice = format!("コマンドを取得できません: {e}");
                        }
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }
    pub(super) fn close_palette(&mut self, w: &mut Window) {
        self.palette_open = false;
        w.focus(
            self.palette_return
                .take()
                .as_ref()
                .unwrap_or(&self.root_focus),
        );
    }
    fn choose(&mut self, choice: &Choice, w: &mut Window, cx: &mut Context<Self>) {
        self.close_palette(w);
        match choice {
            Choice::Pi(h, i) => {
                if let Some(nav) = self.navigation.clone() {
                    self.navigate(&nav, Route::Home, cx);
                }
                self.select(h.clone(), i.clone(), cx);
            }
            Choice::App(c) => self.command(*c, w, cx),
            Choice::Slash(pg, name) if pg.visible(self) => {
                if self.sending {
                    return;
                }
                let t = match pg.command_ticket(name) {
                    Ok(t) => t,
                    Err(e) => {
                        self.finish_send_notice(Err(e));
                        cx.notify();
                        return;
                    }
                };
                self.run_submission(pg, t, false, w, cx);
            }
            _ => {}
        }
        cx.notify();
    }
    pub(super) fn command_palette(&self, p: &Colors, cx: &Context<Self>) -> Option<Div> {
        if !self.palette_open {
            return None;
        }
        let input = &self.search.as_ref()?.0;
        let query = input.read(cx).value().trim().to_lowercase();
        let app_only = query.starts_with('>');
        let query = query.trim_start_matches('>').trim();
        let mut items = Vec::new();
        if !app_only {
            for (h, i, _) in self.pi_order() {
                let data = self.machines[&h].rows.get("instance", &i);
                let label = format!(
                    "{}　{} · {}",
                    pi_title(data, &i),
                    data["cwd"].as_str().unwrap_or(""),
                    self.machines[&h].registration.name
                );
                items.push(("Pi", label, Choice::Pi(h, i)));
            }
            if let Some((pg, _)) = &self.chat_keys {
                for (name, v) in self.machines[pg.host()].rows.group("command") {
                    items.push((
                        "この Pi のコマンド",
                        format!("/{name}　{}", v["description"].as_str().unwrap_or("")),
                        Choice::Slash(pg.clone(), name.clone()),
                    ));
                }
            }
        }
        for &(c, key, label) in SHORTCUTS {
            if c != Command::EscapeTop && c != Command::Search {
                items.push((
                    "アプリ",
                    format!(
                        "{label}　{}",
                        key.replace(
                            "secondary-",
                            if cfg!(target_os = "macos") {
                                "⌘ "
                            } else {
                                "Ctrl "
                            }
                        )
                    ),
                    Choice::App(c),
                ));
            }
        }
        items.retain(|(_, label, _)| label.to_lowercase().contains(query));
        let n = items.len();
        let ix = self.palette_index.min(n.saturating_sub(1));
        let mut group = "";
        let mut list = div()
            .id("palette-list")
            .overflow_y_scroll()
            .max_h(rems(22.))
            .track_scroll(&self.palette_scroll);
        let mut active_row = ix;
        let mut section_count = 0;
        for (i, (section, label, choice)) in items.iter().enumerate() {
            if *section != group {
                group = section;
                section_count += 1;
                list = list.child(p.caption(*section));
            }
            if i == ix {
                active_row = ix + section_count;
            }
            let choice = choice.clone();
            list = list.child(
                p.row(("palette-choice", i), ix == i, cx)
                    .label(label.clone())
                    .on_click(cx.listener(move |app, _, w, cx| app.choose(&choice, w, cx))),
            );
        }
        let choice = items.get(ix).map(|i| i.2.clone());
        Some(
            p.scrim()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _, w, cx| {
                        app.close_palette(w);
                        cx.notify();
                    }),
                )
                .child(
                    p.card()
                        .id("palette")
                        .debug_selector(|| "command-palette".into())
                        .w(px(600.))
                        .shadow_lg()
                        .max_w(relative(0.95))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .key_context("Palette Picker")
                        .on_action(cx.listener(move |app, _: &keys::PickUp, w, cx| {
                            if !input_marked(app, w, cx) {
                                app.palette_index = ix.saturating_sub(1);
                                app.palette_scroll
                                    .scroll_to_item(active_row.saturating_sub(1));
                                cx.notify();
                            }
                        }))
                        .on_action(cx.listener(move |app, _: &keys::PickDown, w, cx| {
                            if !input_marked(app, w, cx) {
                                app.palette_index = (ix + 1).min(n.saturating_sub(1));
                                app.palette_scroll.scroll_to_item(active_row + 1);
                                cx.notify();
                            }
                        }))
                        .on_action(cx.listener(move |app, _: &keys::PickEnter, w, cx| {
                            if !input_marked(app, w, cx)
                                && let Some(choice) = &choice
                            {
                                app.choose(choice, w, cx);
                            }
                        }))
                        .child(Input::new(input).appearance(false))
                        .child(list)
                        .when(n == 0, |d| d.child(p.caption("一致する項目はありません")))
                        .child(p.caption("↑↓ 選ぶ　 Enter 実行　 Esc 閉じる")),
                ),
        )
    }
}
fn input_marked(app: &Desktop, w: &mut Window, cx: &mut Context<Desktop>) -> bool {
    app.search
        .as_ref()
        .is_some_and(|(input, _)| input.update(cx, |s, cx| s.marked_text_range(w, cx).is_some()))
}
