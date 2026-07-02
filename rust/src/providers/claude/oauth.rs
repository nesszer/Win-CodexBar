//! Claude OAuth implementation
//!
//! Loads OAuth credentials from Claude CLI and fetches usage from the API.

use chrono::{DateTime, Utc};
use reqwest::Client;
use reqwest::header::{HeaderValue, RETRY_AFTER};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::core::{NamedRateWindow, ProviderError, ProviderFetchResult, RateWindow, UsageSnapshot};

/// OAuth credentials from Claude CLI
#[derive(Debug, Clone)]
pub struct ClaudeOAuthCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub rate_limit_tier: Option<String>,
}

impl ClaudeOAuthCredentials {
    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            // Consider expired if within 5 minutes of expiry
            expires_at <= Utc::now() + chrono::Duration::minutes(5)
        } else {
            // No expiry info = don't assume expired, try it
            false
        }
    }

    /// Check if the credentials have a specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

/// Raw JSON structure from Claude CLI credentials file
#[derive(Debug, Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OAuthData>,
}

#[derive(Debug, Deserialize)]
struct OAuthData {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<f64>, // milliseconds since epoch
    scopes: Option<Vec<String>>,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

/// OAuth usage response from Claude API
#[derive(Debug, Deserialize)]
pub struct OAuthUsageResponse {
    #[serde(rename = "fiveHour", alias = "five_hour")]
    pub five_hour: Option<UsageWindow>,

    #[serde(rename = "sevenDay", alias = "seven_day")]
    pub seven_day: Option<UsageWindow>,

    #[serde(rename = "sevenDaySonnet", alias = "seven_day_sonnet")]
    pub seven_day_sonnet: Option<UsageWindow>,

    #[serde(rename = "sevenDayOpus", alias = "seven_day_opus")]
    pub seven_day_opus: Option<UsageWindow>,

    #[serde(
        rename = "sevenDayDesign",
        alias = "seven_day_design",
        alias = "seven_day_oauth_apps"
    )]
    pub seven_day_design: Option<UsageWindow>,

    #[serde(
        rename = "sevenDayRoutines",
        alias = "seven_day_routines",
        alias = "seven_day_omelette"
    )]
    pub seven_day_routines: Option<UsageWindow>,

    #[serde(rename = "extraUsage", alias = "extra_usage")]
    pub extra_usage: Option<ExtraUsage>,
}

/// A usage window from the OAuth API
#[derive(Debug, Deserialize)]
pub struct UsageWindow {
    pub utilization: Option<f64>,

    #[serde(rename = "resetsAt", alias = "resets_at")]
    pub resets_at: Option<String>,
}

/// Extra usage (credits) info
#[derive(Debug, Deserialize)]
pub struct ExtraUsage {
    #[serde(rename = "isEnabled", alias = "is_enabled")]
    pub is_enabled: Option<bool>,

    #[serde(rename = "usedCredits", alias = "used_credits")]
    pub used_credits: Option<f64>,

    #[serde(rename = "monthlyLimit", alias = "monthly_limit")]
    pub monthly_limit: Option<f64>,

    pub currency: Option<String>,
}

/// Claude OAuth fetcher
pub struct ClaudeOAuthFetcher {
    client: Client,
}

static RATE_LIMIT_BACKOFF_UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

impl ClaudeOAuthFetcher {
    const USAGE_URL: &'static str = "https://api.anthropic.com/api/oauth/usage";
    const CREDENTIALS_PATH: &'static str = ".claude/.credentials.json";
    const KEYRING_SERVICE: &'static str = "Claude Code-credentials";
    const ENV_TOKEN_KEY: &'static str = "CODEXBAR_CLAUDE_OAUTH_TOKEN";
    const ENV_SCOPES_KEY: &'static str = "CODEXBAR_CLAUDE_OAUTH_SCOPES";
    const DEFAULT_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(5 * 60);

    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Load credentials and fetch usage
    pub async fn fetch(&self) -> Result<ProviderFetchResult, ProviderError> {
        let credentials = self.load_credentials()?;
        self.fetch_with_credentials(credentials).await
    }

