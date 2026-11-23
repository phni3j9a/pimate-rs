//! Native UI boundary. Transport, listener ordering and operation guards stay in core.
mod markdown;
mod types;
use pimate_core::{
    Allowance, Change, Credential, Error as CoreError, Host, Operation, SessionStats, Snapshot,
    Target,
};
use std::sync::Arc;
uniffi::setup_scaffolding!();

#[uniffi::export(with_foreign)]
pub trait Listener: Send + Sync {
    fn changed(&self, revision: u64, changes: Vec<Change>);
}
#[uniffi::export(with_foreign)]
pub trait SecretStore: Send + Sync {
    fn hosts(&self) -> Result<Vec<Host>, CoreError>;
    fn save(&self, host: Host, token: String) -> Result<(), CoreError>;
    fn load(&self, id: String) -> Result<Credential, CoreError>;
    fn invalidate(&self, id: String, device_id: String) -> Result<(), CoreError>;
    fn forget(&self, id: String) -> Result<(), CoreError>;
}
struct Delivery(Arc<dyn Listener>);
impl pimate_core::Listener for Delivery {
    fn changed(&self, revision: u64, changes: Vec<Change>) {
        self.0.changed(revision, changes)
    }
}
struct Store(Arc<dyn SecretStore>);
impl pimate_core::SecretStore for Store {
    fn hosts(&self) -> Result<Vec<Host>, CoreError> {
        self.0.hosts()
    }
    fn save(&self, host: Host, token: String) -> Result<(), CoreError> {
        self.0.save(host, token)
    }
    fn load(&self, id: String) -> Result<Credential, CoreError> {
        self.0.load(id)
    }
    fn invalidate(&self, id: String, device_id: String) -> Result<(), CoreError> {
        self.0.invalidate(id, device_id)
    }
    fn forget(&self, id: String) -> Result<(), CoreError> {
        self.0.forget(id)
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
        Ok(Arc::new(Connection(
            self.0.connect(id, Arc::new(Delivery(listener)))?,
        )))
    }
}
#[derive(uniffi::Object)]
pub struct Connection(Arc<pimate_core::Connection>);
#[uniffi::export]
impl Connection {
    pub fn subscribe(&self, instance: Option<String>) -> Result<(), CoreError> {
        self.0.subscribe(instance)
    }
    pub fn snapshot(&self) -> Snapshot {
        self.0.snapshot()
    }
    pub fn check(&self, operation: Operation) -> Result<(), CoreError> {
        self.0.check(operation)
    }
    pub async fn perform(&self, operation: Operation) -> Result<String, CoreError> {
        self.0.perform(operation).await
    }
    pub async fn open_login(&self, hold_id: String) -> Result<Target, CoreError> {
        self.0.open_login(hold_id).await
    }
    pub async fn follow_login(&self, target: Target) -> Result<(), CoreError> {
        self.0.follow_login(target).await
    }
    pub fn session_stats(&self) -> SessionStats {
        self.0.session_stats()
    }
    pub fn allowances(&self, now_ms: u64) -> Vec<Allowance> {
        self.0.allowances(now_ms)
    }
    pub fn command_suggestions(&self, text: String) -> Vec<String> {
        self.0.command_suggestions(text)
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
