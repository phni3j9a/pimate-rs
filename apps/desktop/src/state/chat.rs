use super::*;
use gpui::{Context, Window};
use pimate_protocol::{self as p, RcMethod as M};
use serde_json::json;
use std::collections::HashMap;

pub enum SheetAction {
    Commands,
    Models,
    Select(p::ModelRef),
    Favorites(Vec<p::ModelRef>),
    Default(p::SetDefaultModelParams),
    Thinking(p::ThinkingLevel),
    Sessions,
    Tree,
    New,
    Reload,
    Resume(String),
    Fork(String),
    Move(String, bool),
    Label(p::SetLabelParams),
    Compact,
}
impl SheetAction {
    fn wire(self) -> (M, Value) {
        use SheetAction::*;
        match self {
            Commands => (M::Commands, json!({})),
            Models => (M::Models, json!({})),
            Select(m) => (M::SetModel, json!(m)),
            Favorites(m) => (M::SetScopedModels, json!({"models":m})),
            Default(v) => (M::SetDefaultModel, json!(v)),
            Thinking(level) => (M::SetThinking, json!({"level":level})),
            Sessions => (M::Sessions, json!({})),
            Tree => (M::SessionTree, json!({})),
            New => (M::SessionOp, json!({"op":"new"})),
            Reload => (M::SessionOp, json!({"op":"reload"})),
            Resume(arg) => (M::SessionOp, json!({"op":"switch","arg":arg})),
            Fork(arg) => (M::SessionOp, json!({"op":"fork","arg":arg})),
            Move(arg, summarize) => (
                M::SessionOp,
                json!({"op":"tree","arg":arg,"summarize":summarize}),
            ),
            Label(v) => (M::SetLabel, json!(v)),
            Compact => (M::Compact, json!({})),
        }
    }
}
impl Page {
    fn sheet_operation(&self, a: SheetAction) -> Result<Operation, Error> {
        if matches!(
            a,
            SheetAction::New
                | SheetAction::Reload
                | SheetAction::Resume(_)
                | SheetAction::Fork(_)
                | SheetAction::Move(_, _)
        ) {
            let rows = self.current()?;
            if rows.get("view", "status") != "idle" || rows.get("view", "agentActive") == true {
                return Err(Error::Blocked);
            }
        }
        let (m, p) = a.wire();
        Ok(Operation::Rc {
            target: self.editor()?.target(),
            method: m,
            params: p.to_string(),
        })
    }
    pub async fn sheet(&self, a: SheetAction) -> Result<Value, Error> {
        self.perform(self.sheet_operation(a)?).await
    }
    pub async fn answer(
        &self,
        id: &str,
        shown: &Value,
        mut fields: p::UiRespondParams,
    ) -> Result<Value, Error> {
        if shown.is_null() || self.current()?.get("question", id) != shown {
            return Err(Error::StaleTarget);
        }
        fields.request_id = id.into();
        self.perform(Operation::Answer {
            target: self.editor()?.target(),
            params: json!(fields).to_string(),
        })
        .await
    }
    pub fn command_ticket(&self, name: &str) -> Result<Submission, Error> {
        let r = self.current()?;
        if r.get("command", name).is_null() {
            return Err(Error::StaleTarget);
        }
        let text = format!("/{name}");
        let kind = submission_kind(&r, &text, false)?;
        Ok(Submission {
            page: self.clone(),
            text,
            images: vec![],
            kind,
            _lock: Arc::new(()),
        })
    }
    pub fn suggestions(&self, text: String) -> Result<Vec<String>, Error> {
        self.current()?;
        Ok(self.live()?.command_suggestions(text))
    }
    pub fn usage(
        &self,
        now_ms: u64,
    ) -> Result<(pimate_core::SessionStats, Vec<pimate_core::Allowance>), Error> {
        self.current()?;
        let c = self.live()?;
        Ok((c.session_stats(), c.allowances(now_ms)))
    }
    pub async fn entry_image(&self, id: &str, index: u64, thumbnail: bool) -> Result<Value, Error> {
        self.rc(
            M::EntryImage,
            json!({"entryId":id,"index":index,"thumbnail":thumbnail}),
        )
        .await
    }
    pub async fn file_image(&self, path: &str, thumbnail: bool) -> Result<Value, Error> {
        self.rc(M::FileImage, json!({"path":path,"thumbnail":thumbnail}))
            .await
    }
}
pub fn attach_command(name: &str) -> Option<String> {
    (name.len() <= 100
        && name.as_bytes().first()?.is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
    .then(|| format!("pimate attach {name}"))
}
#[derive(Default)]
pub struct Attachments {
    items: HashMap<EditorKey, Vec<p::PromptImage>>,
    picking: Weak<()>,
    submitting: Weak<()>,
}
impl Attachments {
    pub(super) fn forget(&mut self, id: &str) {
        self.items.retain(|k, _| k.host != id)
    }
    pub fn images(&self, k: &EditorKey) -> &[p::PromptImage] {
        self.items.get(k).map(Vec::as_slice).unwrap_or_default()
    }
    pub fn busy(&self) -> bool {
        self.picking.strong_count() > 0 || self.submitting.strong_count() > 0
    }
    fn replace(&mut self, k: &EditorKey, images: Vec<p::PromptImage>) {
        if images.is_empty() {
            self.items.remove(k);
        } else {
            self.items.insert(k.clone(), images);
        }
    }
    pub fn remove(
        &mut self,
        p: &Page,
        shown: &[p::PromptImage],
        index: usize,
    ) -> Result<(), Error> {
        p.current()?;
        let k = p.editor()?;
        if self.submitting.strong_count() > 0 {
            return Err(Error::Blocked);
        }
        if self.images(k) != shown || index >= shown.len() {
            return Err(Error::StaleTarget);
        }
        let mut next = shown.to_vec();
        next.remove(index);
        self.replace(k, next);
        Ok(())
    }
}
pub struct Picker {
    page: Page,
    original: Vec<p::PromptImage>,
    _lock: Arc<()>,
}
impl Desktop {
    pub fn start_picker(&mut self, page: &Page) -> Result<Picker, Error> {
        if self.sending || self.state.attachments.busy() || !page.visible(self) {
            return Err(Error::Blocked);
        }
        let r = page.current()?;
        if r.get("view", "promptImages") != true {
            return Err(Error::Unsupported);
        }
        let original = self.state.attachments.images(page.editor()?).to_vec();
        if original.len() >= image_limit(&r) {
            return Err(Error::Blocked);
        }
        let lock = Arc::new(());
        self.state.attachments.picking = Arc::downgrade(&lock);
        Ok(Picker {
            page: page.clone(),
            original,
            _lock: lock,
        })
    }
    pub fn finish_picker(
        &mut self,
        ticket: Picker,
        images: Vec<p::PromptImage>,
    ) -> Result<(), Error> {
        let r = ticket.page.current()?;
        let k = ticket.page.editor()?;
        if !ticket.page.visible(self) || self.state.attachments.images(k) != ticket.original {
            return Err(Error::StaleTarget);
        }
        if images.is_empty() {
            return Ok(());
        }
        let mut next = ticket.original;
        if r.get("view", "promptImages") != true
            || images.len() > image_limit(&r).saturating_sub(next.len())
        {
            return Err(Error::Blocked);
        }
        next.extend(images);
        if !p::valid_prompt_images(&json!(next)) {
            return Err(Error::BadRequest);
        }
        self.state.attachments.replace(k, next);
        Ok(())
    }
    pub fn restore_draft(
        &mut self,
        page: &Page,
        id: String,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), Error> {
        let k = page.editor()?;
        page.current()?;
        if !page.visible(self)
            || self.composing
            || (self.input.is_some() && !self.live_matches(k, "", w, cx))
            || self.sending
            || self.state.attachments.busy()
            || !self.state.attachments.images(k).is_empty()
            || self.drafts.get(k).is_some_and(|s| !s.is_empty())
        {
            return Err(Error::Blocked);
        }
        let d = page.live()?.take_unconfirmed(k.target(), id)?;
        let images = d
            .images
            .into_iter()
            .map(|i| {
                serde_json::from_value(json!({"type":"image","mimeType":i.mime_type,"data":i.data}))
                    .map_err(|_| Error::InvalidMessage)
            })
            .collect::<Result<_, _>>()?;
        self.state.attachments.replace(k, images);
        self.drafts.insert(k.clone(), d.text);
        self.input = None;
        Ok(())
    }
    pub fn prepare_submission(
        &mut self,
        page: &Page,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Submission, Error> {
        let k = page.editor()?;
        let r = page.current()?;
        let text = self.drafts.get(k).cloned().unwrap_or_default();
        let images = self.state.attachments.images(k).to_vec();
        if !page.visible(self)
            || self.sending
            || self.state.attachments.busy()
            || !self.live_matches(k, &text, w, cx)
            || self.machines[&k.host]
                .rows
                .pending_move_with(k, |old| {
                    self.drafts.get(old).is_some_and(|s| !s.is_empty())
                        || !self.state.attachments.images(old).is_empty()
                })
                .is_some()
        {
            return Err(Error::Blocked);
        }
        let kind = submission_kind(&r, &text, !images.is_empty())?;
        let lock = Arc::new(());
        self.state.attachments.submitting = Arc::downgrade(&lock);
        Ok(Submission {
            page: page.clone(),
            text,
            images,
            kind,
            _lock: lock,
        })
    }
}
fn submission_kind(r: &Rows, text: &str, images: bool) -> Result<SubmitKind, Error> {
    Ok(
        if let Some(name) = p::slash_command_name(text)
            && text[name.len() + 1..]
                .chars()
                .all(|c| c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}'))
            && r.get("command", name)["source"] == "builtin"
        {
            if images {
                return Err(Error::BadRequest);
            }
            match name {
                "model" | "resume" | "fork" | "tree" => SubmitKind::Open(name.to_owned()),
                "new" => SubmitKind::Sheet(SheetAction::New),
                "reload" => SubmitKind::Sheet(SheetAction::Reload),
                "compact" => SubmitKind::Sheet(SheetAction::Compact),
                _ => return Err(Error::Unsupported),
            }
        } else {
            SubmitKind::Send
        },
    )
}
fn image_limit(r: &Rows) -> usize {
    r.get("view", "promptImageLimit")
        .as_u64()
        .unwrap_or(1)
        .min(4) as usize
}
enum SubmitKind {
    Open(String),
    Sheet(SheetAction),
    Send,
}
pub struct Submission {
    page: Page,
    text: String,
    images: Vec<p::PromptImage>,
    kind: SubmitKind,
    _lock: Arc<()>,
}
pub struct Accepted {
    ticket: Submission,
    open: Option<String>,
}
impl Submission {
    pub async fn submit(mut self) -> Result<Accepted, Error> {
        self.page.current()?;
        let mut open = None;
        match std::mem::replace(&mut self.kind, SubmitKind::Send) {
            SubmitKind::Open(s) => open = Some(s),
            SubmitKind::Sheet(a) => {
                // The receipt is correlated with our request, even if it caused a view switch.
                let op = self.page.sheet_operation(a)?;
                self.page.live()?.perform(op).await?;
            }
            SubmitKind::Send => {
                let images = self
                    .images
                    .iter()
                    .map(|i| pimate_core::Image {
                        mime_type: json!(i.mime_type).as_str().unwrap().into(),
                        data: i.data.clone(),
                    })
                    .collect();
                self.page
                    .live()?
                    .perform(Operation::Send {
                        target: self.page.editor()?.target(),
                        text: self.text.clone(),
                        images,
                    })
                    .await?;
            }
        }
        Ok(Accepted { ticket: self, open })
    }
}
impl Accepted {
    pub fn opened_sheet(self) -> Option<String> {
        self.open
    }
    pub fn finish(
        self,
        app: &mut Desktop,
        w: &mut Window,
        cx: &mut Context<Desktop>,
    ) -> Option<String> {
        let t = self.ticket;
        let k = t.page.editor().ok()?;
        let bound = app.input.as_ref().is_some_and(|i| &i.0 == k);
        if t.page
            .conn
            .ptr_eq(&Arc::downgrade(&app.machines.get(&k.host)?.conn))
            && app.drafts.get(k).map(String::as_str).unwrap_or_default() == t.text
            && app.state.attachments.images(k) == t.images
            && (!bound || app.live_matches(k, &t.text, w, cx))
        {
            app.drafts.remove(k);
            app.state.attachments.replace(k, vec![]);
            if bound {
                app.input = None;
            }
        }
        self.open.filter(|_| t.page.visible(app))
    }
}
impl Picker {
    fn image_budget(&self, count: usize) -> Result<usize, Error> {
        let r = self.page.current()?;
        if count == 0 || count > image_limit(&r).saturating_sub(self.original.len()) {
            return Err(Error::Blocked);
        }
        let used = self
            .original
            .iter()
            .map(|i| i.data.len() / 4 * 3 - i.data.bytes().rev().take_while(|&b| b == b'=').count())
            .sum();
        Ok(p::MAX_PROMPT_IMAGE_BYTES
            .min(p::MAX_PROMPT_TOTAL_IMAGE_BYTES.saturating_sub(used) / count))
    }
    pub fn clipboard(&self, images: &[Vec<u8>]) -> Result<Vec<p::PromptImage>, Error> {
        let budget = self.image_budget(images.len())?;
        images
            .iter()
            .map(|b| super::images::encode_bytes(b, budget))
            .collect()
    }
    pub fn encode(&self, paths: &[std::path::PathBuf]) -> Result<Vec<p::PromptImage>, Error> {
        let budget = self.image_budget(paths.len())?;
        paths
            .iter()
            .map(|path| {
                self.page.live()?;
                super::images::encode_file(path, budget)
            })
            .collect()
    }
}
impl Desktop {
    /// Poll after rendering a ready editor. Do not put returned fork text over user edits.
    pub fn receive_editor_text(&mut self, w: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(k) = self.editor() else { return false };
        let Some(session) = &k.session else {
            return false;
        };
        let empty = self.drafts.get(&k).is_none_or(String::is_empty)
            && self.state.attachments.images(&k).is_empty()
            && !self.state.attachments.busy()
            && !self.sending
            && (self.input.is_none() || self.live_matches(&k, "", w, cx));
        let rows = &mut self.machines.get_mut(&k.host).unwrap().rows;
        let start = rows.get("view", "startedAt").as_u64();
        if let Some(text) = rows.receipts.take_text(&k.instance, start, session)
            && empty
        {
            self.drafts.insert(k, text);
            self.input = None;
            return true;
        }
        false
    }
}

pub struct Shutdown(Page);
impl Page {
    pub fn shutdown_confirmation(&self) -> Result<Shutdown, Error> {
        self.editor()?;
        if self.start.as_u64().is_none() {
            return Err(Error::StaleTarget);
        }
        self.current()?;
        Ok(Shutdown(self.clone()))
    }
}
impl Shutdown {
    pub async fn confirm(self) -> Result<Value, Error> {
        self.0.rc(M::Shutdown, json!({})).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SheetPage {
    Model,
    Usage,
    Resume,
    Fork,
    Tree,
    Commands,
}
pub struct Sheet {
    _life: Arc<()>,
    pub page: Page,
    pub kind: SheetPage,
    pending: Weak<()>,
    pub reply: Option<Value>,
    pub error: Option<Error>,
}
pub struct SheetRequest {
    page: Page,
    lock: Arc<()>,
    action: SheetAction,
}
impl Desktop {
    pub fn open_sheet(&mut self, kind: SheetPage) -> Result<Page, Error> {
        // A sheet has its own lifetime: closing it invalidates its requests, not the inspector behind it.
        let life = Arc::new(());
        let mut page = self.background_chat().ok_or(Error::StaleTarget)?;
        page.page = Arc::downgrade(&life);
        self.push_overlay(crate::screen::keys::Overlay::Sheet);
        self.state.sheet = Some(Sheet {
            _life: life,
            page: page.clone(),
            kind,
            pending: Weak::new(),
            reply: None,
            error: None,
        });
        Ok(page)
    }
    pub fn active_sheet(&self) -> Option<&Sheet> {
        self.state.sheet.as_ref().filter(|s| s.page.visible(self))
    }
}
impl Sheet {
    pub fn loading(&self) -> bool {
        self.pending.strong_count() > 0
    }
    pub fn begin(&mut self, action: SheetAction) -> Result<SheetRequest, Error> {
        self.page.live()?;
        if self.loading() {
            return Err(Error::Blocked);
        }
        let lock = Arc::new(());
        self.pending = Arc::downgrade(&lock);
        self.reply = None;
        self.error = None;
        Ok(SheetRequest {
            page: self.page.clone(),
            lock,
            action,
        })
    }
}
pub struct SheetReply {
    page: Page,
    lock: Arc<()>,
    result: Result<Value, Error>,
}
impl SheetRequest {
    pub async fn run(self) -> SheetReply {
        let result = self.page.sheet(self.action).await;
        SheetReply {
            page: self.page,
            lock: self.lock,
            result,
        }
    }
}
impl SheetReply {
    pub fn finish(self, app: &mut Desktop) {
        if self.page.visible(app)
            && let Some(s) = &mut app.state.sheet
            && s.pending.ptr_eq(&Arc::downgrade(&self.lock))
        {
            match self.result {
                Ok(v) => s.reply = Some(v),
                Err(e) => s.error = Some(e),
            }
        }
    }
}
/// The value being edited belongs to one version of one question, never to a bare request ID.
pub struct Question {
    page: Page,
    id: String,
    shown: Value,
    pub fields: p::UiRespondParams,
}
impl Page {
    pub fn question(&self, id: &str, shown: &Value) -> Result<Question, Error> {
        if shown.is_null() || self.current()?.get("question", id) != shown {
            return Err(Error::StaleTarget);
        }
        Ok(Question {
            page: self.clone(),
            id: id.into(),
            shown: shown.clone(),
            fields: p::UiRespondParams {
                request_id: id.into(),
                value: None,
                confirmed: None,
                cancelled: None,
                answers: None,
            },
        })
    }
    pub async fn abort(&self) -> Result<Value, Error> {
        if !self.current()?.can_stop() {
            return Err(Error::Blocked);
        }
        self.perform(Operation::Abort {
            target: self.editor()?.target(),
        })
        .await
    }
}
impl Question {
    pub async fn answer(self) -> Result<Value, Error> {
        self.page.answer(&self.id, &self.shown, self.fields).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slash_requires_exact_builtin_without_images() {
        let mut r = Rows::default();
        r.apply(
            (
                1,
                vec![
                    super::super::tests::row("command", "new", json!({"source":"builtin"})),
                    super::super::tests::row("command", "tree", json!({"source":"builtin"})),
                ],
            ),
            || panic!(),
        );
        assert!(matches!(
            submission_kind(&r, "/new", false),
            Ok(SubmitKind::Sheet(SheetAction::New))
        ));
        assert!(matches!(
            submission_kind(&r, " /new", false),
            Ok(SubmitKind::Send)
        ));
        assert!(matches!(submission_kind(&r,"/tree",false),Ok(SubmitKind::Open(s))if s=="tree"));
        assert!(matches!(
            submission_kind(&r, "/new", true),
            Err(Error::BadRequest)
        ));
    }
    #[test]
    fn slash_uses_ecmascript_not_rust_whitespace() {
        let mut rows = Rows::default();
        rows.apply(
            (
                1,
                vec![super::super::tests::row(
                    "command",
                    "new",
                    json!({"source":"builtin"}),
                )],
            ),
            || panic!(),
        );
        assert!(matches!(
            submission_kind(&rows, "/new\u{85}", false),
            Ok(SubmitKind::Send)
        ));
        assert!(matches!(
            submission_kind(&rows, "/new\u{feff}", false),
            Ok(SubmitKind::Sheet(SheetAction::New))
        ));
        assert!(matches!(
            submission_kind(&rows, "/new arg", false),
            Ok(SubmitKind::Send)
        ));
    }
    #[test]
    fn no_shell_syntax_from_host() {
        for s in ["", "--help", "a b", "a;whoami", "a\n", "$(x)"] {
            assert!(attach_command(s).is_none())
        }
        assert_eq!(
            attach_command("pimate-pi.1").unwrap(),
            "pimate attach pimate-pi.1"
        );
    }
}
#[cfg(test)]
#[tokio::test]
async fn builtin_ack_after_view_change_belongs_to_original_submission() {
    let (mut m, rx, mut wire, server) = super::tests::fixture("chat").await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        m.conn.subscribe(Some("pi".into())).unwrap();
        while m.rows.editor("chat", "pi").is_none() {
            m.rows.apply(rx.recv().await.unwrap(), || m.conn.snapshot());
        }
        let mut s = DesktopState::default();
        let page = s.open("chat", &m, m.rows.editor("chat", "pi"));
        let submission = Submission {
            page,
            text: "/new".into(),
            images: vec![],
            kind: SubmitKind::Sheet(SheetAction::New),
            _lock: Arc::new(()),
        };
        let ack = submission.submit().await.unwrap();
        assert_eq!(wire.recv().await.unwrap(), "session_op");
        assert!(matches!(ack.ticket.page.current(), Err(Error::StaleTarget)));
        assert_eq!(
            ack.ticket.page.editor().unwrap().session.as_deref(),
            Some("same")
        );
        assert!(ack.open.is_none());
    })
    .await
    .unwrap();
    server.abort();
}

#[cfg(test)]
#[gpui::test]
fn accepted_submission_clears_only_an_unchanged_origin(cx: &mut gpui::TestAppContext) {
    use pimate_core::{Core, Credential, Host, SecretStore};
    struct Empty;
    impl SecretStore for Empty {
        fn hosts(&self) -> Result<Vec<Host>, Error> {
            Ok(vec![])
        }
        fn load(&self, _: String) -> Result<Credential, Error> {
            unreachable!()
        }
        fn save(&self, _: Host, _: String) -> Result<(), Error> {
            unreachable!()
        }
        fn invalidate(&self, _: String, _: String) -> Result<(), Error> {
            unreachable!()
        }
        fn forget(&self, _: String) -> Result<(), Error> {
            unreachable!()
        }
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (machine, _rx, _wire, server) = rt.block_on(super::tests::fixture("origin"));
    cx.update(gpui_component::init);
    cx.add_window_view(move |w, cx| {
        let key = EditorKey {
            host: "origin".into(),
            instance: "pi".into(),
            session: Some("s".into()),
        };
        let other = EditorKey {
            host: "other".into(),
            ..key.clone()
        };
        let mut app = Desktop::new(Arc::new(Core::new(Arc::new(Empty))), w, cx);
        let page = app.state.open("origin", &machine, Some(key.clone()));
        app.machines.insert(key.host.clone(), machine);
        let ack = || Accepted {
            ticket: Submission {
                page: page.clone(),
                text: "sent".into(),
                images: vec![],
                kind: SubmitKind::Send,
                _lock: Arc::new(()),
            },
            open: None,
        };
        app.drafts.insert(other.clone(), "sent".into());
        app.drafts.insert(key.clone(), "later edit".into());
        ack().finish(&mut app, w, cx);
        assert_eq!(app.drafts[&key], "later edit");
        app.drafts.insert(key.clone(), "sent".into());
        ack().finish(&mut app, w, cx);
        assert!(!app.drafts.contains_key(&key));
        assert_eq!(app.drafts[&other], "sent");
        app
    });
    server.abort();
}
