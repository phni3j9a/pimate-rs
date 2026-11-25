use pimate_core::{Credential, Error, Host, SecretStore};

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub struct Store;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl Store {
    pub fn open() -> Self {
        Self
    }
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use desktop::Store;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod desktop {
    use super::*;
    #[cfg(target_os = "macos")]
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
    #[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
        fn read(&self) -> Result<Records, Error> {
            match get_generic_password(&self.service, ACCOUNT) {
                Ok(v) => serde_json::from_slice(&v).map_err(|_| Error::Storage),
                Err(e) if e.code() == NOT_FOUND => Ok(Records::new()),
                Err(_) => Err(Error::Storage),
            }
        }
        fn edit(&self, change: impl FnOnce(&mut Records) -> bool) -> Result<(), Error> {
            // Serialize read/compare/update across processes. No credential enters the lock file.
            #[cfg(target_os = "linux")]
            let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").ok_or(Error::Storage)?)
                .join("pimate-rs");
            #[cfg(target_os = "macos")]
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
            self.edit_locked(change)
        }
        #[cfg(target_os = "macos")]
        fn edit_locked(&self, change: impl FnOnce(&mut Records) -> bool) -> Result<(), Error> {
            let mut records = self.read()?;
            if !change(&mut records) {
                return Ok(());
            }
            self.write(&records)
        }
        #[cfg(target_os = "macos")]
        fn write(&self, records: &Records) -> Result<(), Error> {
            if records.is_empty() {
                delete_generic_password(&self.service, ACCOUNT)
            } else {
                set_generic_password(
                    &self.service,
                    ACCOUNT,
                    &serde_json::to_vec(records).map_err(|_| Error::Storage)?,
                )
            }
            .map_err(|_| Error::Storage)
        }
    }
    #[cfg(target_os = "linux")]
    impl Store {
        fn edit_locked(&self, change: impl FnOnce(&mut Records) -> bool) -> Result<(), Error> {
            self.with_collection(|collection| async move {
                let mut records = self.read_from(&collection).await?;
                if change(&mut records) {
                    collection
                        .create_item(
                            "PiMate RS registrations",
                            &self.attributes(),
                            serde_json::to_vec(&records).map_err(|_| Error::Storage)?,
                            true,
                            None,
                        )
                        .await
                        .map_err(|_| Error::Storage)?;
                }
                Ok(())
            })
        }
        fn attributes(&self) -> std::collections::HashMap<&str, &str> {
            [("application", self.service.as_str()), ("account", ACCOUNT)].into()
        }
        async fn collection() -> Result<oo7::dbus::Collection<'static>, Error> {
            let collection = oo7::dbus::Service::encrypted()
                .await
                .map_err(|_| Error::Storage)?
                .with_alias("default")
                .await
                .map_err(|_| Error::Storage)?
                .ok_or(Error::Storage)?;
            if collection.is_locked().await.map_err(|_| Error::Storage)? {
                return Err(Error::Storage);
            }
            Ok(collection)
        }
        fn with_collection<T, F: Future<Output = Result<T, Error>>>(
            &self,
            action: impl FnOnce(oo7::dbus::Collection<'static>) -> F,
        ) -> Result<T, Error> {
            futures_lite::future::block_on(futures_lite::future::or(
                async { action(Self::collection().await?).await },
                async {
                    async_io::Timer::after(std::time::Duration::from_secs(5)).await;
                    Err(Error::Storage)
                },
            ))
        }
        fn read(&self) -> Result<Records, Error> {
            self.with_collection(|collection| async move { self.read_from(&collection).await })
        }
        async fn read_from(
            &self,
            collection: &oo7::dbus::Collection<'_>,
        ) -> Result<Records, Error> {
            let items = collection
                .search_items(&self.attributes())
                .await
                .map_err(|_| Error::Storage)?;
            match items.as_slice() {
                // Some services hide items if the collection locks during search.
                [] if !collection.is_locked().await.map_err(|_| Error::Storage)? => {
                    Ok(Records::new())
                }
                [item] => serde_json::from_slice(
                    item.secret().await.map_err(|_| Error::Storage)?.as_bytes(),
                )
                .map_err(|_| Error::Storage),
                _ => Err(Error::Storage),
            }
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
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires disposable locked or unavailable Secret Service"]
    fn secret_service_failure_is_not_empty_registration() {
        let store = Store {
            service: format!("{SERVICE}.test.{}", uuid::Uuid::new_v4()),
        };
        assert!(matches!(store.hosts(), Err(Error::Storage)));
        assert!(matches!(
            store.forget("missing".into()),
            Err(Error::Storage)
        ));
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires disposable Secret Service"]
    fn alias_change_during_edit_does_not_redirect_write() {
        assert_eq!(
            std::env::var("PIMATE_PRIVATE_STORE_TEST").as_deref(),
            Ok("1")
        );
        fn alias(collection: &str) {
            assert!(
                std::process::Command::new("gdbus")
                    .args([
                        "call",
                        "--session",
                        "-d",
                        "org.freedesktop.secrets",
                        "-o",
                        "/org/freedesktop/secrets",
                        "-m",
                        "org.freedesktop.Secret.Service.SetAlias",
                        "default",
                        collection
                    ])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let store = Store {
            service: format!("{SERVICE}.test.{}", uuid::Uuid::new_v4()),
        };
        store
            .edit(|records| {
                alias("/org/freedesktop/secrets/collection/session");
                records.insert(
                    "test".into(),
                    Record {
                        name: "one".into(),
                        address: "localhost:80".into(),
                        device: "test".into(),
                        revoked: false,
                        token: "first".into(),
                    },
                );
                true
            })
            .unwrap();
        assert!(store.hosts().unwrap().is_empty());
        alias("/org/freedesktop/secrets/collection/login");
        assert_eq!(store.hosts().unwrap().len(), 1);
    }
    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "requires disposable Secret Service; use test-linux-store.sh"
    )]
    fn replacement_survives_old_invalidation() {
        #[cfg(target_os = "linux")]
        assert_eq!(
            std::env::var("PIMATE_PRIVATE_STORE_TEST").as_deref(),
            Ok("1")
        );
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
        s.invalidate("other".into(), "new".into()).unwrap();
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