    /// Fetch usage with an explicit OAuth access token.
    pub async fn fetch_with_access_token(
        &self,
        access_token: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let access_token = access_token.trim();
        if access_token.is_empty() {
            return Err(ProviderError::OAuth(
                "Claude OAuth access token is empty.".to_string(),
            ));
        }

        let credentials = ClaudeOAuthCredentials {
            access_token: access_token.to_string(),
            refresh_token: None,
            expires_at: None,
            scopes: vec!["user:profile".to_string()],
            rate_limit_tier: None,
        };

        self.fetch_with_credentials(credentials).await
    }

    async fn fetch_with_credentials(
        &self,
        credentials: ClaudeOAuthCredentials,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let usage_response = self.fetch_usage(&credentials).await?;
        let usage = self.build_usage_snapshot(&usage_response, &credentials);
        Ok(ProviderFetchResult::new(usage, "oauth"))
    }

    /// Load OAuth credentials from environment, file, or Claude Code's OS credential store.
    pub fn load_credentials(&self) -> Result<ClaudeOAuthCredentials, ProviderError> {
        // Try environment variables first
        if let Some(creds) = self.load_from_environment() {
            return Ok(creds);
        }

        // Try credentials file
        let file_error = match self.load_from_file() {
            Ok(creds) => return Ok(creds),
            Err(err) => err,
        };

        // Current Claude Code builds store the same JSON payload in the OS credential store.
        if let Some(creds) = self.load_from_keyring()? {
            return Ok(creds);
        }

        Err(file_error)
    }

    /// Load credentials from environment variables
    fn load_from_environment(&self) -> Option<ClaudeOAuthCredentials> {
        let token = std::env::var(Self::ENV_TOKEN_KEY).ok()?;
        let token = token.trim();
        if token.is_empty() {
            return None;
        }

        let scopes: Vec<String> = std::env::var(Self::ENV_SCOPES_KEY)
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| vec!["user:profile".to_string()]);

