//! Codex API client: identity, OAuth refresh, quota fetch and recovery.
//!
//! Port of `windows/.../codex_api.py` (MIT). Reads a Codex home's `auth.json`,
//! refreshes tokens via the OpenAI OAuth endpoint, fetches `wham/usage` (or a
//! configured custom base URL) and normalizes the quota windows.

use std::path::Path;

use chrono::{DateTime, Utc};
use thiserror::Error;

pub use super::credentials::{
    AuthBackedIdentity, AuthCredentials, jwt_payload, load_credentials, load_identity,
    parse_credentials_json, save_credentials,
};
use super::credentials::{
    account_id_from_id_token, identity_from_credentials, normalize_string, string_value,
};
use super::models::{
    AccountUsageSnapshot, CodexExtraUsageCost, CreditsBalanceSnapshot, UsageWindowSnapshot,
    WindowRole,
};
use crate::core::{ProviderStateKind, credentialed_http_client_builder};
use crate::providers::openai::OpenAISubscriptionFetchResult;

#[path = "subscription.rs"]
mod subscription;

pub const REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
pub const USAGE_DEFAULT_BASE: &str = "https://chatgpt.com/backend-api";
pub const REFRESH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REQUEST_TIMEOUT_SECONDS: u64 = 30;
const UNAUTHORIZED_MESSAGE: &str = "The Codex usage API request returned unauthorized.";

/// Friendly error surfaced to callers.
#[derive(Debug, Error)]
pub enum CodexApiError {
    #[error("{0}")]
    AuthenticationRequired(String),
    #[error("{0}")]
    SessionExpired(String),
    #[error("{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    Message(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("failed to parse Codex payload: {0}")]
    Parse(String),
}

impl CodexApiError {
    /// Classify account API failures before their display text is flattened.
    pub fn state_kind(&self) -> ProviderStateKind {
        match self {
            Self::AuthenticationRequired(_) => ProviderStateKind::NeedsAuthentication,
            Self::SessionExpired(_) => ProviderStateKind::ExpiredSession,
            Self::PermissionDenied(_) | Self::Message(_) | Self::Network(_) | Self::Parse(_) => {
                ProviderStateKind::Unknown
            }
        }
    }
}

// ── Quota fetching ──────────────────────────────────────────────────────────

/// Client for live quota reads. Stateless per call; refresh decisions happen in
/// `CodexAccountApi::fetch_snapshot`.
pub struct CodexAccountApi {
    client: reqwest::Client,
}

impl CodexAccountApi {
    pub fn new() -> Self {
        let client = credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECONDS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }

    /// Fetch a verified (or single) quota snapshot for the account at
    /// `codex_home_path`, refreshing credentials when needed.
    pub async fn fetch_snapshot(
        &self,
        codex_home_path: &Path,
        email_hint: Option<&str>,
        verify_live_data: bool,
    ) -> Result<AccountUsageSnapshot, CodexApiError> {
        self.fetch_snapshot_for_workspace(codex_home_path, email_hint, None, verify_live_data)
            .await
    }

    /// Fetch a snapshot while scoping every usage/credits request to the
    /// app-selected workspace. The selected id is request metadata only: the
    /// auth file remains untouched and may retain a different default.
    pub async fn fetch_snapshot_for_workspace(
        &self,
        codex_home_path: &Path,
        email_hint: Option<&str>,
        workspace_account_id: Option<&str>,
        verify_live_data: bool,
    ) -> Result<AccountUsageSnapshot, CodexApiError> {
        super::fetch_coordination::fetch_snapshot(
            self,
            codex_home_path,
            email_hint,
            workspace_account_id,
            verify_live_data,
        )
        .await
    }

