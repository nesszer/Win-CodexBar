//! Claude OAuth implementation
//!
//! Loads OAuth credentials from Claude CLI and fetches usage from the API.

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;

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

impl ClaudeOAuthFetcher {
    const USAGE_URL: &'static str = "https://api.anthropic.com/api/oauth/usage";
    const CREDENTIALS_PATH: &'static str = ".claude/.credentials.json";
    const ENV_TOKEN_KEY: &'static str = "CODEXBAR_CLAUDE_OAUTH_TOKEN";
    const ENV_SCOPES_KEY: &'static str = "CODEXBAR_CLAUDE_OAUTH_SCOPES";

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

    /// Load OAuth credentials from environment or file
    pub fn load_credentials(&self) -> Result<ClaudeOAuthCredentials, ProviderError> {
        // Try environment variables first
        if let Some(creds) = self.load_from_environment() {
            return Ok(creds);
        }

        // Try credentials file
        self.load_from_file()
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

        let file: CredentialsFile = serde_json::from_str(&content)
            .map_err(|e| ProviderError::OAuth(format!("Invalid credentials format: {}", e)))?;

        let oauth = file.claude_ai_oauth.ok_or_else(|| {
            ProviderError::OAuth(
                "Claude OAuth credentials missing. Run `claude` to authenticate.".to_string(),
            )
        })?;

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

        Ok(usage)
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

        let extra_windows = [
            (
                "claude-design",
                "Designs",
                response
                    .seven_day_design
                    .as_ref()
                    .and_then(|w| Self::to_rate_window(w, Some(10080))),
            ),
            (
                "claude-routines",
                "Daily Routines",
                response
                    .seven_day_routines
                    .as_ref()
                    .and_then(|w| Self::to_rate_window(w, Some(10080))),
            ),
        ];
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
        assert_eq!(usage.extra_rate_windows[0].id, "claude-design");
    }
}
