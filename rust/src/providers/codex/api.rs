//! Codex API client for fetching usage information
//!
//! Uses OAuth tokens stored by the Codex CLI in ~/.codex/auth.json

use super::{pat, weekly_reset};
use crate::core::{
    CostSnapshot, NamedRateWindow, ProviderError, RateWindow, RateWindowCadence, UsageSnapshot,
};
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Mutex as AsyncMutex;

#[path = "subscription.rs"]
mod subscription;

const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const USAGE_PATH: &str = "/wham/usage";
const RESET_CREDITS_PATH: &str = "/wham/rate-limit-reset-credits";
const CREDENTIAL_CACHE_TTL: Duration = Duration::from_secs(5);
/// Upstream 0.69.0 #4088: the Codex CLI owns `auth.json` and may be publishing a
/// replacement while we read it. A failed or stale read is repeated up to this many
/// times, `CREDENTIAL_READ_RETRY_DELAY` apart, before the error is reported.
const CREDENTIAL_READ_RETRIES: u32 = 2;
const CREDENTIAL_READ_RETRY_DELAY: Duration = Duration::from_millis(50);
const RESET_CREDITS_CACHE_TTL: Duration = Duration::from_secs(600);
const EXTERNAL_OAUTH_REFRESH_WINDOW: chrono::TimeDelta = chrono::Duration::minutes(5);

static CREDENTIAL_CACHE: OnceLock<Mutex<Option<CachedCodexCredentials>>> = OnceLock::new();
static RESET_CREDITS_CACHE: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<ResetCreditsCache>>>>> =
    OnceLock::new();

#[derive(Default)]
struct ResetCreditsCache {
    loaded_at: Option<Instant>,
    value: Option<ResetCredits>,
    confirmation_failure_at: Option<Instant>,
}

impl ResetCreditsCache {
    fn confirmation_failed_recently(&self) -> bool {
        self.confirmation_failure_at
            .is_some_and(|failed| failed.elapsed() < RESET_CREDITS_CACHE_TTL)
    }
}

/// Codex API client
pub struct CodexApi {
    client: reqwest::Client,
    home_dir: PathBuf,
    /// When set, overrides CODEX_HOME / ~/.codex for auth.json + config.toml (tests).
    codex_home_override: Option<PathBuf>,
}

