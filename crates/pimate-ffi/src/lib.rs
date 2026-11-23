//! Native UI boundary. Transport, listener ordering and operation guards stay in core.
mod markdown;
mod types;
use pimate_core::{
    Allowance, Change, Credential, Error as CoreError, Host, Operation, SessionStats, Snapshot,
    Target,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CallbackError {
    #[error("callback failed")]
    Failed,
}
impl From<uniffi::UnexpectedUniFFICallbackError> for CallbackError {
    fn from(_: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Failed
    }
}
#[uniffi::export(with_foreign)]
pub trait Listener: Send + Sync {
    fn changed(&self, revision: u64, changes: Vec<Change>) -> Result<(), CallbackError>;
}
#[uniffi::export(with_foreign)]
pub trait SecretStore: Send + Sync {
    fn hosts(&self) -> Result<Vec<Host>, CallbackError>;
    fn save(&self, host: Host, token: String) -> Result<(), CallbackError>;
    fn load(&self, id: String) -> Result<Credential, CallbackError>;
    fn invalidate(&self, id: String, device_id: String) -> Result<(), CallbackError>;
    fn forget(&self, id: String) -> Result<(), CallbackError>;
}
struct Delivery {
    listener: Arc<dyn Listener>,
    closed: Mutex<mpsc::Receiver<()>>,
}
impl pimate_core::Listener for Delivery {
    fn changed(&self, revision: u64, changes: Vec<Change>) {
        if self.listener.changed(revision, changes).is_ok() {
            return;
        }
        // Runs on the dedicated delivery thread. Even the final revision may need a retry.
        let mut delay = Duration::from_secs(1);
        while matches!(
            self.closed.lock().unwrap().recv_timeout(delay),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            if self
                .listener
                .changed(revision, vec![Change::ResyncRequired])
                .is_ok()
            {
                break;
            }
            delay = (delay * 2).min(Duration::from_secs(16));
        }
    }
}
struct Store(Arc<dyn SecretStore>);
impl pimate_core::SecretStore for Store {
    fn hosts(&self) -> Result<Vec<Host>, CoreError> {
        self.0.hosts().map_err(|_| CoreError::Storage)
    }
    fn save(&self, host: Host, token: String) -> Result<(), CoreError> {
        self.0.save(host, token).map_err(|_| CoreError::Storage)
    }
    fn load(&self, id: String) -> Result<Credential, CoreError> {
        self.0.load(id).map_err(|_| CoreError::Storage)
    }
    fn invalidate(&self, id: String, device_id: String) -> Result<(), CoreError> {
        self.0
            .invalidate(id, device_id)
            .map_err(|_| CoreError::Storage)
    }
    fn forget(&self, id: String) -> Result<(), CoreError> {
        self.0.forget(id).map_err(|_| CoreError::Storage)
    }
}
#[derive(uniffi::Object)]
pub struct Core(pimate_core::Core);
#[uniffi::export]
impl Core {
    #[uniffi::constructor]
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self(pimate_core::Core::new(Arc::new(Store(store))))
    }
    pub fn hosts(&self) -> Result<Vec<Host>, CoreError> {
        self.0.hosts()
    }
    pub async fn pair(&self, link: String, name: String) -> Result<Host, CoreError> {
        self.0.pair(link, name).await
    }
    pub fn forget(&self, id: String) -> Result<(), CoreError> {
        self.0.forget(id)
    }
    pub fn connect(
        &self,
        id: String,
        listener: Arc<dyn Listener>,
    ) -> Result<Arc<Connection>, CoreError> {
        let (lifetime, closed) = mpsc::channel();
        let core = self.0.connect(
            id,
            Arc::new(Delivery {
                listener,
                closed: Mutex::new(closed),
            }),
        )?;
        Ok(Arc::new(Connection {
            core,
            _lifetime: lifetime,
        }))
    }
}
#[derive(uniffi::Object)]
pub struct Connection {
    core: Arc<pimate_core::Connection>,
    _lifetime: mpsc::Sender<()>,
}
#[uniffi::export]
impl Connection {
    pub fn subscribe(&self, instance: Option<String>) -> Result<(), CoreError> {
        self.core.subscribe(instance)
    }
    pub fn snapshot(&self) -> Snapshot {
        self.core.snapshot()
    }
    pub fn take_unconfirmed(
        &self,
        target: Target,
        id: String,
    ) -> Result<pimate_core::RestoredDraft, CoreError> {
        self.core.take_unconfirmed(target, id)
    }
    pub fn check(&self, operation: Operation) -> Result<(), CoreError> {
        self.core.check(operation)
    }
    pub async fn perform(&self, operation: Operation) -> Result<String, CoreError> {
        self.core.perform(operation).await
    }
    pub async fn open_login(&self, hold_id: String) -> Result<Target, CoreError> {
        self.core.open_login(hold_id).await
    }
    pub fn close_login(&self, hold_id: String) {
        self.core.close_login(hold_id)
    }
    pub async fn follow_login_once(&self, target: Target) -> Result<bool, CoreError> {
        self.core.follow_login_once(target).await
    }
    pub fn session_stats(&self) -> SessionStats {
        self.core.session_stats()
    }
    pub fn allowances(&self, now_ms: u64) -> Vec<Allowance> {
        self.core.allowances(now_ms)
    }
    pub fn command_suggestions(&self, text: String) -> Vec<String> {
        self.core.command_suggestions(text)
    }
}
#[uniffi::export]
pub fn init_logging() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("PiMate")
            .with_max_level(log::STATIC_MAX_LEVEL),
    );
    #[cfg(target_vendor = "apple")]
    if oslog::OsLogger::new("app.pimate.core").init().is_ok() {
        log::set_max_level(log::STATIC_MAX_LEVEL);
    }
}
