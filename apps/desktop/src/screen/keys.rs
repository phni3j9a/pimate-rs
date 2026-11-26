use super::*;

#[cfg(target_os = "macos")]
actions!(pimate, [Quit]);

// The same command table drives shortcuts, macOS menus and the command palette.
macro_rules! commands {
    ($($name:ident => ($key:literal, $label:literal)),* $(,)?) => {
        actions!(pimate, [$($name,)* Newline, PickUp, PickDown, PickTab, PickEnter, OpenContext]);
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum Command { $($name,)* }
        pub(super) const SHORTCUTS: &[(Command, &str, &str)] = &[$((Command::$name, $key, $label)),*];
        pub(crate) fn init(cx: &mut App) {
            cx.bind_keys([$(KeyBinding::new($key, $name, Some("PiMate")),)*]);
            // Text inputs have their own Ctrl+Up/Down and Ctrl+End bindings. Keep the
            // documented app shortcuts in the composer without changing other inputs.
            cx.bind_keys([$(KeyBinding::new($key,$name,Some("Composer > Input")),)*]);
            cx.bind_keys([
                KeyBinding::new("menu", OpenContext, Some("ContextTarget")),
                KeyBinding::new("shift-f10", OpenContext, Some("ContextTarget")),
                KeyBinding::new("shift-enter", Newline, Some("Composer > Input")),
                KeyBinding::new("up", PickUp, Some("Picker > Input")),
                KeyBinding::new("down", PickDown, Some("Picker > Input")),
                KeyBinding::new("tab", PickTab, Some("Picker > Input")),
                KeyBinding::new("tab", PickDown, Some("Palette > Input")),
                KeyBinding::new("shift-tab", PickUp, Some("Palette > Input")),
                KeyBinding::new("enter", PickEnter, Some("Palette > Input")),
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
            root.key_context("PiMate")$(.on_action(cx.listener(|app,_:&$name,w,cx| app.command(Command::$name,w,cx))))*
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
impl Desktop {
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
    pub(super) fn stop_chat(&mut self, page: &state::Page, cx: &mut Context<Self>) {
        if self.stopping
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
                if let Err(e) = result
                    && page.visible(app)
                {
                    app.notice = format!("停止できません: {e}");
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn command(&mut self, c: Command, w: &mut Window, cx: &mut Context<Self>) {
        use Command::*;
        let Some(nav) = self.navigation.clone() else {
            return;
        };
        if (self.confirmation.is_some() || self.palette_open || self.context_menu.is_some())
            && c != EscapeTop
        {
            return;
        }
        let narrow = Layout::of(w.viewport_size().width) == Layout::Narrow;
        let home = nav.read(cx).route == Route::Home;
        match c {
            EscapeTop => {
                if self.context_menu.take().is_some() {
                    w.focus(&self.root_focus);
                } else if self.palette_open {
                    self.close_palette(w);
                } else if self.confirmation.take().is_some() {
                } else if self.sidebar_open {
                    self.sidebar_open = false;
                } else if !home {
                    let back = nav.read(cx).back.clone();
                    self.navigate(&nav, back, cx);
                } else if let Some(viewer) = self.media_keys.take() {
                    viewer.update(cx, |s, cx| {
                        s.close();
                        cx.notify();
                    });
                } else if self.active_sheet().is_some() {
                    self.state.close_page();
                } else if let Some((page, ins)) = self.chat_keys.clone()
                    && page.visible(self)
                {
                    if ins.read(cx).agents || ins.read(cx).review_open {
                        ins.update(cx, |s, cx| {
                            *s = Default::default();
                            cx.notify()
                        });
                    } else if !self.composing {
                        self.stop_chat(&page, cx)
                    }
                }
            }
            Search | Keys => {
                self.open_palette(c == Keys, w, cx);
            }
            Preferences => self.navigate(&nav, Route::Settings, cx),
            NewPi => self.navigate(&nav, self.new_route(), cx),
            Sidebar => {
                if narrow {
                    self.sidebar_open = !self.sidebar_open
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
            PageUp | PageDown if home => self
                .scroll
                .scroll_by(w.viewport_size().height * if c == PageUp { -0.75 } else { 0.75 }),
            End if home => self.scroll.scroll_to(ListOffset {
                item_ix: self.scroll.item_count(),
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
        cx.notify();
    }
}
