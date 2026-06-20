//! Codex API client for fetching usage information
//!
//! Uses OAuth tokens stored by the Codex CLI in ~/.codex/auth.json

use crate::core::{CostSnapshot, NamedRateWindow, ProviderError, RateWindow, UsageSnapshot};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const USAGE_PATH: &str = "/wham/usage";
const RESET_CREDITS_PATH: &str = "/wham/rate-limit-reset-credits";
const CREDENTIAL_CACHE_TTL: Duration = Duration::from_secs(5);

static CREDENTIAL_CACHE: OnceLock<Mutex<Option<CachedCodexCredentials>>> = OnceLock::new();

/// Codex API client
pub struct CodexApi {
    client: reqwest::Client,
    home_dir: PathBuf,
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
        }
    }

    /// Fetch usage information from Codex API
    /// Returns (UsageSnapshot, optional CostSnapshot)
    pub async fn fetch_usage(
        &self,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
        // Load credentials
        let creds = self.load_credentials()?;

        // Build request URL
        let base_url = self.resolve_base_url();
        let url = format!("{}{}", base_url, USAGE_PATH);

        // Build request
        let mut request = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("User-Agent", "CodexBar")
            .header("Accept", "application/json")
            .timeout(std::time::Duration::from_secs(30));

        if let Some(account_id) = &creds.account_id
            && !account_id.is_empty()
        {
            request = request.header("ChatGPT-Account-Id", account_id);
        }

        let response = request.send().await?;

        if response.status() == 401 || response.status() == 403 {
            return Err(ProviderError::AuthRequired);
        }

        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Codex API returned {}",
                response.status()
            )));
        }

        // Parse as raw JSON first for flexibility
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let (mut usage, cost) = self.build_result_from_json(&json)?;
        if let Ok(reset_credits) = self.fetch_rate_limit_reset_credits(&creds, &base_url).await
            && reset_credits.available_count > 0
        {
            let mut window = RateWindow::new(0.0);
            window.reset_description = Some(format!(
                "{} reset credit{} available",
                reset_credits.available_count,
                if reset_credits.available_count == 1 {
                    ""
                } else {
                    "s"
                }
            ));
            usage = usage.with_extra_rate_window("reset-credits", "Reset credits", window);
        }
        Ok((usage, cost))
    }

    async fn fetch_rate_limit_reset_credits(
        &self,
        creds: &CodexCredentials,
        base_url: &str,
    ) -> Result<ResetCredits, ProviderError> {
        let mut request = self
            .client
            .get(format!("{}{}", base_url, RESET_CREDITS_PATH))
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("User-Agent", "CodexBar")
            .header("Accept", "application/json");
        if let Some(account_id) = &creds.account_id
            && !account_id.is_empty()
        {
            request = request.header("ChatGPT-Account-Id", account_id);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Codex reset credits returned {}",
                response.status()
            )));
        }
        decode_reset_credits(&response.bytes().await?)
    }

    fn load_credentials(&self) -> Result<CodexCredentials, ProviderError> {
        let auth_path = self.get_auth_path();

        if !auth_path.exists() {
            return Err(ProviderError::NotInstalled(
                "Codex auth.json not found. Run `codex login` in a terminal to sign in."
                    .to_string(),
            ));
        }

        let modified = std::fs::metadata(&auth_path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        if let Some(cached) = Self::cached_credentials(&auth_path, modified) {
            return Ok(cached);
        }

        let content = std::fs::read_to_string(&auth_path).map_err(|e| {
            ProviderError::Other(format!("Failed to read Codex credentials: {}", e))
        })?;

        let credentials = Self::parse_credentials_json(&content)?;
        Self::store_cached_credentials(auth_path, modified, credentials.clone());
        Ok(credentials)
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
                });
            }
        }

        // Otherwise, look for tokens object
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

        Ok(CodexCredentials {
            access_token,
            account_id,
        })
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
        // Check CODEX_HOME env var
        if let Ok(codex_home) = std::env::var("CODEX_HOME") {
            let trimmed = codex_home.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join("auth.json");
            }
        }

        self.home_dir.join(".codex").join("auth.json")
    }

    fn resolve_base_url(&self) -> String {
        // Check CODEX_HOME for config.toml
        let config_path = if let Ok(codex_home) = std::env::var("CODEX_HOME") {
            let trimmed = codex_home.trim();
            if !trimmed.is_empty() {
                PathBuf::from(trimmed).join("config.toml")
            } else {
                self.home_dir.join(".codex").join("config.toml")
            }
        } else {
            self.home_dir.join(".codex").join("config.toml")
        };

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
        let (primary, secondary, code_review) = self.extract_rate_limits(json);

        // Build login method string
        let login_method = plan_type.as_ref().map(|pt| match pt.as_str() {
            "guest" => "Guest".to_string(),
            "free" => "ChatGPT Free".to_string(),
            "go" => "Codex Go".to_string(),
            "plus" => "ChatGPT Plus".to_string(),
            "pro" | "pro_lite" | "prolite" | "pro-lite" => {
                if pt == "pro" {
                    "ChatGPT Pro".to_string()
                } else {
                    "Pro Lite".to_string()
                }
            }
            "team" => "ChatGPT Team".to_string(),
            "business" => "ChatGPT Business".to_string(),
            "enterprise" => "ChatGPT Enterprise".to_string(),
            "education" | "edu" => "ChatGPT Education".to_string(),
            "free_workspace" | "freeWorkspace" => "Free Workspace".to_string(),
            "quorum" => "Codex Quorum".to_string(),
            "k12" => "Codex K12".to_string(),
            other => format!("ChatGPT {}", capitalize(other)),
        });

        let mut usage = UsageSnapshot::new(primary);
        if let Some(sec) = secondary {
            usage = usage.with_secondary(sec);
        }
        if let Some(cr) = code_review {
            usage = usage.with_model_specific(cr);
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

    fn extract_rate_limits(
        &self,
        json: &serde_json::Value,
    ) -> (RateWindow, Option<RateWindow>, Option<RateWindow>) {
        // Try rate_limit object
        if let Some(rate_limit) = json.get("rate_limit") {
            let primary_opt = rate_limit
                .get("primary_window")
                .map(|w| self.parse_window(w));

            let secondary_opt = rate_limit
                .get("secondary_window")
                .map(|w| self.parse_window(w));

            let code_review = rate_limit
                .get("code_review_window")
                .map(|w| self.parse_window(w));

            // If primary is missing, promote secondary to primary (weekly-only plans)
            let (primary, secondary) = match (primary_opt, secondary_opt) {
                (Some(p), s) => (p, s),
                (None, Some(s)) => (s, None),
                (None, None) => (RateWindow::new(0.0), None),
            };

            return (primary, secondary, code_review);
        }

        // Try rate_limits array
        if let Some(rate_limits) = json.get("rate_limits").and_then(|v| v.as_array())
            && let Some(first) = rate_limits.first()
        {
            let primary = self.parse_window(first);
            let secondary = rate_limits.get(1).map(|w| self.parse_window(w));
            let code_review = rate_limits.get(2).map(|w| self.parse_window(w));
            return (primary, secondary, code_review);
        }

        // Try direct fields
        let used_percent = json
            .get("used_percent")
            .or_else(|| json.get("usage_percent"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        (RateWindow::new(used_percent), None, None)
    }

    fn parse_window(&self, window: &serde_json::Value) -> RateWindow {
        let used_percent = window
            .get("used_percent")
            .or_else(|| window.get("usage_percent"))
            .and_then(json_f64)
            .unwrap_or(0.0);

        let window_minutes = window
            .get("limit_window_seconds")
            .and_then(|v| v.as_i64())
            .map(|s| (s / 60) as u32);

        let reset_at = window
            .get("reset_at")
            .and_then(|v| v.as_i64())
            .and_then(|ts| Utc.timestamp_opt(ts, 0).single());

        RateWindow::with_details(
            used_percent,
            window_minutes,
            reset_at,
            format_reset_countdown(reset_at),
        )
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

    fn build_result(
        &self,
        response: UsageResponse,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
        // Extract primary rate window
        let primary = if let Some(ref rate_limit) = response.rate_limit {
            if let Some(ref primary_window) = rate_limit.primary_window {
                let reset_at = timestamp_to_datetime(primary_window.reset_at);
                RateWindow::with_details(
                    primary_window.used_percent as f64,
                    primary_window.limit_window_seconds.map(|s| (s / 60) as u32),
                    reset_at,
                    format_reset_countdown(reset_at),
                )
            } else {
                RateWindow::new(0.0)
            }
        } else {
            RateWindow::new(0.0)
        };

        // Extract secondary rate window
        let secondary = response
            .rate_limit
            .as_ref()
            .and_then(|rl| rl.secondary_window.as_ref())
            .map(|window| {
                let reset_at = timestamp_to_datetime(window.reset_at);
                RateWindow::with_details(
                    window.used_percent as f64,
                    window.limit_window_seconds.map(|s| (s / 60) as u32),
                    reset_at,
                    format_reset_countdown(reset_at),
                )
            });

        // Extract code review rate window
        let code_review = response
            .rate_limit
            .as_ref()
            .and_then(|rl| rl.code_review_window.as_ref())
            .map(|window| {
                let reset_at = timestamp_to_datetime(window.reset_at);
                RateWindow::with_details(
                    window.used_percent as f64,
                    window.limit_window_seconds.map(|s| (s / 60) as u32),
                    reset_at,
                    format_reset_countdown(reset_at),
                )
            });

        // Build usage snapshot
        let login_method = response.plan_type.as_ref().map(|pt| match pt.as_str() {
            "guest" => "Guest".to_string(),
            "free" => "ChatGPT Free".to_string(),
            "go" => "ChatGPT Go".to_string(),
            "plus" => "ChatGPT Plus".to_string(),
            "pro" => "ChatGPT Pro".to_string(),
            "team" => "ChatGPT Team".to_string(),
            "business" => "ChatGPT Business".to_string(),
            "enterprise" => "ChatGPT Enterprise".to_string(),
            "education" | "edu" => "ChatGPT Education".to_string(),
            other => format!("ChatGPT {}", capitalize(other)),
        });

        let mut usage = UsageSnapshot::new(primary);
        if let Some(sec) = secondary {
            usage = usage.with_secondary(sec);
        }
        if let Some(cr) = code_review {
            usage = usage.with_model_specific(cr);
        }
        if let Some(method) = login_method {
            usage = usage.with_login_method(method);
        }

        // Build cost snapshot if credits are present
        let cost = response.credits.as_ref().and_then(|credits| {
            if credits.has_credits() {
                let balance = credits.balance.unwrap_or(0.0);
                if credits.unlimited() {
                    None // Unlimited credits, no need to show
                } else {
                    Some(CostSnapshot::new(balance, "USD", "Credits"))
                }
            } else {
                None
            }
        });

        Ok((usage, cost))
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
}

struct CachedCodexCredentials {
    path: PathBuf,
    modified: Option<SystemTime>,
    loaded_at: Instant,
    credentials: CodexCredentials,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<RateLimitDetails>,
    credits: Option<CreditDetails>,
}

#[derive(Debug, Deserialize)]
struct RateLimitDetails {
    primary_window: Option<WindowSnapshot>,
    secondary_window: Option<WindowSnapshot>,
    code_review_window: Option<WindowSnapshot>,
}

#[derive(Debug, Deserialize)]
struct WindowSnapshot {
    used_percent: i32,
    reset_at: Option<i64>,
    limit_window_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreditDetails {
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    balance: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResetCredits {
    #[serde(default)]
    credits: Vec<serde_json::Value>,
    #[serde(default)]
    available_count: u32,
}

fn decode_reset_credits(data: &[u8]) -> Result<ResetCredits, ProviderError> {
    serde_json::from_slice(data)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Codex reset credits: {e}")))
}

impl CreditDetails {
    // Helper to safely check has_credits
    fn has_credits(&self) -> bool {
        self.has_credits.unwrap_or(false)
    }

    fn unlimited(&self) -> bool {
        self.unlimited.unwrap_or(false)
    }
}

// --- Helper functions ---

fn timestamp_to_datetime(timestamp: Option<i64>) -> Option<DateTime<Utc>> {
    timestamp.and_then(|ts| Utc.timestamp_opt(ts, 0).single())
}

fn json_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn is_placeholder_window(window: &serde_json::Value) -> bool {
    let has_usage = window
        .get("used_percent")
        .or_else(|| window.get("usage_percent"))
        .and_then(json_f64)
        .is_some();
    let has_duration = window
        .get("limit_window_seconds")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse::<i64>().ok()))
        .is_some();
    let has_reset = window.get("reset_at").is_some();

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
mod tests {
    use super::*;
    use serde_json::json;

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
        let credits = decode_reset_credits(br#"{"available_count":2,"credits":[{"id":"a"}]}"#)
            .expect("reset credits");
        assert_eq!(credits.available_count, 2);
        assert_eq!(credits.credits.len(), 1);
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
}
