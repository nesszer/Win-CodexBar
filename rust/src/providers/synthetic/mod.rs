//! Synthetic provider implementation
//!
//! Synthetic is an AI coding assistant
//! Fetches usage data from Synthetic's local config or API

use async_trait::async_trait;
use std::path::PathBuf;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

/// Synthetic provider
pub struct SyntheticProvider {
    metadata: ProviderMetadata,
}

impl SyntheticProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Synthetic,
                display_name: "Synthetic",
                session_label: "Usage",
                weekly_label: "Monthly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://synthetic.computer/account"),
                status_page_url: None,
            },
        }
    }

    /// Get Synthetic config directory
    fn get_synthetic_config_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            dirs::config_dir().map(|p| p.join("synthetic"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            dirs::home_dir().map(|p| p.join(".synthetic"))
        }
    }

    /// Read Synthetic access token
    async fn read_access_token(&self, ctx: &FetchContext) -> Result<String, ProviderError> {
        if let Some(token) = Self::api_key_from_context(ctx).or_else(Self::api_key_from_env) {
            return Ok(token);
        }

        Self::api_key_from_config_file()
            .await
            .ok_or(ProviderError::AuthRequired)
    }

    fn api_key_from_context(ctx: &FetchContext) -> Option<String> {
        ctx.api_key.as_ref().filter(|key| !key.is_empty()).cloned()
    }

    fn api_key_from_env() -> Option<String> {
        std::env::var("SYNTHETIC_API_KEY")
            .or_else(|_| std::env::var("SYNTHETIC_ACCESS_TOKEN"))
            .ok()
    }

    async fn api_key_from_config_file() -> Option<String> {
        let config_path = Self::get_synthetic_config_path()?;
        if let Some(token) = Self::api_key_from_json_file(
            &config_path.join("config.json"),
            &["apiKey", "accessToken"],
        )
        .await
        {
            return Some(token);
        }

        Self::api_key_from_json_file(&config_path.join("credentials.json"), &["apiKey", "token"])
            .await
    }

    async fn api_key_from_json_file(path: &std::path::Path, keys: &[&str]) -> Option<String> {
        if !path.exists() {
            return None;
        }

        let content = tokio::fs::read_to_string(path).await.ok()?;
        let json = serde_json::from_str::<serde_json::Value>(&content).ok()?;
        keys.iter()
            .find_map(|key| json.get(*key).and_then(|v| v.as_str()))
            .map(ToString::to_string)
    }

    /// Fetch usage via Synthetic API
    async fn fetch_via_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let token = self.read_access_token(ctx).await?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        // Synthetic usage API (hypothetical - adjust based on actual API)
        let resp = client
            .get("https://api.synthetic.computer/v1/usage")
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ProviderError::AuthRequired);
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        self.parse_usage_response(&json)
    }

    fn parse_usage_response(
        &self,
        json: &serde_json::Value,
    ) -> Result<UsageSnapshot, ProviderError> {
        // Try structured 3-slot quota parsing (upstream 0.22 format)
        let data = json.get("data").unwrap_or(json);

        let rolling = data
            .get("rollingFiveHourLimit")
            .or_else(|| data.get("rolling_five_hour_limit"));
        let weekly = data
            .get("weeklyTokenLimit")
            .or_else(|| data.get("weekly_token_limit"));
        let search = data.get("search").and_then(|s| s.get("hourly"));

        if rolling.is_some() || weekly.is_some() {
            // Structured multi-slot format
            let primary = rolling
                .map(|w| self.parse_quota_slot(w, Some(300)))
                .unwrap_or_else(|| RateWindow::new(0.0));
            let secondary = weekly.map(|w| self.parse_quota_slot(w, Some(10080)));
            let tertiary = search.map(|w| self.parse_quota_slot(w, Some(60)));

            let plan = json
                .get("plan")
                .or_else(|| json.get("tier"))
                .and_then(|v| v.as_str())
                .unwrap_or("Synthetic");

            let mut usage = UsageSnapshot::new(primary).with_login_method(plan);
            if let Some(sec) = secondary {
                usage = usage.with_secondary(sec);
            }
            if let Some(ter) = tertiary {
                usage = usage.with_model_specific(ter);
            }
            return Ok(usage);
        }

        // Fallback: flat single-window format
        let used = json
            .get("usage")
            .or_else(|| json.get("used"))
            .or_else(|| json.get("tokensUsed"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let limit = json
            .get("limit")
            .or_else(|| json.get("quota"))
            .or_else(|| json.get("tokensLimit"))
            .and_then(|v| v.as_f64())
            .unwrap_or(1000000.0);

        let used_percent = if limit > 0.0 {
            (used / limit) * 100.0
        } else {
            0.0
        };

        let plan = json
            .get("plan")
            .or_else(|| json.get("tier"))
            .or_else(|| json.get("subscription"))
            .and_then(|v| v.as_str())
            .unwrap_or("Synthetic");

        let reset_time = json
            .get("resetAt")
            .or_else(|| json.get("periodEnd"))
            .or_else(|| json.get("resetsAt"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let primary_window = RateWindow::with_details(used_percent, None, None, reset_time);
        let usage = UsageSnapshot::new(primary_window).with_login_method(plan);

        Ok(usage)
    }

    /// Parse a structured quota slot (5-hour, weekly, or search)
    fn parse_quota_slot(&self, obj: &serde_json::Value, window_minutes: Option<u32>) -> RateWindow {
        let percent_keys = [
            "usagePercent",
            "usedPercent",
            "percentUsed",
            "percent",
            "usage_percent",
        ];
        let reset_keys = [
            "resetInSec",
            "resetInSeconds",
            "resetSeconds",
            "reset_in_sec",
        ];

        let mut percent: Option<f64> = None;
        for key in percent_keys {
            if let Some(val) = obj.get(key).and_then(|v| v.as_f64()) {
                percent = Some(if val <= 1.0 { val * 100.0 } else { val });
                break;
            }
        }

        // Fallback: compute from used/limit
        if percent.is_none() {
            let used = obj
                .get("used")
                .or_else(|| obj.get("tokensUsed"))
                .and_then(|v| v.as_f64());
            let limit = obj
                .get("limit")
                .or_else(|| obj.get("tokensLimit"))
                .and_then(|v| v.as_f64());
            if let (Some(u), Some(l)) = (used, limit)
                && l > 0.0
            {
                percent = Some((u / l) * 100.0);
            }
        }

        let mut reset_sec: Option<i64> = None;
        for key in reset_keys {
            if let Some(val) = obj.get(key).and_then(|v| v.as_i64()) {
                reset_sec = Some(val);
                break;
            }
        }

        let reset_at = reset_sec.and_then(|s| {
            chrono::Utc::now()
                .checked_add_signed(chrono::Duration::seconds(s))
                .map(|t| t.to_rfc3339())
        });

        RateWindow::with_details(
            percent.unwrap_or(0.0).clamp(0.0, 100.0),
            window_minutes,
            None,
            reset_at,
        )
    }

    /// Probe for Synthetic installation
    async fn probe_cli(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        // Check ctx.api_key first
        let has_api_key = ctx.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false);

        let has_env = std::env::var("SYNTHETIC_API_KEY").is_ok()
            || std::env::var("SYNTHETIC_ACCESS_TOKEN").is_ok();

        let has_config = Self::get_synthetic_config_path()
            .map(|p| p.join("config.json").exists() || p.join("credentials.json").exists())
            .unwrap_or(false);

        if has_api_key || has_env || has_config {
            let usage = UsageSnapshot::new(RateWindow::new(0.0))
                .with_login_method("Synthetic (configured)");
            Ok(usage)
        } else {
            Err(ProviderError::NotInstalled(
                "Synthetic not configured. Set SYNTHETIC_API_KEY environment variable or configure Synthetic.".to_string()
            ))
        }
    }
}

impl Default for SyntheticProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for SyntheticProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Synthetic
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Synthetic usage");

        match ctx.source_mode {
            SourceMode::Auto => {
                if let Ok(usage) = self.fetch_via_web(ctx).await {
                    return Ok(ProviderFetchResult::new(usage, "web"));
                }
                let usage = self.probe_cli(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "cli"))
            }
            SourceMode::Web => {
                let usage = self.fetch_via_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli => {
                let usage = self.probe_cli(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "cli"))
            }
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::Cli]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        true
    }
}