    pub(super) async fn fetch_locked_snapshot(
        &self,
        codex_home_path: &Path,
        email_hint: Option<&str>,
        workspace_account_id: Option<&str>,
        verify_live_data: bool,
    ) -> Result<AccountUsageSnapshot, CodexApiError> {
        let workspace_account_id = workspace_account_id.and_then(|id| normalize_string(Some(id)));
        let mut credentials = load_credentials(codex_home_path)?;

        if credentials.needs_refresh()
            && !credentials.refresh_token.is_empty()
            && let Ok(refreshed) = self.refresh(&credentials).await
        {
            // Best-effort credential persist: a write failure here cannot
            // block the in-memory refresh already in hand.
            let _saved_refreshed = save_credentials(codex_home_path, &refreshed);
            credentials = refreshed;
        }

        let result = self
            .fetch_once(
                codex_home_path,
                &credentials,
                email_hint,
                workspace_account_id.as_deref(),
                verify_live_data,
            )
            .await;
        if !matches!(&result, Err(CodexApiError::SessionExpired(_)))
            || credentials.refresh_token.is_empty()
        {
            return result;
        }

        if let Ok(refreshed) = self.refresh(&credentials).await {
            // Best-effort credential persist before the retry; a write error
            // cannot block the fetch already in progress.
            let _saved_retry = save_credentials(codex_home_path, &refreshed);
            return self
                .fetch_once(
                    codex_home_path,
                    &refreshed,
                    email_hint,
                    workspace_account_id.as_deref(),
                    verify_live_data,
                )
                .await;
        }
        result
    }

    async fn fetch_once(
        &self,
        codex_home_path: &Path,
        credentials: &AuthCredentials,
        email_hint: Option<&str>,
        workspace_account_id: Option<&str>,
        verify_live_data: bool,
    ) -> Result<AccountUsageSnapshot, CodexApiError> {
        let snapshot = if verify_live_data {
            self.fetch_verified(
                codex_home_path,
                credentials,
                email_hint,
                workspace_account_id,
            )
            .await?
        } else {
            self.fetch_single(
                codex_home_path,
                credentials,
                email_hint,
                workspace_account_id,
            )
            .await?
        };
        Ok(self
            .enrich_subscription_metadata(
                codex_home_path,
                credentials,
                email_hint,
                workspace_account_id,
                snapshot,
            )
            .await)
    }

    /// Fetch subscription dates only after the selected account's quota data
    /// has been obtained. The request is scoped with the same workspace account
    /// header, and the optional result never turns a successful quota read into
    /// an error.
    async fn enrich_subscription_metadata(
        &self,
        codex_home_path: &Path,
        credentials: &AuthCredentials,
        email_hint: Option<&str>,
        workspace_account_id: Option<&str>,
        snapshot: AccountUsageSnapshot,
    ) -> AccountUsageSnapshot {
        subscription::enrich_subscription_metadata(
            self,
            codex_home_path,
            credentials,
            email_hint,
            workspace_account_id,
            snapshot,
        )
        .await
    }

    async fn fetch_subscription_metadata(
        &self,
        codex_home_path: &Path,
        credentials: &AuthCredentials,
        account_id: Option<&str>,
    ) -> OpenAISubscriptionFetchResult {
        subscription::fetch_subscription_metadata(self, codex_home_path, credentials, account_id)
            .await
    }

    /// Fetch three reads and require equivalence (CodexControl accuracy model).
    async fn fetch_verified(
        &self,
        codex_home_path: &Path,
        credentials: &AuthCredentials,
        email_hint: Option<&str>,
        workspace_account_id: Option<&str>,
    ) -> Result<AccountUsageSnapshot, CodexApiError> {
        let first = self
            .fetch_single(
                codex_home_path,
                credentials,
                email_hint,
                workspace_account_id,
            )
            .await?;
        let second = self
            .fetch_single(
                codex_home_path,
                credentials,
                email_hint,
                workspace_account_id,
            )
            .await?;
        if is_equivalent(&first, &second) {
            return Ok(second);
        }
        let third = self
            .fetch_single(
                codex_home_path,
                credentials,
                email_hint,
                workspace_account_id,
            )
            .await?;
        if is_equivalent(&first, &third) || is_equivalent(&second, &third) {
            return Ok(third);
        }
        Err(CodexApiError::Message(
            "Live API responses were inconsistent. The data could not be verified.".to_string(),
        ))
    }