        Some(ClaudeOAuthCredentials {
            access_token: token.to_string(),
            refresh_token: None,
            expires_at: None, // Environment tokens don't expire
            scopes,
            rate_limit_tier: None,
        })
    }

    /// Load credentials from ~/.claude/.credentials.json
    fn load_from_file(&self) -> Result<ClaudeOAuthCredentials, ProviderError> {
        let path = self.credentials_path()?;

        if !path.exists() {
            return Err(ProviderError::OAuth(
                "Claude OAuth credentials not found. Run `claude` to authenticate.".to_string(),
            ));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ProviderError::OAuth(format!("Failed to read credentials file: {}", e)))?;

        Self::parse_credentials_json(&content)
    }

    /// Load credentials from Claude Code's OS keychain / credential manager entry.
    fn load_from_keyring(&self) -> Result<Option<ClaudeOAuthCredentials>, ProviderError> {
        for account in Self::keyring_account_candidates() {
            let entry = match keyring::Entry::new(Self::KEYRING_SERVICE, &account) {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::debug!(
                        "Failed to open Claude Code credential entry for account {}: {}",
                        account,
                        err
                    );
                    continue;
                }
            };

            let content = match entry.get_password() {
                Ok(content) => content,
                Err(keyring::Error::NoEntry) => continue,
                Err(keyring::Error::Ambiguous(_)) => continue,
                Err(err) => {
                    tracing::debug!(
                        "Failed to read Claude Code credential entry for account {}: {}",
                        account,
                        err
                    );
                    continue;
                }
            };

            if content.trim().is_empty() {
                continue;
            }

            return Self::parse_credentials_json(&content).map(Some);
        }

        #[cfg(target_os = "macos")]
        if let Some(creds) = self.load_from_macos_security_cli()? {
            return Ok(Some(creds));
        }

        Ok(None)
    }

    #[cfg(target_os = "macos")]
    fn load_from_macos_security_cli(
        &self,
    ) -> Result<Option<ClaudeOAuthCredentials>, ProviderError> {
        for account in Self::keyring_account_candidates() {
            let output = match std::process::Command::new("/usr/bin/security")
                .args([
                    "find-generic-password",
                    "-s",
                    Self::KEYRING_SERVICE,
                    "-a",
                    &account,
                    "-w",
                ])
                .output()
            {
                Ok(output) => output,
                Err(err) => {
                    tracing::debug!(
                        "Failed to run macOS security CLI for Claude credentials: {}",
                        err
                    );
                    continue;
                }
            };

            if !output.status.success() {
                continue;
            }

            let content = String::from_utf8_lossy(&output.stdout);
            if content.trim().is_empty() {
                continue;
            }

            return Self::parse_credentials_json(content.trim()).map(Some);
        }

        Ok(None)
    }

    fn parse_credentials_json(content: &str) -> Result<ClaudeOAuthCredentials, ProviderError> {
        if let Ok(file) = serde_json::from_str::<CredentialsFile>(content)
            && let Some(oauth) = file.claude_ai_oauth
        {
            return Self::credentials_from_oauth_data(oauth);
        }

        let oauth: OAuthData = serde_json::from_str(content)
            .map_err(|e| ProviderError::OAuth(format!("Invalid credentials format: {}", e)))?;
        Self::credentials_from_oauth_data(oauth)
    }

    fn credentials_from_oauth_data(
        oauth: OAuthData,
    ) -> Result<ClaudeOAuthCredentials, ProviderError> {
        let access_token = oauth.access_token.ok_or_else(|| {
            ProviderError::OAuth(
                "Claude OAuth access token missing. Run `claude` to authenticate.".to_string(),
            )
        })?;

        let access_token = access_token.trim().to_string();
        if access_token.is_empty() {
            return Err(ProviderError::OAuth(
                "Claude OAuth access token is empty. Run `claude` to authenticate.".to_string(),
            ));
        }

        // Convert milliseconds to DateTime
        let expires_at = oauth.expires_at.map(|millis| {
            let secs = (millis / 1000.0) as i64;
            DateTime::from_timestamp(secs, 0).unwrap_or_else(Utc::now)
        });

        Ok(ClaudeOAuthCredentials {
            access_token,
            refresh_token: oauth.refresh_token,
            expires_at,
            scopes: oauth.scopes.unwrap_or_default(),
            rate_limit_tier: oauth.rate_limit_tier,
        })
    }

    fn keyring_account_candidates() -> Vec<String> {
        let mut candidates = Vec::new();
        for key in ["USER", "USERNAME"] {
            if let Ok(value) = std::env::var(key) {
                Self::push_keyring_candidate(&mut candidates, value);
            }
        }

        #[cfg(not(windows))]
        {
            if let Ok(output) = std::process::Command::new("whoami").output()
                && output.status.success()
            {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Self::push_keyring_candidate(&mut candidates, value.clone());
                if let Some((_, username)) = value.rsplit_once('\\') {
                    Self::push_keyring_candidate(&mut candidates, username.to_string());
                }
                if let Some((_, username)) = value.rsplit_once('/') {
                    Self::push_keyring_candidate(&mut candidates, username.to_string());
                }
            }
        }

        candidates
    }

    fn push_keyring_candidate(candidates: &mut Vec<String>, value: String) {
        let value = value.trim();
        if value.is_empty() || candidates.iter().any(|candidate| candidate == value) {
            return;
        }
        candidates.push(value.to_string());
    }

    /// Get the credentials file path
    fn credentials_path(&self) -> Result<PathBuf, ProviderError> {
        dirs::home_dir()
            .map(|home| home.join(Self::CREDENTIALS_PATH))
            .ok_or_else(|| ProviderError::OAuth("Could not find home directory".to_string()))
    }

    /// Fetch usage data using OAuth credentials
    pub async fn fetch_usage(
        &self,
        credentials: &ClaudeOAuthCredentials,
    ) -> Result<OAuthUsageResponse, ProviderError> {
        if credentials.is_expired() {
            return Err(ProviderError::OAuth(
                "OAuth token expired. Run `claude` to refresh.".to_string(),
            ));
        }

        // Check for required scope
        if !credentials.scopes.is_empty() && !credentials.has_scope("user:profile") {
            return Err(ProviderError::OAuth(format!(
                "OAuth token missing 'user:profile' scope (has: {}). Run `claude setup-token` to regenerate.",
                credentials.scopes.join(", ")
            )));
        }

        if let Some(remaining) = Self::rate_limit_backoff_remaining() {
            return Err(Self::rate_limited_error(remaining));
        }

        let response = self
            .client
            .get(Self::USAGE_URL)
            .header(
                "Authorization",
                format!("Bearer {}", credentials.access_token),
            )
            .header("Accept", "application/json")
            .header("anthropic-beta", "oauth-2025-04-20")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let retry_after = Self::retry_after_duration(response.headers().get(RETRY_AFTER));
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 {
                return Err(ProviderError::OAuth(
                    "OAuth token invalid or expired. Run `claude` to re-authenticate.".to_string(),
                ));
            }

            if status.as_u16() == 403 && body.contains("user:profile") {
                return Err(ProviderError::OAuth(
                    "OAuth token does not meet scope requirement 'user:profile'. Run `claude setup-token` to regenerate.".to_string(),
                ));
            }

            if status.as_u16() == 429 {
                Self::record_rate_limit(retry_after);
                return Err(Self::rate_limited_error(retry_after));
            }

            return Err(ProviderError::OAuth(format!(
                "API error {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            )));
        }

        let usage: OAuthUsageResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse OAuth response: {}", e)))?;

        Self::clear_rate_limit();
        Ok(usage)
    }

    fn rate_limit_gate() -> &'static Mutex<Option<Instant>> {
        RATE_LIMIT_BACKOFF_UNTIL.get_or_init(|| Mutex::new(None))
    }

    fn rate_limit_backoff_remaining() -> Option<Duration> {
        let mut guard = Self::rate_limit_gate().lock().ok()?;
        let until = (*guard)?;
        let now = Instant::now();
        if until <= now {
            *guard = None;
            None
        } else {
            Some(until.saturating_duration_since(now))
        }
    }

    fn record_rate_limit(duration: Duration) {
        if let Ok(mut guard) = Self::rate_limit_gate().lock() {
            *guard = Some(Instant::now() + duration);
        }
    }

    fn clear_rate_limit() {
        if let Ok(mut guard) = Self::rate_limit_gate().lock() {
            *guard = None;
        }
    }

    fn retry_after_duration(value: Option<&HeaderValue>) -> Duration {
        let Some(value) = value.and_then(|value| value.to_str().ok()) else {
            return Self::DEFAULT_RATE_LIMIT_BACKOFF;
        };

        if let Ok(seconds) = value.trim().parse::<u64>() {
            return Duration::from_secs(seconds);
        }

        if let Ok(date) = DateTime::parse_from_rfc2822(value.trim()) {
            let now = Utc::now();
            let date = date.with_timezone(&Utc);
            if date > now {
                return (date - now)
                    .to_std()
                    .unwrap_or(Self::DEFAULT_RATE_LIMIT_BACKOFF);
            }
        }

        Self::DEFAULT_RATE_LIMIT_BACKOFF
    }

    fn rate_limited_error(duration: Duration) -> ProviderError {
        ProviderError::OAuth(format!(
            "Claude OAuth usage endpoint is rate limited. Retrying in about {}s; credentials were preserved.",
            duration.as_secs().max(1)
        ))
    }

    /// Build UsageSnapshot from OAuth response
    fn build_usage_snapshot(
        &self,
        response: &OAuthUsageResponse,
        credentials: &ClaudeOAuthCredentials,
    ) -> UsageSnapshot {
        // Primary: 5-hour session window
        let primary = response
            .five_hour
            .as_ref()
            .and_then(|w| Self::to_rate_window(w, Some(300)))
            .unwrap_or_else(|| RateWindow::new(0.0));

        let mut usage = UsageSnapshot::new(primary);

        // Secondary: 7-day window
        if let Some(weekly) = response
            .seven_day
            .as_ref()
            .and_then(|w| Self::to_rate_window(w, Some(10080)))
        {
            usage = usage.with_secondary(weekly);
        }

        // Model-specific: Opus or Sonnet
        if let Some(opus) = response
            .seven_day_opus
            .as_ref()
            .and_then(|w| Self::to_rate_window(w, Some(10080)))
        {
            usage = usage.with_model_specific(opus);
        } else if let Some(sonnet) = response
            .seven_day_sonnet
            .as_ref()
            .and_then(|w| Self::to_rate_window(w, Some(10080)))
        {
            usage = usage.with_model_specific(sonnet);
        }

        let extra_windows = [(
            "claude-routines",
            "Daily Routines",
            response
                .seven_day_routines
                .as_ref()
                .and_then(|w| Self::to_rate_window(w, Some(10080))),
        )];
        for (id, title, window) in extra_windows {
            if let Some(window) = window {
                usage
                    .extra_rate_windows
                    .push(NamedRateWindow::new(id, title, window));
            }
        }

        // Login method from rate limit tier or default
        if let Some(ref tier) = credentials.rate_limit_tier {
            usage = usage.with_login_method(tier);
        } else {
            usage = usage.with_login_method("Claude (OAuth)");
        }

        usage
    }

    /// Convert OAuth usage window to RateWindow
    fn to_rate_window(window: &UsageWindow, window_minutes: Option<u32>) -> Option<RateWindow> {
        let utilization = normalize_utilization(window.utilization?);

        let resets_at = window
            .resets_at
            .as_ref()
            .and_then(|s| parse_iso8601_date(s));

        let reset_description = resets_at.map(format_reset_date);

        Some(RateWindow::with_details(
            utilization,
            window_minutes,
            resets_at,
            reset_description,
        ))
    }
}

