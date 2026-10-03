//! Saved Grok CLI logins. Only auth.json identity and tokens move; sessions,
//! skills, and the rest of ~/.grok stay in the ambient home.

mod login;
mod orca;
mod orca_runtime;
mod refresh;

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::atomic_file::replace_staged;
use crate::secure_file;

pub use login::{begin_login, cancel_login, cleanup_abandoned_logins, login};
pub use orca::select_account as select_orca_account;

pub static CREDENTIAL_OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokAccount {
    pub id: String,
    pub email: String,
    pub organization: Option<String>,
    pub plan: Option<String>,
    pub is_active: bool,
    pub is_saved: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokAccountUsage {
    pub status: GrokUsageStatus,
    pub usage_available: bool,
    pub used_percent: Option<f64>,
    pub plan: Option<String>,
    pub window_minutes: Option<u32>,
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GrokUsageStatus {
    Ready,
    SignInRequired,
    Unavailable,
    Failed,
}

impl GrokAccountUsage {
    pub fn unavailable(status: GrokUsageStatus) -> Self {
        Self {
            status,
            usage_available: false,
            used_percent: None,
            plan: None,
            window_minutes: None,
            resets_at: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GrokAuthFileError {
    #[error("Failed to decode Grok auth.json: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Grok auth.json must be an object.")]
    NotObject,
    #[error("Grok login is missing an account. Sign in again.")]
    MissingAccount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrokAuthKind {
    Cli,
    OAuth,
}

pub(crate) struct ParsedGrokAuthFile {
    root: Value,
}

#[derive(Clone, Copy)]
pub(crate) struct GrokAuthFile<'a> {
    entries: &'a Map<String, Value>,
}

#[derive(Clone, Copy)]
pub(crate) struct GrokAuthEntry<'a> {
    scope: &'a str,
    value: &'a Value,
}

impl ParsedGrokAuthFile {
    pub(crate) fn parse(text: &str) -> Result<Self, GrokAuthFileError> {
        Ok(Self {
            root: serde_json::from_str(text)?,
        })
    }

    pub(crate) fn view(&self) -> Result<GrokAuthFile<'_>, GrokAuthFileError> {
        GrokAuthFile::from_value(&self.root)
    }
}

impl<'a> GrokAuthFile<'a> {
    fn from_value(value: &'a Value) -> Result<Self, GrokAuthFileError> {
        Ok(Self {
            entries: value.as_object().ok_or(GrokAuthFileError::NotObject)?,
        })
    }

    pub(crate) fn select(&self, kind: GrokAuthKind) -> Option<GrokAuthEntry<'a>> {
        self.entries.iter().find_map(|(scope, value)| {
            let entry = GrokAuthEntry { scope, value };
            (entry.has_key() && entry.kind() == kind).then_some(entry)
        })
    }

    fn select_account(&self) -> Result<GrokAuthEntry<'a>, GrokAuthFileError> {
        self.select(GrokAuthKind::OAuth)
            .or_else(|| {
                self.entries.iter().find_map(|(scope, value)| {
                    let entry = GrokAuthEntry { scope, value };
                    (entry.has_key() && text_field(value, "email").is_some()).then_some(entry)
                })
            })
            .ok_or(GrokAuthFileError::MissingAccount)
    }
}

impl<'a> GrokAuthEntry<'a> {
    pub(crate) fn value(self) -> &'a Value {
        self.value
    }

    pub(crate) fn key(self) -> Option<&'a str> {
        self.value
            .get("key")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    }

    fn has_key(self) -> bool {
        self.key().is_some()
    }

    fn kind(self) -> GrokAuthKind {
        if self.scope.starts_with("https://auth.x.ai::")
            || self
                .value
                .get("auth_mode")
                .and_then(Value::as_str)
                .is_some_and(|mode| mode.eq_ignore_ascii_case("oidc"))
        {
            GrokAuthKind::OAuth
        } else {
            GrokAuthKind::Cli
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedLogin {
    auth: Value,
}

impl SavedLogin {
    fn id(&self) -> io::Result<String> {
        identity_id(&self.auth)
    }

    fn validate(&self) -> io::Result<()> {
        let entry = auth_file(&self.auth)?
            .select_account()
            .map_err(io::Error::other)?;
        required_string(entry.value(), "user_id")?;
        required_string(entry.value(), "email")?;
        required_string(entry.value(), "key")?;
        Ok(())
    }

    fn summary(&self, active: bool, saved: bool) -> io::Result<GrokAccount> {
        let entry = auth_file(&self.auth)?
            .select_account()
            .map_err(io::Error::other)?;
        Ok(GrokAccount {
            id: self.id()?,
            email: required_string(entry.value(), "email")?.to_owned(),
            organization: text_field(entry.value(), "team_id"),
            plan: plan_name(entry.value()),
            is_active: active,
            is_saved: saved,
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    accounts: Vec<SavedLogin>,
}

pub struct AccountManager {
    root: PathBuf,
    ambient_auth: PathBuf,
}

pub fn ambient_home() -> io::Result<PathBuf> {
    if let Ok(home) = std::env::var("GROK_HOME")
        && !home.trim().is_empty()
    {
        let path = PathBuf::from(home);
        if !path.is_absolute() {
            return Err(io::Error::other("GROK_HOME must be an absolute path."));
        }
        return Ok(path);
    }
    dirs::home_dir()
        .map(|p| p.join(".grok"))
        .ok_or_else(|| io::Error::other("Home directory not found."))
}

impl AccountManager {
    pub fn new() -> io::Result<Self> {
        let root = dirs::config_dir()
            .ok_or_else(|| io::Error::other("Configuration directory not found."))?
            .join("CodexBar/grok-accounts");
        Ok(Self {
            ambient_auth: ambient_home()?.join("auth.json"),
            root,
        })
    }

    fn load(&self) -> io::Result<Store> {
        let path = self.root.join("accounts.json");
        match secure_file::read_string(&path) {
            Ok(data) => serde_json::from_str(&data).map_err(|_| {
                io::Error::other(
                    "Saved Grok accounts are unreadable. The existing file has been preserved.",
                )
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Store::default()),
            Err(e) => Err(e),
        }
    }

    fn save(&self, store: &Store) -> io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.root.join("accounts.json");
        let temp = self.root.join(format!("{}.tmp", Uuid::new_v4()));
        let result = (|| {
            secure_file::write_string(
                &temp,
                &serde_json::to_string(store).map_err(io::Error::other)?,
            )?;
            replace_staged(&temp, &path)
        })();
        if result.is_err() {
            let _cleanup = std::fs::remove_file(temp);
        }
        result
    }

    pub fn list(&self) -> io::Result<Vec<GrokAccount>> {
        let store = self.load()?;
        let current = read_login(&self.ambient_auth)?;
        let active = current.as_ref().map(SavedLogin::id).transpose()?;
        let mut accounts = store
            .accounts
            .iter()
            .map(|a| a.summary(active.as_ref() == a.id().ok().as_ref(), true))
            .collect::<io::Result<Vec<_>>>()?;
        if let Some(current) = current
            && !accounts.iter().any(|a| Some(&a.id) == active.as_ref())
        {
            accounts.insert(0, current.summary(true, false)?);
        }
        Ok(accounts)
    }

    pub fn save_current(&self) -> io::Result<()> {
        let current = read_login(&self.ambient_auth)?
            .ok_or_else(|| io::Error::other("No Grok login found. Add an account first."))?;
        self.import(current)
    }

    pub fn import(&self, login: SavedLogin) -> io::Result<()> {
        login.validate()?;
        let mut store = self.load()?;
        upsert(&mut store, login)?;
        self.save(&store)
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        let mut store = self.load()?;
        store
            .accounts
            .retain(|a| a.id().ok().as_deref() != Some(id));
        self.save(&store)
    }

    /// Refresh only the requested identity, preserving the currently selected account.
    pub fn reauthenticate(&self, id: &str, login: SavedLogin) -> io::Result<()> {
        login.validate()?;
        if login.id()? != id {
            return Err(io::Error::other(
                "Signed in to a different Grok account. Sign in to the selected account instead.",
            ));
        }
        if !self.list()?.iter().any(|account| account.id == id) {
            return Err(io::Error::other("Grok account no longer exists."));
        }
        let active = read_login(&self.ambient_auth)?
            .is_some_and(|current| current.id().ok().as_deref() == Some(id));
        self.import(login.clone())?;
        orca::update_login(id, &login)?;
        if active {
            let staged = stage_json(&self.ambient_auth, &login.auth)?;
            if let Err(error) = replace_staged(&staged, &self.ambient_auth) {
                let _cleanup = std::fs::remove_file(staged);
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn switch(&self, id: &str) -> io::Result<()> {
        let mut store = self.load()?;
        let target = store
            .accounts
            .iter()
            .find(|a| a.id().ok().as_deref() == Some(id))
            .cloned()
            .ok_or_else(|| io::Error::other("Saved Grok account not found."))?;
        let target = orca::saved_login(id)?.unwrap_or(target);
        target.validate()?;
        if let Some(current) = read_login(&self.ambient_auth)? {
            if current.id()? == id {
                return Ok(());
            }
            upsert(&mut store, current)?;
            self.save(&store)?;
        }
        if let Some(parent) = self.ambient_auth.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staged = stage_json(&self.ambient_auth, &target.auth)?;
        if let Err(e) = replace_staged(&staged, &self.ambient_auth) {
            let _cleanup = std::fs::remove_file(staged);
            return Err(e);
        }
        Ok(())
    }

    pub fn auth_text_for(&self, id: &str) -> io::Result<String> {
        if let Some(login) = orca::saved_login(id)? {
            return serde_json::to_string(&login.auth).map_err(io::Error::other);
        }
        // Switch writes the live login to ~/.grok/auth.json. Prefer that file
        // when the id matches so the Active row uses the same credentials as
        // the Weekly card, not a stale saved copy.
        let current = read_login(&self.ambient_auth);
        if let Ok(Some(login)) = &current
            && login.id()? == id
        {
            // Serialize the same snapshot whose identity was checked. A second
            // file read could race with a switch to another account.
            return serde_json::to_string(&login.auth).map_err(io::Error::other);
        }
        let store = self.load()?;
        if let Some(login) = store
            .accounts
            .iter()
            .find(|a| a.id().ok().as_deref() == Some(id))
        {
            return serde_json::to_string(&login.auth).map_err(io::Error::other);
        }
        // A damaged ambient file must not prevent usage reads for a saved
        // account. If no saved identity matched, retain the ambient diagnostic.
        current.map(|_| ())?;
        Err(io::Error::other("Grok account not found."))
    }
}

fn upsert(store: &mut Store, login: SavedLogin) -> io::Result<()> {
    let id = login.id()?;
    if let Some(old) = store
        .accounts
        .iter_mut()
        .find(|a| a.id().ok().as_deref() == Some(id.as_str()))
    {
        *old = login;
    } else {
        store.accounts.push(login);
    }
    Ok(())
}

fn auth_file(value: &Value) -> io::Result<GrokAuthFile<'_>> {
    GrokAuthFile::from_value(value).map_err(io::Error::other)
}

fn required_string<'a>(object: &'a Value, key: &str) -> io::Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| io::Error::other(format!("Grok login is missing {key}. Sign in again.")))
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn identity_id(auth: &Value) -> io::Result<String> {
    let entry = auth_file(auth)?
        .select_account()
        .map_err(io::Error::other)?;
    required_string(entry.value(), "user_id").map(str::to_owned)
}

fn plan_name(entry: &Value) -> Option<String> {
    match text_field(entry, "auth_mode")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "oidc" => Some("SuperGrok".to_string()),
        other if !other.is_empty() => Some(other.to_string()),
        _ => None,
    }
}

pub(super) fn read_login(path: &Path) -> io::Result<Option<SavedLogin>> {
    match std::fs::read(path) {
        Ok(data) => {
            let auth: Value = serde_json::from_slice(&data).map_err(|_| {
                io::Error::other("Grok auth.json is invalid JSON; it has not been changed.")
            })?;
            let login = SavedLogin { auth };
            login.validate()?;
            Ok(Some(login))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn stage_json(path: &Path, value: &Value) -> io::Result<PathBuf> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(value).map_err(io::Error::other)?)?;
        file.sync_all()
    })();
    if let Err(error) = result {
        let _cleanup = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(temp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn login(user: &str, email: &str, token: &str) -> SavedLogin {
        SavedLogin {
            auth: json!({
                "https://auth.x.ai::client": {
                    "key": token,
                    "refresh_token": format!("refresh-{token}"),
                    "auth_mode": "oidc",
                    "email": email,
                    "user_id": user,
                    "team_id": "team-one",
                    "expires_at": "2099-01-01T00:00:00Z"
                }
            }),
        }
    }

    fn manager(dir: &Path) -> AccountManager {
        AccountManager {
            root: dir.join("store"),
            ambient_auth: dir.join("home").join("auth.json"),
        }
    }

    fn activate(manager: &AccountManager, login: &SavedLogin) {
        std::fs::create_dir_all(manager.ambient_auth.parent().unwrap()).unwrap();
        std::fs::write(
            &manager.ambient_auth,
            serde_json::to_vec_pretty(&login.auth).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn reauthentication_updates_only_the_selected_saved_identity() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let first = login("user-a", "a@example.com", "original");
        activate(&manager, &first);
        manager.save_current().unwrap();
        manager
            .import(login("user-b", "b@example.com", "other"))
            .unwrap();
        let ambient = std::fs::read(&manager.ambient_auth).unwrap();
        manager
            .reauthenticate("user-b", login("user-b", "b@example.com", "renewed"))
            .unwrap();
        assert_eq!(manager.list().unwrap().len(), 2);
        assert!(manager.auth_text_for("user-b").unwrap().contains("renewed"));
        assert_eq!(std::fs::read(&manager.ambient_auth).unwrap(), ambient);
        manager
            .reauthenticate("user-a", login("user-a", "a@example.com", "active-renewed"))
            .unwrap();
        assert!(
            manager
                .auth_text_for("user-a")
                .unwrap()
                .contains("active-renewed")
        );
        assert!(
            std::fs::read_to_string(&manager.ambient_auth)
                .unwrap()
                .contains("active-renewed")
        );
        assert_eq!(manager.list().unwrap().len(), 2);
    }

    #[test]
    fn reauthentication_rejects_a_different_identity_without_changing_any_login() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        activate(&manager, &login("user-a", "a@example.com", "original"));
        manager.save_current().unwrap();
        let ambient = std::fs::read(&manager.ambient_auth).unwrap();
        let saved = manager.auth_text_for("user-a").unwrap();
        assert!(
            manager
                .reauthenticate("user-a", login("user-b", "b@example.com", "wrong"))
                .is_err()
        );
        assert_eq!(manager.list().unwrap().len(), 1);
        assert_eq!(manager.auth_text_for("user-a").unwrap(), saved);
        assert_eq!(std::fs::read(&manager.ambient_auth).unwrap(), ambient);
    }

    #[test]
    fn add_and_switch_preserves_outgoing_account() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let first = login("user-a", "a@example.com", "token-a");
        let second = login("user-b", "b@example.com", "token-b");
        activate(&manager, &first);
        manager.import(second.clone()).unwrap();
        let listed = manager.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .any(|a| a.email == "a@example.com" && a.is_active && !a.is_saved)
        );
        assert!(
            listed
                .iter()
                .any(|a| a.email == "b@example.com" && a.is_saved && !a.is_active)
        );

        manager.switch("user-b").unwrap();
        let listed = manager.list().unwrap();
        assert!(
            listed
                .iter()
                .any(|a| a.id == "user-b" && a.is_active && a.is_saved)
        );
        assert!(
            listed
                .iter()
                .any(|a| a.id == "user-a" && a.is_saved && !a.is_active)
        );

        manager.switch("user-a").unwrap();
        let restored: Value =
            serde_json::from_slice(&std::fs::read(&manager.ambient_auth).unwrap()).unwrap();
        assert_eq!(restored["https://auth.x.ai::client"]["key"], "token-a");
    }

    #[test]
    fn remove_does_not_log_out_ambient() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let current = login("user-a", "a@example.com", "token-a");
        activate(&manager, &current);
        manager.save_current().unwrap();
        manager.remove("user-a").unwrap();
        let listed = manager.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].is_active && !listed[0].is_saved);
        assert!(manager.ambient_auth.exists());
    }

    #[test]
    fn metadata_bridge_never_contains_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager
            .import(login("user-a", "a@example.com", "secret-token"))
            .unwrap();
        let json = serde_json::to_string(&manager.list().unwrap()).unwrap();
        assert!(!json.contains("secret-token"));
        assert!(!json.contains("refresh-"));
        assert!(json.contains("isSaved"));
        assert!(json.contains("SuperGrok"));
    }

    #[test]
    fn unknown_target_does_not_replace_ambient() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let original = login("user-a", "a@example.com", "original");
        activate(&manager, &original);
        manager
            .import(login("user-b", "b@example.com", "other"))
            .unwrap();
        let before = std::fs::read(&manager.ambient_auth).unwrap();
        assert!(manager.switch("missing").is_err());
        assert_eq!(std::fs::read(&manager.ambient_auth).unwrap(), before);
    }

    #[test]
    fn auth_text_for_saved_and_ambient_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let first = login("user-a", "a@example.com", "token-a");
        let second = login("user-b", "b@example.com", "token-b");
        activate(&manager, &first);
        manager.import(second).unwrap();
        let saved = manager.auth_text_for("user-b").unwrap();
        assert!(saved.contains("token-b"));
        let ambient = manager.auth_text_for("user-a").unwrap();
        assert!(ambient.contains("token-a"));
        assert!(manager.auth_text_for("missing").is_err());
    }

    #[test]
    fn auth_text_for_prefers_live_login_over_saved_copy() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager
            .import(login("user-a", "a@example.com", "token-old"))
            .unwrap();
        activate(&manager, &login("user-a", "a@example.com", "token-live"));
        let text = manager.auth_text_for("user-a").unwrap();
        assert!(text.contains("token-live"));
        assert!(!text.contains("token-old"));
    }

    #[test]
    fn auth_text_for_saved_account_survives_invalid_ambient_login() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager
            .import(login("user-a", "a@example.com", "token-saved"))
            .unwrap();
        std::fs::create_dir_all(manager.ambient_auth.parent().unwrap()).unwrap();
        std::fs::write(&manager.ambient_auth, "invalid-json").unwrap();
        assert!(
            manager
                .auth_text_for("user-a")
                .unwrap()
                .contains("token-saved")
        );
        assert!(manager.auth_text_for("missing").is_err());
        assert_eq!(
            std::fs::read_to_string(&manager.ambient_auth).unwrap(),
            "invalid-json"
        );
    }

    #[test]
    fn same_user_upserts_instead_of_duplicating() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager
            .import(login("user-a", "a@example.com", "first"))
            .unwrap();
        manager
            .import(login("user-a", "a@example.com", "updated"))
            .unwrap();
        assert_eq!(manager.list().unwrap().len(), 1);
        assert_eq!(
            manager.load().unwrap().accounts[0].auth["https://auth.x.ai::client"]["key"],
            "updated"
        );
    }
}
