use super::*;

#[cfg(target_os = "macos")]
actions!(pimate, [Quit]);

// The same command table drives shortcuts, macOS menus and the command palette.
macro_rules! commands {
    ($($name:ident => ($key:literal, $label:literal)),* $(,)?) => {
        actions!(pimate, [$($name,)* Newline, PickUp, PickDown, PickTab, PickEnter, OpenContext, PairTab, PairEnter]);
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum Command { $($name,)* }
        pub(super) const SHORTCUTS: &[(Command, &str, &str)] = &[$((Command::$name, $key, $label)),*];
        pub(crate) fn init(cx: &mut App) {
            cx.bind_keys([$(KeyBinding::new($key, $name, Some("PiMate")),)*]);
            // Text inputs have their own Ctrl+Up/Down and Ctrl+End bindings. Keep the
            // documented app shortcuts in the composer without changing other inputs.
            cx.bind_keys([$(KeyBinding::new($key,$name,Some("Composer > Input")),)*]);
            cx.bind_keys([
                KeyBinding::new("secondary-enter", Enter {secondary:true},Some("QuestionCard")),
                KeyBinding::new("enter", PairEnter, Some("PairDialog")),
                KeyBinding::new("tab", PairTab, Some("PairDialog")),
                KeyBinding::new("shift-tab", PairTab, Some("PairDialog")),
                KeyBinding::new("menu", OpenContext, Some("ContextTarget")),
                KeyBinding::new("shift-f10", OpenContext, Some("ContextTarget")),
                KeyBinding::new("shift-enter", Newline, Some("Composer > Input")),
                KeyBinding::new("up", PickUp, Some("Picker > Input")),
                KeyBinding::new("down", PickDown, Some("Picker > Input")),
                KeyBinding::new("tab", PickTab, Some("Picker > Input")),
                KeyBinding::new("enter", PickEnter, Some("Picker > Input")),
            ]);
            #[cfg(target_os="macos")]
            {
                use gpui_component::input;
                cx.bind_keys([KeyBinding::new("cmd-q",Quit,None)]);
                cx.on_action(|_:&Quit,cx|cx.quit());
                cx.set_menus(vec![
                    Menu{name:"PiMate".into(),items:vec![MenuItem::action("設定…",Preferences),MenuItem::action("PiMate を終了",Quit)]},
                    Menu{name:"編集".into(),items:vec![MenuItem::os_action("元に戻す",input::Undo,OsAction::Undo),MenuItem::os_action("やり直す",input::Redo,OsAction::Redo),MenuItem::separator(),MenuItem::os_action("切り取り",input::Cut,OsAction::Cut),MenuItem::os_action("コピー",input::Copy,OsAction::Copy),MenuItem::os_action("貼り付け",input::Paste,OsAction::Paste),MenuItem::os_action("すべてを選択",input::SelectAll,OsAction::SelectAll)]},
                    Menu{name:"操作".into(),items:vec![$(MenuItem::action($label,$name)),*]},
                ]);
            }
        }
        pub(super) fn listeners(root: Div, cx: &Context<Desktop>) -> Div {
            root.capture_any_mouse_down(cx.listener(Desktop::remember_pointer_layer))
                .capture_key_up(cx.listener(Desktop::overlay_key_up))
                .key_context("PiMate")$(.on_action(cx.listener(|app,_:&$name,w,cx| app.command(Command::$name,w,cx))))*
        }
    }
}
commands! {
    Search=>("secondary-k","Pi・コマンドを検索"),
    NewPi=>("secondary-n","新しい Pi"),
    Preferences=>("secondary-,","設定"),
    Keys=>("secondary-/","キーボードショートカット"),
    Sidebar=>("secondary-b","サイドバーを表示／非表示"),
    NextPane=>("f6","次のペイン"),
    Composer=>("secondary-l","入力欄へ"),
    EscapeTop=>("escape","閉じる／停止"),
    Model=>("secondary-shift-m","モデル・Thinking"),
    Review=>("secondary-shift-d","変更をレビュー"),
    Agents=>("secondary-shift-s","サブエージェント"),
    Github=>("secondary-shift-g","GitHub"),
    Previous=>("secondary-up","前の Pi"),
    Next=>("secondary-down","次の Pi"),
    Attention=>("secondary-shift-a","次の要対応"),
    First=>("secondary-1","1 番目の Pi"),
    Second=>("secondary-2","2 番目の Pi"),
    Third=>("secondary-3","3 番目の Pi"),
    Fourth=>("secondary-4","4 番目の Pi"),
    Fifth=>("secondary-5","5 番目の Pi"),
    Sixth=>("secondary-6","6 番目の Pi"),
    Seventh=>("secondary-7","7 番目の Pi"),
    Eighth=>("secondary-8","8 番目の Pi"),
    Ninth=>("secondary-9","9 番目の Pi"),
    PageUp=>("pageup","会話を上にスクロール"),
    PageDown=>("pagedown","会話を下にスクロール"),
    End=>("secondary-end","会話の末尾"),
    Larger=>("secondary-+","文字を大きく"),
    Smaller=>("secondary--","文字を小さく"),
    ResetSize=>("secondary-0","標準の文字サイズ"),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Overlay {
    Prompt,
    Tooltip,
    Menu,
    Pair,
    Palette,
    Sidebar,
    Slash,
    Route,
    Media,
    Sheet,
    Inspector,
}

// Intercept before GPUI resolves bindings, including Root tab navigation.
pub(crate) fn intercept(w: &Window, cx: &mut Context<Desktop>) -> Subscription {
    let window = w.window_handle().window_id();
    let listener = cx.listener(move |d, e: &KeystrokeEvent, w, cx| {
        if w.window_handle().window_id() != window {
            return;
        }
        let key = e.keystroke.key.as_str();
        // X11 can report repeats as fresh presses. Only the physical release re-arms a consumed key.
        if let Some(text_target) = d.consumed_keys.get(key) {
            if !text_target
                .as_ref()
                .and_then(|f| f.upgrade())
                .is_some_and(|f| f.is_focused(w))
            {
                cx.stop_propagation();
            }
            return;
        }
        if !matches!(key, "enter" | "space" | "escape" | "tab") {
            return;
        }
        let tip_tab = key == "tab" && w.tooltip_visible();
        if tip_tab {
            w.dismiss_tooltip();
        }
        let top = d.front(w, cx);
        let text_space = key == "space" && e.context_stack.iter().any(|c| c.contains("Input"));
        let owns = match key {
            "escape" => true,
            "enter" => {
                (top.is_none()
                    && e.keystroke.modifiers.secondary()
                    && e.context_stack.iter().any(|c| c.contains("QuestionCard")))
                    || matches!(
                        top,
                        Some(
                            Overlay::Menu
                                | Overlay::Pair
                                | Overlay::Palette
                                | Overlay::Slash
                                | Overlay::Tooltip
                        )
                    )
            }
            "tab" => {
                tip_tab
                    || matches!(
                        top,
                        Some(
                            Overlay::Menu
                                | Overlay::Pair
                                | Overlay::Palette
                                | Overlay::Slash
                                | Overlay::Sidebar
                        )
                    )
            }
            "space" => matches!(
                top,
                Some(
                    Overlay::Menu
                        | Overlay::Tooltip
                        | Overlay::Palette
                        | Overlay::Slash
                        | Overlay::Pair
                )
            ),
            _ => false,
        };
        if owns {
            // Text may repeat in its original field, never in a field exposed after dismissal.
            let repeat_in = if text_space {
                w.focused(cx).map(|f| f.downgrade())
            } else {
                None
            };
            d.consumed_keys.insert(key.to_owned(), repeat_in);
        }
        match (key, top) {
            ("escape", Some(Overlay::Tooltip)) => {
                w.dismiss_tooltip();
                d.restore_input(w, cx);
                cx.stop_propagation();
            }
            ("tab", Some(Overlay::Sidebar)) => {
                w.cycle_focus_within(&d.side_focus, e.keystroke.modifiers.shift);
                cx.stop_propagation();
            }
            ("tab", Some(Overlay::Menu)) => cx.stop_propagation(),
            ("tab", Some(Overlay::Palette)) => {
                if let Some((input, _)) = &d.search {
                    input.focus_handle(cx).focus(w);
                }
                w.dispatch_action(
                    if e.keystroke.modifiers.shift {
                        Box::new(PickUp)
                    } else {
                        Box::new(PickDown)
                    },
                    cx,
                );
                cx.stop_propagation();
            }
            (key @ ("enter" | "space"), layer) => {
                // Space edits the palette query; in a menu it activates the selected item.
                if key == "space"
                    && layer == Some(Overlay::Menu)
                    && e.keystroke.modifiers == Modifiers::none()
                {
                    let enter = Keystroke::parse("enter").unwrap();
                    let action = cx
                        .key_bindings()
                        .borrow()
                        .bindings_for_input(&[enter], &e.context_stack)
                        .0
                        .first()
                        .map(|b| b.action().boxed_clone());
                    if let Some(action) = action {
                        w.dispatch_action(action, cx);
                    }
                    cx.stop_propagation();
                }
                if layer == Some(Overlay::Tooltip) && !text_space {
                    cx.stop_propagation();
                }
            }
            _ => {}
        }
    });
    cx.intercept_keystrokes(listener)
}
impl Desktop {
    fn remember_pointer_layer(
        &mut self,
        e: &MouseDownEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A tooltip or scrim can disappear between mouse-down and the button click.
        if e.button == MouseButton::Left {
            self.pointer_down_on_overlay = self.overlay_open(w, cx);
        }
    }
    pub(super) fn background_click(&self, e: &ClickEvent) -> bool {
        matches!(e, ClickEvent::Keyboard(_)) || self.pointer_down_on_overlay
    }
    fn overlay_key_up(&mut self, e: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self
            .consumed_keys
            .remove(e.keystroke.key.as_str())
            .is_some()
        {
            cx.stop_propagation();
        }
    }
    pub(super) fn restore_input(&self, w: &mut Window, cx: &App) {
        match self.front(w, cx) {
            Some(Overlay::Palette) => {
                if let Some((s, _)) = &self.search {
                    w.focus(&s.focus_handle(cx));
                }
            }
            Some(Overlay::Sidebar) => w.focus(&self.side_focus),
            None | Some(Overlay::Slash | Overlay::Tooltip) => w.focus(
                &self
                    .input
                    .as_ref()
                    .map(|(_, s, _)| s.focus_handle(cx))
                    .unwrap_or_else(|| self.main_focus.clone()),
            ),
            _ => {}
        }
    }
    pub(super) fn front(&self, w: &Window, cx: &App) -> Option<Overlay> {
        use Overlay::*;
        if w.has_rendered_prompt() {
            Some(Prompt)
        } else if w.tooltip_visible() {
            Some(Tooltip)
        } else if self.context_menu.is_some() {
            Some(Menu)
        } else if self.confirmation.is_some() {
            Some(Pair)
        } else if self.palette_open {
            Some(Palette)
        } else if self.sidebar_open
            && !self.hosts.is_empty()
            && Layout::of(w.viewport_size().width) == Layout::Narrow
        {
            Some(Sidebar)
        } else if self.slash_visible {
            Some(Slash)
        } else if self
            .navigation
            .as_ref()
            .is_some_and(|n| n.read(cx).route != super::Route::Home)
        {
            Some(Route)
        } else if self.media_keys.is_some() {
            Some(Media)
        } else if self.active_sheet().is_some() {
            Some(Sheet)
        } else if self
            .chat_keys
            .as_ref()
            .is_some_and(|(_, i)| i.read(cx).agents || i.read(cx).review_open)
        {
            Some(Inspector)
        } else {
            None
        }
    }
    pub(super) fn overlay_open(&self, w: &Window, cx: &App) -> bool {
        self.front(w, cx).is_some()
    }

    pub(super) fn pi_order(&self) -> Vec<(String, String, bool)> {
        let mut rows: Vec<_> = self
            .hosts
            .iter()
            .filter_map(|h| Some((h, self.machines.get(&h.id)?)))
            .flat_map(|(h, m)| {
                m.rows
                    .group("instance")
                    .iter()
                    .map(move |(id, v)| (h.id.clone(), id.clone(), v["section"] == "waiting"))
            })
            .collect();
        rows.sort_by_key(|r| !r.2);
        rows
    }
    pub(super) fn stop_chat(&mut self, page: &state::Page, w: &Window, cx: &mut Context<Self>) {
        if self.overlay_open(w, cx)
            || self.stopping
            || !page.visible(self)
            || !self
                .machines
                .get(page.host())
                .is_some_and(|m| m.rows.can_stop())
        {
            return;
        }
        self.stopping = true;
        let page = page.clone();
        cx.spawn(async move |app, cx| {
            let result = page.abort().await;
            let _ = app.update(cx, |app, cx| {
                app.stopping = false;
                if page.visible(app) {
                    app.notice = match result {
                        Ok(_) => "停止を要求しました。履歴と差分で結果を確認してください。".into(),
                        Err(e) => format!("停止できません: {e}"),
                    };
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn command(&mut self, c: Command, w: &mut Window, cx: &mut Context<Self>) {
        use Command::*;
        let top = self.front(w, cx);
        if top == Some(Overlay::Prompt) {
            return;
        }
        if (matches!(top, Some(Overlay::Menu | Overlay::Palette | Overlay::Pair)) && c != EscapeTop)
            || (top == Some(Overlay::Sidebar)
                && !matches!(c, EscapeTop | NextPane | Sidebar | Search | Keys))
        {
            return;
        }
        self.execute_command(c, w, cx);
    }
    pub(super) fn execute_command(&mut self, c: Command, w: &mut Window, cx: &mut Context<Self>) {
        use Command::*;
        let Some(nav) = self.navigation.clone() else {
            return;
        };
        let top = self.front(w, cx);
        let narrow = Layout::of(w.viewport_size().width) == Layout::Narrow;
        let home = nav.read(cx).route == Route::Home;
        match c {
            EscapeTop => match top {
                Some(Overlay::Prompt) => {}
                Some(Overlay::Tooltip) => w.dismiss_tooltip(),
                Some(Overlay::Menu) => {
                    self.context_menu = None;
                }
                Some(Overlay::Palette) => self.close_palette(w, cx),
                Some(Overlay::Pair) => self.confirmation = None,
                Some(Overlay::Sidebar) => {
                    self.sidebar_open = false;
                }
                Some(Overlay::Slash) => self.slash_hidden = true,
                Some(Overlay::Route) => {
                    let back = nav.read(cx).back.clone();
                    self.navigate(&nav, back, cx);
                }
                Some(Overlay::Media) => self.media_keys.take().unwrap().update(cx, |s, cx| {
                    s.close();
                    cx.notify();
                }),
                Some(Overlay::Sheet) => {
                    self.state.close_page();
                }
                Some(Overlay::Inspector) => {
                    self.chat_keys.as_ref().unwrap().1.update(cx, |s, cx| {
                        *s = Default::default();
                        cx.notify();
                    })
                }
                None if !self.composing
                    && let Some((page, _)) = self.chat_keys.clone() =>
                {
                    self.stop_chat(&page, w, cx)
                }
                None => {}
            },
            Search | Keys => {
                self.open_palette(c == Keys, w, cx);
            }
            Preferences => self.navigate(&nav, Route::Settings, cx),
            NewPi => self.navigate(&nav, self.new_route(), cx),
            Sidebar => {
                if narrow {
                    self.sidebar_open = !self.sidebar_open;
                    w.focus(if self.sidebar_open {
                        &self.side_focus
                    } else {
                        &self.main_focus
                    });
                } else {
                    self.sidebar_hidden = !self.sidebar_hidden
                }
            }
            NextPane => {
                self.keyboard_focus = true;
                if !self.hosts.is_empty() && !self.side_focus.contains_focused(w, cx) {
                    if narrow {
                        self.sidebar_open = true
                    } else {
                        self.sidebar_hidden = false
                    };
                    w.focus(&self.side_focus);
                } else {
                    self.sidebar_open = false;
                    w.focus(&self.main_focus);
                }
            }
            Composer if home => {
                if let Some((_, s, _)) = &self.input {
                    s.update(cx, |s, cx| s.focus(w, cx));
                }
            }
            Model if home && self.editor().is_some() => {
                self.show_sheet(state::SheetPage::Model, cx)
            }
            Review | Agents if home => {
                if let Some((page, ins)) = &self.chat_keys
                    && page.visible(self)
                {
                    ins.update(cx, |s, cx| {
                        if c == Review {
                            s.agents = false;
                            s.load(page.clone(), None, true, cx)
                        } else {
                            s.review_open = false;
                            s.agents = true;
                            cx.notify()
                        }
                    });
                }
            }
            Github if home => {
                if let Some((host, id)) = &self.selected
                    && github_available(&self.machines[host].rows, host, id)
                {
                    self.navigate(&nav, Route::Github(pimate_protocol::GitHubQuery::List), cx)
                }
            }
            PageUp | PageDown if home && let Some(h) = &self.history => h
                .scroll
                .scroll_by(w.viewport_size().height * if c == PageUp { -0.75 } else { 0.75 }),
            End if home && let Some(h) = &self.history => h.scroll.scroll_to(ListOffset {
                item_ix: h.scroll.item_count(),
                offset_in_item: px(0.),
            }),
            Larger | Smaller | ResetSize => {
                use state::FontSize::*;
                self.state.settings.font_size = match (c, &self.state.settings.font_size) {
                    (Larger, Standard | Large) => Large,
                    (Smaller, Small | Standard) => Small,
                    _ => Standard,
                };
                let failed = self
                    .settings_path
                    .as_ref()
                    .is_none_or(|p| self.state.settings.save(p).is_err());
                nav.update(cx, |n, _| n.pref_error = failed);
            }
            Previous | Next | Attention | First | Second | Third | Fourth | Fifth | Sixth
            | Seventh | Eighth | Ninth => {
                let rows = self.pi_order();
                let count = rows.len();
                if count == 0 {
                    return;
                }
                let pos = rows.iter().position(|(h, i, _)| {
                    self.selected
                        .as_ref()
                        .is_some_and(|p| &p.0 == h && &p.1 == i)
                });
                let ix = match c {
                    Next => pos.map_or(0, |p| (p + 1) % count),
                    Previous => pos.map_or(count - 1, |p| (p + count - 1) % count),
                    Attention => {
                        let start = pos.map_or(0, |p| p + 1);
                        let Some(ix) = (start..start + count)
                            .map(|i| i % count)
                            .find(|&i| rows[i].2)
                        else {
                            return;
                        };
                        ix
                    }
                    _ => [
                        First, Second, Third, Fourth, Fifth, Sixth, Seventh, Eighth, Ninth,
                    ]
                    .iter()
                    .position(|key| *key == c)
                    .unwrap(),
                };
                if let Some((h, i, _)) = rows.get(ix) {
                    if !home {
                        self.navigate(&nav, Route::Home, cx)
                    }
                    self.select(h.clone(), i.clone(), cx);
                }
            }
            _ => {}
        }
        if (c == EscapeTop && top.is_some()) || (c == Sidebar && narrow && !self.sidebar_open) {
            self.restore_input(w, cx);
        }
        cx.notify();
    }
}
