//! One `Connection` per paired host. Only `SecretStore` sees credentials.
mod catalog;
mod connection;
mod operation;
mod state;
pub use connection::Connection;
pub use operation::{Image, Operation, Target};
use pimate_protocol::{PairRequest, PairResponse, parse_pairing_link};
pub use state::{Allowance, Change, Item, Listener, QuotaWindow, SessionStats, Snapshot};
#[cfg(test)]
mod tests;
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::runtime::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{self:?}")]
pub enum Error {
    BuiltinCommand,
    Rejected,
    InvalidLink,
    InvalidCode,
    ExpiredCode,
    BadRequest,
    HostMismatch,
    Storage,
    Offline,
    Timeout,
    InvalidMessage,
    OutcomeUnknown,
    Blocked,
    Unsupported,
    StaleTarget,
    Revoked,
    InvalidToken,
    ProtocolMismatch,
    AuthFailed,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub address: String,
    pub device_id: String,
    pub needs_pairing: bool,
}
/// Implement with platform secure storage. Replace metadata and token atomically by host ID.
pub struct Credential {
    pub host: Host,
    pub token: String,
}
pub trait SecretStore: Send + Sync {
    fn hosts(&self) -> Result<Vec<Host>, Error>;
    fn save(&self, host: Host, token: String) -> Result<(), Error>;
    fn load(&self, id: String) -> Result<Credential, Error>;
    /// Mark only the matching device; a late close must not invalidate a replacement pairing.
    fn invalidate(&self, id: String, device_id: String) -> Result<(), Error>;
    fn forget(&self, id: String) -> Result<(), Error>;
}
fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("core runtime")
    })
}
pub struct Core {
    store: Arc<dyn SecretStore>,
}
impl Core {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self { store }
    }
    pub fn hosts(&self) -> Result<Vec<Host>, Error> {
        self.store.hosts()
    }
    pub fn forget(&self, id: String) -> Result<(), Error> {
        self.store.forget(id)
    }
    pub async fn pair(&self, link: String, name: String) -> Result<Host, Error> {
        let store = self.store.clone();
        runtime()
            .spawn(async move {
                let link = parse_pairing_link(&link).ok_or(Error::InvalidLink)?;
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(Duration::from_secs(10))
                    .build()
                    .map_err(|_| Error::Offline)?;
                let name = name.trim().chars().take(200).collect::<String>();
                let mut res = client
                    .post(format!("http://{}/pair", link.address))
                    .json(&PairRequest {
                        code: link.code,
                        device_name: if name.is_empty() {
                            "Phone".into()
                        } else {
                            name
                        },
                    })
                    .send()
                    .await
                    .map_err(|e| {
                        if e.is_timeout() {
                            Error::Timeout
                        } else {
                            Error::Offline
                        }
                    })?;
                match res.status().as_u16() {
                    200 => {}
                    400 => return Err(Error::BadRequest),
                    401 => return Err(Error::InvalidCode),
                    410 => return Err(Error::ExpiredCode),
                    _ => return Err(Error::InvalidMessage),
                };
                let mut bytes = Vec::new();
                while let Some(chunk) = res.chunk().await.map_err(|_| Error::InvalidMessage)? {
                    if bytes.len() + chunk.len() > 16 * 1024 {
                        return Err(Error::InvalidMessage);
                    }
                    bytes.extend_from_slice(&chunk)
                }
                let p: PairResponse =
                    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidMessage)?;
                if p.token.is_empty() || p.device_id.is_empty() {
                    return Err(Error::InvalidMessage);
                }
                if p.host_id != link.host_id {
                    return Err(Error::HostMismatch);
                }
                let host = Host {
                    id: p.host_id,
                    name: p.host_name,
                    address: link.address,
                    device_id: p.device_id,
                    needs_pairing: false,
                };
                store.save(host.clone(), p.token)?;
                Ok(host)
            })
            .await
            .map_err(|_| Error::Offline)?
    }
    pub fn connect(
        &self,
        id: String,
        listener: Arc<dyn Listener>,
    ) -> Result<Arc<Connection>, Error> {
        let Credential { host, token } = self.store.load(id.clone())?;
        if host.id != id {
            return Err(Error::Storage);
        }
        if host.needs_pairing {
            return Err(Error::Revoked);
        }
        if !pimate_protocol::valid_pairing_address(&host.address) {
            return Err(Error::InvalidLink);
        }
        Ok(Connection::start(host, token, listener, self.store.clone()))
    }
}
// Fail closed if a future version/feature combination changes the static filter.
const _: () = assert!(matches!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off));
const _: () = assert!(
    tracing::level_filters::STATIC_MAX_LEVEL
        .into_level()
        .is_none()
);
