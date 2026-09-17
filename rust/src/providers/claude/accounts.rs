//! Saved Claude Code logins. Only OAuth credentials and account identity move;
//! projects, MCP secrets, preferences, and Claude Desktop data stay in place.

mod login;

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::secure_file;

pub use login::{begin_login, cancel_login, cleanup_abandoned_logins, login, require_cli_closed};

/// Serializes account changes with our own OAuth and CLI token refreshes.
pub static CREDENTIAL_OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccount {
    pub id: String,
    pub email: String,
    pub organization: Option<String>,
    pub plan: Option<String>,
    pub is_active: bool,
    pub is_saved: bool,
}

// Never serialize these objects across the invoke bridge or log them.
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedLogin {
    oauth: Value,
    identity: Value,
}

impl SavedLogin {
    fn id(&self) -> io::Result<String> {
        identity_id(&self.identity)
    }

    fn validate(&self) -> io::Result<()> {
        self.id()?;
        required_string(&self.identity, "emailAddress")?;
        required_string(&self.oauth, "accessToken")?;
        required_string(&self.oauth, "refreshToken")?;
        Ok(())
    }

    fn summary(&self, active: bool, saved: bool) -> io::Result<ClaudeAccount> {
        Ok(ClaudeAccount {
            id: self.id()?,
            email: required_string(&self.identity, "emailAddress")?.to_owned(),
            organization: self.identity["organizationName"]
                .as_str()
                .map(str::to_owned),
            plan: self.oauth["subscriptionType"].as_str().map(str::to_owned),
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
    config_dir: PathBuf,
    config_file: PathBuf,
}

pub fn config_dir() -> io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(dir);
        if !path.is_absolute() {
            return Err(io::Error::other(
                "CLAUDE_CONFIG_DIR must be an absolute path.",
            ));
        }
        return Ok(path);
    }
    dirs::home_dir()
        .map(|p| p.join(".claude"))
        .ok_or_else(|| io::Error::other("Home directory not found."))
}

impl AccountManager {
    pub fn new() -> io::Result<Self> {
        let config_dir = config_dir()?;
        let config_file = if std::env::var_os("CLAUDE_CONFIG_DIR").is_some_and(|v| !v.is_empty()) {
            config_dir.join(".claude.json")
        } else {
            dirs::home_dir()
                .ok_or_else(|| io::Error::other("Home directory not found."))?
                .join(".claude.json")
        };
        let root = dirs::config_dir()
            .ok_or_else(|| io::Error::other("Configuration directory not found."))?
            .join("CodexBar/claude-accounts");
        Ok(Self {
            root,
            config_dir,
            config_file,
        })
    }

