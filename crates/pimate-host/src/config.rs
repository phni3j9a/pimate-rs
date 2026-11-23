use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use nix::unistd::{Uid, gethostname};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tempfile::NamedTempFile;
use uuid::Uuid;
fn var(s: &str) -> Option<PathBuf> {
    env::var_os(s).filter(|v| !v.is_empty()).map(Into::into)
}
pub fn config_dir() -> Result<PathBuf> {
    if let Some(p) = var("PIMATE_CONFIG_DIR") {
        return Ok(p);
    }
    Ok(match var("XDG_CONFIG_HOME") {
        Some(p) => p,
        None => var("HOME")
            .ok_or_else(|| anyhow::anyhow!("HOME missing"))?
            .join(".config"),
    }
    .join("pimate"))
}
pub fn run_dir() -> PathBuf {
    var("PIMATE_RUN_DIR")
        .or_else(|| var("XDG_RUNTIME_DIR").map(|p| p.join("pimate")))
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/pimate-{}", Uid::current())))
}
pub fn private(p: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)?;
    let m = p.symlink_metadata()?;
    ensure!(
        m.is_dir() && m.uid() == Uid::current().as_raw() && m.mode() & 0o777 == 0o700,
        "not an owned 0700 directory: {}",
        p.display()
    );
    Ok(())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn digest(s: &str) -> String {
    format!("{:x}", Sha256::digest(s))
}
fn secret<const N: usize>() -> String {
    let mut b = [0; N];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub device_id: String,
    name: String,
    token_hash: String,
    created_at: u64,
    last_seen_at: u64,
}
impl Device {
    pub fn public(&self) -> Value {
        json!({"deviceId":self.device_id,"name":self.name,"createdAt":self.created_at,"lastSeenAt":self.last_seen_at})
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub host_id: String,
    pub name: String,
}
pub struct Config {
    dir: PathBuf,
    pub host: Host,
}
struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
impl Config {
    pub fn open(dir: PathBuf) -> Result<Self> {
        private(&dir)?;
        private(&dir.join("pairing"))?;
        let p = dir.join("host.json");
        if !p.try_exists()? {
            let f = NamedTempFile::new_in(&dir)?;
            serde_json::to_writer(
                &f,
                &json!({"hostId":Uuid::new_v4().to_string(),"name":gethostname()?.to_string_lossy()}),
            )?;
            if let Err(e) = f.persist_noclobber(&p)
                && e.error.kind() != ErrorKind::AlreadyExists
            {
                return Err(e.into());
            }
        }
        Ok(Self {
            dir,
            host: serde_json::from_reader(File::open(p)?)?,
        })
    }
    pub fn devices(&self) -> Result<Vec<Device>> {
        match File::open(self.dir.join("devices.json")) {
            Ok(f) => Ok(serde_json::from_reader(f)?),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(e.into()),
        }
    }
    fn update<T>(&self, edit: impl FnOnce(&mut Vec<Device>) -> Result<T>) -> Result<T> {
        let p = self.dir.join("devices.lock");
        let _lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)?;
        let _cleanup = Remove(p);
        let mut ds = self.devices()?;
        let result = edit(&mut ds)?;
        let f = NamedTempFile::new_in(&self.dir)?;
        serde_json::to_writer(&f, &ds)?;
        f.persist(self.dir.join("devices.json"))?;
        Ok(result)
    }
    fn pending(&self, code: &str) -> PathBuf {
        self.dir
            .join("pairing")
            .join(format!("{}.json", digest(code)))
    }
    pub fn code(&self) -> Result<String> {
        let code = secret::<24>();
        let f = NamedTempFile::new_in(self.dir.join("pairing"))?;
        serde_json::to_writer(&f, &json!({"expiresAt":now()+600000}))?;
        f.persist_noclobber(self.pending(&code))?;
        Ok(code)
    }
    pub fn pair(&self, code: &str, name: &str) -> Result<Value> {
        self.update(|ds| {
            let p=self.pending(code);
            let claimed=p.with_extension(format!("json.{}.claimed",Uuid::new_v4()));
            if let Err(e)=fs::rename(&p,&claimed){
                if e.kind()==ErrorKind::NotFound{anyhow::bail!("invalid_code")}
                return Err(e.into())
            }
            let _cleanup=Remove(claimed.clone());
            let exp:Value=serde_json::from_reader(File::open(claimed)?)?;
            ensure!(exp["expiresAt"].as_u64().unwrap_or(0)>now(),"expired_code");
            let token=secret::<32>();let id=Uuid::new_v4().to_string();
            ds.push(Device{device_id:id.clone(),name:name.into(),token_hash:digest(&token),created_at:now(),last_seen_at:now()});
            Ok(json!({"hostId":self.host.host_id,"hostName":self.host.name,"token":token,"deviceId":id}))
        })
    }
    pub fn authenticate(&self, t: &str) -> Result<Option<String>> {
        let h = digest(t);
        Ok(self
            .devices()?
            .into_iter()
            .find(|d| bool::from(d.token_hash.as_bytes().ct_eq(h.as_bytes())))
            .map(|d| d.device_id))
    }
    pub fn active(&self, id: &str) -> Result<bool> {
        let Some(d) = self.devices()?.into_iter().find(|d| d.device_id == id) else {
            return Ok(false);
        };
        if now().saturating_sub(d.last_seen_at) >= 60000 {
            return self.update(|ds| {
                if let Some(d) = ds.iter_mut().find(|d| d.device_id == id) {
                    d.last_seen_at = now();
                    Ok(true)
                } else {
                    Ok(false)
                }
            });
        }
        Ok(true)
    }
    pub fn revoke(&self, id: &str) -> Result<()> {
        self.update(|ds| {
            ds.retain(|d| d.device_id != id);
            Ok(())
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn ts_files_and_lock() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let p = tmp.path().join("cfg");
        let c = Config::open(p.clone())?;
        let id = Uuid::new_v4().to_string();
        let ds = json!([{"deviceId":id,"name":"TS","createdAt":1,"lastSeenAt":2,"tokenHash":digest("ts-token")}]);
        let db = p.join("devices.json");
        fs::write(&db, ds.to_string())?;
        assert_eq!(c.authenticate("ts-token")?, Some(id.clone()));
        let lock = p.join("devices.lock");
        fs::write(&lock, "")?;
        assert!(c.revoke(&id).is_err());
        assert!(lock.exists());
        assert_eq!(c.devices()?.len(), 1);
        fs::remove_file(lock)?;
        let code = c.code()?;
        let pending = c.pending(&code);
        for (f, mode) in [
            (p.clone(), 0o700),
            (p.join("host.json"), 0o600),
            (pending.clone(), 0o600),
        ] {
            assert_eq!(f.metadata()?.mode() & 0o777, mode)
        }
        fs::write(&pending, r#"{"expiresAt":0}"#)?;
        assert_eq!(
            c.pair(&code, "name").unwrap_err().to_string(),
            "expired_code"
        );
        assert!(!pending.exists());
        c.revoke(&id)?;
        assert_eq!(db.metadata()?.mode() & 0o777, 0o600);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755))?;
        assert!(Config::open(p).is_err());
        Ok(())
    }
}
