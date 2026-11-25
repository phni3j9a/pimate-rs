use super::*;
use pimate_protocol::{self as p, RcMethod};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}
#[derive(Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum FontSize {
    Small,
    #[default]
    Standard,
    Large,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    pub appearance: Appearance,
    pub font_size: FontSize,
    pub ask_tool: bool,
}
impl Settings {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        match std::fs::read(path) {
            Ok(v) => Ok(serde_json::from_slice(&v)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
        std::fs::create_dir_all(dir)?;
        let temp = dir.join(format!(".settings-{}-{}", std::process::id(), sequence()));
        let mut f = std::fs::File::create_new(&temp)?;
        let result = (|| {
            f.write_all(&serde_json::to_vec(self)?)?;
            f.sync_all()?;
            std::fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
}
fn sequence() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}
#[derive(Debug, PartialEq, Eq)]
pub enum FirstStep {
    Loading,
    Offline,
    Empty,
    List,
}
impl Rows {
    pub fn first_step(&self) -> FirstStep {
        match self.get("host", "connection").as_str() {
            Some("connected") if self.get("host", "listKnown") == true => {
                if self.group("instance").is_empty() {
                    FirstStep::Empty
                } else {
                    FirstStep::List
                }
            }
            Some("offline" | "invalid_token" | "auth_failed" | "revoked" | "protocol_mismatch") => {
                FirstStep::Offline
            }
            _ => FirstStep::Loading,
        }
    }
}
pub enum MachineQuery {
    Folders,
    Dirs(String),
    UpdateCheck,
    GithubStatus,
    Devices,
}
pub enum Maintenance {
    UpdateLatest,
    Restart,
    Revoke(String),
}
pub struct Confirmation(Page, Maintenance);
impl Page {
    fn machine(&self) -> Result<(), Error> {
        if self.key.is_some() {
            return Err(Error::StaleTarget);
        }
        self.current().map(|_| ())
    }
    pub async fn query(&self, q: MachineQuery) -> Result<Value, Error> {
        self.machine()?;
        self.perform(match q {
            MachineQuery::Folders => Operation::Folders,
            MachineQuery::Dirs(path) => Operation::Dirs { path },
            MachineQuery::UpdateCheck => Operation::UpdateCheck,
            MachineQuery::GithubStatus => Operation::GithubStatus,
            MachineQuery::Devices => Operation::Devices,
        })
        .await
    }
    pub fn confirmation(&self, a: Maintenance) -> Result<Confirmation, Error> {
        self.machine()?;
        Ok(Confirmation(self.clone(), a))
    }
    pub async fn github(&self, q: p::GitHubQuery) -> Result<Value, Error> {
        self.perform(Operation::Github {
            instance: self.editor()?.instance.clone(),
            query: json!(q).to_string(),
        })
        .await
    }
    pub async fn host_models(&self) -> Result<Option<Value>, Error> {
        self.machine()?;
        let r = self.current()?;
        let mut recent: Vec<_> = r.group("instance").iter().collect();
        recent.sort_by_key(|(_, v)| std::cmp::Reverse(v["updatedAt"].as_u64()));
        for (id, _) in recent.into_iter().take(3) {
            match self
                .perform(Operation::HostModels {
                    instance: id.clone(),
                })
                .await
            {
                Ok(v) => return Ok(Some(v)),
                Err(Error::Rejected | Error::Blocked | Error::Unsupported) => (),
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }
    pub fn spawn_form(&self, ask_tool: bool) -> Result<SpawnForm, Error> {
        self.machine()?;
        Ok(SpawnForm {
            page: self.clone(),
            folder: String::new(),
            trusted: None,
            ask_tool,
            message: String::new(),
            model: None,
            thinking: None,
        })
    }
    pub async fn login(&self) -> Result<Login, Error> {
        self.machine()?;
        let c = self.live()?;
        let id = format!("desktop-{}-{}", std::process::id(), sequence());
        let guard = Hold(c, id);
        let target = guard.0.open_login(guard.1.clone()).await?;
        self.machine()?;
        Ok(Login {
            page: self.clone(),
            target,
            _hold: guard,
        })
    }
}
impl Confirmation {
    pub fn warning(&self) -> &str {
        match self.1 {
            Maintenance::UpdateLatest => "このマシンを実行時の最新版に更新します。",
            Maintenance::Restart => "この Host を再起動します。接続が一時切断されます。",
            Maintenance::Revoke(_) => "選んだ端末の登録を PC から取り消します。",
        }
    }
    pub async fn confirm(self) -> Result<Value, Error> {
        self.0
            .perform(match self.1 {
                Maintenance::UpdateLatest => Operation::Update,
                Maintenance::Restart => Operation::Restart,
                Maintenance::Revoke(device_id) => Operation::Revoke { device_id },
            })
            .await
    }
}
pub struct SpawnForm {
    page: Page,
    folder: String,
    trusted: Option<String>,
    pub ask_tool: bool,
    pub message: String,
    pub model: Option<p::ModelRef>,
    pub thinking: Option<p::ThinkingLevel>,
}
impl SpawnForm {
    pub fn folder(&mut self, path: String, trusted: bool) {
        self.trusted = trusted.then(|| path.clone());
        self.folder = path;
    }
    fn params(&self) -> Result<Value, Error> {
        if self.folder.trim().is_empty() {
            return Err(Error::BadRequest);
        }
        let mut p = json!({"cwd":self.folder,"trust":self.trusted.as_ref()==Some(&self.folder),"askTool":self.ask_tool});
        if let Some(m) = &self.model {
            p["model"] = json!(m);
            if let Some(t) = self.thinking {
                p["thinking"] = json!(t);
            }
        }
        if !self.message.trim().is_empty() {
            p["message"] = json!(self.message.trim());
        }
        Ok(p)
    }
    pub async fn spawn(self) -> Result<Spawned, Error> {
        let v = self
            .page
            .perform(Operation::Spawn {
                params: self.params()?.to_string(),
            })
            .await?;
        let id = v["instanceId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(Error::OutcomeUnknown)?
            .into();
        Ok(Spawned {
            page: self.page,
            id,
            message_unconfirmed: v["messageError"].as_str().is_some_and(|s| !s.is_empty()),
        })
    }
}
pub struct Spawned {
    page: Page,
    pub id: String,
    pub message_unconfirmed: bool,
}
impl Spawned {
    pub fn ready(&self) -> bool {
        !self.message_unconfirmed
            && self
                .page
                .current()
                .is_ok_and(|r| !r.get("instance", &self.id).is_null())
    }
}
struct Hold(Arc<Connection>, String);
impl Drop for Hold {
    fn drop(&mut self) {
        self.0.close_login(self.1.clone());
    }
}
pub struct Login {
    page: Page,
    pub target: pimate_core::Target,
    _hold: Hold,
}
pub enum AuthAction {
    List,
    Begin(p::AuthBeginParams),
    Respond(p::AuthRespondParams),
    Cancel(p::AuthCancelParams),
    Logout(p::AuthLogoutParams),
}
impl Login {
    pub async fn request(&self, action: AuthAction) -> Result<Value, Error> {
        let (method, p) = match action {
            AuthAction::List => (RcMethod::AuthList, json!({})),
            AuthAction::Begin(v) => (RcMethod::AuthBegin, json!(v)),
            AuthAction::Respond(v) => (RcMethod::AuthRespond, json!(v)),
            AuthAction::Cancel(v) => (RcMethod::AuthCancel, json!(v)),
            AuthAction::Logout(v) => (RcMethod::AuthLogout, json!(v)),
        };
        self.page
            .perform(Operation::Rc {
                method,
                target: self.target.clone(),
                params: p.to_string(),
            })
            .await
    }
    pub async fn follow_once(&self) -> Result<bool, Error> {
        self.page.machine()?;
        let more = self
            .page
            .live()?
            .follow_login_once(self.target.clone())
            .await?;
        self.page.machine()?;
        Ok(more)
    }
}
pub enum CredentialOutcome {
    Active,
    Saved,
    NeedsReload,
    StorageUnknown,
    Failed,
    Expired,
    Unknown,
}
impl CredentialOutcome {
    pub fn from_flow(v: &Value) -> Self {
        match v["status"].as_str() {
            Some("working" | "input" | "saving") => Self::Active,
            Some("succeeded") if v["refreshRequired"] == true => Self::NeedsReload,
            Some("succeeded") => Self::Saved,
            Some("failed") if v["error"] == "storage_failed" => Self::StorageUnknown,
            Some("failed") => Self::Failed,
            Some("expired") => Self::Expired,
            _ => Self::Unknown,
        }
    }
}

pub struct Forget(pimate_core::Host);
impl Desktop {
    pub fn forget_confirmation(&self, id: &str) -> Result<Forget, Error> {
        Ok(Forget(
            self.hosts
                .iter()
                .find(|h| h.id == id)
                .ok_or(Error::StaleTarget)?
                .clone(),
        ))
    }
    pub fn removed(&mut self, host: &pimate_core::Host, cx: &mut gpui::Context<Self>) {
        if self.hosts.contains(host) {
            self.state.close_page();
            self.machines.remove(&host.id);
            self.drafts.retain(|k, _| k.host != host.id);
            self.state.attachments.forget(&host.id);
            self.state.instructions.forget(&host.id);
            if self.selected.as_ref().is_some_and(|(h, _)| h == &host.id) {
                self.selected = None;
                self.input = None;
                self.epoch += 1;
            }
            self.reload(cx)
        }
    }
}
impl Forget {
    /// Blocking secure storage: dispatch on the background executor.
    pub fn confirm(self) -> Result<pimate_core::Host, Error> {
        crate::store::Store::open().forget_matching(&self.0)?;
        Ok(self.0)
    }
}
