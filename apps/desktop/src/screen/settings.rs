use super::*;
use crate::state::{Appearance, FontSize, Page, Settings};
use gpui_component::{Theme, ThemeMode};

#[derive(Default, Clone, PartialEq)]
pub(super) enum Route {
    #[default]
    Home,
    Settings,
    Tailscale,
    SetupPc,
    First(String),
    New(String),
    Machine(String),
    Providers(String),
    Github(pimate_protocol::GitHubQuery),
}
#[derive(Default)]
pub(super) struct Nav {
    pub route: Route,
    pub page: Option<Page>,
    pub note: String,
    pub pref_error: bool,
    pub machine: super::machine::MachineData,
    pub providers: super::providers::ProviderData,
    pub github: super::github::Github,
    pub spawn: Option<super::new_pi::Fields>,
    pub back: Route,
    pub generation: Arc<()>,
    pub scroll: ScrollHandle,
    pub tasks: Vec<Task<()>>,
}
pub(super) type Navigation = Entity<Nav>;
pub(super) fn group(title: impl Into<SharedString>, p: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(p.border)
        .child(
            p.caption(title)
                .font_weight(FontWeight::BOLD)
                .text_color(p.accent),
        )
}
pub(super) fn line() -> Div {
    div().flex().flex_wrap().items_center().gap_2()
}
pub(super) fn link(id: &'static str, title: &'static str, url: &'static str) -> Button {
    Button::new(id)
        .label(title)
        .on_click(move |_, _, cx| cx.open_url(url))
}
impl Desktop {
    pub(super) fn init_navigation(&mut self, w: &mut Window, cx: &mut Context<Self>) -> Navigation {
        #[cfg(target_os = "macos")]
        let app = cx.weak_entity();
        w.use_keyed_state("desktop-other-navigation", cx, |_w, _cx| {
            #[cfg(target_os = "macos")]
            {
                let nav = _cx.weak_entity();
                let window = _w.window_handle().window_id();
                _cx.intercept_keystrokes(move |e, w, cx| {
                    if window == w.window_handle().window_id()
                        && e.keystroke.modifiers == Modifiers::command()
                        && e.keystroke.key == ","
                        && let Some(nav) = nav.upgrade()
                    {
                        let _ = app.update(cx, |app, cx| app.navigate(&nav, Route::Settings, cx));
                        cx.stop_propagation();
                    }
                })
                .detach();
            }

            let settings = Settings::path().and_then(|p| Settings::load(&p));
            let pref_error = settings.is_err();
            self.state.settings = settings.unwrap_or_default();
            Nav {
                pref_error,
                ..Default::default()
            }
        })
    }
    pub(super) fn palette(&mut self, w: &mut Window, cx: &mut Context<Self>) -> bool {
        let size = match self.state.settings.font_size {
            FontSize::Small => 14.,
            FontSize::Standard => 16.,
            FontSize::Large => 19.,
        };
        if self.size != size {
            self.size = size;
            self.scroll.reset(self.scroll.item_count());
        }
        let dark = match self.state.settings.appearance {
            Appearance::System => matches!(
                w.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            Appearance::Dark => true,
            Appearance::Light => false,
        };
        if Theme::global(cx).mode.is_dark() != dark {
            Theme::change(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                Some(w),
                cx,
            );
        }
        dark
    }
    pub(super) fn navigate(&mut self, n: &Navigation, route: Route, cx: &mut Context<Self>) {
        let previous = &n.read(cx).route;
        let back = match (&route, previous) {
            (Route::New(_), Route::First(_))
            | (Route::SetupPc, Route::Settings)
            | (Route::Tailscale, Route::Settings) => previous.clone(),
            (Route::New(_), Route::New(_)) => n.read(cx).back.clone(),
            (Route::Github(q), _) if q != &pimate_protocol::GitHubQuery::List => {
                Route::Github(pimate_protocol::GitHubQuery::List)
            }
            (Route::Providers(id), _) => Route::Machine(id.clone()),
            (Route::Machine(_), _) => Route::Settings,
            (Route::SetupPc, _) => Route::Tailscale,
            _ => Route::Home,
        };
        self.state.close_page();
        let page = match &route {
            Route::Machine(id) | Route::Providers(id) | Route::New(id) => {
                self.open_machine(id).ok()
            }
            Route::Github(_) => self.open_chat().ok(),
            _ => None,
        };
        n.update(cx, |s, _| {
            *s = Nav {
                route,
                back,
                page,
                pref_error: s.pref_error,
                ..Default::default()
            }
        });
        cx.notify();
    }
    pub(super) fn page_task<T: 'static, F: Future<Output = T> + 'static>(
        &self,
        n: &Navigation,
        w: &mut Window,
        cx: &mut Context<Self>,
        run: impl FnOnce(Page) -> F + 'static,
        done: impl FnOnce(&mut Desktop, &Navigation, T, &mut Window, &mut Context<Desktop>) + 'static,
    ) {
        let Some(page) = n.read(cx).page.clone().filter(|p| p.visible(self)) else {
            return;
        };
        let nav = n.downgrade();
        let task = cx.spawn_in(w, async move |app, cx| {
            let result = run(page.clone()).await;
            let _ = app.update_in(cx, |app, w, cx| {
                if page.visible(app)
                    && let Some(n) = nav.upgrade()
                {
                    done(app, &n, result, w, cx);
                    cx.notify();
                }
            });
        });
        n.update(cx, |s, _| s.tasks.push(task));
        cx.notify();
    }
    pub(super) fn nav_button(
        &self,
        n: &Navigation,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        r: Route,
        cx: &Context<Self>,
    ) -> Button {
        let n = n.clone();
        let id = id.into();
        Button::new(id.clone())
            .debug_selector(|| format!("nav-{id}"))
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(&n, r.clone(), cx)))
    }
    pub(super) fn host_label(&self, h: &Host) -> String {
        if self.hosts.iter().filter(|x| x.name == h.name).count() > 1 {
            format!("{} · {}", h.name, h.address)
        } else {
            h.name.clone()
        }
    }
    pub(super) fn new_route(&self) -> Route {
        Route::New(
            self.selected
                .as_ref()
                .map(|v| v.0.clone())
                .or_else(|| self.hosts.first().map(|h| h.id.clone()))
                .unwrap_or_default(),
        )
    }
    fn preference(
        &self,
        n: &Navigation,
        id: &'static str,
        label: &'static str,
        selected: bool,
        apply: fn(&mut Settings),
        cx: &Context<Self>,
    ) -> Button {
        let n = n.clone();
        Button::new(id)
            .label(if selected {
                format!("✓ {label}")
            } else {
                label.to_owned()
            })
            .selected(selected)
            .on_click(cx.listener(move |this, _, _, cx| {
                apply(&mut this.state.settings);
                let failed = Settings::path()
                    .and_then(|p| this.state.settings.save(&p))
                    .is_err();
                n.update(cx, |s, _| s.pref_error = failed);
                cx.notify();
            }))
    }
    fn settings_view(&self, n: &Navigation, p: &Colors, cx: &Context<Self>) -> Div {
        let s = &self.state.settings;
        let mut machines = group("マシン", p);
        for h in &self.hosts {
            machines = machines.child(
                self.nav_button(
                    n,
                    SharedString::from(format!("settings-{}", h.id)),
                    format!(
                        "{}　·　{}　›",
                        self.host_label(h),
                        connection(h, self.machines.get(&h.id).map(|m| &m.rows))
                    ),
                    Route::Machine(h.id.clone()),
                    cx,
                )
                .justify_start(),
            );
        }
        if self.hosts.is_empty() {
            machines = machines.child(p.caption("登録済みのマシンはありません"));
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(machines.child(self.nav_button(
                n,
                "register-machine",
                "＋ マシンを追加（ペアリング）",
                Route::Tailscale,
                cx,
            )))
            .child(group("新しい Pi の既定", p).child(self.preference(
                n,
                "ask",
                if s.ask_tool {
                    "質問ツール（pimate_ask）　オン"
                } else {
                    "質問ツール（pimate_ask）　オフ"
                },
                s.ask_tool,
                |s| s.ask_tool = !s.ask_tool,
                cx,
            )))
            .child(
                group("表示", p)
                    .child(
                        line()
                            .child("テーマ　　")
                            .child(self.preference(
                                n,
                                "system",
                                "システム",
                                s.appearance == Appearance::System,
                                |s| s.appearance = Appearance::System,
                                cx,
                            ))
                            .child(self.preference(
                                n,
                                "light",
                                "ライト",
                                s.appearance == Appearance::Light,
                                |s| s.appearance = Appearance::Light,
                                cx,
                            ))
                            .child(self.preference(
                                n,
                                "dark",
                                "ダーク",
                                s.appearance == Appearance::Dark,
                                |s| s.appearance = Appearance::Dark,
                                cx,
                            )),
                    )
                    .child(
                        line()
                            .child("文字サイズ")
                            .child(self.preference(
                                n,
                                "small",
                                "小",
                                s.font_size == FontSize::Small,
                                |s| s.font_size = FontSize::Small,
                                cx,
                            ))
                            .child(self.preference(
                                n,
                                "standard",
                                "標準",
                                s.font_size == FontSize::Standard,
                                |s| s.font_size = FontSize::Standard,
                                cx,
                            ))
                            .child(self.preference(
                                n,
                                "large",
                                "大",
                                s.font_size == FontSize::Large,
                                |s| s.font_size = FontSize::Large,
                                cx,
                            )),
                    ),
            )
            .child(
                group("情報", p)
                    .child(format!("PiMate Desktop　{}", env!("CARGO_PKG_VERSION")))
                    .child(
                        line()
                            .child(link(
                                "releases",
                                "公開版を確認 ↗",
                                "https://github.com/phni3j9a/pimate-rs/releases?q=desktop-v",
                            ))
                            .child(link(
                                "repo",
                                "GitHub ↗",
                                "https://github.com/phni3j9a/pimate-rs",
                            ))
                            .child(link(
                                "bug",
                                "不具合を報告 ↗",
                                "https://github.com/phni3j9a/pimate-rs/issues/new",
                            )),
                    ),
            )
    }
    pub(super) fn other_panel(
        &mut self,
        n: &Navigation,
        p: &Colors,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let r = n.read(cx).route.clone();
        let (title, body) = match &r {
            Route::Home => return None,
            Route::Providers(id) => (
                "プロバイダーのログイン",
                self.providers_view(n, p, id, w, cx),
            ),
            Route::Machine(id) => ("マシン詳細", self.machine_view(n, p, id, w, cx)),
            Route::New(id) => ("新しい Pi", self.new_pi(n, p, id, w, cx)),
            Route::Settings => ("設定", self.settings_view(n, p, cx)),
            Route::First(id) => ("最初の Pi", self.first_step(n, p, id, cx)),
            Route::Tailscale => ("Tailscale でつなぐ", self.guide(n, p, false, cx)),
            Route::SetupPc => ("PC に PiMate を入れる", self.guide(n, p, true, cx)),
            Route::Github(q) => {
                if n.read(cx)
                    .page
                    .as_ref()
                    .is_some_and(|page| page.visible(self))
                    && let Some(viewer) = n.read(cx).github.viewer.clone()
                    && let Some(panel) = chat_media::Viewer::panel(&viewer, None, p, w, cx)
                {
                    return Some(div().min_w_0().flex_1().child(panel));
                }
                ("GitHub", self.github_view(n, p, q, w, cx))
            }
        };
        let back = n.read(cx).back.clone();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(p.border)
            .child(p.caption("PiMate  /  ワークスペース"))
            .child(self.nav_button(n, "back", "戻る", back, cx).ghost());
        let scroll = n.read(cx).scroll.clone();
        let body = div()
            .mx_auto()
            .w_full()
            .max_w(rems(45.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(rems(1.55))
                    .child(title),
            )
            .when(n.read(cx).pref_error, |v| {
                v.child(p.caption("設定ファイルを読み書きできません。この起動中のみ反映します。"))
            })
            .when(
                !n.read(cx).note.is_empty()
                    && n.read(cx)
                        .page
                        .as_ref()
                        .is_none_or(|page| page.visible(self)),
                |v| v.child(p.caption(n.read(cx).note.clone()).p_2().bg(p.surface)),
            )
            .child(body);
        let content = div()
            .id("other-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .child(body);
        let footer = self.spawn_footer(n, p, cx);
        Some(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(header)
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(content)
                        .vertical_scrollbar(&scroll),
                )
                .children(footer),
        )
    }
}
