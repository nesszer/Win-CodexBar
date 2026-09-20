//! Nous Portal credential resolution.
//!
//! Reads Hermes-held credentials without refreshing or writing them:
//! explicit token -> environment -> auth files in precedence order, the
//! multi-entry credential pool comparator, portal-origin trust policy, and
//! JWT expiry inspection.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::Url;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::ProviderError;
use crate::providers::validated_https_url;

pub(super) const DEFAULT_PORTAL_URL: &str = "https://portal.nousresearch.com";
pub(super) const ACCESS_TOKEN_ENV: &str = "NOUS_PORTAL_ACCESS_TOKEN";
pub(super) const PORTAL_URL_ENVS: &[&str] = &["NOUS_PORTAL_BASE_URL", "HERMES_PORTAL_BASE_URL"];
pub(super) const HERMES_HOME_ENV: &str = "HERMES_HOME";
pub(super) const EXPIRY_SKEW: i64 = 60;
const TRUSTED_PORTAL_HOST: &str = "nousresearch.com";

#[derive(Debug, Clone)]
pub(super) struct Credential {
    pub(super) token: String,
    pub(super) portal_url: Url,
    expires_at: Option<DateTime<Utc>>,
}

impl Credential {
    fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= now + ChronoDuration::seconds(EXPIRY_SKEW))
    }
}