impl Default for ClaudeOAuthFetcher {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_utilization(utilization: f64) -> f64 {
    if utilization > 0.0 && utilization <= 1.0 {
        utilization * 100.0
    } else {
        utilization
    }
}

/// Parse an ISO8601 date string
fn parse_iso8601_date(s: &str) -> Option<DateTime<Utc>> {
    // Try parsing with various formats
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            // Try without timezone
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
}

/// Format a reset date for display
fn format_reset_date(date: DateTime<Utc>) -> String {
    date.format("%b %-d at %-I:%M%p").to_string()
}

#[cfg(test)]
mod tests {
    use super::{ClaudeOAuthCredentials, ClaudeOAuthFetcher, OAuthUsageResponse, UsageWindow};
    use reqwest::header::HeaderValue;
    use std::time::Duration;

    #[test]
    fn converts_fractional_utilization_to_percent() {
        let window = UsageWindow {
            utilization: Some(0.23),
            resets_at: None,
        };

        let rate = ClaudeOAuthFetcher::to_rate_window(&window, Some(300)).expect("rate window");

        assert!((rate.used_percent - 23.0).abs() < f64::EPSILON);
    }

    #[test]
    fn preserves_existing_percentage_utilization() {
        let window = UsageWindow {
            utilization: Some(23.0),
            resets_at: None,
        };

        let rate = ClaudeOAuthFetcher::to_rate_window(&window, Some(300)).expect("rate window");

        assert!((rate.used_percent - 23.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_current_snake_case_oauth_usage_response() {
        let response: OAuthUsageResponse = serde_json::from_str(
            r#"{
                "five_hour": {"utilization": 1.0, "resets_at": "2026-05-22T22:10:00Z"},
                "seven_day": {"utilization": 0.14, "resets_at": "2026-05-29T10:00:00Z"},
                "seven_day_oauth_apps": {"utilization": 0.0},
                "extra_usage": {"is_enabled": true, "used_credits": 0, "monthly_limit": 1000, "currency": "USD"}
            }"#,
        )
        .expect("snake_case OAuth response should parse");

        let credentials = ClaudeOAuthCredentials {
            access_token: "token".to_string(),
            refresh_token: None,
            expires_at: None,
            scopes: vec!["user:profile".to_string()],
            rate_limit_tier: Some("default_claude_ai".to_string()),
        };
        let usage = ClaudeOAuthFetcher::new().build_usage_snapshot(&response, &credentials);

        assert_eq!(usage.primary.used_percent, 100.0);
        assert!((usage.secondary.expect("weekly").used_percent - 14.0).abs() < 0.001);
        assert!(usage.extra_rate_windows.is_empty());
    }

    #[test]
    fn parses_claude_code_credentials_payload() {
        let credentials = ClaudeOAuthFetcher::parse_credentials_json(
            r#"{
                "claudeAiOauth": {
                    "accessToken": "token",
                    "refreshToken": "refresh",
                    "expiresAt": 1770000000000,
                    "scopes": ["user:profile"],
                    "rateLimitTier": "default_claude_ai"
                }
            }"#,
        )
        .expect("Claude Code credential payload should parse");

        assert_eq!(credentials.access_token, "token");
        assert_eq!(credentials.refresh_token.as_deref(), Some("refresh"));
        assert_eq!(credentials.scopes, vec!["user:profile"]);
        assert_eq!(
            credentials.rate_limit_tier.as_deref(),
            Some("default_claude_ai")
        );
        assert!(credentials.expires_at.is_some());
    }

    #[test]
    fn parses_direct_oauth_credentials_payload() {
        let credentials = ClaudeOAuthFetcher::parse_credentials_json(
            r#"{
                "accessToken": "token",
                "scopes": ["user:profile"]
            }"#,
        )
        .expect("direct OAuth payload should parse");

        assert_eq!(credentials.access_token, "token");
        assert_eq!(credentials.scopes, vec!["user:profile"]);
    }

