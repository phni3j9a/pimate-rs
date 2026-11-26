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
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Section {
    Add,
    Defaults,
    #[default]
    Display,
    Keyboard,
    About,
}
#[derive(Default)]
pub(super) enum Release {
    #[default]
    Idle,
    Checking,
    Unknown,
    Latest,
    Available(String),
}

pub(super) fn version_cmp(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    fn parts(s: &str) -> Option<[(usize, &str); 3]> {
        let mut it = s.split('.');
        let mut parts = [(0, ""); 3];
        for part in &mut parts {
            let raw = it.next()?;
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let n = raw.trim_start_matches('0');
            *part = (n.len(), n);
        }
        it.next().is_none().then_some(parts)
    }
    Some(parts(a)?.cmp(&parts(b)?))
}
fn desktop_release(rows: &Value) -> Release {
    let version = rows
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["draft"] == false && v["prerelease"] == false)
        .filter_map(|v| v["tag_name"].as_str()?.strip_prefix("desktop-v"))
        .filter(|v| version_cmp(v, "0.0.0").is_some())
        .max_by(|a, b| version_cmp(a, b).unwrap());
    match version {
        Some(v)
            if version_cmp(v, env!("CARGO_PKG_VERSION")) == Some(std::cmp::Ordering::Greater) =>
        {
            Release::Available(v.into())
        }
        Some(_) => Release::Latest,
        None => Release::Unknown,
    }
}
fn query_release() -> Option<Release> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?
        .block_on(async {
            let mut response = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .user_agent("PiMate Desktop")
                .build()
                .ok()?
                .get("https://api.github.com/repos/phni3j9a/pimate-rs/releases?per_page=100")
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?;
            let mut bytes = Vec::new();
            while let Some(part) = response.chunk().await.ok()? {
                if bytes.len() + part.len() > 2_000_000 {
                    return None;
                }
                bytes.extend_from_slice(&part);
            }
            Some(desktop_release(&serde_json::from_slice(&bytes).ok()?))
        })
}
#[derive(Default)]
pub(crate) struct Nav {
    pub(super) route: Route,
    pub(super) section: Section,
    pub(super) release: Release,
    pub(super) page: Option<Page>,
    pub(super) note: String,
    pub(super) pref_error: bool,
    pub(super) machine: super::machine::MachineData,
    pub(super) providers: super::providers::ProviderData,
    pub(super) github: super::github::Github,
    pub(super) initial_folder: Option<String>,
    pub(super) spawn_drafts: HashMap<String, super::new_pi::SpawnDraft>,
    pub(super) spawn: Option<super::new_pi::Fields>,
    pub(super) back: Route,
    pub(super) generation: Arc<()>,
    pub(super) scroll: ScrollHandle,
    pub(super) tasks: Vec<Task<()>>,
}
pub(crate) type Navigation = Entity<Nav>;
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
pub(super) fn section(title: impl Into<SharedString>, p: &Colors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .py_2()
        .child(p.caption(title).font_weight(FontWeight::SEMIBOLD))
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
        let pref_error = self.pref_error;
        w.use_keyed_state("desktop-other-navigation", cx, |_, _| Nav {
            pref_error,
            ..Default::default()
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
            if let Some(h) = &self.history {
                h.scroll.reset(h.scroll.item_count());
            }
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
    pub(super) fn navigate(
        &mut self,
        n: &Navigation,
        route: Route,
        cx: &mut Context<Self>,
    ) -> bool {
        if n.read(cx).spawn.as_ref().is_some_and(|f| f.submitting)
            && n.read(cx)
                .page
                .as_ref()
                .is_some_and(|page| page.visible(self))
        {
            return false;
        }
        if matches!(route, Route::New(_)) {
            self.sidebar_open = false;
            self.context_menu = None;
        }
        if route != Route::Home {
            self.push_overlay(keys::Overlay::Route);
        }
        self.remember_spawn(n, cx);
        let previous = &n.read(cx).route;
        let back = match (&route, previous) {
            (Route::New(_), Route::First(_))
            | (Route::SetupPc, Route::Machine(_))
            | (Route::SetupPc, Route::Settings)
            | (Route::Tailscale, Route::Settings) => previous.clone(),
            (Route::New(_), Route::New(_)) => n.read(cx).back.clone(),
            (Route::SetupPc, _) => Route::Tailscale,
            _ => Route::Home,
        };
        self.row_action = None;
        // A New Pi form overlays Home; it owns its requests, not the conversation's inspector.
        match (previous, &route) {
            (Route::Home | Route::New(_), Route::New(_)) | (Route::New(_), Route::Home) => {
                self.state.sheet = None
            }
            _ => self.state.close_page(),
        }
        let generation = Arc::new(());
        let page = match &route {
            Route::New(id) => self.modal_machine(id, &generation),
            Route::Machine(id) | Route::Providers(id) => self.open_machine(id).ok(),
            _ => None,
        };
        n.update(cx, |s, _| {
            *s = Nav {
                route,
                generation,
                back,
                page,
                pref_error: s.pref_error,
                spawn_drafts: std::mem::take(&mut s.spawn_drafts),
                ..Default::default()
            }
        });
        cx.notify();
        true
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.navigate(&n, r.clone(), cx);
            }))
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
            .debug_selector(move || format!("pref-{id}"))
            .label(if selected {
                format!("✓ {label}")
            } else {
                label.to_owned()
            })
            .selected(selected)
            .on_click(cx.listener(move |this, _, _, cx| {
                apply(&mut this.state.settings);
                let failed = this
                    .settings_path
                    .as_ref()
                    .is_none_or(|p| this.state.settings.save(p).is_err());
                n.update(cx, |s, _| s.pref_error = failed);
                cx.notify();
            }))
    }
    fn settings_nav(&self, n: &Navigation, p: &Colors, cx: &Context<Self>) -> Div {
        let nav = n.read(cx);
        let mut left = div()
            .id("settings-items")
            .overflow_y_scroll()
            .h_full()
            .w(px(180.))
            .flex_none()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .border_r_1()
            .border_color(p.border)
            .child(
                self.nav_button(n, "back", "‹　設定", Route::Home, cx)
                    .ghost()
                    .justify_start(),
            )
            .child(p.caption("マシン").mt_3().px_2());
        for h in &self.hosts {
            let on = matches!(&nav.route, Route::Machine(id) | Route::Providers(id) if id == &h.id);
            left = left.child(
                self.nav_button(
                    n,
                    SharedString::from(format!("settings-{}", h.id)),
                    self.host_label(h),
                    Route::Machine(h.id.clone()),
                    cx,
                )
                .selected(on)
                .ghost()
                .justify_start(),
            );
        }
        for (section, id, title) in [
            (Section::Add, "register-machine", "＋ マシンを追加"),
            (Section::Defaults, "defaults", "新しい Pi の既定"),
            (Section::Display, "display", "表示"),
            (Section::Keyboard, "keyboard", "キーボード"),
            (Section::About, "about", "情報"),
        ] {
            let n = n.clone();
            left = left.child(
                p.row(
                    id,
                    nav.route == Route::Settings && section == nav.section,
                    cx,
                )
                .debug_selector(move || format!("settings-{id}"))
                .label(title)
                .on_click(cx.listener(move |app, _, _, cx| {
                    if n.read(cx).route != Route::Settings {
                        app.navigate(&n, Route::Settings, cx);
                    }
                    n.update(cx, |s, _| {
                        s.section = section;
                        s.note.clear();
                        s.scroll.scroll_to_top_of_item(0);
                    });
                    if section == Section::About && matches!(n.read(cx).release, Release::Idle) {
                        app.check_release(&n, cx);
                    }
                    cx.notify();
                })),
            );
        }
        div()
            .h_full()
            .flex_none()
            .debug_selector(|| "settings-rail".into())
            .child(
                left.child(
                    self.reload_button("settings-reload", "登録を読み直す", p, cx)
                        .debug_selector(|| "settings-reload".into()),
                ),
            )
    }
    fn check_release(&self, n: &Navigation, cx: &mut Context<Self>) {
        let nav = n.clone();
        let generation = nav.read(cx).generation.clone();
        nav.update(cx, |s, _| s.release = Release::Checking);
        let query = cx
            .background_executor()
            .spawn(async { query_release().unwrap_or(Release::Unknown) });
        let task = cx.spawn(async move |app, cx| {
            let release = query.await;
            let _ = app.update(cx, |_, cx| {
                if Arc::ptr_eq(&generation, &nav.read(cx).generation) {
                    nav.update(cx, |s, _| s.release = release);
                    cx.notify();
                }
            });
        });
        n.update(cx, |s, _| s.tasks.push(task));
    }
    fn settings_view(&self, n: &Navigation, p: &Colors, cx: &Context<Self>) -> Div {
        let s = &self.state.settings;
        match n.read(cx).section {
            Section::Add => {
                section("マシンを追加", p)
                    .child(p.caption(
                        "PC の Pi で /pimate pair を実行し、ペアリングリンクをコピーします。",
                    ))
                    .child(
                        p.primary("add-paste", cx)
                            .label("リンクを貼り付ける")
                            .disabled(self.pairing)
                            .on_click(cx.listener(|app, _, _, cx| app.paste(cx))),
                    )
                    .child(self.nav_button(n, "setup", "接続方法", Route::Tailscale, cx))
            }
            Section::Keyboard => section("キーボード", p)
                .child(p.caption(if self.ctrl_enter {
                    "Enter で改行します。"
                } else {
                    "Enter で送信、Shift+Enter で改行します。"
                }))
                .child(
                    p.text_button("shortcut-list", p.accent_ink, cx)
                        .label("ショートカット一覧")
                        .on_click(cx.listener(|app, _, w, cx| app.open_palette(true, w, cx))),
                )
                .child({
                    let n = n.clone();
                    Button::new("send-key")
                        .label(if cfg!(target_os = "macos") {
                            "⌘+Enter で送信"
                        } else {
                            "Ctrl+Enter で送信"
                        })
                        .selected(self.ctrl_enter)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.ctrl_enter = !this.ctrl_enter;
                            let failed = this
                                .settings_path
                                .as_ref()
                                .is_none_or(|p| state::save_send_key(p, this.ctrl_enter).is_err());
                            n.update(cx, |s, _| s.pref_error = failed);
                            cx.notify();
                        }))
                }),
            Section::Defaults => section("新しい Pi の既定", p).child(self.preference(
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
            )),
            Section::Display => section("表示", p)
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
            Section::About => section("情報", p)
                .child(
                    line()
                        .child(icon("pimate-mark", 24., p.ink))
                        .child(format!("PiMate Desktop　{}", env!("CARGO_PKG_VERSION"))),
                )
                .child({
                    let nav = n.clone();
                    match &n.read(cx).release {
                        Release::Available(v) => {
                            let url = format!(
                                "https://github.com/phni3j9a/pimate-rs/releases/tag/desktop-v{v}"
                            );
                            line().child(
                                Button::new("release-page")
                                    .label(format!("{v} に更新できます ↗"))
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                            )
                        }
                        Release::Checking => line().child(p.caption("公開版を確認中…")),
                        Release::Latest => line().child(p.caption("最新です")),
                        _ => line()
                            .child(p.caption("公開版を確認できませんでした"))
                            .child(Button::new("release-retry").label("再確認").on_click(
                                cx.listener(move |app, _, _, cx| {
                                    app.check_release(&nav, cx);
                                    cx.notify();
                                }),
                            )),
                    }
                })
                .child(
                    line()
                        .child(
                            link("repo", "GitHub ↗", "https://github.com/phni3j9a/pimate-rs")
                                .child(icon("github-mark", 16., p.ink)),
                        )
                        .child(link(
                            "bug",
                            "不具合を報告 ↗",
                            "https://github.com/phni3j9a/pimate-rs/issues/new",
                        )),
                ),
        }
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
        };
        let settings = matches!(r, Route::Settings | Route::Providers(_) | Route::Machine(_));
        let back = n.read(cx).back.clone();
        let is_new = matches!(r, Route::New(_));
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(p.border)
            .child(p.caption(if is_new {
                title
            } else {
                "PiMate  /  ワークスペース"
            }))
            .child(
                self.nav_button(n, "back", if is_new { "閉じる" } else { "戻る" }, back, cx)
                    .ghost()
                    .disabled(
                        n.read(cx).spawn.as_ref().is_some_and(|f| f.submitting)
                            && n.read(cx)
                                .page
                                .as_ref()
                                .is_some_and(|page| page.visible(self)),
                    ),
            );
        let scroll = n.read(cx).scroll.clone();
        let body = div()
            .mx_auto()
            .w_full()
            .max_w(rems(45.))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .when(settings, |d| d.mx_0().max_w(px(684.)).px_6())
            .when(!is_new && !settings, |d| {
                d.child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_size(rems(1.55))
                        .child(title),
                )
            })
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
                .when(!settings, |d| d.child(header))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .when(settings, |d| d.child(self.settings_nav(n, p, cx)))
                        .child(
                            div()
                                .min_w_0()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .child(content)
                                .vertical_scrollbar(&scroll),
                        ),
                )
                .children(footer),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{FontSize, Release, desktop_release, version_cmp};
    use std::cmp::Ordering::*;
    #[test]
    fn release_numbers_compare_only_stable_triples() {
        for (latest, current, expected) in [
            ("1.10.0", "1.9.99", Some(Greater)),
            ("1.02.0", "1.2.0", Some(Equal)),
            ("0.2.0", "0.3.0", Some(Less)),
            ("0.2.1-beta", "0.1.0", None),
            ("", "1.0.0", None),
            ("1.0.1", "dev", None),
        ] {
            assert_eq!(version_cmp(latest, current), expected);
        }
        let rows = serde_json::json!([{"tag_name":"app-v9.9.9","draft":false,"prerelease":false},{"tag_name":"desktop-v9.9.9","draft":true,"prerelease":false}]);
        assert!(matches!(desktop_release(&rows), Release::Unknown));
        for (v, newer) in [("0.0.1", false), ("999.1.0", true)] {
            let rows = serde_json::json!([{"tag_name":format!("desktop-v{v}"),"draft":false,"prerelease":false}]);
            assert_eq!(
                matches!(desktop_release(&rows), Release::Available(_)),
                newer
            );
        }
    }
    #[gpui::test]
    fn settings_click_changes_text_not_layout(cx: &mut gpui::TestAppContext) {
        let (desktop, w, _socket) = super::super::tests::open_chat(cx);
        let click = |w: &mut gpui::VisualTestContext, id| {
            let pos = w.debug_bounds(id).unwrap().center();
            w.simulate_click(pos, Default::default());
            w.run_until_parked();
        };
        click(w, "nav-settings");
        click(w, "pref-small");
        let metrics = |w: &mut gpui::VisualTestContext| {
            (
                w.debug_bounds("sidebar").unwrap().size.width,
                w.debug_bounds("settings-rail").unwrap().size.width,
                w.update(|w, _| {
                    (
                        w.rem_size(),
                        w.text_style().font_size.to_pixels(w.rem_size()),
                    )
                }),
            )
        };
        let small = metrics(w);
        click(w, "pref-large");
        let large = metrics(w);
        assert_eq!((small.0, small.1, small.2.0), (large.0, large.1, large.2.0));
        assert!(large.2.1 > small.2.1);
        desktop.read_with(w, |d, _| {
            assert!(d.state.settings.font_size == FontSize::Large)
        });
    }
}
