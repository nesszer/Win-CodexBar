//! OpenRouter provider implementation
//!
//! Fetches credit balance and usage data from OpenRouter's REST API
//! Requires API key for authentication

mod activity;
mod diagnostics;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use activity::ActivityReport;
use diagnostics::{Degraded, Observations};

/// OpenRouter API base URL — the bare `/api/v1` prefix, matching upstream
/// (steipete/CodexBar `OpenRouterSettingsReader.apiURL`).
///
/// Both endpoints append their path to this base: `/credits` and `/key`.
/// The fork's original bug baked `/auth` into the base (`.../api/v1/auth`),
/// which turned the credits call into `/api/v1/auth/credits` -> 404.
const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";
/// Per-request deadline for `/credits`, `/key`, and Activity. Upstream
/// `openrouter.js` (v0.61.0) gives each optional request four seconds; a
/// degraded request is reported with a safe reason, never fatal on its own.
const OPENROUTER_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
const OPENROUTER_ACTIVITY_URL: &str = "https://openrouter.ai/api/v1/activity";
const OPENROUTER_MANAGEMENT_ENV: &str = "OPENROUTER_MANAGEMENT_API_KEY";
/// Shown when no primary key resolves. The optional Management key field never
/// substitutes for the primary key when selecting an account for quota/balance.
const MISSING_API_KEY_MESSAGE: &str = "Enter a regular API key or a Management API key in the API key field, or set OPENROUTER_API_KEY. In Settings, the optional Management API key field does not replace it.";

/// Windows Credential Manager target for OpenRouter API token
const OPENROUTER_CREDENTIAL_TARGET: &str = "codexbar-openrouter";

/// Statuses that mean the credential itself was rejected.
const AUTH_REJECTED: &[reqwest::StatusCode] = &[
    reqwest::StatusCode::UNAUTHORIZED,
    reqwest::StatusCode::FORBIDDEN,
];

