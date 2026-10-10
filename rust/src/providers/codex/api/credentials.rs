use super::CodexApi;
use crate::core::ProviderError;
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const CREDENTIAL_CACHE_TTL: Duration = Duration::from_secs(5);
/// Upstream 0.69.0 #4088: the Codex CLI owns `auth.json` and may be publishing a
/// replacement while we read it. A failed or stale read is repeated up to this many
/// times, `CREDENTIAL_READ_RETRY_DELAY` apart, before the error is reported.
const CREDENTIAL_READ_RETRIES: u32 = 2;
pub(super) const CREDENTIAL_READ_RETRY_DELAY: Duration = Duration::from_millis(50);
const EXTERNAL_OAUTH_REFRESH_WINDOW: chrono::TimeDelta = chrono::Duration::minutes(5);

static CREDENTIAL_CACHE: OnceLock<Mutex<Option<CachedCodexCredentials>>> = OnceLock::new();

impl CodexApi {
    /// Load credentials, tolerating a brief owner publication of `auth.json`.
    ///
    /// Upstream 0.69.0 #4088 (`CodexOAuthFetchStrategy.loadCredentials` on the
    /// usage path, `retryStale: true`): every failed read is repeated. That covers
    /// a missing (`NotInstalled`), unreadable (`Other`), malformed or incomplete
    /// (`Parse`) file, and a credential the gate rejects as stale (`AuthRequired`,
    /// such as a token inside its renewal window), because the CLI may be
    /// publishing its renewal. This only rereads the file: no token is redeemed,
    /// nothing is written, and the credential cache semantics are unchanged. After
    /// the last read the error keeps its category, so unchanged stale credentials
    /// still need their owner's renewal.
    pub(super) async fn load_credentials(&self) -> Result<CodexCredentials, ProviderError> {
        Self::reread_during_owner_publication(|| self.load_credentials_once()).await
    }

    /// The bounded reread behind [`Self::load_credentials`]: `read` runs up to
    /// `1 + CREDENTIAL_READ_RETRIES` times, `CREDENTIAL_READ_RETRY_DELAY` apart,
    /// until it succeeds, and the last result is returned unchanged. Dropping the
    /// returned future cancels the pending delay and any further read (upstream
    /// checks task cancellation before each read).
    pub(super) async fn reread_during_owner_publication<T>(
        mut read: impl FnMut() -> Result<T, ProviderError>,
    ) -> Result<T, ProviderError> {
        let mut retries_remaining = CREDENTIAL_READ_RETRIES;
        loop {
            match read() {
                Err(_) if retries_remaining > 0 => {
                    retries_remaining -= 1;
                    tokio::time::sleep(CREDENTIAL_READ_RETRY_DELAY).await;
                }
                result => return result,
            }
        }
    }

    pub(super) fn load_credentials_once(&self) -> Result<CodexCredentials, ProviderError> {
        let auth_path = self.get_auth_path();

        let metadata =
            std::fs::metadata(&auth_path).map_err(|error| self.credential_file_error(error))?;
        let modified = metadata.modified().ok();
        if let Some(cached) = Self::cached_credentials(&auth_path, modified) {
            Self::enforce_external_oauth_gate(&cached)?;
            return Ok(cached);
        }

        let content = std::fs::read_to_string(&auth_path)
            .map_err(|error| self.credential_file_error(error))?;

        let credentials = Self::parse_credentials_json(&content)?;
        Self::enforce_external_oauth_gate(&credentials)?;
        Self::store_cached_credentials(auth_path, modified, credentials.clone());
        Ok(credentials)
    }

    fn missing_credentials_error(&self) -> ProviderError {
        // Upstream 0.50.0 #2679: when the CLI targets Amazon Bedrock or
        // another custom backend without ChatGPT auth, sign-in guidance
        // is wrong — rate limits simply are not available there.
        if self.uses_custom_backend() {
            return ProviderError::NotInstalled(
                "Codex uses a custom backend (chatgpt_base_url / model_provider) without \
                 ChatGPT auth. ChatGPT rate limits are unavailable for this setup."
                    .to_string(),
            );
        }

        ProviderError::NotInstalled(
            "Codex auth.json not found. Run `codex login` in a terminal to sign in.".to_string(),
        )
    }

    fn credential_file_error(&self, error: std::io::Error) -> ProviderError {
        if error.kind() == std::io::ErrorKind::NotFound {
            return self.missing_credentials_error();
        }

        ProviderError::Other(format!("Failed to read Codex credentials: {error}"))
    }

    pub(super) fn parse_credentials_json(content: &str) -> Result<CodexCredentials, ProviderError> {
        let json: serde_json::Value = serde_json::from_str(content)
            .map_err(|e| ProviderError::Parse(format!("Invalid Codex credentials JSON: {}", e)))?;

        // Check for OPENAI_API_KEY first
        if let Some(api_key) = json.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
            let trimmed = api_key.trim();
            if !trimmed.is_empty() {
                return Ok(CodexCredentials {
                    access_token: trimmed.to_string(),
                    account_id: None,
                    is_external_oauth: false,
                    access_token_expires_at: None,
                    last_refresh: None,
                });
            }
        }