#[derive(Debug, Clone)]
pub(super) struct StoredCredential {
    pub(super) token: String,
    portal_base_url: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

pub(super) fn resolve_credential(
    explicit_token: Option<&str>,
) -> Result<Credential, ProviderError> {
    let environment: HashMap<String, String> = std::env::vars().collect();
    let home = dirs::home_dir().ok_or_else(missing_credentials)?;
    resolve_credential_from(explicit_token, &environment, &home, Utc::now())
}

pub(super) fn resolve_credential_from(
    explicit_token: Option<&str>,
    environment: &HashMap<String, String>,
    home_directory: &Path,
    now: DateTime<Utc>,
) -> Result<Credential, ProviderError> {
    if let Some(token) = cleaned(explicit_token) {
        return usable_credential(token, resolve_portal_url(environment, None), now);
    }
    if let Some(token) = cleaned(environment.get(ACCESS_TOKEN_ENV).map(String::as_str)) {
        return usable_credential(token, resolve_portal_url(environment, None), now);
    }

    let candidates = auth_file_candidates(environment, home_directory);
    let mut saw_file = false;
    let mut expired: Option<Credential> = None;
    for path in &candidates {
        let Ok(contents) = std::fs::read(path) else {
            continue;
        };
        saw_file = true;
        let Some(stored) = parse_auth_file(&contents) else {
            continue;
        };
        let credential = Credential {
            expires_at: stored.expires_at.or_else(|| jwt_expiry(&stored.token)),
            portal_url: resolve_portal_url(environment, stored.portal_base_url.as_deref()),
            token: stored.token,
        };
        if credential.is_expired(now) {
            expired.get_or_insert(credential);
        } else {
            return Ok(credential);
        }
    }

    if expired.is_some() {
        return Err(ProviderError::OAuthExpired(
            "Nous Portal Hermes login expired. Run hermes to refresh it.".to_string(),
        ));
    }
    if saw_file {
        return Err(ProviderError::NotInstalled(
            "Nous Portal auth files contain no usable login. Run hermes to sign in again."
                .to_string(),
        ));
    }
    Err(missing_credentials())
}

fn usable_credential(
    token: String,
    portal_url: Url,
    now: DateTime<Utc>,
) -> Result<Credential, ProviderError> {
    let credential = Credential {
        expires_at: jwt_expiry(&token),
        token,
        portal_url,
    };
    if credential.is_expired(now) {
        return Err(ProviderError::OAuthExpired(
            "Nous Portal access token expired. Run hermes to refresh it.".to_string(),
        ));
    }
    Ok(credential)
}

fn missing_credentials() -> ProviderError {
    ProviderError::NotInstalled(
        "Nous Portal login not found. Run hermes to sign in, then refresh CodexBar.".to_string(),
    )
}

fn auth_file_candidates(
    environment: &HashMap<String, String>,
    home_directory: &Path,
) -> Vec<PathBuf> {
    let root = environment
        .get(HERMES_HOME_ENV)
        .and_then(|raw| cleaned(Some(raw.as_str())))
        .map(|raw| expand_home(&raw, home_directory))
        .unwrap_or_else(|| {
            let home = environment
                .get("HOME")
                .and_then(|raw| cleaned(Some(raw.as_str())))
                .map(|raw| expand_home(&raw, home_directory))
                .unwrap_or_else(|| home_directory.to_path_buf());
            home.join(".hermes")
        });
    vec![
        root.join("auth.json"),
        root.join("shared").join("nous_auth.json"),
    ]
}

fn expand_home(raw: &str, home_directory: &Path) -> PathBuf {
    if raw == "~" {
        return home_directory.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home_directory.join(rest);
    }
    PathBuf::from(raw)
}

/// Parse one Hermes auth file. Three shapes exist because Hermes evolved;
/// each branch is tried in order and the first match wins:
///
/// 1. Current: `{"providers": {"nous": {...}}}` — the per-provider state
///    Hermes writes today.
/// 2. Legacy pool: `{"credential_pool": {"nous": [...]}}` — the multi-entry
///    pool the agent key rotation used before the single-provider shape.
/// 3. Oldest fallback: the root object itself is the credential, matching
///    the earliest hand-edited `auth.json` layout.
pub(super) fn parse_auth_file(contents: &[u8]) -> Option<StoredCredential> {
    let root: Value = serde_json::from_slice(contents).ok()?;
    let root_object = root.as_object()?;

    if let Some(providers) = root_object.get("providers").and_then(Value::as_object)
        && let Some(nous) = providers.get("nous")
        && let Some(stored) = stored_credential(nous)
    {
        return Some(stored);
    }

    if let Some(entries) = root_object
        .get("credential_pool")
        .and_then(Value::as_object)
        .and_then(|pool| pool.get("nous"))
        .and_then(Value::as_array)
    {
        return select_pool_credential(entries);
    }

    stored_credential(&root)
}

/// Pool selection order: freshest agent key first, then freshest access
/// token, then *lower* `priority` wins. An entry with no parseable expiry
/// (`unwrap_or(0)`) intentionally ranks below any known expiry — an unknown
/// expiry is treated as the riskiest credential in the pool.
fn select_pool_credential(entries: &[Value]) -> Option<StoredCredential> {
    entries
        .iter()
        .filter_map(|entry| {
            let stored = stored_credential(entry)?;
            Some((entry, stored))
        })
        .max_by_key(|(entry, stored)| {
            let agent_expiry = entry
                .get("agent_key_expires_at")
                .and_then(Value::as_str)
                .and_then(parse_iso)
                .map(|value| value.timestamp())
                .unwrap_or(0);
            let access_expiry = stored
                .expires_at
                .or_else(|| jwt_expiry(&stored.token))
                .map(|value| value.timestamp())
                .unwrap_or(0);
            let priority = entry.get("priority").and_then(Value::as_i64).unwrap_or(0);
            (agent_expiry, access_expiry, std::cmp::Reverse(priority))
        })
        .map(|(_, stored)| stored)
}

fn stored_credential(value: &Value) -> Option<StoredCredential> {
    let object = value.as_object()?;
    let token = cleaned(object.get("access_token").and_then(Value::as_str))?;
    Some(StoredCredential {
        token,
        portal_base_url: cleaned(object.get("portal_base_url").and_then(Value::as_str)),
        expires_at: object
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(parse_iso),
    })
}

/// Resolve the portal origin for one fetch.
///
/// Trust policy is asymmetric by design: a *stored* `portal_base_url` is
/// allow-listed to `nousresearch.com` and its subdomains, but the
/// `NOUS_PORTAL_BASE_URL` / `HERMES_PORTAL_BASE_URL` environment overrides
/// bypass the allow-list entirely and win over stored values. In a local
/// desktop app the operator who can set the process environment already
/// controls the app, so the env override is a developer escape hatch, not a
/// security boundary — the allow-list only protects persisted settings.
pub(super) fn resolve_portal_url(
    environment: &HashMap<String, String>,
    stored: Option<&str>,
) -> Url {
    for key in PORTAL_URL_ENVS {
        if let Some(raw) = environment.get(*key).and_then(|value| cleaned(Some(value)))
            && let Ok(url) = validated_https_url(&raw, "Nous Portal")
            && is_plain_origin(&url)
        {
            return url;
        }
    }
    if let Some(raw) = stored.and_then(|value| cleaned(Some(value)))
        && let Ok(url) = validated_https_url(&raw, "Nous Portal")
        && is_plain_origin(&url)
        && is_trusted_portal_host(url.host_str())
    {
        return url;
    }
    Url::parse(DEFAULT_PORTAL_URL).expect("default Nous Portal URL is valid")
}

/// The canonical validator allows non-empty paths, queries, and fragments;
/// a portal origin is the bare `https://host` root, so pin that on top.
fn is_plain_origin(url: &Url) -> bool {
    (url.path().is_empty() || url.path() == "/")
        && url.query().is_none()
        && url.fragment().is_none()
}

fn is_trusted_portal_host(host: Option<&str>) -> bool {
    let Some(host) = host.map(str::to_ascii_lowercase) else {
        return false;
    };
    host == TRUSTED_PORTAL_HOST || host.ends_with(&format!(".{TRUSTED_PORTAL_HOST}"))
}

fn cleaned(value: Option<&str>) -> Option<String> {
    let mut value = value?.trim().to_string();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value = value[1..value.len() - 1].trim().to_string();
    }
    (!value.is_empty()).then_some(value)
}

pub(super) fn parse_iso(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    let payload = crate::codex_accounts::api::jwt_payload(token)?;
    let seconds = payload.get("exp")?.as_f64()?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "JWT expiration is converted to whole epoch seconds"
    )]
    let seconds = seconds.trunc() as i64;
    DateTime::from_timestamp(seconds, 0)
}