    async fn fetch_single(
        &self,
        codex_home_path: &Path,
        credentials: &AuthCredentials,
        fallback_email: Option<&str>,
        workspace_account_id: Option<&str>,
    ) -> Result<AccountUsageSnapshot, CodexApiError> {
        let identity = identity_from_credentials(credentials);
        let remote_account_id = workspace_account_id
            .and_then(|id| normalize_string(Some(id)))
            .or_else(|| identity.provider_account_id.clone())
            .or_else(|| credentials.account_id.clone());
        let response = self
            .fetch_usage(
                codex_home_path,
                &credentials.access_token,
                remote_account_id.as_deref(),
            )
            .await?;
        let rate_limit = response.get("rate_limit").and_then(|v| v.as_object());
        let (primary_window, secondary_window) = make_normalized_windows(rate_limit);
        let credits = response
            .get("credits")
            .and_then(|v| v.as_object())
            .map(make_credits);

        let cost_account_id = remote_account_id.clone();
        Ok(AccountUsageSnapshot {
            email: identity.email.or_else(|| normalize_string(fallback_email)),
            provider_account_id: remote_account_id,
            plan: normalize_string(response.get("plan_type").and_then(|v| v.as_str()))
                .or(identity.plan),
            allowed: rate_limit
                .and_then(|r| r.get("allowed"))
                .and_then(|v| v.as_bool()),
            limit_reached: rate_limit
                .and_then(|r| r.get("limit_reached"))
                .and_then(|v| v.as_bool()),
            primary_window,
            secondary_window,
            credits: credits.clone(),
            cost: CodexExtraUsageCost::from_credits(
                credits.as_ref(),
                Utc::now(),
                cost_account_id.as_deref(),
                None,
            ),
            updated_at: Utc::now(),
            subscription: None,
        })
    }

    async fn fetch_usage(
        &self,
        codex_home_path: &Path,
        access_token: &str,
        account_id: Option<&str>,
    ) -> Result<serde_json::Value, CodexApiError> {
        let url = resolve_usage_url(codex_home_path);
        let mut request = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", "codex-cli")
            .header("Accept", "application/json")
            .header("Cache-Control", "no-cache, no-store, max-age=0")
            .header("Pragma", "no-cache");
        if let Some(account_id) = account_id {
            request = request.header("ChatGPT-Account-Id", account_id);
        }

        let response = request
            .send()
            .await
            .map_err(|e| CodexApiError::Network(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(CodexApiError::SessionExpired(
                    UNAUTHORIZED_MESSAGE.to_string(),
                ));
            }
            if status == reqwest::StatusCode::FORBIDDEN {
                return Err(CodexApiError::PermissionDenied(
                    "The Codex usage API request was forbidden.".to_string(),
                ));
            }
            let body = response.text().await.unwrap_or_default().trim().to_string();
            let msg = if body.is_empty() {
                format!("Codex API error {status}.")
            } else {
                format!("Codex API error {status}: {body}")
            };
            return Err(CodexApiError::Message(msg));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CodexApiError::Parse(e.to_string()))?;
        if !json.is_object() {
            return Err(CodexApiError::Parse(
                "The Codex API response was not in the expected format.".to_string(),
            ));
        }
        Ok(json)
    }