    fn load(&self) -> io::Result<Store> {
        let path = self.root.join("accounts.json");
        match secure_file::read_string(&path) {
            Ok(data) => serde_json::from_str(&data).map_err(|_| {
                io::Error::other(
                    "Saved Claude accounts are unreadable. The existing file has been preserved.",
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
            std::fs::rename(&temp, &path)
        })();
        if result.is_err() {
            let _cleanup = std::fs::remove_file(temp);
        }
        result
    }

    pub fn list(&self) -> io::Result<Vec<ClaudeAccount>> {
        self.list_with_consent(super::claude_code_consent())
    }

    fn list_with_consent(&self, consent: bool) -> io::Result<Vec<ClaudeAccount>> {
        let store = self.load()?;
        let current = if consent {
            read_login(&self.config_dir, &self.config_file)?
        } else {
            None
        };
        // Without credential-read consent, activity is unknown. Stale identity
        // metadata or unrelated credential entries cannot prove a live login.
        // Leave saved accounts switchable; selecting the current one is a no-op.
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
        let current = read_login(&self.config_dir, &self.config_file)?.ok_or_else(|| {
            io::Error::other("No Claude Code subscription login found. Add an account first.")
        })?;
        self.import(current)
    }

    /// Keep an already-saved account's OAuth blob aligned with the live CLI
    /// login after a token refresh. Does not create a new saved account.
    pub fn sync_saved_oauth_from_current(&self) -> io::Result<()> {
        let Some(current) = read_login(&self.config_dir, &self.config_file)? else {
            return Ok(());
        };
        current.validate()?;
        let id = current.id()?;
        let mut store = self.load()?;
        let Some(saved) = store
            .accounts
            .iter_mut()
            .find(|account| account.id().ok().as_deref() == Some(id.as_str()))
        else {
            return Ok(());
        };
        saved.oauth = current.oauth;
        self.save(&store)
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

    pub fn switch(&self, id: &str) -> io::Result<()> {
        let mut store = self.load()?;
        let target = store
            .accounts
            .iter()
            .find(|a| a.id().ok().as_deref() == Some(id))
            .cloned()
            .ok_or_else(|| io::Error::other("Saved Claude account not found."))?;
        target.validate()?;
        if let Some(current) = read_login(&self.config_dir, &self.config_file)? {
            if current.id()? == id {
                return Ok(());
            }
            // Preserve the latest refresh token before replacing the active login.
            upsert(&mut store, current)?;
            self.save(&store)?;
        }
        let credential_path = self.config_dir.join(".credentials.json");
        let old_credentials = read_object(&credential_path)?;
        let mut credentials = old_credentials.clone();
        let mut config = read_object(&self.config_file)?;
        credentials["claudeAiOauth"] = target.oauth;
        config["oauthAccount"] = target.identity;
        // Stage both files before touching either destination. Config is only
        // metadata; on failure restore credentials while retaining both logins.
        std::fs::create_dir_all(&self.config_dir)?;
        let staged_credentials = stage_json(&credential_path, &credentials)?;
        let staged_config = match stage_json(&self.config_file, &config) {
            Ok(path) => path,
            Err(e) => {
                let _cleanup = std::fs::remove_file(staged_credentials);
                return Err(e);
            }
        };
        if let Err(e) = std::fs::rename(&staged_credentials, &credential_path) {
            let _cleanup = std::fs::remove_file(staged_credentials);
            let _cleanup = std::fs::remove_file(staged_config);
            return Err(e);
        }
        if let Err(e) = std::fs::rename(&staged_config, &self.config_file) {
            let _cleanup = std::fs::remove_file(staged_config);
            let restored = stage_json(&credential_path, &old_credentials)
                .and_then(|p| std::fs::rename(p, &credential_path));
            return Err(io::Error::other(if restored.is_ok() {
                format!("Could not update Claude identity; the previous login was restored: {e}")
            } else {
                "Claude identity update failed. Both accounts remain saved; close Claude Code and retry switching.".into()
            }));
        }
        super::clear_account_caches(&credential_path);
        Ok(())
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

fn required_string<'a>(object: &'a Value, key: &str) -> io::Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| io::Error::other(format!("Claude login is missing {key}. Sign in again.")))
}

fn identity_id(identity: &Value) -> io::Result<String> {
    let account = required_string(identity, "accountUuid")?;
    let org = required_string(identity, "organizationUuid")?;
    Ok(format!("{account}:{org}"))
}

fn read_object(path: &Path) -> io::Result<Value> {
    match std::fs::read(path) {
        Ok(data) => {
            let value: Value = serde_json::from_slice(&data).map_err(|_| {
                io::Error::other("Claude configuration is invalid JSON; it has not been changed.")
            })?;
            if !value.is_object() {
                return Err(io::Error::other("Claude configuration must be an object."));
            }
            Ok(value)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e),
    }
}

fn read_login(dir: &Path, config: &Path) -> io::Result<Option<SavedLogin>> {
    let credentials = read_object(&dir.join(".credentials.json"))?;
    let Some(oauth) = credentials.get("claudeAiOauth").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let config = read_object(config)?;
    let login = SavedLogin {
        oauth: oauth.clone(),
        identity: config["oauthAccount"].clone(),
    };
    login.validate()?;
    Ok(Some(login))
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
    if let Err(e) = result {
        let _cleanup = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(temp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(account: &str, org: &str, token: &str) -> SavedLogin {
        SavedLogin {
            oauth: json!({"accessToken":token,"refreshToken":format!("refresh-{token}"),"expiresAt":9999999999999_i64,"subscriptionType":"max"}),
            identity: json!({"accountUuid":account,"organizationUuid":org,"emailAddress":"same@example.com","organizationName":org}),
        }
    }

    fn manager(dir: &Path) -> AccountManager {
        let config_dir = dir.join("cli");
        std::fs::create_dir_all(&config_dir).unwrap();
        AccountManager {
            root: dir.join("store"),
            config_file: config_dir.join(".claude.json"),
            config_dir,
        }
    }

    fn activate(manager: &AccountManager, login: &SavedLogin) {
        std::fs::write(manager.config_dir.join(".credentials.json"), json!({"claudeAiOauth":login.oauth,"mcpOAuth":{"secret":"preserved"},"pluginSecrets":{"x":"secret"}}).to_string()).unwrap();
        std::fs::write(&manager.config_file, json!({"oauthAccount":login.identity,"projects":{"test":"preferences"},"hasCompletedOnboarding":true}).to_string()).unwrap();
    }

    #[test]
    fn discovery_without_consent_never_opens_ambient_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager.import(login("saved", "one", "stored")).unwrap();
        // Invalid identity metadata leaves activity unknown; never parse credentials.
        std::fs::write(manager.config_dir.join(".credentials.json"), "invalid JSON").unwrap();
        std::fs::write(&manager.config_file, "invalid JSON").unwrap();
        let list = manager.list_with_consent(false).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].is_saved && !list[0].is_active);
        assert!(manager.list_with_consent(true).is_err());
    }

    #[test]
    fn unknown_activity_keeps_saved_accounts_switchable_after_logout() {
        let dir = tempfile::tempdir().unwrap();
        let account_manager = manager(dir.path());
        let saved = login("b", "two", "saved-token");
        account_manager.import(saved.clone()).unwrap();
        activate(&account_manager, &saved);
        assert!(account_manager.list_with_consent(true).unwrap()[0].is_active);
        assert!(!account_manager.list_with_consent(false).unwrap()[0].is_active);

        // Logout can preserve unrelated secrets and stale account metadata.
        let credential_path = account_manager.config_dir.join(".credentials.json");
        let unrelated = json!({"mcpOAuth":{"secret":"preserved"},"pluginSecrets":{"x":"secret"}});
        std::fs::write(&credential_path, unrelated.to_string()).unwrap();
        for consent in [false, true] {
            let accounts = account_manager.list_with_consent(consent).unwrap();
            assert_eq!(accounts.len(), 1);
            assert!(accounts[0].is_saved && !accounts[0].is_active);
        }
        account_manager.switch("b:two").unwrap();
        let restored = read_object(&credential_path).unwrap();
        assert_eq!(restored["claudeAiOauth"]["accessToken"], "saved-token");
        assert_eq!(restored["mcpOAuth"], unrelated["mcpOAuth"]);
        assert_eq!(restored["pluginSecrets"], unrelated["pluginSecrets"]);
        assert!(account_manager.list_with_consent(true).unwrap()[0].is_active);
        assert!(!account_manager.list_with_consent(false).unwrap()[0].is_active);
    }
    #[cfg(windows)]
    #[test]
    fn switching_does_not_require_a_metadata_write_after_credentials_change() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager.import(login("b", "two", "new")).unwrap();
        let store_path = manager.root.join("accounts.json");
        let original_permissions = std::fs::metadata(&store_path).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&store_path, readonly).unwrap();
        let result = manager.switch("b:two");
        std::fs::set_permissions(&store_path, original_permissions).unwrap();
        result.unwrap();
        assert_eq!(
            read_login(&manager.config_dir, &manager.config_file)
                .unwrap()
                .unwrap()
                .id()
                .unwrap(),
            "b:two"
        );
    }

