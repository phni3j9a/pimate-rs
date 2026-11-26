mod markdown;
mod model;
mod screen;
pub(crate) mod state;
mod store;
use gpui::{prelude::*, *};
use gpui_component::{
    Root,
    input::{InputEvent, InputState},
};
use model::{Delivery, EditorKey, Rows};
use pimate_core::{Connection, Core, Error, Host};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

struct Machine {
    registration: Host,
    conn: Arc<Connection>,
    rows: Rows,
}
struct Desktop {
    #[allow(dead_code)] // Screen integration lives in the designer branch.
    state: state::DesktopState,
    core: Arc<Core>,
    reload_task: Option<Task<()>>,
    spinner_tick: Option<Task<()>>,
    spinner_phase: u8,
    hosts: Vec<Host>,
    machines: BTreeMap<String, Machine>,
    selected: Option<(String, String)>,
    confirmation: Option<(String, String)>,
    pairing: bool,
    notice: String,
    drafts: HashMap<EditorKey, String>,
    input: Option<(EditorKey, Entity<InputState>, [Subscription; 2])>,
    composing: bool,
    sending: bool,
    epoch: u64,
    size: f32,
    scroll: ListState,
    /// 760px 未満で重ねて出すサイドバーの開閉。
    sidebar_open: bool,
    search: Option<(Entity<InputState>, Subscription)>,
    title: String,
}
impl Desktop {
    fn new(core: Arc<Core>, w: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(w, |_, w, cx| {
            gpui_component::Theme::sync_system_appearance(Some(w), cx);
            cx.notify()
        })
        .detach();
        let mut app = Self {
            core,
            state: Default::default(),
            reload_task: None,
            spinner_tick: None,
            spinner_phase: 0,
            hosts: vec![],
            machines: Default::default(),
            selected: None,
            confirmation: None,
            pairing: false,
            notice: String::new(),
            drafts: Default::default(),
            input: None,
            composing: false,
            sending: false,
            epoch: 0,
            size: 16.,
            scroll: screen::transcript(),
            sidebar_open: false,
            search: None,
            title: String::new(),
        };
        app.reload(cx);
        app
    }
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.state.registration.listing_known = false;
        let core = self.core.clone();
        // Replace any earlier reload: late results must not resurrect an old registration.
        self.reload_task = Some(cx.spawn(async move |this, cx| {
            let reader = core.clone();
            let hosts = cx
                .background_executor()
                .spawn(async move { reader.hosts() })
                .await;
            let Ok(pending) = this.update(cx, |this, cx| match hosts {
                Ok(hosts) => this.accept_hosts(hosts, cx),
                Err(e) => {
                    this.state.registration.read_failed(e);
                    cx.notify();
                    vec![]
                }
            }) else {
                return;
            };
            for h in pending {
                let (sender, rx) = Delivery::channel();
                let id = h.id.clone();
                let reader = core.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { reader.connect(id, Arc::new(sender)) })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        match result {
                            Ok(conn) => this.attach(h, conn, rx, cx),
                            Err(e) => {
                                this.state.registration.connection_errors.insert(h.id, e);
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    fn accept_hosts(&mut self, hosts: Vec<Host>, cx: &mut Context<Self>) -> Vec<Host> {
        self.state.registration.read_ok();
        for h in &self.hosts {
            if !model::registered(h, &hosts) {
                self.state.purge(&h.id, &mut self.drafts);
            }
        }
        self.machines
            .retain(|_, m| model::registered(&m.registration, &hosts));
        if self
            .selected
            .as_ref()
            .is_some_and(|(host, _)| !self.machines.contains_key(host))
        {
            self.selected = None;
            self.epoch += 1;
            self.input = None;
        }
        let pending = hosts
            .iter()
            .filter(|h| !h.needs_pairing && !self.machines.contains_key(&h.id))
            .cloned()
            .collect();
        self.hosts = hosts;
        cx.notify();
        pending
    }
    fn attach(
        &mut self,
        h: Host,
        conn: Arc<Connection>,
        rx: async_channel::Receiver<model::Batch>,
        cx: &mut Context<Self>,
    ) {
        let id = h.id.clone();
        self.state.registration.connection_errors.remove(&id);
        let generation = Arc::downgrade(&conn);
        let rows = Rows::snapshot(conn.snapshot());
        self.machines.insert(
            id.clone(),
            Machine {
                registration: h,
                conn,
                rows,
            },
        );
        cx.spawn(async move |this, cx| {
            while let Ok(batch) = rx.recv().await {
                if this
                    .update(cx, |this, cx| {
                        let old = this.editor();
                        let Some(m) = this.machines.get_mut(&id) else {
                            return;
                        };
                        if !generation.ptr_eq(&Arc::downgrade(&m.conn)) {
                            return;
                        }
                        m.apply(batch, &mut this.state.instructions);
                        if old != this.editor() {
                            this.epoch += 1;
                            this.notice.clear();
                            this.input = None;
                        }
                        cx.notify()
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
    fn paste(&mut self, cx: &mut Context<Self>) {
        self.confirmation = None;
        self.state.registration.pair_message.clear();
        if let Some(link) = cx.read_from_clipboard().and_then(|v| v.text())
            && let Some(p) = pimate_protocol::parse_pairing_link(link.trim())
        {
            self.confirmation = Some((
                link.trim().to_owned(),
                format!("{} · {}", p.host_name, p.address),
            ));
        } else {
            self.state.registration.pair_message =
                "有効なペアリングリンクをコピーしてください".into();
        }
        cx.notify();
    }
    fn pair(&mut self, cx: &mut Context<Self>) {
        if self.pairing {
            return;
        }
        let Some((link, _)) = self.confirmation.take() else {
            return;
        };
        let first = self.state.registration.listing_known && self.hosts.is_empty();
        self.pairing = true;
        let core = self.core.clone();
        cx.spawn(async move |this, cx| {
            let result = core.pair(link, "PiMate Desktop".into()).await;
            let _ = this.update(cx, |this, cx| this.finish_pair(result, first, cx));
        })
        .detach();
        cx.notify();
    }
    fn finish_pair(&mut self, result: Result<Host, Error>, first: bool, cx: &mut Context<Self>) {
        self.pairing = false;
        match result {
            Ok(h) => {
                self.state.purge(&h.id, &mut self.drafts);
                self.state
                    .registration
                    .paired(&h.id, first && self.hosts.is_empty());
                self.machines.remove(&h.id);
                self.state.registration.pair_message = "登録しました".into();
                self.reload(cx)
            }
            Err(Error::OutcomeUnknown) => {
                self.state.registration.save_unknown = true;
                self.reload(cx);
            }
            Err(e) => self.state.registration.pair_message = format!("登録できません: {e}"),
        };
        cx.notify();
    }
    fn select(&mut self, h: String, i: String, cx: &mut Context<Self>) {
        if let Some(m) = self.machines.get(&h) {
            if m.conn.subscribe(Some(i.clone())).is_err() {
                return;
            }
            if let Some((old, _)) = &self.selected
                && old != &h
                && let Some(m) = self.machines.get(old)
            {
                let _ = m.conn.subscribe(None);
            }
            self.epoch += 1;
            self.state.close_page();
            self.scroll = screen::transcript();
            self.input = None;
            self.selected = Some((h, i));
            self.sidebar_open = false;
            // 「登録しました」は次の操作まで。失敗の知らせ（error・save_unknown）は残す。
            self.state.registration.pair_message.clear();
            self.notice.clear();
            cx.notify()
        }
    }
    fn editor(&self) -> Option<EditorKey> {
        let (h, i) = self.selected.as_ref()?;
        self.machines.get(h)?.rows.editor(h, i)
    }
    fn bind_editor(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let key = self.editor();
        if self.input.as_ref().map(|v| &v.0) == key.as_ref() {
            return;
        }
        self.input = None;
        self.composing = false;
        let Some(key) = key else { return };
        let value = self.drafts.get(&key).cloned().unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(w, cx)
                .multi_line(true)
                .default_value(value)
                .placeholder("メッセージ")
        });
        let shown = key.clone();
        let sub = cx.subscribe(&input, move |this, state, event, cx| {
            if matches!(event, InputEvent::Change)
                && this.editor().as_ref() == Some(&shown)
                && this.input.as_ref().is_some_and(|v| v.1 == state)
            {
                this.drafts
                    .insert(shown.clone(), state.read(cx).value().to_string());
                cx.notify()
            }
        });
        // Marked-text edits notify the entity without emitting InputEvent::Change.
        let observer = cx.observe_in(&input, w, |this, state, w, cx| {
            let Some((bound, _, _)) = this.input.as_ref().filter(|v| v.1 == state) else {
                return;
            };
            if sync_composition(
                &state,
                w,
                cx,
                &mut this.composing,
                this.drafts.entry(bound.clone()).or_default(),
            ) {
                cx.notify();
            }
        });
        self.input = Some((key, input, [sub, observer]));
    }
    fn live_matches(
        &self,
        key: &EditorKey,
        text: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.input
            .as_ref()
            .is_some_and(|(k, input, _)| k == key && committed_input_matches(input, text, w, cx))
    }
    fn finish_send_notice(&mut self, result: Result<String, Error>) {
        self.notice = match result {
            Ok(_) => "Pi が受付（履歴は別途確認）",
            Err(
                Error::Blocked
                | Error::Rejected
                | Error::BadRequest
                | Error::Unsupported
                | Error::BuiltinCommand,
            ) => "送信は拒否されました",
            Err(Error::StaleTarget) => "対象が変わりました。現在の履歴を確認してください",
            Err(Error::Offline) => "送信していません。接続先を確認してください",
            Err(_) => "結果不明です。再送前に履歴を確認してください",
        }
        .into()
    }
}

fn sync_composition(
    input: &Entity<InputState>,
    w: &mut Window,
    cx: &mut App,
    composing: &mut bool,
    draft: &mut String,
) -> bool {
    input.update(cx, |s, cx| {
        let marked = s.marked_text_range(w, cx).is_some();
        if marked == *composing {
            return false;
        }
        *composing = marked;
        // macOS unmarkText can commit the displayed text without a Change event.
        if !marked {
            *draft = s.value().to_string();
        }
        true
    })
}

fn committed_input_matches(
    input: &Entity<InputState>,
    expected: &str,
    w: &mut Window,
    cx: &mut App,
) -> bool {
    input.update(cx, |s, cx| {
        s.marked_text_range(w, cx).is_none() && s.value().as_str() == expected
    })
}
/// Linux のタイトルバーは OS のものを使う。GNOME（Wayland の Mutter）だけは OS が描かず、
/// gpui からも見分けられないので、デスクトップ名で決めてアプリが描く（screen.rs）。
fn draws_own_titlebar(desktop: &str, wayland: bool) -> bool {
    cfg!(target_os = "linux")
        && wayland
        && desktop
            .split(':')
            .any(|name| name.eq_ignore_ascii_case("gnome"))
}
fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--version")) {
        println!("pimate-desktop {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    Application::new().with_assets(screen::Assets).run(|cx| {
        gpui_component::init(cx);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit()
            }
        })
        .detach();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.), px(800.)),
                    cx,
                ))),
                window_min_size: Some(size(px(480.), px(480.))),
                window_decorations: Some(
                    if draws_own_titlebar(
                        &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
                        std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
                    ) {
                        WindowDecorations::Client
                    } else {
                        WindowDecorations::Server
                    },
                ),
                ..Default::default()
            },
            |w, cx| {
                let view = cx.new(|cx| {
                    Desktop::new(Arc::new(Core::new(Arc::new(store::Store::open()))), w, cx)
                });
                cx.new(|cx| Root::new(view, w, cx))
            },
        )
        .expect("open PiMate window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::{committed_input_matches, sync_composition};
    #[test]
    #[cfg(target_os = "linux")]
    fn only_gnome_on_wayland_draws_its_own_titlebar() {
        for (desktop, wayland, own) in [
            ("GNOME", true, true),
            ("ubuntu:GNOME", true, true),
            ("ubuntu:GNOME", false, false),
            ("KDE", true, false),
            ("", true, false),
        ] {
            assert_eq!(
                super::draws_own_titlebar(desktop, wayland),
                own,
                "{desktop}"
            );
        }
    }
    use gpui::{AppContext, EntityInputHandler};
    use gpui_component::input::InputState;
    #[gpui::test]
    fn unmark_without_replacement_commits_composed_draft(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        cx.add_empty_window().update(|w, cx| {
            let mut draft = String::from("明日");
            let mut composing = false;
            let input = cx.new(|cx| {
                InputState::new(w, cx)
                    .multi_line(true)
                    .default_value(draft.clone())
            });
            input.update(cx, |s, cx| {
                s.replace_and_mark_text_in_range(Some(2..2), "会う", None, w, cx)
            });
            assert!(sync_composition(&input, w, cx, &mut composing, &mut draft));
            assert!(composing);
            assert_eq!(draft, "明日");
            input.update(cx, |s, cx| s.unmark_text(w, cx));
            assert!(sync_composition(&input, w, cx, &mut composing, &mut draft));
            assert!(!composing);
            assert_eq!(draft, "明日会う");
            assert!(committed_input_matches(&input, &draft, w, cx));
        });
    }
    #[gpui::test]
    fn send_and_ack_require_the_live_committed_input(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        cx.update(|w, cx| {
            let input = cx.new(|cx| {
                InputState::new(w, cx)
                    .multi_line(true)
                    .default_value("sent")
            });
            assert!(committed_input_matches(&input, "sent", w, cx));
            input.update(cx, |s, cx| {
                s.replace_and_mark_text_in_range(Some(4..4), "追加", None, w, cx)
            });
            assert!(!committed_input_matches(&input, "sent", w, cx));
            assert!(!committed_input_matches(&input, "sent追加", w, cx));
            input.update(cx, |s, cx| s.replace_text_in_range(None, "追加", w, cx));
            assert!(committed_input_matches(&input, "sent追加", w, cx));
        });
    }
}
