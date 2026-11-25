//! Screen-facing state; no credentials, drafts or responses are logged.
#![allow(dead_code)] // Used by the separately developed GPUI screens.
use crate::{
    Desktop, Machine,
    model::{EditorKey, Rows},
};
use pimate_core::{Connection, Error, Operation};
use serde_json::Value;
use std::sync::{Arc, Weak};
mod agents;
mod chat;
pub(crate) use agents::*;
pub(crate) mod images;
mod machine;
pub(crate) mod receipts;
pub(crate) mod review;
pub(crate) use chat::*;
pub(crate) use machine::*;

#[derive(Default)]
pub(crate) struct DesktopState {
    page: Arc<()>,
    pub registration: Registration,
    pub sheet: Option<Sheet>,
    pub attachments: Attachments,
    pub settings: Settings,
    pub instructions: Instructions,
}
#[derive(Clone)]
pub(crate) struct Page {
    host: String,
    images: Arc<images::Images>,
    key: Option<EditorKey>,
    start: Value,
    conn: Weak<Connection>,
    life: Weak<()>,
    page: Weak<()>,
}
impl Desktop {
    pub(crate) fn open_machine(&mut self, id: &str) -> Result<Page, Error> {
        let machine = self.machines.get(id).ok_or(Error::StaleTarget)?;
        Ok(self.state.open(id, machine, None))
    }
    pub(crate) fn open_chat(&mut self) -> Result<Page, Error> {
        let k = self.editor().ok_or(Error::StaleTarget)?;
        Ok(self
            .state
            .open(&k.host, &self.machines[&k.host], Some(k.clone())))
    }
}
impl DesktopState {
    pub fn close_page(&mut self) {
        self.page = Arc::new(());
        self.sheet = None;
    }
    fn open(&mut self, host: &str, m: &Machine, key: Option<EditorKey>) -> Page {
        self.close_page();
        Page {
            host: host.into(),
            images: Default::default(),
            start: m.rows.get("view", "startedAt").clone(),
            conn: Arc::downgrade(&m.conn),
            page: Arc::downgrade(&self.page),
            life: Arc::downgrade(if key.is_some() {
                &m.rows.view_life
            } else {
                &m.rows.host_life
            }),
            key,
        }
    }
}
impl Page {
    pub fn host(&self) -> &str {
        &self.host
    }
    fn live(&self) -> Result<Arc<Connection>, Error> {
        if self.page.strong_count() == 0 || self.life.strong_count() == 0 {
            return Err(Error::StaleTarget);
        }
        self.conn.upgrade().ok_or(Error::StaleTarget)
    }
    fn current(&self) -> Result<Rows, Error> {
        let r = Rows::snapshot(self.live()?.snapshot());
        if r.get("host", "connection") != "connected" {
            return Err(Error::Offline);
        }
        if let Some(k) = &self.key
            && (r.editor(&k.host, &k.instance).as_ref() != Some(k)
                || r.get("view", "startedAt") != &self.start)
        {
            return Err(Error::StaleTarget);
        }
        Ok(r)
    }
    fn editor(&self) -> Result<&EditorKey, Error> {
        self.key.as_ref().ok_or(Error::StaleTarget)
    }
    async fn perform(&self, op: Operation) -> Result<Value, Error> {
        self.current()?;
        let raw = self.live()?.perform(op).await?;
        // A write may already have run. Never convert lost attribution into success.
        self.current().map_err(|_| Error::OutcomeUnknown)?;
        serde_json::from_str(&raw).map_err(|_| Error::OutcomeUnknown)
    }
    async fn rc(&self, method: pimate_protocol::RcMethod, params: Value) -> Result<Value, Error> {
        self.perform(Operation::Rc {
            target: self.editor()?.target(),
            method,
            params: params.to_string(),
        })
        .await
    }
    /// Recheck when consuming an async reply on the UI thread, including error banners.
    pub fn visible(&self, app: &Desktop) -> bool {
        self.live().is_ok()
            && self
                .key
                .as_ref()
                .is_none_or(|k| app.editor().as_ref() == Some(k))
    }
}

#[derive(Default)]
pub struct Registration {
    pub listing_known: bool,
    pub pair_message: String,
    pub save_unknown: bool,
    pub error: Option<Error>,
    pub paired_host: Option<String>,
}
impl Registration {
    pub fn read_failed(&mut self, e: Error) {
        self.listing_known = false;
        self.error = Some(e)
    }
    pub fn read_ok(&mut self) {
        self.listing_known = true;
        self.error = None;
    }
    pub fn paired(&mut self, id: &str, first: bool) {
        self.save_unknown = false;
        self.error = None;
        self.paired_host = first.then(|| id.into());
    }
    pub fn message(&self) -> String {
        match (self.save_unknown, self.error) {
            (true, Some(e)) => format!(
                "保存の結果が不明です。登録も読み出せません: {e}。再登録前に一覧を確認してください"
            ),
            (true, None) => "保存の結果が不明です。再登録前に一覧を確認してください".into(),
            (false, Some(e)) => format!("登録を読み出せません: {e}"),
            _ => self.pair_message.clone(),
        }
    }
}
impl DesktopState {
    pub(crate) fn purge(
        &mut self,
        host: &str,
        drafts: &mut std::collections::HashMap<EditorKey, String>,
    ) {
        self.attachments.forget(host);
        self.instructions.forget(host);
        drafts.retain(|k, _| k.host != host);
        if self.registration.paired_host.as_deref() == Some(host) {
            self.registration.paired_host = None;
        }
    }
}
#[cfg(test)]
mod tests;
