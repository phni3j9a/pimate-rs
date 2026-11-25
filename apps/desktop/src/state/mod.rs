//! Screen-facing state; no credentials, drafts or responses are logged.
#![allow(dead_code)] // Used by the separately developed GPUI screens.
use crate::{
    Desktop, Machine,
    model::{EditorKey, Rows},
};
use pimate_core::{Connection, Error, Operation};
use serde_json::Value;
use std::sync::{Arc, Weak};
mod chat;
mod machine;
pub(crate) use chat::*;
pub(crate) use machine::*;

#[derive(Default)]
pub(crate) struct DesktopState {
    page: Arc<()>,
    pub attachments: Attachments,
    pub settings: Settings,
}
#[derive(Clone)]
pub(crate) struct Page {
    pub host: String,
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
    }
    fn open(&mut self, host: &str, m: &Machine, key: Option<EditorKey>) -> Page {
        self.close_page();
        Page {
            host: host.into(),
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
