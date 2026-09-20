//! OpenRouter provider implementation
//!
//! Fetches credit balance and usage data from OpenRouter's REST API
//! Requires API key for authentication

mod activity;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

/// OpenRouter API base URL — the bare `/api/v1` prefix, matching upstream
/// (steipete/CodexBar `OpenRouterSettingsReader.apiURL`).
///
/// Both endpoints append their path to this base: `/credits` and `/key`.
/// The fork's original bug baked `/auth` into the base (`.../api/v1/auth`),
/// which turned the credits call into `/api/v1/auth/credits` -> 404.
const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
const OPENROUTER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Optional key-quota enrichment joins on a one-second fast deadline
/// (upstream 0.49.0 #2778) so a slow `/key` endpoint can never stall the
/// refresh; degraded enrichment is logged and skipped, never fatal.
const OPENROUTER_KEY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);
const OPENROUTER_ACTIVITY_URL: &str = "https://openrouter.ai/api/v1/activity";
const OPENROUTER_MANAGEMENT_ENV: &str = "OPENROUTER_MANAGEMENT_API_KEY";

/// Windows Credential Manager target for OpenRouter API token
const OPENROUTER_CREDENTIAL_TARGET: &str = "codexbar-openrouter";

/// OpenRouter /credits response
#[derive(Debug, Clone, Deserialize)]
struct CreditsResponse {
    data: CreditsData,
}

#[derive(Debug, Clone, Deserialize)]
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

    fn validate(&self) -> Result<(), ProviderError> {
        for (field, value) in [
            ("total_credits", self.total_credits),
            ("total_usage", self.total_usage),
        ] {
            if !value.is_finite() {
                return Err(ProviderError::Parse(format!(
                    "OpenRouter credits.{field} must be a finite number"
                )));
            }
        }
        Ok(())
    }
}

/// OpenRouter /key response
#[derive(Debug, Clone, Deserialize)]
struct KeyResponse {
    data: KeyData,
}

#[derive(Debug, Clone, Deserialize)]
struct KeyData {
    limit: Option<f64>,
    /// Server-reported current-period remaining for the key limit
    /// (upstream 0.48.0 F14: `limit_remaining`).
    limit_remaining: Option<f64>,
    /// Declared reset window for the key limit, e.g. `"monthly"`
    /// (`limit_reset`); picks which period usage field is the quota fallback.
    limit_reset: Option<String>,
    usage: Option<f64>,
    usage_daily: Option<f64>,
    usage_weekly: Option<f64>,
    usage_monthly: Option<f64>,
    is_management_key: Option<bool>,
}

impl KeyData {
    fn validate(&self) -> Result<(), ProviderError> {
        for (field, value) in [
            ("limit", self.limit),
            ("limit_remaining", self.limit_remaining),
            ("usage", self.usage),
            ("usage_daily", self.usage_daily),
            ("usage_weekly", self.usage_weekly),
            ("usage_monthly", self.usage_monthly),
        ] {
            if value.is_some_and(|value| !value.is_finite()) {
                return Err(ProviderError::Parse(format!(
                    "OpenRouter key.{field} must be a finite number"
                )));
            }
        }
        Ok(())
    }
}

/// OpenRouter provider
pub struct OpenRouterProvider {
    metadata: ProviderMetadata,
}

/// Usage value for quota math when the server does not report remaining: the
/// field matching the declared reset window when known, otherwise cumulative
/// usage (upstream `OpenRouterUsageSnapshot.quotaFallbackUsage`).
fn quota_fallback_usage(key_data: &KeyData) -> Option<f64> {
    let reset_usage = match key_data
        .limit_reset
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("daily") => key_data.usage_daily,
        Some("weekly") => key_data.usage_weekly,
        Some("monthly") => key_data.usage_monthly,
        _ => None,
    };
    reset_usage.or(key_data.usage)
}

