use pimate_core::{Credential, Error, Host, SecretStore};

#[cfg(not(target_os = "macos"))]
pub struct Store;
#[cfg(not(target_os = "macos"))]
impl Store {
    pub fn open() -> Self {
        Self
    }
}
#[cfg(not(target_os = "macos"))]
impl SecretStore for Store {
    fn hosts(&self) -> Result<Vec<Host>, Error> {
        Err(Error::Unsupported)
    }
    fn save(&self, _: Host, _: String) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn load(&self, _: String) -> Result<Credential, Error> {
        Err(Error::Unsupported)
    }
    fn invalidate(&self, _: String, _: String) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn forget(&self, _: String) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}
#[cfg(target_os = "macos")]
pub use mac::Store;
#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    use serde::{Deserialize, Serialize};
    use std::{
        collections::BTreeMap,
        fs::{self, OpenOptions},
        os::unix::fs::OpenOptionsExt,
        path::PathBuf,
    };
    const SERVICE: &str = "dev.pimate.rs.desktop";
    const ACCOUNT: &str = "registrations";
    const NOT_FOUND: i32 = -25300;
    #[derive(Serialize, Deserialize)]
    struct Record {
        name: String,
        address: String,
        device: String,
        revoked: bool,
        token: String,
    }
    impl Record {
        fn host(&self, id: String) -> Host {
            Host {
                id,
                name: self.name.clone(),
                address: self.address.clone(),
                device_id: self.device.clone(),
                needs_pairing: self.revoked,
            }
        }
    }
    type Records = BTreeMap<String, Record>;
    pub struct Store {
        service: String,
    }
    impl Store {
        pub fn open() -> Self {
            Self {
                service: if cfg!(debug_assertions) && std::env::args().any(|v| v == "--demo") {
                    format!("{SERVICE}.demo")
                } else {
                    SERVICE.into()
                },
            }
        }
        fn read(&self) -> Result<Records, Error> {
            match get_generic_password(&self.service, ACCOUNT) {
                Ok(v) => serde_json::from_slice(&v).map_err(|_| Error::Storage),
                Err(e) if e.code() == NOT_FOUND => Ok(Records::new()),
                Err(_) => Err(Error::Storage),
            }
        }
        fn edit(&self, change: impl FnOnce(&mut Records) -> bool) -> Result<(), Error> {
            // Serialize read/compare/update across processes. No credential enters the lock file.
            let dir = PathBuf::from(std::env::var_os("HOME").ok_or(Error::Storage)?)
                .join("Library/Application Support/PiMate RS");
            fs::create_dir_all(&dir).map_err(|_| Error::Storage)?;
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .mode(0o600)
                .open(dir.join(format!("{}.lock", self.service)))
                .map_err(|_| Error::Storage)?;
            lock.lock().map_err(|_| Error::Storage)?;
            let mut records = self.read()?;
            if !change(&mut records) {
                return Ok(());
            }
            if records.is_empty() {
                delete_generic_password(&self.service, ACCOUNT)
            } else {
                set_generic_password(
                    &self.service,
                    ACCOUNT,
                    &serde_json::to_vec(&records).map_err(|_| Error::Storage)?,
                )
            }
            .map_err(|_| Error::Storage)
        }
    }
    impl SecretStore for Store {
        fn hosts(&self) -> Result<Vec<Host>, Error> {
            Ok(self.read()?.into_iter().map(|(id, r)| r.host(id)).collect())
        }
        fn load(&self, id: String) -> Result<Credential, Error> {
            let r = self.read()?.remove(&id).ok_or(Error::Storage)?;
            Ok(Credential {
                host: r.host(id),
                token: r.token,
            })
        }
        fn save(&self, h: Host, token: String) -> Result<(), Error> {
            self.edit(|r| {
                r.insert(
                    h.id,
                    Record {
                        name: h.name,
                        address: h.address,
                        device: h.device_id,
                        revoked: h.needs_pairing,
                        token,
                    },
                );
                true
            })
        }
        fn invalidate(&self, id: String, device: String) -> Result<(), Error> {
            self.edit(|r| {
                if let Some(row) = r.get_mut(&id)
                    && row.device == device
                    && !row.revoked
                {
                    row.revoked = true;
                    row.token.clear();
                    true
                } else {
                    false
                }
            })
        }
        fn forget(&self, id: String) -> Result<(), Error> {
            self.edit(|r| r.remove(&id).is_some())
        }
    }
    #[test]
    fn replacement_survives_old_invalidation() {
        let s = Store {
            service: format!("{SERVICE}.test.{}", uuid::Uuid::new_v4()),
        };
        let id = "test".to_string();
        let mut h = Host {
            id: id.clone(),
            name: "one".into(),
            address: "localhost:80".into(),
            device_id: "old".into(),
            needs_pairing: false,
        };
        s.save(h.clone(), "first".into()).unwrap();
        h.device_id = "new".into();
        h.name = "two".into();
        s.save(h.clone(), "second".into()).unwrap();
        s.invalidate(id.clone(), "old".into()).unwrap();
        let c = s.load(id.clone()).unwrap();
        assert_eq!(c.host, h);
        assert!(c.token == "second");
        s.invalidate(id.clone(), "new".into()).unwrap();
        assert!(s.hosts().unwrap()[0].needs_pairing);
        assert!(s.load(id.clone()).unwrap().token.is_empty());
        s.forget(id).unwrap();
        assert!(s.hosts().unwrap().is_empty());
    }
}