impl CodexApi {
    pub fn new() -> Self {
        // Build client with proper TLS settings
        let client = crate::core::credentialed_http_client_builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            home_dir: dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            codex_home_override: None,
        }
    }

    /// Point the client at a specific Codex home directory (contains auth.json / config.toml).
    pub fn with_codex_home(mut self, codex_home: impl Into<PathBuf>) -> Self {
        self.codex_home_override = Some(codex_home.into());
        self
    }

    fn codex_dir(&self) -> PathBuf {
        if let Some(override_dir) = &self.codex_home_override {
            return override_dir.clone();
        }
        if let Ok(codex_home) = std::env::var("CODEX_HOME") {
            let trimmed = codex_home.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        self.home_dir.join(".codex")
    }

    pub(super) fn has_pat_credentials(&self) -> bool {
        pat::load_token(&self.get_auth_path()).is_ok()
    }

    pub(super) async fn fetch_usage_pat(
        &self,
        cli_version: Option<&str>,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>, Option<String>), ProviderError> {
        let token = pat::load_token(&self.get_auth_path())?;
        let (json, whoami) =
            pat::fetch_usage(&self.client, &self.resolve_base_url(), &token, cli_version).await?;
        let account_id = whoami.account_id.clone();
        // Email is display metadata, not a stable credential/account binding.
        // A PAT without the provider's account id must fail closed for any
        // operation that could reopen a local session.
        let account_identity = account_id.clone();
        let (mut usage, cost) = self.build_result_from_json(&json)?;
        if let Some(email) = whoami.email {
            usage = usage.with_email(email);
        }
        if usage.login_method.is_none()
            && let Some(plan_type) = whoami.plan_type
        {
            usage = usage.with_login_method(format_plan_type(&plan_type));
        }
        let usage = subscription::enrich_subscription_metadata(
            self,
            &self.resolve_base_url(),
            &token,
            account_id.as_deref(),
            usage,
        )
        .await;
        Ok((usage, cost, account_identity))
    }

    /// Fetch usage information from Codex API.
    ///
    /// v0.55.1 weekly-reset publication is account-scoped and persistent: a
    /// suspicious early drop to <=1% is confirmed before it can replace the
    /// last published weekly window, and reset-credit inventory is evidence
    /// only. The app never redeems or decrements credits on observation.
    pub async fn fetch_usage(
        &self,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>, Option<String>), ProviderError> {
        let (usage, cost, account, _) = self.fetch_usage_with_reset_credits().await?;
        Ok((usage, cost, account))
    }

    pub(super) async fn fetch_usage_with_reset_credits(
        &self,
    ) -> Result<
        (
            UsageSnapshot,
            Option<CostSnapshot>,
            Option<String>,
            Option<ResetCredits>,
        ),
        ProviderError,
    > {
        let creds = self.load_credentials().await?;
        let base_url = self.resolve_base_url();
        let auth_path = self.get_auth_path();
        let scope = weekly_reset::scope_key(creds.account_id.as_deref(), &auth_path);
        let exact_oauth = creds.is_external_oauth;
        let mut state = weekly_reset::load(&scope);

        let first_fetch_started = Instant::now();
        let (first_usage, first_cost, first_credits) =
            self.fetch_usage_once(&creds, &base_url).await?;
        let first_credits = self
            .initial_reset_credits(
                &creds,
                &base_url,
                first_fetch_started,
                first_credits,
                state.has_delayed_candidate(),
            )
            .await;
        let mut displayed_credits = first_credits.clone();
        let observed_at = Utc::now();
        let first_inventory = weekly_reset::inventory(first_credits.as_ref(), observed_at);
        let (usage, cost) = match weekly_reset::initial_decision(
            &mut state,
            &first_usage,
            first_inventory.as_ref(),
            exact_oauth,
            observed_at,
        ) {
            weekly_reset::InitialDecision::Publish => {
                weekly_reset::commit_publication(&mut state, &first_usage, first_inventory);
                weekly_reset::save(&scope, &state);
                (first_usage, first_cost)
            }
            weekly_reset::InitialDecision::Preserve => {
                let usage = weekly_reset::preserve_weekly(&state, first_usage);
                weekly_reset::save(&scope, &state);
                (usage, first_cost)
            }
            weekly_reset::InitialDecision::RequiresConfirmation => {
                // Confirmation must compare independent inventory observations,
                // even when the ordinary 10-minute cache is still fresh.
                let initial_credits = self
                    .fresh_reset_credits_for_confirmation(&creds, &base_url, first_fetch_started)
                    .await;
                let confirmation = self.fetch_usage_once(&creds, &base_url).await;
                let (confirmation_usage, confirmation_cost, _) = match confirmation {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::debug!(
                            %error,
                            "Codex weekly reset confirmation failed; preserving first successful usage"
                        );
                        let usage = weekly_reset::preserve_weekly(&state, first_usage);
                        weekly_reset::save(&scope, &state);
                        let usage = subscription::enrich_subscription_metadata(
                            self,
                            &base_url,
                            &creds.access_token,
                            creds.account_id.as_deref(),
                            usage,
                        )
                        .await;
                        return Ok((
                            usage,
                            first_cost,
                            creds.account_id.clone(),
                            displayed_credits,
                        ));
                    }
                };
                let confirmation_credits = if initial_credits.is_some() {
                    self.fetch_rate_limit_reset_credits_fresh(&creds, &base_url)
                        .await
                } else {
                    None
                };
                displayed_credits = confirmation_credits
                    .clone()
                    .or(initial_credits.clone())
                    .or(displayed_credits);
                let confirmation_inventory =
                    weekly_reset::inventory(confirmation_credits.as_ref(), Utc::now());
                let initial_inventory =
                    weekly_reset::inventory(initial_credits.as_ref(), observed_at);
                match weekly_reset::confirmation_decision(
                    &mut state,
                    &first_usage,
                    initial_inventory.as_ref(),
                    &confirmation_usage,
                    confirmation_inventory.as_ref(),
                    exact_oauth,
                    observed_at,
                ) {
                    weekly_reset::ConfirmationDecision::Publish => {
                        weekly_reset::commit_publication(
                            &mut state,
                            &confirmation_usage,
                            confirmation_inventory,
                        );
                        weekly_reset::save(&scope, &state);
                        (confirmation_usage, confirmation_cost)
                    }
                    weekly_reset::ConfirmationDecision::Preserve => {
                        let usage = weekly_reset::preserve_weekly(&state, first_usage);
                        weekly_reset::save(&scope, &state);
                        (usage, first_cost)
                    }
                }
            }
        };
        let usage = subscription::enrich_subscription_metadata(
            self,
            &base_url,
            &creds.access_token,
            creds.account_id.as_deref(),
            usage,
        )
        .await;
        let usage = apply_reset_credits_window(usage, displayed_credits.as_ref());
        Ok((usage, cost, creds.account_id.clone(), displayed_credits))
    }

    /// Bearer GET shared by the ChatGPT backend endpoints; an empty account id
    /// sends no `ChatGPT-Account-Id` header.
    pub(super) fn authed_get(
        &self,
        url: &str,
        access_token: &str,
        account_id: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let request = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", "CodexBar")
            .header("Accept", "application/json");
        match account_id.filter(|id| !id.is_empty()) {
            Some(account_id) => request.header("ChatGPT-Account-Id", account_id),
            None => request,
        }
    }

    async fn fetch_usage_once(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>, Option<ResetCredits>), ProviderError> {
        let url = format!("{}{}", base_url, USAGE_PATH);
        let response = self
            .authed_get(&url, &creds.access_token, creds.account_id.as_deref())
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(super::authenticated_http_error(response, "Codex API").await);
        }
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        let (mut usage, cost) = self.build_result_from_json(&json)?;
        let reset_credits = self
            .fetch_rate_limit_reset_credits_cached(creds, base_url)
            .await;
        usage = apply_reset_credits_window(usage, reset_credits.as_ref());
        Ok((usage, cost, reset_credits))
    }

    fn reset_credits_cache_slot(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Arc<AsyncMutex<ResetCreditsCache>> {
        // The Codex home is part of the scope: two homes never share an
        // observation, even when they hold the same account and token.
        let auth_path = self.get_auth_path();
        let home = weekly_reset::scope_key(None, &auth_path);
        let account = weekly_reset::scope_key(creds.account_id.as_deref(), &auth_path);
        let token = weekly_reset::scope_key(Some(&creds.access_token), &auth_path);
        let key = format!(
            "{}|{home}|{account}|{token}",
            base_url.trim_end_matches('/')
        );
        let cache = RESET_CREDITS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut cache = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(
            cache
                .entry(key)
                .or_insert_with(|| Arc::new(AsyncMutex::new(ResetCreditsCache::default()))),
        )
    }

    async fn fetch_rate_limit_reset_credits_cached(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Option<ResetCredits> {
        let slot = self.reset_credits_cache_slot(creds, base_url);
        let mut cache = slot.lock().await;
        if cache
            .loaded_at
            .is_some_and(|loaded| loaded.elapsed() < RESET_CREDITS_CACHE_TTL)
        {
            return cache.value.clone();
        }
        cache.value = self
            .fetch_rate_limit_reset_credits(creds, base_url)
            .await
            .ok();
        cache.loaded_at = Some(Instant::now());
        cache.confirmation_failure_at = None;
        cache.value.clone()
    }

    /// Reset credits for the initial weekly-reset decision. A pending delayed
    /// candidate is revalidated against the current inventory, which must be
    /// observed after the candidate was stored: the ten-minute cache can still
    /// hold the very observation that created it, so that case reads fresh.
    async fn initial_reset_credits(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
        started: Instant,
        observed: Option<ResetCredits>,
        candidate_pending: bool,
    ) -> Option<ResetCredits> {
        if !candidate_pending {
            return observed;
        }
        self.fresh_reset_credits_for_confirmation(creds, base_url, started)
            .await
    }

    async fn fresh_reset_credits_for_confirmation(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
        started: Instant,
    ) -> Option<ResetCredits> {
        let slot = self.reset_credits_cache_slot(creds, base_url);
        let mut cache = slot.lock().await;
        cache.value.as_ref()?;
        if cache.confirmation_failed_recently() {
            return None;
        }
        if cache.loaded_at.is_some_and(|loaded| loaded >= started) {
            return cache.value.clone();
        }
        self.refetch_reset_credits_into(&mut cache, creds, base_url)
            .await
    }

    async fn fetch_rate_limit_reset_credits_fresh(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Option<ResetCredits> {
        let slot = self.reset_credits_cache_slot(creds, base_url);
        let mut cache = slot.lock().await;
        if cache.confirmation_failed_recently() {
            return None;
        }
        self.refetch_reset_credits_into(&mut cache, creds, base_url)
            .await
    }

    /// A failed read keeps the cached inventory but blocks further
    /// confirmation reads for one cache TTL.
    async fn refetch_reset_credits_into(
        &self,
        cache: &mut ResetCreditsCache,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Option<ResetCredits> {
        let fresh = self
            .fetch_rate_limit_reset_credits(creds, base_url)
            .await
            .ok();
        if let Some(value) = fresh.as_ref() {
            cache.value = Some(value.clone());
            cache.loaded_at = Some(Instant::now());
            cache.confirmation_failure_at = None;
        } else {
            cache.confirmation_failure_at = Some(Instant::now());
        }
        fresh
    }

    async fn fetch_rate_limit_reset_credits(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Result<ResetCredits, ProviderError> {
        let response = self
            .authed_get(
                &format!("{}{}", base_url, RESET_CREDITS_PATH),
                &creds.access_token,
                creds.account_id.as_deref(),
            )
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(super::authenticated_http_error(response, "Codex reset credits").await);
        }
        decode_reset_credits(&response.bytes().await?)
    }

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
    async fn load_credentials(&self) -> Result<CodexCredentials, ProviderError> {
        Self::reread_during_owner_publication(|| self.load_credentials_once()).await
    }

    /// The bounded reread behind [`Self::load_credentials`]: `read` runs up to
    /// `1 + CREDENTIAL_READ_RETRIES` times, `CREDENTIAL_READ_RETRY_DELAY` apart,
    /// until it succeeds, and the last result is returned unchanged. Dropping the
    /// returned future cancels the pending delay and any further read (upstream
    /// checks task cancellation before each read).
    async fn reread_during_owner_publication<T>(
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

    fn load_credentials_once(&self) -> Result<CodexCredentials, ProviderError> {
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

    fn parse_credentials_json(content: &str) -> Result<CodexCredentials, ProviderError> {
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
    fn enforce_external_oauth_gate(credentials: &CodexCredentials) -> Result<(), ProviderError> {
        if !credentials.is_external_oauth {
            return Ok(());
        }
        // The opt-in only matters without refresh provenance. Skip the settings
        // load otherwise: credential reads repeat while the owner publishes.
        let external_sources_allowed = credentials.last_refresh.is_some()
            || crate::settings::Settings::load().codex_external_oauth_sources_allowed;
        Self::enforce_external_oauth_gate_at(credentials, external_sources_allowed, Utc::now())
    }

    fn enforce_external_oauth_gate_at(
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

    fn get_auth_path(&self) -> PathBuf {
        self.codex_dir().join("auth.json")
    }

    fn resolve_base_url(&self) -> String {
        let config_path = self.codex_dir().join("config.toml");

        if let Ok(content) = std::fs::read_to_string(&config_path)
            && let Some(base_url) = parse_chatgpt_base_url(&content)
        {
            let normalized = normalize_base_url(&base_url);
            // Only allow HTTPS URLs for custom base URLs to prevent token exfiltration
            if normalized.starts_with("https://")
                || normalized.starts_with("http://127.0.0.1")
                || normalized.starts_with("http://localhost")
            {
                return normalized;
            }
            tracing::warn!(
                "Ignoring insecure custom chatgpt_base_url (must be HTTPS): {}",
                normalized
            );
        }

        DEFAULT_BASE_URL.to_string()
    }

    /// Whether config.toml points the CLI at a backend that does not
    /// authenticate against ChatGPT (Bedrock / other custom providers).
    fn uses_custom_backend(&self) -> bool {
        let Ok(content) = std::fs::read_to_string(self.codex_dir().join("config.toml")) else {
            return false;
        };
        parse_chatgpt_base_url(&content).is_some() || config_uses_non_chatgpt_provider(&content)
    }

    fn build_result_from_json(
        &self,
        json: &serde_json::Value,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
        // Extract plan type
        let plan_type = json
            .get("plan_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract rate limit info - handle multiple possible structures
        let (primary, secondary, monthly, code_review, code_review_verified) =
            self.extract_rate_limits(json);

        // Build login method string
        let login_method = plan_type.as_deref().map(format_plan_type);

        let mut usage = UsageSnapshot::new(primary);
        if let Some(sec) = secondary {
            usage = usage.with_secondary(sec);
        }
        // F5 (upstream 0.48.0): monthly (30-day) windows go to tertiary so the
        // bridge and frontend can show a monthly reset instead of swallowing it.
        if let Some(mo) = monthly {
            usage = usage.with_tertiary(mo);
        }
        if let Some(cr) = code_review {
            usage = if code_review_verified {
                usage.with_code_review(cr)
            } else {
                usage.with_model_specific(cr)
            };
        }
        for extra in self.extract_additional_rate_limits(json) {
            usage.extra_rate_windows.push(extra);
        }
        if let Some(method) = login_method {
            usage = usage.with_login_method(method);
        }

        // Extract credits if present
        let cost = self.extract_credits(json);

        Ok((usage, cost))
    }

    #[cfg(test)]
    pub(crate) fn build_result_from_json_for_test(
        &self,
        json: &serde_json::Value,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
        self.build_result_from_json(json)
    }

    fn extract_rate_limits(
        &self,
        json: &serde_json::Value,
    ) -> (
        RateWindow,
        Option<RateWindow>,
        Option<RateWindow>,
        Option<RateWindow>,
        bool,
    ) {
        // Try rate_limit object
        if let Some(rate_limit) = json.get("rate_limit") {
            let primary_opt = rate_limit
                .get("primary_window")
                .and_then(|w| self.parse_window_if_present(w));

            let secondary_opt = rate_limit
                .get("secondary_window")
                .and_then(|w| self.parse_window_if_present(w));

            let code_review = rate_limit
                .get("code_review_window")
                .and_then(|w| self.parse_window_if_present(w));

            let (primary, secondary) = normalize_named_windows(primary_opt, secondary_opt);

            // F5 (upstream 0.48.0): named windows carry only session/weekly/code_review.
            // Monthly is extracted separately (from array windows) — return None here.
            let code_review_verified = code_review.is_some();
            return (primary, secondary, None, code_review, code_review_verified);
        }

        // Try rate_limits array
        if let Some(rate_limits) = json.get("rate_limits").and_then(|v| v.as_array()) {
            let windows = rate_limits
                .iter()
                .filter_map(|window| self.parse_window_if_present(window))
                .collect::<Vec<_>>();
            let (primary, secondary, monthly, code_review) = normalize_array_windows(windows);
            // F5 (upstream 0.48.0): route monthly to its own tertiary lane.
            return (primary, secondary, monthly, code_review, false);
        }

        // Try direct fields
        let used_percent = json
            .get("used_percent")
            .or_else(|| json.get("usage_percent"))
            .and_then(json_f64);
        let primary = RateWindow::new(used_percent.unwrap_or(0.0))
            .with_usage_known(valid_used_percent(used_percent));

        (primary, None, None, None, false)
    }

    fn parse_window(&self, window: &serde_json::Value) -> RateWindow {
        let used_percent = window
            .get("used_percent")
            .or_else(|| window.get("usage_percent"))
            .and_then(json_f64);

        let window_minutes = window
            .get("limit_window_seconds")
            .and_then(json_i64)
            .and_then(|seconds| u32::try_from(seconds / 60).ok());

        let reset_at = window
            .get("reset_at")
            .and_then(json_i64)
            .and_then(|ts| Utc.timestamp_opt(ts, 0).single());

        RateWindow::with_details(
            used_percent.unwrap_or(0.0),
            window_minutes,
            reset_at,
            format_reset_countdown(reset_at),
        )
        .with_usage_known(valid_used_percent(used_percent))
    }

    fn parse_window_if_present(&self, window: &serde_json::Value) -> Option<RateWindow> {
        (!window.is_null() && !is_placeholder_window(window)).then(|| self.parse_window(window))
    }

    fn extract_additional_rate_limits(&self, json: &serde_json::Value) -> Vec<NamedRateWindow> {
        json.get("additional_rate_limits")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|entry| self.parse_additional_rate_limit(entry))
            .collect()
    }

    fn parse_additional_rate_limit(&self, entry: &serde_json::Value) -> Option<NamedRateWindow> {
        let metered_feature = entry
            .get("metered_feature")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let limit_name = entry
            .get("limit_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty());

        let rate_limit = entry.get("rate_limit").unwrap_or(entry);
        let primary = rate_limit.get("primary_window");
        let secondary = rate_limit.get("secondary_window");
        let window = primary.or(secondary)?;
        if is_placeholder_window(window) {
            return None;
        }

        let parsed = self.parse_window(window);
        let feature = metered_feature.unwrap_or_default();
        let limit = limit_name.unwrap_or_default();
        let is_spark = feature.eq_ignore_ascii_case("codex_spark")
            || feature.eq_ignore_ascii_case("spark")
            || limit.to_ascii_lowercase().contains("spark");

        if is_spark {
            let is_weekly = secondary.is_some() && primary.is_none()
                || parsed
                    .window_minutes
                    .is_some_and(|mins| mins >= 7 * 24 * 60);
            let (id, title) = if is_weekly {
                ("codex-spark-weekly", "Codex Spark Weekly")
            } else {
                ("codex-spark", "Codex Spark 5-hour")
            };
            return Some(NamedRateWindow::new(id, title, parsed));
        }

        let label = limit_name.or(metered_feature)?;
        let slug = slugify(label);
        if slug.is_empty() {
            return None;
        }

        Some(NamedRateWindow::new(
            format!("codex-{slug}"),
            titleize_limit_label(label),
            parsed,
        ))
    }

    fn extract_credits(&self, json: &serde_json::Value) -> Option<CostSnapshot> {
        let credits = json.get("credits")?;

        let has_credits = credits
            .get("has_credits")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !has_credits {
            return None;
        }

        let unlimited = credits
            .get("unlimited")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if unlimited {
            return None;
        }

        let balance = credits
            .get("balance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        Some(CostSnapshot::new(balance, "USD", "Credits"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexWindowRole {
    Session,
    Weekly,
    Monthly,
    Unknown,
}

fn codex_window_role(window: &RateWindow) -> CodexWindowRole {
    match window
        .window_minutes
        .map(RateWindowCadence::from_minutes)
        .unwrap_or(RateWindowCadence::Unknown)
    {
        RateWindowCadence::Session => CodexWindowRole::Session,
        RateWindowCadence::Monthly => CodexWindowRole::Monthly,
        RateWindowCadence::Weekly => CodexWindowRole::Weekly,
        RateWindowCadence::Unknown => CodexWindowRole::Unknown,
    }
}

/// Normalize the named `primary_window`/`secondary_window` fields by duration.
fn normalize_named_windows(
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
) -> (RateWindow, Option<RateWindow>) {
    match (primary, secondary) {
        (None, None) => (RateWindow::no_active_session(), None),
        (Some(window), None) | (None, Some(window)) => {
            if codex_window_role(&window) == CodexWindowRole::Weekly {
                (RateWindow::no_active_session(), Some(window))
            } else {
                (window, None)
            }
        }
        (Some(primary), Some(secondary)) => {
            match (codex_window_role(&primary), codex_window_role(&secondary)) {
                (CodexWindowRole::Weekly, CodexWindowRole::Session) => (secondary, Some(primary)),
                (CodexWindowRole::Weekly, CodexWindowRole::Unknown) => {
                    (RateWindow::no_active_session(), Some(primary))
                }
                (CodexWindowRole::Unknown, CodexWindowRole::Session) => (secondary, Some(primary)),
                _ => (primary, Some(secondary)),
            }
        }
    }
}

/// Normalize an array of Codex windows without relying on the API's ordering.
///
/// Returns (session, weekly, monthly, code_review). F5 (upstream 0.48.0):
/// monthly (30-day) windows are routed to their own lane so surfaces can
/// display a monthly reset instead of swallowing it into the weekly label.
fn normalize_array_windows(
    windows: Vec<RateWindow>,
) -> (
    RateWindow,
    Option<RateWindow>,
    Option<RateWindow>,
    Option<RateWindow>,
) {
    if windows.is_empty() {
        return (RateWindow::no_active_session(), None, None, None);
    }

    // Preserve the old positional fallback when the API provides no role
    // metadata at all. There is no safe way to infer session vs weekly then.
    if !windows
        .iter()
        .any(|window| codex_window_role(window) != CodexWindowRole::Unknown)
    {
        let mut windows = windows.into_iter();
        return (
            windows.next().unwrap_or_else(RateWindow::no_active_session),
            windows.next(),
            windows.next(),
            windows.next(),
        );
    }

    let mut session = None;
    let mut weekly = None;
    let mut monthly = None;
    let mut remaining = Vec::new();

    for window in windows {
        match codex_window_role(&window) {
            CodexWindowRole::Session if session.is_none() => session = Some(window),
            CodexWindowRole::Weekly if weekly.is_none() => weekly = Some(window),
            CodexWindowRole::Monthly if monthly.is_none() => monthly = Some(window),
            _ => remaining.push(window),
        }
    }

    (
        session.unwrap_or_else(RateWindow::no_active_session),
        weekly,
        monthly,
        remaining.into_iter().next(),
    )
}

fn format_plan_type(plan_type: &str) -> String {
    match plan_type {
        "guest" => "Guest".to_string(),
        "free" => "ChatGPT Free".to_string(),
        "go" => "Codex Go".to_string(),
        "plus" => "ChatGPT Plus".to_string(),
        "pro" => "ChatGPT Pro".to_string(),
        "pro_lite" | "prolite" | "pro-lite" => "Pro Lite".to_string(),
        "team" => "ChatGPT Team".to_string(),
        "business" => "ChatGPT Business".to_string(),
        "enterprise" => "ChatGPT Enterprise".to_string(),
        "education" | "edu" => "ChatGPT Education".to_string(),
        "free_workspace" | "freeWorkspace" => "Free Workspace".to_string(),
        "quorum" => "Codex Quorum".to_string(),
        "k12" => "Codex K12".to_string(),
        other => format!("ChatGPT {}", capitalize(other)),
    }
}

impl Default for CodexApi {
    fn default() -> Self {
        Self::new()
    }
}

// --- Data structures ---

#[derive(Clone)]
struct CodexCredentials {
    access_token: String,
    account_id: Option<String>,
    /// True when the source is an external OAuth token set (has a
    /// `refresh_token`), as opposed to an `OPENAI_API_KEY`. The Codex CLI owns
    /// refresh and persistence for this source; this app only reads it. The
    /// `codex_external_oauth_sources_allowed` setting gates that read
    /// (upstream 0.50.1 #2944).
    is_external_oauth: bool,
    /// Native access-token JWT expiry. When available, this is authoritative
    /// for validity; the Codex CLI still owns the refresh lifecycle.
    access_token_expires_at: Option<DateTime<Utc>>,
    /// `last_refresh` timestamp from auth.json, when present. Its presence
    /// supplies provenance when the external-source opt-in setting is OFF;
    /// its age is not an access-token expiry signal.
    last_refresh: Option<DateTime<Utc>>,
}

struct CachedCodexCredentials {
    path: PathBuf,
    modified: Option<SystemTime>,
    loaded_at: Instant,
    credentials: CodexCredentials,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ResetCredit {
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default, alias = "resetType")]
    pub(super) reset_type: Option<String>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ResetCredits {
    #[serde(default)]
    pub(super) credits: Vec<ResetCredit>,
    pub(super) available_count: u32,
}

fn decode_reset_credits(data: &[u8]) -> Result<ResetCredits, ProviderError> {
    serde_json::from_slice(data)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Codex reset credits: {e}")))
}

fn parse_credit_expiry(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn is_available_credit(credit: &ResetCredit) -> bool {
    match credit.status.as_deref() {
        None | Some("") => true,
        Some(status) => status.eq_ignore_ascii_case("available"),
    }
}

pub(super) fn next_available_reset_credit_expiry(
    credits: &[ResetCredit],
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    credits
        .iter()
        .filter(|credit| is_available_credit(credit))
        .filter_map(|credit| credit.expires_at.as_deref().and_then(parse_credit_expiry))
        .filter(|expires_at| *expires_at > now)
        .min()
}

fn reset_credits_rate_window(reset: &ResetCredits, now: DateTime<Utc>) -> RateWindow {
    let description = format!(
        "{} reset credit{} available",
        reset.available_count,
        if reset.available_count == 1 { "" } else { "s" }
    );
    let mut window = RateWindow::informational(description);
    window.resets_at = next_available_reset_credit_expiry(&reset.credits, now);
    window
}

fn apply_reset_credits_window(
    mut usage: UsageSnapshot,
    reset: Option<&ResetCredits>,
) -> UsageSnapshot {
    usage
        .extra_rate_windows
        .retain(|window| window.id != "reset-credits");
    if let Some(reset) = reset.filter(|reset| reset.available_count > 0) {
        let window = reset_credits_rate_window(reset, Utc::now());
        usage = usage.with_extra_rate_window("reset-credits", "Reset credits", window);
    }
    usage
}

// --- Helper functions ---

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
fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
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

fn json_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn json_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

fn valid_used_percent(value: Option<f64>) -> bool {
    value.is_some_and(|value| value.is_finite() && (0.0..=100.0).contains(&value))
}

fn is_placeholder_window(window: &serde_json::Value) -> bool {
    let has_usage = window
        .get("used_percent")
        .or_else(|| window.get("usage_percent"))
        .and_then(json_f64)
        .is_some();
    let has_duration = window
        .get("limit_window_seconds")
        .and_then(json_i64)
        .is_some();
    let has_reset = window.get("reset_at").and_then(json_i64).is_some();

    !has_usage && !has_duration && !has_reset
}

fn slugify(label: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;

    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn titleize_limit_label(label: &str) -> String {
    label
        .split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn format_reset_countdown(reset_at: Option<DateTime<Utc>>) -> Option<String> {
    let dt = reset_at?;
    let now = Utc::now();
    if dt <= now {
        return Some("now".to_string());
    }
    let diff = dt - now;
    let total_mins = diff.num_minutes();
    let hours = diff.num_hours();
    let mins = total_mins % 60;
    if hours >= 24 {
        let days = hours / 24;
        let rem_h = hours % 24;
        if rem_h == 0 {
            Some(format!("{}d", days))
        } else {
            Some(format!("{}d {}h", days, rem_h))
        }
    } else if hours > 0 {
        if mins == 0 {
            Some(format!("{}h", hours))
        } else {
            Some(format!("{}h {}m", hours, mins))
        }
    } else {
        Some(format!("{}m", mins))
    }
}

/// Whether config.toml selects a non-ChatGPT model provider (e.g. Bedrock),
/// meaning the CLI never authenticates against ChatGPT.
fn config_uses_non_chatgpt_provider(config_content: &str) -> bool {
    config_content.lines().any(|line| {
        let Some((key, value)) = line.trim().split_once('=') else {
            return false;
        };
        if !key.trim().eq_ignore_ascii_case("model_provider") {
            return false;
        }
        let provider = value.trim().trim_matches('"').trim_matches('\'');
        !provider.is_empty() && !provider.eq_ignore_ascii_case("openai")
    })
}

fn parse_chatgpt_base_url(config_content: &str) -> Option<String> {
    for line in config_content.lines() {
        // Skip comments
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        // Look for chatgpt_base_url = "..."
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if key == "chatgpt_base_url" {
                let mut value = value.trim();
                // Remove quotes
                if (value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\''))
                {
                    value = &value[1..value.len() - 1];
                }
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn normalize_base_url(url: &str) -> String {
    let mut trimmed = url.trim().to_string();
    if trimmed.is_empty() {
        return DEFAULT_BASE_URL.to_string();
    }

    // Remove trailing slashes
    while trimmed.ends_with('/') {
        trimmed.pop();
    }

    // Add /backend-api if needed
    if (trimmed.starts_with("https://chatgpt.com")
        || trimmed.starts_with("https://chat.openai.com"))
        && !trimmed.contains("/backend-api")
    {
        trimmed.push_str("/backend-api");
    }

    trimmed
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod credential_retry_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::test_support::{mock_response, mock_response_expect, mock_status_expect};
    use serde_json::json;

    #[test]
    fn non_chatgpt_model_provider_is_detected_for_guidance() {
        // Upstream 0.50.0 #2679: Bedrock and other custom backends get
        // rate-limit guidance instead of login instructions.
        assert!(config_uses_non_chatgpt_provider(
            "model_provider = \"bedrock\"\n"
        ));
        assert!(config_uses_non_chatgpt_provider(
            "# relay\nmodel_provider = 'ollama'"
        ));
        assert!(!config_uses_non_chatgpt_provider(
            "model_provider = \"openai\""
        ));
        assert!(!config_uses_non_chatgpt_provider(
            "model = \"gpt-5\"\napproval_policy = \"never\""
        ));
    }

    #[test]
    fn parses_codex_credentials_without_retaining_refresh_token() {
        let credentials = CodexApi::parse_credentials_json(
            r#"{
                "tokens": {
                    "access_token": "access",
                    "refresh_token": "refresh",
                    "account_id": "acct_123"
                }
            }"#,
        )
        .expect("credentials");

        assert_eq!(credentials.access_token, "access");
        assert_eq!(credentials.account_id.as_deref(), Some("acct_123"));
    }

    #[test]
    fn decodes_reset_credits() {
        let credits = decode_reset_credits(
            br#"{"available_count":2,"credits":[{"id":"a","status":"available","expires_at":"2026-08-01T12:00:00Z"}]}"#,
        )
        .expect("reset credits");
        assert_eq!(credits.available_count, 2);
        assert_eq!(credits.credits.len(), 1);
        assert_eq!(credits.credits[0].status.as_deref(), Some("available"));
        assert_eq!(
            credits.credits[0].expires_at.as_deref(),
            Some("2026-08-01T12:00:00Z")
        );
    }

    #[test]
    fn missing_reset_credit_count_is_unavailable_not_zero() {
        assert!(decode_reset_credits(br#"{"credits":[]}"#).is_err());
    }

    #[test]
    fn next_expiry_picks_soonest_available() {
        let now = utc("2026-07-01T00:00:00Z");
        let credits = vec![
            credit(Some("available"), "2026-07-10T00:00:00Z"),
            credit(Some("available"), "2026-07-05T00:00:00Z"),
            credit(Some("available"), "2026-07-20T00:00:00Z"),
        ];
        let expiry = next_available_reset_credit_expiry(&credits, now).expect("expiry");
        assert_eq!(expiry, utc("2026-07-05T00:00:00Z"));
    }

    #[test]
    fn next_expiry_skips_past_and_non_available() {
        let now = utc("2026-07-01T00:00:00Z");
        let credits = vec![
            credit(Some("available"), "2026-06-01T00:00:00Z"),
            credit(Some("used"), "2026-07-03T00:00:00Z"),
            credit(Some("AVAILABLE"), "2026-07-08T00:00:00Z"),
            credit(None, "2026-07-09T00:00:00Z"),
        ];
        let expiry = next_available_reset_credit_expiry(&credits, now).expect("expiry");
        assert_eq!(expiry, utc("2026-07-08T00:00:00Z"));
    }

    #[test]
    fn reset_credits_window_sets_informational_and_expiry() {
        let now = utc("2026-07-01T00:00:00Z");
        let reset = ResetCredits {
            available_count: 2,
            credits: vec![
                credit(Some("available"), "2026-07-15T12:00:00Z"),
                credit(Some("available"), "2026-07-10T12:00:00Z"),
            ],
        };
        let window = reset_credits_rate_window(&reset, now);
        assert!(window.is_informational);
        assert_eq!(
            window.reset_description.as_deref(),
            Some("2 reset credits available")
        );
        assert_eq!(window.resets_at, Some(utc("2026-07-10T12:00:00Z")));
    }

    #[test]
    fn reset_credits_window_count_only_without_expiry() {
        let now = utc("2026-07-01T00:00:00Z");
        let reset = ResetCredits {
            available_count: 1,
            credits: vec![],
        };
        let window = reset_credits_rate_window(&reset, now);
        assert!(window.is_informational);
        assert_eq!(
            window.reset_description.as_deref(),
            Some("1 reset credit available")
        );
        assert!(window.resets_at.is_none());
    }

    const PLUS_USAGE: &str = r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":18000}}}"#;

    fn utc(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn credit(status: Option<&str>, expires_at: &str) -> ResetCredit {
        ResetCredit {
            id: None,
            reset_type: None,
            status: status.map(str::to_string),
            expires_at: Some(expires_at.to_string()),
        }
    }

    /// An unsigned JWT whose payload carries only `exp` (seconds since the epoch).
    fn jwt_with_exp(exp: i64) -> String {
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
        format!("header.{payload}.signature")
    }

    fn external_creds(access_token: &str, last_refresh: Option<DateTime<Utc>>) -> CodexCredentials {
        CodexCredentials {
            access_token: access_token.to_string(),
            account_id: None,
            is_external_oauth: true,
            access_token_expires_at: None,
            last_refresh,
        }
    }

    async fn mock_reset_credits(
        server: &mut mockito::ServerGuard,
        available_count: u32,
    ) -> mockito::Mock {
        mock_response_expect(
            server,
            "GET",
            RESET_CREDITS_PATH,
            200,
            format!(r#"{{"available_count":{available_count},"credits":[]}}"#),
            1,
        )
        .await
    }

    fn write_codex_home(base_url: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp codex home");
        std::fs::write(
            dir.path().join("auth.json"),
            r#"{"tokens":{"access_token":"test-token","account_id":"acct_test"}}"#,
        )
        .expect("auth.json");
        std::fs::write(
            dir.path().join("config.toml"),
            format!("chatgpt_base_url = \"{base_url}\""),
        )
        .expect("config.toml");
        dir
    }

    #[tokio::test]
    async fn reset_credit_cache_single_flight_and_unknown_are_ten_minute_observations() {
        let mut server = mockito::Server::new_async().await;
        let request = mock_status_expect(&mut server, "GET", RESET_CREDITS_PATH, 503, 1).await;
        let home = write_codex_home(&server.url());
        let api = CodexApi::new().with_codex_home(home.path());
        let creds = api.load_credentials().await.unwrap();
        let base = server.url();
        let (first, second) = tokio::join!(
            api.fetch_rate_limit_reset_credits_cached(&creds, &base),
            api.fetch_rate_limit_reset_credits_cached(&creds, &base),
        );
        assert!(first.is_none() && second.is_none());
        assert!(
            api.fetch_rate_limit_reset_credits_cached(&creds, &base)
                .await
                .is_none()
        );
        request.assert_async().await;
        assert!(RESET_CREDITS_CACHE_TTL == Duration::from_secs(600));
    }

    #[tokio::test]
    async fn reset_credit_cache_expires_and_token_rotation_uses_new_scope() {
        let mut server = mockito::Server::new_async().await;
        let first = server
            .mock("GET", "/wham/rate-limit-reset-credits")
            .match_header("authorization", "Bearer test-token")
            .expect(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"available_count":2,"credits":[]}"#)
            .create_async()
            .await;
        let home = write_codex_home(&server.url());
        let api = CodexApi::new().with_codex_home(home.path());
        let mut creds = api.load_credentials().await.unwrap();
        let base = server.url();
        assert_eq!(
            api.fetch_rate_limit_reset_credits_cached(&creds, &base)
                .await
                .unwrap()
                .available_count,
            2
        );
        assert_eq!(
            api.fetch_rate_limit_reset_credits_cached(&creds, &base)
                .await
                .unwrap()
                .available_count,
            2
        );
        first.assert_async().await;
        first.remove_async().await;
        let second = server
            .mock("GET", "/wham/rate-limit-reset-credits")
            .match_header("authorization", "Bearer test-token")
            .expect(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"available_count":1,"credits":[]}"#)
            .create_async()
            .await;
        api.reset_credits_cache_slot(&creds, &base)
            .lock()
            .await
            .loaded_at = Some(Instant::now() - RESET_CREDITS_CACHE_TTL);
        assert_eq!(
            api.fetch_rate_limit_reset_credits_cached(&creds, &base)
                .await
                .unwrap()
                .available_count,
            1
        );
        second.assert_async().await;
        second.remove_async().await;
        creds.access_token = "rotated-token".into();
        let rotated = server
            .mock("GET", "/wham/rate-limit-reset-credits")
            .match_header("authorization", "Bearer rotated-token")
            .expect(1)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"available_count":3,"credits":[]}"#)
            .create_async()
            .await;
        assert_eq!(
            api.fetch_rate_limit_reset_credits_cached(&creds, &base)
                .await
                .unwrap()
                .available_count,
            3
        );
        rotated.assert_async().await;
    }

    #[tokio::test]
    async fn reset_credit_cache_is_scoped_to_the_codex_home() {
        // Same base URL, account and token, different Codex homes: each home
        // makes its own observation instead of reading the other's.
        let mut server = mockito::Server::new_async().await;
        let base = server.url();
        let first = mock_reset_credits(&mut server, 2).await;
        let first_home = write_codex_home(&base);
        let first_api = CodexApi::new().with_codex_home(first_home.path());
        let first_creds = first_api.load_credentials().await.unwrap();
        let first_count = first_api
            .fetch_rate_limit_reset_credits_cached(&first_creds, &base)
            .await
            .map(|credits| credits.available_count);
        assert_eq!(first_count, Some(2));
        first.assert_async().await;
        first.remove_async().await;

        let second = mock_status_expect(&mut server, "GET", RESET_CREDITS_PATH, 503, 1).await;
        let second_home = write_codex_home(&base);
        let second_api = CodexApi::new().with_codex_home(second_home.path());
        let second_creds = second_api.load_credentials().await.unwrap();
        assert_eq!(first_creds.access_token, second_creds.access_token);
        assert_eq!(first_creds.account_id, second_creds.account_id);
        assert!(
            second_api
                .fetch_rate_limit_reset_credits_cached(&second_creds, &base)
                .await
                .is_none()
        );
        second.assert_async().await;
    }

    #[tokio::test]
    async fn suspicious_weekly_reset_uses_independent_credit_observations() {
        let mut server = mockito::Server::new_async().await;
        let cached_response = mock_reset_credits(&mut server, 2).await;
        let home = write_codex_home(&server.url());
        let api = CodexApi::new().with_codex_home(home.path());
        let creds = api.load_credentials().await.unwrap();
        let base = server.url();
        api.fetch_rate_limit_reset_credits_cached(&creds, &base)
            .await
            .unwrap();
        cached_response.assert_async().await;
        cached_response.remove_async().await;

        let started = Instant::now();
        let initial_response = mock_reset_credits(&mut server, 1).await;
        let initial = api
            .fresh_reset_credits_for_confirmation(&creds, &base, started)
            .await
            .unwrap();
        assert_eq!(initial.available_count, 1);
        initial_response.assert_async().await;
        initial_response.remove_async().await;

        let confirmation_response = mock_reset_credits(&mut server, 0).await;
        let confirmation = api
            .fetch_rate_limit_reset_credits_fresh(&creds, &base)
            .await
            .unwrap();
        assert_eq!(confirmation.available_count, 0);
        confirmation_response.assert_async().await;
    }

    #[tokio::test]
    async fn pending_delayed_candidate_revalidates_with_a_fresh_credit_observation() {
        let mut server = mockito::Server::new_async().await;
        let candidate_observation = mock_reset_credits(&mut server, 2).await;
        let home = write_codex_home(&server.url());
        let api = CodexApi::new().with_codex_home(home.path());
        let creds = api.load_credentials().await.unwrap();
        let base = server.url();
        let cached = api
            .fetch_rate_limit_reset_credits_cached(&creds, &base)
            .await;
        candidate_observation.assert_async().await;
        candidate_observation.remove_async().await;

        // A later refresh: the ten-minute cache still holds the observation
        // that created the candidate.
        let started = Instant::now();
        let changed = mock_reset_credits(&mut server, 1).await;
        let without_candidate = api
            .initial_reset_credits(&creds, &base, started, cached.clone(), false)
            .await;
        assert_eq!(
            without_candidate.map(|credits| credits.available_count),
            Some(2)
        );
        let with_candidate = api
            .initial_reset_credits(&creds, &base, started, cached, true)
            .await;
        assert_eq!(
            with_candidate.map(|credits| credits.available_count),
            Some(1)
        );
        changed.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_usage_attaches_reset_credits_from_http() {
        let mut server = mockito::Server::new_async().await;
        let soonest = (Utc::now() + chrono::Duration::days(5)).to_rfc3339();
        let later = (Utc::now() + chrono::Duration::days(12)).to_rfc3339();

        let usage_mock = server
            .mock("GET", "/wham/usage")
            .match_header("authorization", "Bearer test-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(PLUS_USAGE)
            .create_async()
            .await;

        let reset_body = format!(
            r#"{{"available_count":2,"credits":[
                {{"status":"available","expires_at":"{later}"}},
                {{"status":"available","expires_at":"{soonest}"}}
            ]}}"#
        );
        let reset_mock = server
            .mock("GET", "/wham/rate-limit-reset-credits")
            .match_header("authorization", "Bearer test-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(reset_body)
            .create_async()
            .await;

        let home = write_codex_home(&server.url());
        let api = CodexApi::new().with_codex_home(home.path());
        let (usage, _, _) = api.fetch_usage().await.expect("fetch_usage");

        usage_mock.assert_async().await;
        reset_mock.assert_async().await;

        let extra = usage
            .extra_rate_windows
            .iter()
            .find(|w| w.id == "reset-credits")
            .expect("reset-credits window attached");
        assert_eq!(extra.title, "Reset credits");
        assert!(extra.window.is_informational);
        assert_eq!(
            extra.window.reset_description.as_deref(),
            Some("2 reset credits available")
        );
        let expected = utc(&soonest);
        assert_eq!(extra.window.resets_at, Some(expected));
    }

    #[tokio::test]
    async fn authenticated_codex_http_distinguishes_401_from_403() {
        for (status, expects_authentication) in [(401, true), (403, false)] {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("GET", "/wham/usage")
                .with_status(status)
                .with_body("fixture refusal")
                .create_async()
                .await;

            let home = write_codex_home(&server.url());
            let api = CodexApi::new().with_codex_home(home.path());
            let error = match api.fetch_usage().await {
                Ok(_) => panic!("expected HTTP {status} to fail"),
                Err(error) => error,
            };

            if expects_authentication {
                assert!(matches!(error, ProviderError::AuthRequired));
            } else {
                let message = error.to_string();
                assert!(message.contains("403"));
                assert!(message.contains("fixture refusal"));
                assert!(!matches!(error, ProviderError::AuthRequired));
            }
            mock.assert_async().await;
        }
    }

    #[tokio::test]
    async fn old_opaque_external_oauth_reaches_usage_request() {
        let mut server = mockito::Server::new_async().await;
        let usage_mock = server
            .mock("GET", "/wham/usage")
            .match_header("authorization", "Bearer opaque-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(PLUS_USAGE)
            .create_async()
            .await;
        let reset_mock = server
            .mock("GET", "/wham/rate-limit-reset-credits")
            .match_header("authorization", "Bearer opaque-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"available_count":0,"credits":[]}"#)
            .create_async()
            .await;

        let creds = CodexApi::parse_credentials_json(
            r#"{
                "tokens": {
                    "access_token": "opaque-token",
                    "refresh_token": "refresh",
                    "account_id": "acct_test"
                },
                "last_refresh": "2026-01-01T00:00:00Z"
            }"#,
        )
        .expect("credentials");
        assert!(CodexApi::enforce_external_oauth_gate_at(&creds, false, Utc::now()).is_ok());

        let api = CodexApi::new();
        let (usage, _, _) = api
            .fetch_usage_once(&creds, &server.url())
            .await
            .expect("opaque OAuth usage request");
        assert_eq!(usage.primary.used_percent, 10.0);
        usage_mock.assert_async().await;
        reset_mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_usage_skips_reset_credits_when_available_count_zero() {
        let mut server = mockito::Server::new_async().await;

        let usage_mock = mock_response(&mut server, "GET", USAGE_PATH, 200, PLUS_USAGE).await;

        let reset_mock = mock_reset_credits(&mut server, 0).await;

        let home = write_codex_home(&server.url());
        let api = CodexApi::new().with_codex_home(home.path());
        let (usage, _, _) = api.fetch_usage().await.expect("fetch_usage");

        usage_mock.assert_async().await;
        reset_mock.assert_async().await;

        assert!(
            usage
                .extra_rate_windows
                .iter()
                .all(|w| w.id != "reset-credits"),
            "available_count=0 must not attach reset-credits"
        );
    }

    #[test]
    fn keeps_weekly_window_in_secondary_when_session_is_absent() {
        let api = CodexApi::new();
        let (usage, _) = api
            .build_result_from_json(&json!({
                "rate_limit": {
                    "secondary_window": {
                        "used_percent": 25,
                        "limit_window_seconds": 604800,
                        "reset_at": 1783036800
                    }
                }
            }))
            .expect("codex usage");

        assert!(usage.primary.is_informational);
        assert_eq!(usage.primary.window_minutes, Some(300));
        assert_eq!(
            usage.primary.reset_description.as_deref(),
            Some("No active 5h session")
        );

        let weekly = usage.secondary.expect("weekly window");
        assert!(!weekly.is_informational);
        assert_eq!(weekly.used_percent, 25.0);
        assert_eq!(weekly.window_minutes, Some(10080));
    }

    #[test]
    fn identifies_rate_limit_array_windows_by_duration() {
        let api = CodexApi::new();
        let (usage, _) = api
            .build_result_from_json(&json!({
                "rate_limits": [
                    {
                        "used_percent": 25,
                        "limit_window_seconds": 604800,
                        "reset_at": 1783036800
                    },
                    {
                        "used_percent": 10,
                        "limit_window_seconds": 18000,
                        "reset_at": 1783018800
                    }
                ]
            }))
            .expect("codex usage");

        assert!(!usage.primary.is_informational);
        assert_eq!(usage.primary.used_percent, 10.0);
        assert_eq!(usage.primary.window_minutes, Some(300));

        let weekly = usage.secondary.expect("weekly window");
        assert_eq!(weekly.used_percent, 25.0);
        assert_eq!(weekly.window_minutes, Some(10080));
    }

    #[test]
    fn identifies_weekly_only_rate_limit_array_without_a_session() {
        let api = CodexApi::new();
        let (usage, _) = api
            .build_result_from_json(&json!({
                "rate_limits": [{
                    "used_percent": 25,
                    "limit_window_seconds": 604800,
                    "reset_at": 1783036800
                }]
            }))
            .expect("codex usage");

        assert!(usage.primary.is_informational);
        assert_eq!(usage.secondary.expect("weekly window").used_percent, 25.0);
    }

    #[test]
    fn maps_codex_spark_additional_rate_limits() {
        let api = CodexApi::new();
        let (usage, _) = api
            .build_result_from_json(&json!({
                "plan_type": "pro",
                "rate_limit": {
                    "primary_window": { "used_percent": 20, "limit_window_seconds": 18000 },
                    "secondary_window": { "used_percent": 40, "limit_window_seconds": 604800 }
                },
                "additional_rate_limits": [
                    {
                        "limit_name": "Codex Spark",
                        "metered_feature": "codex_spark",
                        "rate_limit": {
                            "primary_window": { "used_percent": "17", "limit_window_seconds": 18000 }
                        }
                    },
                    {
                        "limit_name": "Codex Spark Weekly",
                        "metered_feature": "codex_spark",
                        "rate_limit": {
                            "secondary_window": { "used_percent": 62, "limit_window_seconds": 604800 }
                        }
                    }
                ]
            }))
            .expect("codex usage");

        assert_eq!(usage.extra_rate_windows.len(), 2);
        assert_eq!(usage.extra_rate_windows[0].id, "codex-spark");
        assert_eq!(usage.extra_rate_windows[0].title, "Codex Spark 5-hour");
        assert_eq!(usage.extra_rate_windows[0].window.used_percent, 17.0);
        assert_eq!(usage.extra_rate_windows[1].id, "codex-spark-weekly");
        assert_eq!(usage.extra_rate_windows[1].title, "Codex Spark Weekly");
        assert_eq!(usage.extra_rate_windows[1].window.used_percent, 62.0);
    }

    #[test]
    fn ignores_placeholder_additional_rate_limits() {
        let api = CodexApi::new();
        let (usage, _) = api
            .build_result_from_json(&json!({
                "rate_limit": {
                    "primary_window": { "used_percent": 0, "limit_window_seconds": 18000 }
                },
                "additional_rate_limits": [
                    {
                        "limit_name": "placeholder",
                        "metered_feature": "placeholder",
                        "rate_limit": { "primary_window": {} }
                    }
                ]
            }))
            .expect("codex usage");

        assert!(usage.extra_rate_windows.is_empty());
    }

    fn win(minutes: u32, used: f64) -> RateWindow {
        RateWindow::with_details(used, Some(minutes), None, None)
    }

    #[test]
    fn normalize_named_windows_routes_by_role() {
        // Primary is reported as -1.0 for the "No active 5h session" placeholder.
        let s = |used| win(300, used);
        let w = |used| win(10_080, used);
        let u = |used| win(999, used);
        let m = |used| win(43_200, used);
        type Row = (Option<RateWindow>, Option<RateWindow>, f64, Option<f64>);
        let rows: Vec<Row> = vec![
            (None, None, -1.0, None),
            (Some(w(1.0)), None, -1.0, Some(1.0)),
            (Some(s(1.0)), None, 1.0, None),
            (Some(u(1.0)), None, 1.0, None),
            (Some(m(1.0)), None, 1.0, None),
            (None, Some(w(2.0)), -1.0, Some(2.0)),
            (None, Some(s(2.0)), 2.0, None),
            (None, Some(u(2.0)), 2.0, None),
            (Some(w(1.0)), Some(s(2.0)), 2.0, Some(1.0)),
            (Some(w(1.0)), Some(u(2.0)), -1.0, Some(1.0)),
            (Some(u(1.0)), Some(s(2.0)), 2.0, Some(1.0)),
            (Some(s(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
            (Some(u(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
            (Some(s(1.0)), Some(s(2.0)), 1.0, Some(2.0)),
            (Some(w(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
            (Some(u(1.0)), Some(u(2.0)), 1.0, Some(2.0)),
            (Some(m(1.0)), Some(w(2.0)), 1.0, Some(2.0)),
        ];
        for (index, (primary, secondary, want_primary, want_secondary)) in
            rows.into_iter().enumerate()
        {
            let (got_primary, got_secondary) = normalize_named_windows(primary, secondary);
            let got_primary = if got_primary.is_informational {
                -1.0
            } else {
                got_primary.used_percent
            };
            assert_eq!(got_primary, want_primary, "row {index} primary");
            assert_eq!(
                got_secondary.map(|window| window.used_percent),
                want_secondary,
                "row {index} secondary"
            );
        }
    }

    #[tokio::test]
    async fn authed_get_sends_account_header_only_when_non_empty() {
        let mut server = mockito::Server::new_async().await;
        let with_account = server
            .mock("GET", "/with")
            .match_header("authorization", "Bearer tok")
            .match_header("user-agent", "CodexBar")
            .match_header("accept", "application/json")
            .match_header("chatgpt-account-id", "acct-1")
            .with_status(200)
            .create_async()
            .await;
        let without_account = server
            .mock("GET", "/without")
            .match_header("authorization", "Bearer tok")
            .match_header("chatgpt-account-id", mockito::Matcher::Missing)
            .expect(2)
            .with_status(200)
            .create_async()
            .await;
        let api = CodexApi::new();
        for (path, account_id) in [
            ("/with", Some("acct-1")),
            ("/without", Some("")),
            ("/without", None),
        ] {
            let status = api
                .authed_get(&format!("{}{path}", server.url()), "tok", account_id)
                .send()
                .await
                .expect("send")
                .status();
            assert_eq!(status.as_u16(), 200, "{path} {account_id:?}");
        }
        with_account.assert_async().await;
        without_account.assert_async().await;
    }

    #[test]
    fn f5_normalize_array_routes_session_weekly_monthly_to_lanes() {
        // 5h session + weekly + monthly → (session, weekly, monthly, None)
        let windows = vec![win(300, 10.0), win(10_080, 20.0), win(43_200, 30.0)];
        let (primary, secondary, tertiary, code_review) = normalize_array_windows(windows);
        assert_eq!(primary.window_minutes, Some(300));
        assert_eq!(secondary.unwrap().window_minutes, Some(10_080));
        assert_eq!(tertiary.unwrap().window_minutes, Some(43_200));
        assert!(code_review.is_none());
    }

    #[test]
    fn f5_normalize_array_monthly_routes_to_tertiary_not_secondary() {
        // Monthly must go to tertiary, NOT secondary — so #268's weekly math
        // and "Weekly" label stay untouched.
        let windows = vec![win(43_200, 50.0), win(10_080, 20.0)];
        let (primary, secondary, tertiary, _) = normalize_array_windows(windows);
        assert_eq!(primary.window_minutes, Some(300)); // no session → placeholder
        assert_eq!(secondary.unwrap().window_minutes, Some(10_080));
        assert_eq!(tertiary.unwrap().window_minutes, Some(43_200));
    }

    #[test]
    fn f5_normalize_array_empty_returns_placeholder_primary() {
        let (primary, secondary, tertiary, code_review) = normalize_array_windows(vec![]);
        assert!(primary.is_informational);
        assert!(secondary.is_none());
        assert!(tertiary.is_none());
        assert!(code_review.is_none());
    }

    #[test]
    fn f5_normalize_array_unknown_windows_fall_to_code_review() {
        // Windows with unrecognized durations (not 300/10080/43200) go to the
        // remaining/code_review bucket.
        let windows = vec![win(300, 10.0), win(999, 5.0)];
        let (primary, secondary, tertiary, code_review) = normalize_array_windows(windows);
        assert_eq!(primary.window_minutes, Some(300));
        assert!(secondary.is_none());
        assert!(tertiary.is_none());
        assert_eq!(code_review.unwrap().window_minutes, Some(999));
    }

    // ── Upstream 0.50.1 #2944: external OAuth source gate ──────────────────

    #[test]
    fn confirmation_failure_fallback_keeps_first_successful_usage_and_cost() {
        let state = weekly_reset::AccountState::default();
        let first = UsageSnapshot::new(RateWindow::new(10.0)).with_secondary(RateWindow::new(0.5));
        let cost = Some(CostSnapshot::new(3.25, "USD", "Monthly"));
        let (usage, kept_cost) = (weekly_reset::preserve_weekly(&state, first), cost);
        assert!((usage.secondary.expect("weekly").used_percent - 0.5).abs() < f64::EPSILON);
        assert_eq!(kept_cost.expect("cost").used, 3.25);
    }
    #[test]
    fn api_key_credentials_are_not_external_oauth() {
        let creds = CodexApi::parse_credentials_json(r#"{"OPENAI_API_KEY": "sk-test"}"#)
            .expect("credentials");
        assert!(!creds.is_external_oauth);
        assert!(creds.access_token_expires_at.is_none());
        assert!(creds.last_refresh.is_none());
        assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
    }

    #[test]
    fn oauth_tokens_with_refresh_token_are_external_source() {
        let creds = CodexApi::parse_credentials_json(
            r#"{
                "tokens": {
                    "access_token": "access",
                    "refresh_token": "refresh",
                    "account_id": "acct_123"
                }
            }"#,
        )
        .expect("credentials");
        assert!(creds.is_external_oauth);
        assert!(creds.access_token_expires_at.is_none());
        assert!(creds.last_refresh.is_none());
    }

    #[test]
    fn oauth_tokens_without_refresh_token_are_not_external() {
        let creds = CodexApi::parse_credentials_json(
            r#"{
                "tokens": {
                    "access_token": "access",
                    "account_id": "acct_123"
                }
            }"#,
        )
        .expect("credentials");
        assert!(!creds.is_external_oauth);
    }

    #[test]
    fn external_oauth_gate_fails_closed_without_last_refresh() {
        let creds = external_creds("access", None);
        let err = CodexApi::enforce_external_oauth_gate(&creds)
            .expect_err("external OAuth without provenance must fail closed");
        assert!(matches!(err, ProviderError::AuthRequired));
    }

    #[test]
    fn external_oauth_gate_ignores_old_last_refresh_for_opaque_token() {
        let old = Utc::now() - chrono::Duration::days(10);
        let creds = external_creds("access", Some(old));
        assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
    }

    #[test]
    fn external_oauth_gate_allows_refresh_provenance() {
        let fresh = Utc::now() - chrono::Duration::hours(1);
        let creds = external_creds("access", Some(fresh));
        assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
    }

    #[test]
    fn external_oauth_gate_uses_future_jwt_expiry_over_old_last_refresh() {
        let now = Utc::now();
        let future = now + chrono::Duration::hours(2);
        let token = jwt_with_exp(future.timestamp());
        let json = format!(
            r#"{{"tokens":{{"access_token":"{token}","refresh_token":"refresh"}},"last_refresh":"2026-01-01T00:00:00Z"}}"#
        );
        let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
        assert!(creds.access_token_expires_at.is_some());
        assert!(CodexApi::enforce_external_oauth_gate_at(&creds, false, now).is_ok());
        assert!(CodexApi::enforce_external_oauth_gate_at(&creds, true, now).is_ok());
    }

    #[test]
    fn external_oauth_gate_rejects_expired_jwt() {
        let expired = Utc::now() - chrono::Duration::minutes(1);
        let token = jwt_with_exp(expired.timestamp());
        let json = format!(
            r#"{{"tokens":{{"access_token":"{token}","refresh_token":"refresh"}},"last_refresh":"{}"}}"#,
            Utc::now().to_rfc3339()
        );
        let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
        let err = CodexApi::enforce_external_oauth_gate(&creds)
            .expect_err("expired native OAuth must be rejected");
        assert!(matches!(err, ProviderError::AuthRequired));
    }

    #[test]
    fn external_oauth_gate_requires_cli_refresh_when_jwt_is_near_expiry() {
        let soon = Utc::now() + chrono::Duration::minutes(2);
        let token = jwt_with_exp(soon.timestamp());
        let json = format!(
            r#"{{"tokens":{{"access_token":"{token}","refresh_token":"refresh"}},"last_refresh":"{}"}}"#,
            Utc::now().to_rfc3339()
        );
        let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
        let err = CodexApi::enforce_external_oauth_gate(&creds)
            .expect_err("near-expiry native OAuth must refresh through the CLI");
        assert!(matches!(err, ProviderError::AuthRequired));
    }

    #[test]
    fn external_oauth_gate_allows_missing_last_refresh_when_opted_in() {
        let creds = external_creds("opaque-token", None);
        assert!(CodexApi::enforce_external_oauth_gate_at(&creds, true, Utc::now()).is_ok());
    }

    #[test]
    fn opaque_token_uses_refresh_provenance_when_no_jwt_expiry_exists() {
        let fresh = Utc::now().to_rfc3339();
        let json = format!(
            r#"{{"tokens":{{"access_token":"opaque-token","refresh_token":"refresh"}},"last_refresh":"{fresh}"}}"#
        );
        let creds = CodexApi::parse_credentials_json(&json).expect("credentials");
        assert!(creds.access_token_expires_at.is_none());
        assert!(CodexApi::enforce_external_oauth_gate(&creds).is_ok());
    }
    #[test]
    fn parse_timestamp_reads_iso8601() {
        assert!(parse_timestamp("2026-08-17T10:00:00Z").is_some());
        assert!(parse_timestamp("2026-08-17T10:00:00.123Z").is_some());
        assert!(parse_timestamp("  2026-08-17T10:00:00Z  ").is_some());
        assert!(parse_timestamp("").is_none());
        assert!(parse_timestamp("not-a-date").is_none());
    }
}