    #[test]
    fn rejects_credentials_payload_without_access_token() {
        let error = ClaudeOAuthFetcher::parse_credentials_json(
            r#"{
                "claudeAiOauth": {
                    "refreshToken": "refresh",
                    "scopes": ["user:profile"]
                }
            }"#,
        )
        .expect_err("access token is required");

        assert!(
            error
                .to_string()
                .contains("Claude OAuth access token missing")
        );
    }

    #[test]
    fn parses_retry_after_seconds() {
        let header = HeaderValue::from_static("17");
        let duration = ClaudeOAuthFetcher::retry_after_duration(Some(&header));

        assert_eq!(duration, Duration::from_secs(17));
    }

    #[test]
    fn invalid_retry_after_uses_default_backoff() {
        let header = HeaderValue::from_static("not-a-date");
        let duration = ClaudeOAuthFetcher::retry_after_duration(Some(&header));

        assert_eq!(duration, ClaudeOAuthFetcher::DEFAULT_RATE_LIMIT_BACKOFF);
    }

    #[test]
    fn rate_limit_gate_blocks_and_clears() {
        ClaudeOAuthFetcher::clear_rate_limit();

        ClaudeOAuthFetcher::record_rate_limit(Duration::from_secs(30));
        assert!(ClaudeOAuthFetcher::rate_limit_backoff_remaining().is_some());

        ClaudeOAuthFetcher::clear_rate_limit();
        assert!(ClaudeOAuthFetcher::rate_limit_backoff_remaining().is_none());
    }

    #[test]
    fn rate_limited_error_preserves_credentials_language() {
        let error = ClaudeOAuthFetcher::rate_limited_error(Duration::from_secs(5));
        let message = error.to_string();

        assert!(message.contains("rate limited"));
        assert!(message.contains("credentials were preserved"));
    }
}
