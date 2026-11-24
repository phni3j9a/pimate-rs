mod markdown;
mod model;
mod screen;
mod store;
use gpui::{prelude::*, *};
use gpui_component::{
    Root,
    input::{InputEvent, InputState},
};
use model::{Delivery, EditorKey, Rows};
use pimate_core::{Connection, Core, Error, Host, Operation};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

struct Machine {
    conn: Arc<Connection>,
    rows: Rows,
}
struct Desktop {
    core: Arc<Core>,
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
    scroll: ScrollHandle,
}
impl Desktop {
    fn new(w: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(w, |_, w, cx| {
            gpui_component::Theme::sync_system_appearance(Some(w), cx);
            cx.notify()
        })
        .detach();
        let mut app = Self {
            core: Arc::new(Core::new(Arc::new(store::Store::open()))),
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
            scroll: ScrollHandle::new(),
        };
        app.reload(cx);
        app
    }
    fn reload(&mut self, cx: &mut Context<Self>) {
        let hosts = match self.core.hosts() {
            Ok(h) => h,
            Err(e) => {
                self.notice = format!("登録を読み出せません: {e}");
                return;
            }
        };
        for h in &hosts {
            if h.needs_pairing || self.machines.contains_key(&h.id) {
                continue;
            }
            let (sender, rx) = Delivery::channel();
            let id = h.id.clone();
            match self.core.connect(id.clone(), Arc::new(sender)) {
                Err(e) => self.notice = format!("接続を開始できません: {e}"),
                Ok(conn) => {
                    let generation = Arc::downgrade(&conn);
                    let rows = Rows::snapshot(conn.snapshot());
                    self.machines.insert(id.clone(), Machine { conn, rows });
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
                                    m.rows.apply(batch, || m.conn.snapshot());
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
            }
        }
        self.hosts = hosts;
        cx.notify();
    }
    fn paste(&mut self, cx: &mut Context<Self>) {
        self.confirmation = None;
        self.notice.clear();
        if let Some(link) = cx.read_from_clipboard().and_then(|v| v.text())
            && let Some(p) = pimate_protocol::parse_pairing_link(link.trim())
        {
            self.confirmation = Some((
                link.trim().to_owned(),
                format!("{} · {}", p.host_name, p.address),
            ));
        } else {
            self.notice = "有効なペアリングリンクをコピーしてください".into()
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
        self.pairing = true;
        let core = self.core.clone();
        cx.spawn(async move |this, cx| {
            let result = core.pair(link, "PiMate Desktop".into()).await;
            let _ = this.update(cx, |this, cx| {
                this.pairing = false;
                match result {
                    Ok(h) => {
                        this.machines.remove(&h.id);
                        this.notice = "登録しました".into();
                        this.reload(cx)
                    }
                    Err(e) => this.notice = format!("登録できません: {e}"),
                };
                cx.notify()
            });
        })
        .detach();
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
            self.scroll = ScrollHandle::new();
            self.input = None;
            self.selected = Some((h, i));
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
            if !this.input.as_ref().is_some_and(|v| v.1 == state) {
                return;
            }
            let marked = state.update(cx, |s, cx| s.marked_text_range(w, cx).is_some());
            if marked != this.composing {
                this.composing = marked;
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
        self.input.as_ref().is_some_and(|(k, input, _)| {
            k == key
                && input.update(cx, |s, cx| {
                    s.marked_text_range(w, cx).is_none() && s.value().as_str() == text
                })
        })
    }
    fn send(&mut self, key: EditorKey, w: &mut Window, cx: &mut Context<Self>) {
        if self.sending || self.editor().as_ref() != Some(&key) {
            return;
        }
        let Some(text) = self.drafts.get(&key).cloned() else {
            return;
        };
        if !self.live_matches(&key, &text, w, cx) {
            return;
        }
        let conn = self.machines[&key.host].conn.clone();
        let op = Operation::Send {
            target: key.target(),
            text: text.clone(),
            images: vec![],
        };
        if let Err(e) = conn.check(op.clone()) {
            self.notice = format!("送信していません: {e}");
            cx.notify();
            return;
        }
        self.notice.clear();
        self.sending = true;
        let epoch = self.epoch;
        cx.spawn_in(w, async move |this, cx| {
            let result = conn.perform(op).await;
            let _ = this.update_in(cx, |this, w, cx| {
                this.sending = false;
                let bound = this.input.as_ref().is_some_and(|i| i.0 == key);
                if (!bound || this.live_matches(&key, &text, w, cx))
                    && model::clear_accepted(&mut this.drafts, &key, &text, result.is_ok())
                    && bound
                {
                    this.input = None;
                }
                if this
                    .machines
                    .get(&key.host)
                    .is_some_and(|m| Arc::ptr_eq(&m.conn, &conn))
                    && this.epoch == epoch
                    && this.editor().as_ref() == Some(&key)
                {
                    this.notice = match result {
                        Ok(_) => "Pi が受付（履歴は別途確認）",
                        Err(
                            Error::Blocked
                            | Error::Rejected
                            | Error::BadRequest
                            | Error::Unsupported
                            | Error::BuiltinCommand,
                        ) => "送信は拒否されました",
                        Err(Error::StaleTarget) => {
                            "対象が変わりました。現在の履歴を確認してください"
                        }
                        Err(Error::Offline) => "送信していません。接続先を確認してください",
                        Err(_) => "結果不明です。再送前に履歴を確認してください",
                    }
                    .into()
                }
                cx.notify()
            });
        })
        .detach();
        cx.notify();
    }
}

fn main() {
    Application::new().run(|cx| {
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
                    size(px(980.), px(720.)),
                    cx,
                ))),
                ..Default::default()
            },
            |w, cx| {
                w.set_window_title("PiMate RS");
                let view = cx.new(|cx| Desktop::new(w, cx));
                cx.new(|cx| Root::new(view, w, cx))
            },
        )
        .expect("open PiMate window");
        cx.activate(true);
    });
}