    /// Refresh an expired access token via the OpenAI OAuth endpoint.
    pub async fn refresh(
        &self,
        credentials: &AuthCredentials,
    ) -> Result<AuthCredentials, CodexApiError> {
        if credentials.refresh_token.is_empty() {
            return Err(CodexApiError::Message(
                "No refresh token available for this account.".to_string(),
            ));
        }
        let body = serde_json::json!({
            "client_id": REFRESH_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": credentials.refresh_token,
            "scope": "openid profile email",
        });
        let response = self
            .client
            .post(REFRESH_ENDPOINT)
            .json(&body)
            .header("Content-Type", "application/json")
            .header("Cache-Control", "no-cache, no-store, max-age=0")
            .header("Pragma", "no-cache")
            .send()
            .await
            .map_err(|e| CodexApiError::Network(e.to_string()))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            let text = response.text().await.unwrap_or_default();
            let code = extract_error_code(&text).to_lowercase();
            let message = if code == "refresh_token_reused" {
                "The refresh token can no longer be reused. Sign in again for this account."
            } else if code == "refresh_token_invalidated" {
                "The refresh token was revoked. Sign in again for this account."
            } else {
                "The refresh token has expired. Sign in again for this account."
            };
            return Err(CodexApiError::Message(message.to_string()));
        }
        if !response.status().is_success() {
            return Err(CodexApiError::Message(
                "The Codex API response was not in the expected format.".to_string(),
            ));
        }
        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CodexApiError::Parse(e.to_string()))?;
        if !payload.is_object() {
            return Err(CodexApiError::Message(
                "The Codex API response was not in the expected format.".to_string(),
            ));
        }
        let new_id_token = string_value(&payload, "id_token");
        Ok(AuthCredentials {
            access_token: string_value(&payload, "access_token")
                .unwrap_or_else(|| credentials.access_token.clone()),
            refresh_token: string_value(&payload, "refresh_token")
                .unwrap_or_else(|| credentials.refresh_token.clone()),
            id_token: new_id_token
                .clone()
                .or_else(|| credentials.id_token.clone()),
            account_id: credentials
                .account_id
                .clone()
                .or_else(|| account_id_from_id_token(new_id_token.as_deref())),
            last_refresh: Some(Utc::now()),
        })
    }
}

impl Default for CodexAccountApi {
    fn default() -> Self {
        Self::new()
    }
}

// ── URL resolution ──────────────────────────────────────────────────────────

/// Resolve the usage URL from `config.toml` (`chatgpt_base_url`) or the default.
pub fn resolve_usage_url(codex_home_path: &Path) -> String {
    let config_path = codex_home_path.join("config.toml");
    let configured_base = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|raw| parse_chatgpt_base_url(&raw))
    } else {
        None
    };

    let mut base = configured_base.unwrap_or_else(|| USAGE_DEFAULT_BASE.to_string());
    while base.ends_with('/') {
        base.pop();
    }
    if base.starts_with("https://chatgpt.com") && !base.contains("/backend-api") {
        base.push_str("/backend-api");
    }
    if base.starts_with("https://chat.openai.com") && !base.contains("/backend-api") {
        base.push_str("/backend-api");
    }
    let path = if base.contains("/backend-api") {
        "/wham/usage"
    } else {
        "/api/codex/usage"
    };
    format!("{base}{path}")
}

/// Resolve the subscription endpoint only for the real OpenAI dashboard host.
/// Custom Codex backends may reuse the usage URL shape but must never receive
/// a ChatGPT subscription probe or be treated as its authority.
pub fn resolve_subscription_url(codex_home_path: &Path) -> Option<String> {
    subscription::resolve_subscription_url(codex_home_path)
}

/// Extract `chatgpt_base_url` from a Codex `config.toml`.
pub fn parse_chatgpt_base_url(contents: &str) -> Option<String> {
    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '=');
        let key = parts.next()?.trim();
        let value = parts.next()?.trim();
        if key != "chatgpt_base_url" {
            continue;
        }
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        return Some(value.to_string());
    }
    None
}

// ── Window normalization (session/weekly) ───────────────────────────────────

fn make_window(window: &serde_json::Map<String, serde_json::Value>) -> Option<UsageWindowSnapshot> {
    let used_percent = window
        .get("used_percent")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let reset_at = window
        .get("reset_at")
        .and_then(|v| v.as_i64())
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));
    let limit_window_seconds = window
        .get("limit_window_seconds")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    Some(UsageWindowSnapshot::new(
        used_percent,
        reset_at,
        limit_window_seconds,
    ))
}