        // Otherwise, look for tokens object (external OAuth source)
        let tokens = json.get("tokens").ok_or_else(|| {
            ProviderError::Parse("Codex auth.json exists but contains no tokens.".to_string())
        })?;

        let access_token = tokens
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ProviderError::Parse("Missing access_token in Codex credentials".to_string())
            })?
            .to_string();

        let account_id = tokens
            .get("account_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // Upstream 0.50.1 #2944: an OAuth token set with a refresh_token is an
        // external (CLI-owned) OAuth source. The `last_refresh` timestamp is
        // retained only as provenance for the opt-in safety gate.
        let has_refresh_token = tokens
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty());
        let last_refresh = json
            .get("last_refresh")
            .and_then(|v| v.as_str())
            .and_then(parse_timestamp);

        let access_token_expires_at = parse_access_token_expiry(&access_token);

        Ok(CodexCredentials {
            access_token,
            account_id,
            is_external_oauth: has_refresh_token,
            access_token_expires_at,
            last_refresh,
        })
    }

    /// Upstream 0.50.1 #2944: when `codex_external_oauth_sources_allowed` is
    /// OFF (the default), external OAuth credential files without refresh
    /// provenance fail closed instead of being used silently. An external
    /// OAuth source is an auth.json `tokens` object with a `refresh_token`
    /// (CLI-owned OAuth, not an API key). Win-CodexBar never refreshes or
    /// writes this source: the gate only decides whether the read-only usage
    /// request may use it. When the access token is a JWT, its native expiry
    /// is the validity authority; opaque tokens are sent to the server.
    pub(super) fn enforce_external_oauth_gate(
        credentials: &CodexCredentials,
    ) -> Result<(), ProviderError> {
        if !credentials.is_external_oauth {
            return Ok(());
        }
        // The opt-in only matters without refresh provenance. Skip the settings
        // load otherwise: credential reads repeat while the owner publishes.
        let external_sources_allowed = credentials.last_refresh.is_some()
            || crate::settings::Settings::load().codex_external_oauth_sources_allowed;
        Self::enforce_external_oauth_gate_at(credentials, external_sources_allowed, Utc::now())
    }

    pub(super) fn enforce_external_oauth_gate_at(
        credentials: &CodexCredentials,
        external_sources_allowed: bool,
        now: DateTime<Utc>,
    ) -> Result<(), ProviderError> {
        if !credentials.is_external_oauth {
            return Ok(());
        }
        if !external_sources_allowed && credentials.last_refresh.is_none() {
            return Err(ProviderError::AuthRequired);
        }
        if let Some(expires_at) = credentials.access_token_expires_at
            && expires_at - now <= EXTERNAL_OAUTH_REFRESH_WINDOW
        {
            return Err(ProviderError::AuthRequired);
        }
        Ok(())
    }

    fn credential_cache() -> &'static Mutex<Option<CachedCodexCredentials>> {
        CREDENTIAL_CACHE.get_or_init(|| Mutex::new(None))
    }

    fn cached_credentials(
        path: &std::path::Path,
        modified: Option<SystemTime>,
    ) -> Option<CodexCredentials> {
        let guard = Self::credential_cache().lock().ok()?;
        let cached = guard.as_ref()?;
        if cached.path == path
            && cached.modified == modified
            && cached.loaded_at.elapsed() <= CREDENTIAL_CACHE_TTL
        {
            return Some(cached.credentials.clone());
        }
        None
    }

    fn store_cached_credentials(
        path: PathBuf,
        modified: Option<SystemTime>,
        credentials: CodexCredentials,
    ) {
        if let Ok(mut guard) = Self::credential_cache().lock() {
            *guard = Some(CachedCodexCredentials {
                path,
                modified,
                loaded_at: Instant::now(),
                credentials,
            });
        }
    }
}

#[derive(Clone)]
pub(super) struct CodexCredentials {
    pub(super) access_token: String,
    pub(super) account_id: Option<String>,
    /// True when the source is an external OAuth token set (has a
    /// `refresh_token`), as opposed to an `OPENAI_API_KEY`. The Codex CLI owns
    /// refresh and persistence for this source; this app only reads it. The
    /// `codex_external_oauth_sources_allowed` setting gates that read
    /// (upstream 0.50.1 #2944).
    pub(super) is_external_oauth: bool,
    /// Native access-token JWT expiry. When available, this is authoritative
    /// for validity; the Codex CLI still owns the refresh lifecycle.
    pub(super) access_token_expires_at: Option<DateTime<Utc>>,
    /// `last_refresh` timestamp from auth.json, when present. Its presence
    /// supplies provenance when the external-source opt-in setting is OFF;
    /// its age is not an access-token expiry signal.
    pub(super) last_refresh: Option<DateTime<Utc>>,
}

struct CachedCodexCredentials {
    path: PathBuf,
    modified: Option<SystemTime>,
    loaded_at: Instant,
    credentials: CodexCredentials,
}

/// Parse the native `exp` claim from an access-token JWT. Opaque or malformed
/// tokens return `None` and are handled by the read-only usage request.
fn parse_access_token_expiry(token: &str) -> Option<DateTime<Utc>> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let exp = json.get("exp")?.as_i64()?;
    Utc.timestamp_opt(exp, 0).single()
}
pub(super) fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(trimmed)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
        })
}