    #[test]
    fn switching_preserves_settings_and_latest_outgoing_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        let a = login("a", "org-a", "old");
        let b = login("b", "org-b", "second");
        manager.import(a).unwrap();
        manager.import(b.clone()).unwrap();
        activate(&manager, &login("a", "org-a", "rotated"));
        manager.switch(&b.id().unwrap()).unwrap();
        let credentials = read_object(&manager.config_dir.join(".credentials.json")).unwrap();
        assert_eq!(credentials["claudeAiOauth"]["accessToken"], "second");
        assert_eq!(credentials["mcpOAuth"]["secret"], "preserved");
        assert_eq!(credentials["pluginSecrets"]["x"], "secret");
        let config = read_object(&manager.config_file).unwrap();
        assert_eq!(config["oauthAccount"]["accountUuid"], "b");
        assert_eq!(config["projects"]["test"], "preferences");
        assert_eq!(config["hasCompletedOnboarding"], true);
        manager.switch("a:org-a").unwrap();
        assert_eq!(
            read_login(&manager.config_dir, &manager.config_file)
                .unwrap()
                .unwrap()
                .oauth["accessToken"],
            "rotated"
        );
    }

    #[test]
    fn first_switch_automatically_saves_ambient_account() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        activate(&manager, &login("a", "one", "ambient"));
        manager.import(login("b", "two", "new")).unwrap();
        assert!(
            manager
                .list_with_consent(true)
                .unwrap()
                .iter()
                .any(|a| a.is_active && !a.is_saved)
        );
        manager.switch("b:two").unwrap();
        manager.switch("a:one").unwrap();
        assert!(
            manager
                .list_with_consent(true)
                .unwrap()
                .iter()
                .any(|a| a.id == "a:one" && a.is_active && a.is_saved)
        );
    }

    #[test]
    fn same_email_and_different_organizations_remain_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager.import(login("a", "one", "first")).unwrap();
        manager.import(login("a", "two", "second")).unwrap();
        manager.import(login("a", "one", "updated")).unwrap();
        assert_eq!(manager.list_with_consent(true).unwrap().len(), 2);
        assert_eq!(
            manager.load().unwrap().accounts[0].oauth["accessToken"],
            "updated"
        );
    }

    #[test]
    fn token_refresh_updates_saved_account_without_creating_one() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        activate(&manager, &login("a", "one", "rotated"));
        manager.sync_saved_oauth_from_current().unwrap();
        assert!(manager.load().unwrap().accounts.is_empty());

        manager.import(login("a", "one", "stale")).unwrap();
        activate(&manager, &login("a", "one", "rotated"));
        manager.sync_saved_oauth_from_current().unwrap();
        assert_eq!(manager.load().unwrap().accounts.len(), 1);
        assert_eq!(
            manager.load().unwrap().accounts[0].oauth["accessToken"],
            "rotated"
        );
    }

    #[test]
    fn metadata_bridge_never_contains_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        manager.import(login("a", "one", "secret-token")).unwrap();
        let json = serde_json::to_string(&manager.list_with_consent(true).unwrap()).unwrap();
        assert!(!json.contains("secret-token"));
        assert!(!json.contains("refreshToken"));
        assert!(json.contains("isSaved"));
        #[cfg(windows)]
        assert!(
            !std::fs::read_to_string(manager.root.join("accounts.json"))
                .unwrap()
                .contains("secret-token")
        );
    }

    #[test]
    fn corrupt_config_and_unknown_target_do_not_replace_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        activate(&manager, &login("a", "one", "original"));
        manager.import(login("b", "two", "other")).unwrap();
        let path = manager.config_dir.join(".credentials.json");
        let original = std::fs::read(&path).unwrap();
        assert!(manager.switch("unknown").is_err());
        std::fs::write(&manager.config_file, "invalid JSON").unwrap();
        assert!(manager.switch("b:two").is_err());
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn corrupt_store_is_not_replaced_and_removal_does_not_log_out() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        activate(&manager, &login("a", "one", "original"));
        manager.save_current().unwrap();
        manager.remove("a:one").unwrap();
        let list = manager.list_with_consent(true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].is_active && !list[0].is_saved);
        let path = manager.root.join("accounts.json");
        std::fs::write(&path, "broken").unwrap();
        assert!(manager.save_current().is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
    }

    /// Opt-in compatibility smoke test. Uses existing saved logins only inside
    /// a disposable CLI home and makes no model requests. Never switches the
    /// user's real CLI home or prints credentials/identity values.
    #[test]
    #[ignore = "requires native Claude Code and at least two saved subscription accounts"]
    fn installed_cli_recognizes_saved_accounts_in_isolated_home() {
        use std::process::{Command, Stdio};
        let source = AccountManager::new().unwrap().load().unwrap();
        assert!(
            source.accounts.len() >= 2,
            "Save two accounts before running this smoke test."
        );
        let dir = tempfile::tempdir().unwrap();
        let manager = manager(dir.path());
        for account in &source.accounts {
            manager.import(account.clone()).unwrap();
        }
        for account in source.accounts.iter().take(2) {
            manager.switch(&account.id().unwrap()).unwrap();
            let mut command = Command::new(super::login::executable().unwrap());
            command
                .args(["auth", "status", "--json"])
                .env("CLAUDE_CONFIG_DIR", &manager.config_dir)
                .current_dir(&manager.config_dir)
                .stdin(Stdio::null());
            for key in [
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDECODE",
            ] {
                command.env_remove(key);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "Claude auth status failed in isolated home."
            );
            let status: Value =
                serde_json::from_slice(&output.stdout).expect("Claude status must return JSON.");
            assert!(
                status["loggedIn"] == true,
                "Claude must recognize the selected subscription login."
            );
            assert!(
                status["email"] == account.identity["emailAddress"],
                "Claude must report the selected account's identity."
            );
        }
    }
}