impl OpenRouterProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::OpenRouter,
                display_name: "OpenRouter",
                session_label: "Credits",
                weekly_label: "API key limit",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://openrouter.ai/activity"),
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

    fn configured_management_key() -> Option<String> {
        crate::settings::Settings::load()
            .management_api_token(ProviderId::OpenRouter)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                std::env::var(OPENROUTER_MANAGEMENT_ENV)
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
    }

    /// Fetch usage from OpenRouter API. Management Activity spend is optional
    /// enrichment: a missing/denied management key never discards credits/quota.
    async fn fetch_usage_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
        let api_key = Self::get_api_token(ctx.api_key.as_deref())?;
        let client = Self::build_client(OPENROUTER_TIMEOUT)?;
        // OpenRouter can reject the account-level `/credits` request while
        // still returning the selected key's current spend/quota from `/key`.
        // Fetch both independent sources together, then let the pure resolver
        // choose the stable primary/secondary lanes.
        let (credits_result, key_data_result) = tokio::join!(
            Self::fetch_credits(&client, &api_key),
            Self::fetch_key_data(&api_key),
        );
        if let Err(error) = &credits_result {
            tracing::debug!(error = %error, "OpenRouter credits endpoint degraded");
        }
        let key_data = match key_data_result {
            Ok(key_data) => Some(key_data),
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "OpenRouter key endpoint degraded; preserving independent credits data"
                );
                None
            }
        };
        let fallback_cost =
            Self::build_uncapped_cost(key_data.as_ref(), credits_result.as_ref().ok());
        let usage = Self::resolve_usage(credits_result, key_data.clone())?;

        let management_key = Self::configured_management_key();
        let activity_key = management_key.as_deref().or_else(|| {
            key_data
                .as_ref()
                .is_some_and(|key_data| key_data.is_management_key == Some(true))
                .then_some(api_key.as_str())
        });
        let activity_cost = match activity_key {
            Some(key) => match Self::fetch_activity_cost(key).await {
                Ok(cost) => Some(cost),
                Err(error) => {
                    tracing::debug!(
                        error = %error,
                        "OpenRouter management Activity degraded; preserving credits/quota"
                    );
                    None
                }
            },
            None => None,
        };

        Ok((usage, activity_cost.or(fallback_cost)))
    }

    async fn fetch_activity_cost(management_key: &str) -> Result<CostSnapshot, ProviderError> {
        let client = Self::build_client(OPENROUTER_KEY_TIMEOUT)?;
        let now = Utc::now();
        let latest_completed = (now.date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let (history_result, latest_completed_result) = tokio::join!(
            Self::fetch_activity_payload(&client, management_key, None),
            Self::fetch_activity_payload(&client, management_key, Some(&latest_completed)),
        );
        let history = history_result?;
        let latest_completed_payload = latest_completed_result?;
        activity::parse_activity_cost(&[history, latest_completed_payload], now)
    }

    async fn fetch_activity_payload(
        client: &reqwest::Client,
        management_key: &str,
        date: Option<&str>,
    ) -> Result<Value, ProviderError> {
        let mut request = client
            .get(OPENROUTER_ACTIVITY_URL)
            .header("Authorization", format!("Bearer {management_key}"))
            .header("Accept", "application/json");
        if let Some(date) = date {
            request = request.query(&[("date", date)]);
        }
        let response = request.send().await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "OpenRouter Activity request returned HTTP {}",
                response.status()
            )));
        }
        response.json::<Value>().await.map_err(|error| {
            ProviderError::Parse(format!("Invalid OpenRouter Activity response: {error}"))
        })
    }

    fn build_client(timeout: std::time::Duration) -> Result<reqwest::Client, ProviderError> {
        crate::core::credentialed_http_client_builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))
    }

    async fn fetch_credits(
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<CreditsResponse, ProviderError> {
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
                "OpenRouter credits request returned HTTP {}",
                resp.status()
            )));
        }

        let response = resp.json::<CreditsResponse>().await.map_err(|error| {
            ProviderError::Parse(format!("OpenRouter credits response was invalid: {error}"))
        })?;
        response.data.validate()?;
        Ok(response)
    }

    fn build_credits_usage(credits: &CreditsData) -> UsageSnapshot {
        let balance = credits.balance();
        let mut primary = RateWindow::new(credits.used_percent());
        primary.reset_description = Some(format!("${:.2} remaining", balance));

        UsageSnapshot::new(primary).with_login_method(format!("${:.2} balance", balance))
    }

    fn build_uncapped_cost(
        key_data: Option<&KeyData>,
        credits: Option<&CreditsResponse>,
    ) -> Option<CostSnapshot> {
        if key_data.is_some_and(|key_data| key_data.is_management_key == Some(true)) {
            return None;
        }
        if key_data
            .and_then(|key_data| key_data.limit)
            .is_some_and(|limit| limit > 0.0)
        {
            return None;
        }

        let monthly = key_data.and_then(|key_data| key_data.usage_monthly);
        let key_usage = key_data.and_then(|key_data| key_data.usage);
        let (used, period) = if let Some(monthly) = monthly {
            (monthly, "This month (API key)")
        } else if let Some(key_usage) = key_usage {
            (key_usage, "Total key usage")
        } else {
            let credits = credits?;
            (credits.data.total_usage, "Total account usage")
        };

        let mut cost = CostSnapshot::new(used.max(0.0), "USD", period);
        if let Some(credits) = credits {
            cost = cost.with_balance(credits.data.balance());
        }
        Some(cost)
    }

    fn resolve_usage(
        credits_result: Result<CreditsResponse, ProviderError>,
        key_data: Option<KeyData>,
    ) -> Result<UsageSnapshot, ProviderError> {
        match credits_result {
            Ok(credits) => {
                let mut usage = Self::build_credits_usage(&credits.data);
                if let Some(key_data) = key_data {
                    Self::apply_key_lanes(&mut usage, &key_data, "Spending cap, not balance");
                }
                Ok(usage)
            }
            Err(error) => {
                let Some(key_data) = key_data else {
                    return Err(error);
                };
                let Some(usage) = Self::build_key_fallback_usage(&key_data) else {
                    return Err(error);
                };
                Ok(usage)
            }
        }
    }

    /// Build a snapshot from the selected API key when account-level credits
    /// are unavailable. The key limit is the only authoritative percentage in
    /// this situation; the account balance remains deliberately unknown.
    fn build_key_fallback_usage(key_data: &KeyData) -> Option<UsageSnapshot> {
        let mut usage =
            UsageSnapshot::new(RateWindow::informational("Account balance unavailable"));
        Self::apply_key_lanes(&mut usage, key_data, "Account balance unavailable");
        usage.secondary.as_ref()?;
        Some(usage)
    }

    async fn fetch_key_data(api_key: &str) -> Result<KeyData, ProviderError> {
        let key_client = Self::build_client(OPENROUTER_KEY_TIMEOUT)?;
        let key_resp = Self::send_key_request(&key_client, api_key).await?;

        if key_resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || key_resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !key_resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "OpenRouter key request returned HTTP {}",
                key_resp.status()
            )));
        }

        let response = key_resp.json::<KeyResponse>().await.map_err(|error| {
            ProviderError::Parse(format!("OpenRouter key response was invalid: {error}"))
        })?;
        response.data.validate()?;
        Ok(response.data)
    }

    async fn send_key_request(
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let key_url = format!("{}/key", OPENROUTER_API_BASE);
        client
            .get(&key_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .send()
            .await
    }

    fn apply_key_lanes(usage: &mut UsageSnapshot, key_data: &KeyData, quota_suffix: &str) {
        Self::add_key_quota_with_suffix(usage, key_data, quota_suffix);
        Self::add_spend_windows(usage, key_data);
    }

    fn add_spend_windows(usage: &mut UsageSnapshot, key_data: &KeyData) {
        Self::add_spend_window(
            usage,
            key_data.usage_daily,
            "daily-spend",
            "Daily spend",
            "today",
        );
        Self::add_spend_window(
            usage,
            key_data.usage_weekly,
            "weekly-spend",
            "Weekly spend",
            "this week",
        );
        Self::add_spend_window(
            usage,
            key_data.usage_monthly,
            "monthly-spend",
            "Monthly spend",
            "this month",
        );
    }

    fn key_quota_metrics(key_data: &KeyData) -> Option<(f64, f64, f64)> {
        let limit = key_data.limit?;
        if limit <= 0.0 || !limit.is_finite() {
            return None;
        }

        let used = if let Some(remaining) = key_data.limit_remaining {
            if !remaining.is_finite() {
                return None;
            }
            limit - remaining.clamp(0.0, limit)
        } else {
            let fallback = quota_fallback_usage(key_data)?;
            if fallback < 0.0 || !fallback.is_finite() {
                return None;
            }
            fallback
        };

        Some((((used / limit) * 100.0).clamp(0.0, 100.0), used, limit))
    }

    /// Key-limit meter derivation (upstream 0.48.0 #2612): prefer the
    /// server-reported current-period remaining (`limit_remaining`), clamped to
    /// [0, limit] so an overspent key reads 100% and an above-limit reading
    /// reads 0%. Without it, fall back to the period usage matching the
    /// declared reset window, then cumulative usage; with no usable source the
    /// meter stays hidden.
    fn add_key_quota(usage: &mut UsageSnapshot, key_data: &KeyData) {
        Self::add_key_quota_with_suffix(usage, key_data, "Spending cap, not balance");
    }

    fn add_key_quota_with_suffix(usage: &mut UsageSnapshot, key_data: &KeyData, suffix: &str) {
        let Some(key_window) = Self::key_quota_window(key_data, suffix) else {
            return;
        };
        *usage = usage
            .clone()
            .with_secondary(key_window)
            .with_secondary_label("API key limit");
    }

    fn key_quota_window(key_data: &KeyData, suffix: &str) -> Option<RateWindow> {
        let (key_percent, used, limit) = Self::key_quota_metrics(key_data)?;
        let mut key_window = RateWindow::new(key_percent);
        key_window.reset_description =
            Some(format!("${used:.2}/${limit:.2} spending cap · {suffix}"));
        Some(key_window)
    }

    fn add_spend_window(
        usage: &mut UsageSnapshot,
        value: Option<f64>,
        id: &'static str,
        label: &'static str,
        period: &'static str,
    ) {
        let Some(spend) = value else {
            return;
        };

        let mut window = RateWindow::new(0.0);
        window.reset_description = Some(format!("${spend:.2} {period}"));
        *usage = usage.clone().with_extra_rate_window(id, label, window);
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
                let (usage, cost) = self.fetch_usage_api(ctx).await?;
                let mut result = ProviderFetchResult::new(usage, "api");
                if let Some(cost) = cost {
                    result = result.with_cost(cost);
                }
                Ok(result)
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
