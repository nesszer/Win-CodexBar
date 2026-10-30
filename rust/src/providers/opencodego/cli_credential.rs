//! Read-only access to the OpenCode CLI login (upstream #4771).
//!
//! OpenCode v2 keeps credentials in the `credential` table of its SQLite
//! database; older installs keep an `opencode-go` API entry in `auth.json`.
//! Once the credential table exists it is the only source, so a stale legacy
//! file cannot revive a logged-out account. OpenCode owns writes and token
//! renewal; this reader never modifies either store.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::types::ValueRef;
use serde::Deserialize;

use crate::core::ProviderError;

const ACTIVE_CREDENTIAL_SQL: &str = r#"
SELECT integration_id, value FROM credential
WHERE active = 1 AND integration_id IN ('opencode-go', 'opencode')
ORDER BY CASE integration_id WHEN 'opencode-go' THEN 0 ELSE 1 END
LIMIT 2
"#;

#[derive(Deserialize)]
struct StoredCredential {
    #[serde(rename = "type")]
    kind: String,
    key: Option<String>,
    access: Option<String>,
    expires: Option<f64>,
}

/// Where the OpenCode CLI keeps `auth.json` and its database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CredentialStore {
    pub(super) auth_path: PathBuf,
    pub(super) db_path: PathBuf,
}

/// Credential store candidates in lookup order. An absolute `XDG_DATA_HOME`
/// replaces the default data directories, matching the CLI. `OPENCODE_DB`
/// names the database file or gives its absolute path; `:memory:` means the
/// CLI keeps no database file, so there is nothing to read.
pub(super) fn credential_stores(
    xdg_data_home: Option<&str>,
    opencode_db: Option<&str>,
    default_dirs: Vec<PathBuf>,
) -> Vec<CredentialStore> {
    let filename = opencode_db.unwrap_or("opencode.db");
    if filename == ":memory:" {
        return Vec::new();
    }
    let dirs = match xdg_data_home
        .map(Path::new)
        .filter(|path| path.is_absolute())
    {
        Some(data_home) => vec![data_home.join("opencode")],
        None => default_dirs,
    };
    dirs.into_iter()
        .map(|dir| {
            let db_path = if Path::new(filename).is_absolute() {
                PathBuf::from(filename)
            } else {
                dir.join(filename)
            };
            CredentialStore {
                auth_path: dir.join("auth.json"),
                db_path,
            }
        })
        .collect()
}

/// The token of the active OpenCode CLI login, if any. Reread on every call so
/// a logout or renewal in OpenCode takes effect on the next refresh.
pub(super) fn load_cli_credential() -> Result<Option<String>, ProviderError> {
    let default_dirs = super::local::candidate_paths()
        .into_iter()
        .filter_map(|(auth, _)| auth.parent().map(Path::to_path_buf))
        .collect();
    let xdg = std::env::var("XDG_DATA_HOME").ok();
    let db = std::env::var("OPENCODE_DB").ok();
    let stores = credential_stores(xdg.as_deref(), db.as_deref(), default_dirs);
    let Some(store) = stores
        .iter()
        .find(|store| store.db_path.exists() || store.auth_path.exists())
    else {
        return Ok(None);
    };
    load_from_store(store, Utc::now())
}

/// Any read or decode failure becomes a generic credential error so database
/// contents and tokens never reach a user-facing message.
pub(super) fn load_from_store(
    store: &CredentialStore,
    now: DateTime<Utc>,
) -> Result<Option<String>, ProviderError> {
    read_store(store, now).map_err(|()| invalid_credentials())
}

pub(super) fn invalid_credentials() -> ProviderError {
    ProviderError::AuthRequired
}

fn read_store(store: &CredentialStore, now: DateTime<Utc>) -> Result<Option<String>, ()> {
    if !store.db_path.exists() {
        return legacy_credential(&store.auth_path);
    }
    let conn = crate::core::open_readonly_sqlite_connection(
        &store.db_path,
        crate::core::DEFAULT_SQLITE_BUSY_TIMEOUT,
    )
    .map_err(|_| ())?;
    let has_credential_table = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'credential' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .map(|()| true)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(false),
            _ => Err(()),
        })?;
    if !has_credential_table {
        return legacy_credential(&store.auth_path);
    }
    active_credential(&conn, now)
}

fn active_credential(
    conn: &rusqlite::Connection,
    now: DateTime<Utc>,
) -> Result<Option<String>, ()> {
    let mut statement = conn.prepare(ACTIVE_CREDENTIAL_SQL).map_err(|_| ())?;
    let mut rows = statement.query([]).map_err(|_| ())?;
    let Some(first) = rows.next().map_err(|_| ())? else {
        return Ok(None);
    };
    let integration: String = first.get(0).map_err(|_| ())?;
    let value = match first.get_ref(1).map_err(|_| ())? {
        ValueRef::Text(bytes) => bytes.to_vec(),
        _ => return Err(()),
    };
    // Two active rows for the chosen integration are ambiguous; never pick an
    // arbitrary account.
    if let Some(next) = rows.next().map_err(|_| ())? {
        let next_integration: String = next.get(0).map_err(|_| ())?;
        if next_integration == integration {
            return Err(());
        }
    }
    let credential: StoredCredential = serde_json::from_slice(&value).map_err(|_| ())?;
    // Only an OAuth login of the general OpenCode integration also covers Go.
    if integration == "opencode" && credential.kind != "oauth" {
        return Ok(None);
    }
    match credential.kind.as_str() {
        "key" => credential_token(credential.key).map(Some),
        "oauth" => {
            let expires = credential.expires.ok_or(())?;
            // `expires` is a whole number of epoch milliseconds.
            let valid = expires.is_finite()
                && expires >= 0.0
                && expires.trunc() == expires
                && expires > now.timestamp_millis() as f64;
            if !valid {
                return Err(());
            }
            credential_token(credential.access).map(Some)
        }
        _ => Err(()),
    }
}

fn legacy_credential(auth_path: &Path) -> Result<Option<String>, ()> {
    if !auth_path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(auth_path).map_err(|_| ())?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| ())?;
    let entries = value.as_object().ok_or(())?;
    let Some(entry) = entries.get("opencode-go") else {
        return Ok(None);
    };
    let entry = entry.as_object().ok_or(())?;
    if entry.get("type").and_then(serde_json::Value::as_str) != Some("api") {
        return Ok(None);
    }
    credential_token(
        entry
            .get("key")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    )
    .map(Some)
}

fn credential_token(value: Option<String>) -> Result<String, ()> {
    match value {
        Some(token)
            if !token.is_empty() && !token.chars().any(|c| c.is_whitespace() || c.is_control()) =>
        {
            Ok(token)
        }
        _ => Err(()),
    }
}

#[cfg(test)]
#[path = "cli_credential_tests.rs"]
mod tests;