/// Normalize the `rate_limit` object into (primary, secondary) windows with
/// roles assigned and `limit_reached` forced to 100%.
pub fn make_normalized_windows(
    rate_limit: Option<&serde_json::Map<String, serde_json::Value>>,
) -> (Option<UsageWindowSnapshot>, Option<UsageWindowSnapshot>) {
    let Some(rate_limit) = rate_limit else {
        return (None, None);
    };
    let mut primary = rate_limit
        .get("primary_window")
        .and_then(|v| v.as_object())
        .and_then(make_window);
    let mut secondary = rate_limit
        .get("secondary_window")
        .and_then(|v| v.as_object())
        .and_then(make_window);

    if rate_limit.get("limit_reached") == Some(&serde_json::Value::Bool(true)) {
        if let Some(p) = primary.as_mut() {
            p.used_percent = 100.0;
        }
        if let Some(s) = secondary.as_mut() {
            s.used_percent = 100.0;
        }
    }

    normalize_window_roles(primary, secondary)
}

/// Put the session window first and the weekly window second.
pub fn normalize_window_roles(
    primary: Option<UsageWindowSnapshot>,
    secondary: Option<UsageWindowSnapshot>,
) -> (Option<UsageWindowSnapshot>, Option<UsageWindowSnapshot>) {
    if let (Some(p), Some(s)) = (&primary, &secondary) {
        let (pr, sr) = (p.role(), s.role());
        if matches!(
            (pr, sr),
            (WindowRole::Weekly, WindowRole::Session) | (WindowRole::Weekly, WindowRole::Unknown)
        ) {
            return (secondary, primary);
        }
        return (primary, secondary);
    }
    if let Some(p) = &primary {
        if p.role() == WindowRole::Weekly {
            return (None, primary);
        }
        return (primary, None);
    }
    if let Some(s) = &secondary {
        if s.role() == WindowRole::Weekly {
            return (None, secondary);
        }
        return (secondary, None);
    }
    (None, None)
}

fn make_credits(credits: &serde_json::Map<String, serde_json::Value>) -> CreditsBalanceSnapshot {
    CreditsBalanceSnapshot {
        has_credits: credits
            .get("has_credits")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        unlimited: credits
            .get("unlimited")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        balance: credits.get("balance").and_then(|v| v.as_f64()),
    }
}

fn extract_error_code(payload: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(payload) else {
        return String::new();
    };
    let error = parsed.get("error");
    if let Some(error) = error.and_then(|e| e.as_object()) {
        return error
            .get("code")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
    }
    if let Some(error) = error.and_then(|e| e.as_str()) {
        return error.to_string();
    }
    parsed
        .get("code")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Whether two fetched snapshots are equivalent (CodexControl verification).
pub fn is_equivalent(left: &AccountUsageSnapshot, right: &AccountUsageSnapshot) -> bool {
    let email_eq = left.email.as_deref().map(str::to_lowercase)
        == right.email.as_deref().map(str::to_lowercase);
    email_eq
        && left.provider_account_id == right.provider_account_id
        && left.plan == right.plan
        && left.allowed == right.allowed
        && left.limit_reached == right.limit_reached
        && windows_equivalent(&left.primary_window, &right.primary_window)
        && windows_equivalent(&left.secondary_window, &right.secondary_window)
        && credits_equivalent(&left.credits, &right.credits)
}

fn windows_equivalent(
    left: &Option<UsageWindowSnapshot>,
    right: &Option<UsageWindowSnapshot>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (None, Some(_)) | (Some(_), None) => false,
        (Some(l), Some(r)) => {
            let reset_matches = match (l.reset_at, r.reset_at) {
                (None, None) => true,
                (Some(a), Some(b)) => (a - b).num_seconds().abs() <= 1,
                _ => false,
            };
            l.limit_window_seconds == r.limit_window_seconds
                && reset_matches
                && (l.used_percent - r.used_percent).abs() < 0.001
        }
    }
}

fn credits_equivalent(
    left: &Option<CreditsBalanceSnapshot>,
    right: &Option<CreditsBalanceSnapshot>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (None, Some(_)) | (Some(_), None) => false,
        (Some(l), Some(r)) => {
            let balance_matches = match (l.balance, r.balance) {
                (None, None) => true,
                (Some(a), Some(b)) => (a - b).abs() < 0.001,
                _ => false,
            };
            l.has_credits == r.has_credits && l.unlimited == r.unlimited && balance_matches
        }
    }
}

#[cfg(test)]
mod tests;
