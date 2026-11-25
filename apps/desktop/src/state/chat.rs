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
    Shutdown,
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
            Shutdown => (M::Shutdown, json!({})),
        }
    }
}
impl Page {
    pub async fn sheet(&self, a: SheetAction) -> Result<Value, Error> {
        let (m, p) = a.wire();
        self.rc(m, p).await
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
    pub fn restore_draft(&mut self, page: &Page, id: String) -> Result<(), Error> {
        let k = page.editor()?;
        page.current()?;
        if !page.visible(self)
            || self.composing
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
        let kind = if let Some(name) = text.trim_end().strip_prefix('/')
            && r.get("command", name)["source"] == "builtin"
        {
            if !images.is_empty() {
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
        };
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
    pub open: Option<String>,
}
impl Submission {
    pub async fn submit(mut self) -> Result<Accepted, Error> {
        self.page.current()?;
        let mut open = None;
        match std::mem::replace(&mut self.kind, SubmitKind::Send) {
            SubmitKind::Open(s) => open = Some(s),
            SubmitKind::Sheet(a) => {
                self.page.sheet(a).await?;
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
