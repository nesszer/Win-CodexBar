use super::{SavedLogin, read_login};
use crate::core::ProviderError;
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

struct LinkedAccount {
    id: String,
    home: PathBuf,
}

fn resolve_account(root: &Path, user_id: &str) -> io::Result<Option<LinkedAccount>> {
    let index = root.join("accounts.json");
    if !index.exists() {
        return Ok(None);
    }
    if std::fs::symlink_metadata(root)?.file_type().is_symlink()
        || std::fs::symlink_metadata(&index)?.file_type().is_symlink()
    {
        return Err(io::Error::other(
            "Orca account storage is not a regular folder.",
        ));
    }
    let value: Value = serde_json::from_slice(&std::fs::read(index)?).map_err(io::Error::other)?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(io::Error::other("Unsupported Orca account storage."));
    }
    let matches: Vec<_> = value
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("Invalid Orca accounts."))?
        .iter()
        .filter(|account| account.get("userId").and_then(Value::as_str) == Some(user_id))
        .collect();
    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() != 1 {
        return Err(io::Error::other(
            "Choose this Grok account directly in Orca.",
        ));
    }
    let id = matches[0]
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or_else(|| io::Error::other("Invalid Orca account folder."))?;
    let home = root.join(id);
    for path in [
        &home,
        &home.join(".orca-grok-account"),
        &home.join("auth.json"),
    ] {
        if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(io::Error::other("Orca account folder contains a link."));
        }
    }
    if std::fs::read_to_string(home.join(".orca-grok-account"))?.trim() != id
        || home.canonicalize()? != root.canonicalize()?.join(id)
    {
        return Err(io::Error::other(
            "Orca account folder is not owned by Orca.",
        ));
    }
    let login = read_login(&home.join("auth.json"))?
        .ok_or_else(|| io::Error::other("Reconnect this Grok account in Orca."))?;
    if login.id()? != user_id {
        return Err(io::Error::other("Orca login identity changed."));
    }
    Ok(Some(LinkedAccount {
        id: id.to_owned(),
        home,
    }))
}

fn linked_account(user_id: &str) -> io::Result<Option<LinkedAccount>> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let Some(root) = dirs::config_dir() else {
        return Ok(None);
    };
    resolve_account(&root.join("orca/grok-accounts"), user_id)
}

pub(super) fn saved_login(user_id: &str) -> io::Result<Option<SavedLogin>> {
    linked_account(user_id)?
        .map(|account| read_login(&account.home.join("auth.json")))
        .transpose()
        .map(Option::flatten)
}

fn write_login(account: LinkedAccount, user_id: &str, login: &SavedLogin) -> io::Result<()> {
    login.validate()?;
    if login.id()? != user_id {
        return Err(io::Error::other("Orca login identity changed."));
    }
    let destination = account.home.join("auth.json");
    let staged = super::stage_json(&destination, &login.auth)?;
    crate::atomic_file::replace_staged(&staged, &destination)
}

pub(super) fn update_login(user_id: &str, login: &SavedLogin) -> io::Result<()> {
    if let Some(account) = linked_account(user_id)? {
        write_login(account, user_id, login)?;
    }
    Ok(())
}

async fn run_orca(args: &[&str]) -> Result<(), ProviderError> {
    let binary = dirs::data_local_dir()
        .ok_or(ProviderError::AuthRequired)?
        .join("Programs/orca/resources/bin/orca.exe");
    super::orca_runtime::run(&binary, args).await
}

pub(super) async fn refresh_login(user_id: &str) -> Result<Option<String>, ProviderError> {
    let Some(account) = linked_account(user_id).map_err(|_| ProviderError::AuthRequired)? else {
        return Ok(None);
    };
    let mut login = read_login(&account.home.join("auth.json"))
        .map_err(|_| ProviderError::AuthRequired)?
        .ok_or(ProviderError::AuthRequired)?;
    if needs_renewal(&login) {
        run_orca(&["account", "usage", "--agent", "grok"]).await?;
        login = read_login(&account.home.join("auth.json"))
            .map_err(|_| ProviderError::AuthRequired)?
            .ok_or(ProviderError::AuthRequired)?;
    }
    if login.id().map_err(|_| ProviderError::AuthRequired)? != user_id {
        return Err(ProviderError::AuthRequired);
    }
    serde_json::to_string(&login.auth)
        .map(Some)
        .map_err(|_| ProviderError::AuthRequired)
}

fn needs_renewal(login: &SavedLogin) -> bool {
    super::auth_file(&login.auth)
        .ok()
        .and_then(|file| file.select_account().ok())
        .and_then(|entry| super::text_field(entry.value(), "expires_at"))
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
        .is_some_and(|expires| expires <= chrono::Utc::now() + chrono::Duration::minutes(5))
}

pub async fn select_account(user_id: &str) -> Result<(), ProviderError> {
    let Some(account) = linked_account(user_id).map_err(|_| ProviderError::AuthRequired)? else {
        return Ok(());
    };
    run_orca(&[
        "account",
        "select",
        "--agent",
        "grok",
        "--account-id",
        &account.id,
    ])
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_login_reads_do_not_require_the_orca_runtime() {
        let login = |expires: &str| SavedLogin {
            auth: serde_json::json!({"https://auth.x.ai::client":{"key":"fixture","user_id":"user-one","email":"one@example.com","expires_at":expires}}),
        };
        assert!(!needs_renewal(&login("2099-01-01T00:00:00Z")));
        assert!(needs_renewal(&login("2020-01-01T00:00:00Z")));
    }
    #[test]
    fn resolves_only_marked_matching_accounts_and_rejects_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let home = root.join("saved-one");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(home.join(".orca-grok-account"), "saved-one").unwrap();
        std::fs::write(home.join("auth.json"), r#"{"https://auth.x.ai::client":{"key":"fixture","user_id":"user-one","email":"one@example.com"}}"#).unwrap();
        let index = |id: &str| {
            serde_json::json!({"version":1,"accounts":[{"id":id,"userId":"user-one"}]}).to_string()
        };
        std::fs::write(root.join("accounts.json"), index("saved-one")).unwrap();
        assert_eq!(
            resolve_account(root, "user-one").unwrap().unwrap().home,
            home
        );
        assert!(resolve_account(root, "user-two").unwrap().is_none());
        let renewed = SavedLogin {
            auth: serde_json::json!({"https://auth.x.ai::client":{"key":"renewed-fixture","user_id":"user-one","email":"one@example.com"}}),
        };
        write_login(
            resolve_account(root, "user-one").unwrap().unwrap(),
            "user-one",
            &renewed,
        )
        .unwrap();
        assert_eq!(
            read_login(&home.join("auth.json")).unwrap().unwrap().auth,
            renewed.auth
        );
        std::fs::write(root.join("accounts.json"), index("../outside")).unwrap();
        assert!(resolve_account(root, "user-one").is_err());
    }
}
