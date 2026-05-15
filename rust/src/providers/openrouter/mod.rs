//! OpenRouter provider implementation
//!
//! Fetches credit balance and usage data from OpenRouter's REST API
//! Requires API key for authentication

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

/// OpenRouter API base URL
const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1/auth";

/// Windows Credential Manager target for OpenRouter API token
const OPENROUTER_CREDENTIAL_TARGET: &str = "codexbar-openrouter";

/// OpenRouter /credits response
#[derive(Debug, Deserialize)]
struct CreditsResponse {
    data: CreditsData,
}

#[derive(Debug, Deserialize)]
struct CreditsData {
    total_credits: f64,
    total_usage: f64,
}

impl CreditsData {
    fn balance(&self) -> f64 {
        (self.total_credits - self.total_usage).max(0.0)
    }

    fn used_percent(&self) -> f64 {
        if self.total_credits > 0.0 {
            ((self.total_usage / self.total_credits) * 100.0).min(100.0)
        } else {
            0.0
        }
    }
}

/// OpenRouter /key response
#[derive(Debug, Deserialize)]
struct KeyResponse {
    data: KeyData,
}

#[derive(Debug, Deserialize)]
struct KeyData {
    limit: Option<f64>,
    usage: Option<f64>,
    usage_daily: Option<f64>,
    usage_weekly: Option<f64>,
    usage_monthly: Option<f64>,
    rate_limit: Option<RateLimitInfo>,
}

#[derive(Debug, Deserialize)]
struct RateLimitInfo {
    requests: Option<i64>,
    interval: Option<String>,
}

/// OpenRouter provider
pub struct OpenRouterProvider {
    metadata: ProviderMetadata,
}

impl OpenRouterProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::OpenRouter,
                display_name: "OpenRouter",
                session_label: "Credits",
                weekly_label: "Usage",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://openrouter.ai/settings/credits"),
                status_page_url: Some("https://status.openrouter.ai"),
            },
        }
    }

    /// Get API token from ctx, Windows Credential Manager, or env
    fn get_api_token(api_key: Option<&str>) -> Result<String, ProviderError> {
        if let Some(key) = api_key
            && !key.is_empty()
        {
            return Ok(key.to_string());
        }

        match keyring::Entry::new(OPENROUTER_CREDENTIAL_TARGET, "api_token") {
            Ok(entry) => match entry.get_password() {
                Ok(token) => Ok(token),
                Err(_) => std::env::var("OPENROUTER_API_KEY").map_err(|_| {
                    ProviderError::NotInstalled(
                        "OpenRouter API key not found. Set in Preferences → Providers or OPENROUTER_API_KEY environment variable.".to_string(),
                    )
                }),
            },
            Err(_) => std::env::var("OPENROUTER_API_KEY").map_err(|_| {
                ProviderError::NotInstalled(
                    "OpenRouter API key not found. Set in Preferences → Providers or OPENROUTER_API_KEY environment variable.".to_string(),
                )
            }),
        }
    }

    /// Fetch usage from OpenRouter API
    async fn fetch_usage_api(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let api_key = Self::get_api_token(ctx.api_key.as_deref())?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        // Fetch credits (primary endpoint)
        let credits_url = format!("{}/credits", OPENROUTER_API_BASE);
        let resp = client
            .get(&credits_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }

        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "OpenRouter API returned status {}",
                resp.status()
            )));
        }

        let credits: CreditsResponse = resp.json().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse credits response: {}", e))
        })?;

        let balance = credits.data.balance();
        let used_percent = credits.data.used_percent();

        let mut primary = RateWindow::new(used_percent);
        primary.reset_description = Some(format!("${:.2} remaining", balance));

        let mut usage =
            UsageSnapshot::new(primary).with_login_method(format!("${:.2} balance", balance));

        // Try to enrich with /key endpoint data (optional, short timeout)
        let key_url = format!("{}/key", OPENROUTER_API_BASE);
        let key_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        if let Ok(key_resp) = key_client
            .get(&key_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .send()
            .await
            && key_resp.status().is_success()
            && let Ok(key_response) = key_resp.json::<KeyResponse>().await
        {
            let key_data = key_response.data;
            if let (Some(limit), Some(key_usage)) = (key_data.limit, key_data.usage)
                && limit > 0.0
            {
                // If the key has per-key limits, show them as secondary
                let key_percent = ((key_usage / limit) * 100.0).clamp(0.0, 100.0);
                let key_desc = format!("${:.2}/${:.2} key quota", key_usage, limit);
                let mut key_window = RateWindow::new(key_percent);
                key_window.reset_description = Some(key_desc);
                usage = usage.with_secondary(key_window);
            }

            if let Some(daily) = key_data.usage_daily {
                let mut daily_window = RateWindow::new(0.0);
                daily_window.reset_description = Some(format!("${daily:.2} today"));
                usage = usage.with_extra_rate_window("daily-spend", "Daily spend", daily_window);
            }

            if let Some(weekly) = key_data.usage_weekly {
                let mut weekly_window = RateWindow::new(0.0);
                weekly_window.reset_description = Some(format!("${weekly:.2} this week"));
                usage = usage.with_extra_rate_window("weekly-spend", "Weekly spend", weekly_window);
            }

            if let Some(monthly) = key_data.usage_monthly {
                let mut monthly_window = RateWindow::new(0.0);
                monthly_window.reset_description = Some(format!("${monthly:.2} this month"));
                usage =
                    usage.with_extra_rate_window("monthly-spend", "Monthly spend", monthly_window);
            }
        }

        Ok(usage)
    }
}

impl Default for OpenRouterProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching OpenRouter usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let usage = self.fetch_usage_api(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "api"))
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn supports_web(&self) -> bool {
        false
    }

    fn supports_cli(&self) -> bool {
        false
    }
}