/// Snapshot one optional request for the display rows: the usable value, or
/// the safe reason it degraded.
fn observe<T, U>(result: &Result<T, Degraded>, value: impl FnOnce(&T) -> U) -> Result<U, String> {
    match result {
        Ok(ok) => Ok(value(ok)),
        Err(degraded) => Err(degraded.reason.clone()),
    }
}

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
#[derive(Default)]
pub struct OpenRouterProvider;

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
        Self
    }

    /// Get API token from ctx, Windows Credential Manager, or env
    fn get_api_token(api_key: Option<&str>) -> Result<String, ProviderError> {
        if let Some(key) = api_key
            && !key.is_empty()
        {
            return Ok(key.to_string());
        }

        keyring::Entry::new(OPENROUTER_CREDENTIAL_TARGET, "api_token")
            .ok()
            .and_then(|entry| entry.get_password().ok())
            .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
            .ok_or_else(|| ProviderError::NotInstalled(MISSING_API_KEY_MESSAGE.to_string()))
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
    ) -> Result<ProviderFetchResult, ProviderError> {
        let api_key = Self::get_api_token(ctx.api_key.as_deref())?;
        let client = Self::build_client()?;
        // OpenRouter can reject the account-level `/credits` request while
        // still returning the selected key's current spend/quota from `/key`.
        // Fetch both independent sources together, then let the pure resolver
        // choose the stable primary/secondary lanes.
        let (credits_result, key_result) = tokio::join!(
            Self::fetch_credits(&client, &api_key),
            Self::fetch_key_data(&client, &api_key),
        );
        let credits_observed = observe(&credits_result, |credits| credits.data.clone());
        let key_observed = observe(&key_result, Clone::clone);
        if let Err(degraded) = &credits_result {
            tracing::debug!(reason = %degraded.reason, "OpenRouter credits endpoint degraded");
        }
        let key_data = match key_result {
            Ok(key_data) => Some(key_data),
            Err(degraded) => {
                tracing::debug!(
                    reason = %degraded.reason,
                    "OpenRouter key endpoint degraded; preserving independent credits data"
                );
                None
            }
        };
        let credits_result = credits_result.map_err(|degraded| degraded.error);
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
        let activity_result = match activity_key {
            Some(key) => Some(Self::fetch_activity(&client, key).await),
            None => None,
        };
        let activity_observed = match &activity_result {
            Some(result) => observe(result, |report| report.summary),
            None => Err(diagnostics::ACTIVITY_NOT_CONFIGURED.to_string()),
        };
        let activity_cost = match activity_result {
            Some(Ok(report)) => Some(report.cost),
            Some(Err(degraded)) => {
                tracing::debug!(
                    reason = %degraded.reason,
                    "OpenRouter management Activity degraded; preserving credits/quota"
                );
                None
            }
            None => None,
        };

        let mut result = ProviderFetchResult::new(usage, "api");
        if let Some(cost) = activity_cost.or(fallback_cost) {
            result = result.with_cost(cost);
        }
        let details = diagnostics::build_display_details(&Observations {
            credits: &credits_observed,
            key: &key_observed,
            activity: &activity_observed,
        });
        for detail in details {
            result = result.with_display_detail(Some(detail));
        }
        Ok(result)
    }

    async fn fetch_activity(
        client: &reqwest::Client,
        management_key: &str,
    ) -> Result<ActivityReport, Degraded> {
        let now = Utc::now();
        let latest_completed = (now.date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let (history_result, latest_completed_result) = tokio::join!(
            Self::fetch_activity_payload(client, management_key, None),
            Self::fetch_activity_payload(client, management_key, Some(&latest_completed)),
        );
        let history = history_result?;
        let latest_completed_payload = latest_completed_result?;
        activity::parse_activity_cost(&[history, latest_completed_payload], now)
            .map_err(Degraded::invalid)
    }

    async fn fetch_activity_payload(
        client: &reqwest::Client,
        management_key: &str,
        date: Option<&str>,
    ) -> Result<Value, Degraded> {
        let mut request = client.get(OPENROUTER_ACTIVITY_URL);
        if let Some(date) = date {
            request = request.query(&[("date", date)]);
        }
        Self::get_json(
            request,
            management_key,
            "Activity",
            AUTH_REJECTED,
            Some(diagnostics::ACTIVITY_KEY_REQUIRED),
        )
        .await
    }

    /// Bearer-authenticated JSON GET. Statuses in `auth_statuses` are typed as
    /// auth failures; a 403 shows `forbidden_reason` when one is given.
    async fn get_json<T: serde::de::DeserializeOwned>(
        request: reqwest::RequestBuilder,
        api_key: &str,
        label: &str,
        auth_statuses: &[reqwest::StatusCode],
        forbidden_reason: Option<&str>,
    ) -> Result<T, Degraded> {
        let response = request
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let degraded = Degraded::http(label, status, auth_statuses);
            return Err(match forbidden_reason {
                Some(reason) if status == reqwest::StatusCode::FORBIDDEN => {
                    degraded.with_reason(reason)
                }
                _ => degraded,
            });
        }
        response
            .json::<T>()
            .await
            .map_err(|error| Degraded::body(label, error))
    }

    fn build_client() -> Result<reqwest::Client, ProviderError> {
        crate::core::credentialed_http_client_builder()
            .timeout(OPENROUTER_REQUEST_TIMEOUT)
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))
    }

    async fn fetch_credits(
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<CreditsResponse, Degraded> {
        let credits_url = format!("{}/credits", OPENROUTER_API_BASE);
        let response: CreditsResponse = Self::get_json(
            client.get(&credits_url),
            api_key,
            "credits",
            &[reqwest::StatusCode::UNAUTHORIZED],
            None,
        )
        .await?;
        response.data.validate().map_err(Degraded::invalid)?;
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

    async fn fetch_key_data(client: &reqwest::Client, api_key: &str) -> Result<KeyData, Degraded> {
        let key_url = format!("{}/key", OPENROUTER_API_BASE);
        let response: KeyResponse =
            Self::get_json(client.get(&key_url), api_key, "key", AUTH_REJECTED, None).await?;
        response.data.validate().map_err(Degraded::invalid)?;
        Ok(response.data)
    }

    fn apply_key_lanes(usage: &mut UsageSnapshot, key_data: &KeyData, quota_suffix: &str) {
        Self::add_key_quota_with_suffix(usage, key_data, quota_suffix);
        Self::add_spend_windows(usage, key_data);
    }

    fn add_spend_windows(usage: &mut UsageSnapshot, key_data: &KeyData) {
        for (value, id, label, period) in [
            (key_data.usage_daily, "daily-spend", "Daily spend", "today"),
            (
                key_data.usage_weekly,
                "weekly-spend",
                "Weekly spend",
                "this week",
            ),
            (
                key_data.usage_monthly,
                "monthly-spend",
                "Monthly spend",
                "this month",
            ),
        ] {
            Self::add_spend_window(usage, value, id, label, period);
        }
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

#[async_trait]
impl Provider for OpenRouterProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching OpenRouter usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_api(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}
