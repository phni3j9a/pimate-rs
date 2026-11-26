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
    pub fn path() -> std::io::Result<std::path::PathBuf> {
        let home = || {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .ok_or(std::io::ErrorKind::NotFound)
        };
        #[cfg(target_os = "macos")]
        let dir = home()?.join("Library/Application Support");
        #[cfg(not(target_os = "macos"))]
        let dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .map_or_else(|| home().map(|h| h.join(".config")), Ok)?;
        let name = if cfg!(debug_assertions) && std::env::args().any(|s| s == "--demo") {
            "pimate-rs-desktop-demo"
        } else {
            "pimate-rs-desktop"
        };
        Ok(dir.join(name).join("settings.json"))
    }
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
                Err(Error::Rejected | Error::Blocked | Error::Unsupported | Error::StaleTarget) => {
                    self.machine()?;
                }
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
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(20),
            id,
            message_unconfirmed: v["messageError"].as_str().is_some_and(|s| !s.is_empty()),
        })
    }
}
pub struct Spawned {
    deadline: std::time::Instant,
    page: Page,
    pub id: String,
    pub message_unconfirmed: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum SpawnStatus {
    Waiting,
    Ready,
    MessageUnconfirmed,
    NotListed,
}
impl Spawned {
    pub fn status(&self) -> Result<SpawnStatus, Error> {
        let r = self.page.current()?;
        Ok(if self.message_unconfirmed {
            SpawnStatus::MessageUnconfirmed
        } else if !r.get("instance", &self.id).is_null() {
            SpawnStatus::Ready
        } else if std::time::Instant::now() >= self.deadline {
            SpawnStatus::NotListed
        } else {
            SpawnStatus::Waiting
        })
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
    target: pimate_core::Target,
    _hold: Hold,
}
pub enum AuthAction {
    List,
    Begin(p::AuthBeginParams),
    Respond(p::AuthRespondParams),
    Cancel(p::AuthCancelParams),
}
impl Login {
    pub async fn request(&self, action: AuthAction) -> Result<Value, Error> {
        let (method, p) = match action {
            AuthAction::List => (RcMethod::AuthList, json!({})),
            AuthAction::Begin(v) => (RcMethod::AuthBegin, json!(v)),
            AuthAction::Respond(v) => (RcMethod::AuthRespond, json!(v)),
            AuthAction::Cancel(v) => (RcMethod::AuthCancel, json!(v)),
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

impl Desktop {
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
pub struct Logout<'a>(&'a Login, p::AuthLogoutParams);
impl Login {
    pub fn instance(&self) -> &str {
        &self.target.instance
    }
    pub fn logout_confirmation(&self, provider: p::AuthProviderId) -> Logout<'_> {
        Logout(self, p::AuthLogoutParams { provider })
    }
}
impl Logout<'_> {
    pub async fn confirm(self) -> Result<Value, Error> {
        self.0
            .page
            .perform(Operation::Rc {
                target: self.0.target.clone(),
                method: RcMethod::AuthLogout,
                params: json!(self.1).to_string(),
            })
            .await
    }
}
pub struct GithubItem {
    page: Page,
    pub number: u64,
    pub title: String,
    pub url: Option<reqwest::Url>,
}
impl Page {
    pub fn github_item(&self, item: &Value) -> Result<GithubItem, Error> {
        self.editor()?;
        let url = item["url"]
            .as_str()
            .and_then(|s| reqwest::Url::parse(s).ok())
            .filter(|u| {
                matches!(u.scheme(), "http" | "https")
                    && u.has_host()
                    && u.username().is_empty()
                    && u.password().is_none()
            });
        Ok(GithubItem {
            page: self.clone(),
            number: item["number"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or(Error::InvalidMessage)?,
            title: item["title"].as_str().unwrap_or_default().into(),
            url,
        })
    }
}
impl GithubItem {
    pub fn ask_about(
        &self,
        app: &mut Desktop,
        w: &mut gpui::Window,
        cx: &mut gpui::Context<Desktop>,
    ) -> Result<(), Error> {
        let mut s = format!("#{}「{}」", self.number, self.title);
        if let Some(url) = &self.url {
            s.push(' ');
            s.push_str(url.as_str());
        }
        self.page.append_reference(app, &s, w, cx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_trust_is_not_reused_with_trimmed_or_different_path() {
        let mut form = SpawnForm {
            page: super::super::tests::dummy_page(),
            folder: String::new(),
            trusted: None,
            ask_tool: true,
            message: String::new(),
            model: None,
            thinking: None,
        };
        form.folder("/tmp/work ".into(), true);
        assert_eq!(
            form.params().unwrap(),
            json!({"cwd":"/tmp/work ","askTool":true,"trust":true})
        );
        form.folder("/tmp/work".into(), false);
        assert_eq!(form.params().unwrap()["trust"], false);
    }
    #[test]
    fn settings_only_persist_three_nonsecret_preferences() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let path = dir.join("settings.json");
        let mut s = Settings::load(&path).unwrap();
        s.ask_tool = true;
        s.appearance = Appearance::Dark;
        s.font_size = FontSize::Large;
        s.save(&path).unwrap();
        let s = Settings::load(&path).unwrap();
        assert!(s.ask_tool && s.font_size == FontSize::Large && s.appearance == Appearance::Dark);
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 3);
        std::fs::write(&path, "{not-json}").unwrap();
        assert!(Settings::load(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn auth_saved_and_storage_unknown_are_not_failures() {
        assert!(matches!(
            CredentialOutcome::from_flow(&json!({"status":"failed","error":"storage_failed"})),
            CredentialOutcome::StorageUnknown
        ));
        assert!(matches!(
            CredentialOutcome::from_flow(&json!({"status":"succeeded","refreshRequired":true})),
            CredentialOutcome::NeedsReload
        ));
        assert!(matches!(
            CredentialOutcome::from_flow(&json!({})),
            CredentialOutcome::Unknown
        ));
    }
}
