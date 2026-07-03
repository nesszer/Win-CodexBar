//! CrossModel provider implementation.
//!
//! Mirrors upstream v0.38.0 behavior: fetch wallet credits and best-effort
//! day/week/month usage windows from the CrossModel API.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const CREDENTIAL_TARGET: &str = "codexbar-crossmodel";
const DEFAULT_API_BASE: &str = "https://api.crossmodel.ai/v1";
const MICRO_UNITS: f64 = 1_000_000.0;

pub struct CrossModelProvider {
    metadata: ProviderMetadata,
    client: Client,
}

#[derive(Debug, Deserialize)]
struct CreditsResponse {
    currency: String,
    #[serde(alias = "balanceMicro")]
    balance_micro: i64,
    #[serde(default, alias = "uncollectedMicro")]
    uncollected_micro: i64,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    currency: String,
    daily: Option<UsageWindow>,
    weekly: Option<UsageWindow>,
    monthly: Option<UsageWindow>,
}

#[derive(Debug, Clone, Deserialize)]
struct UsageWindow {
    #[serde(alias = "costMicro")]
    cost_micro: i64,
    #[serde(default, alias = "promptTokens")]
    prompt_tokens: Option<i64>,
    #[serde(default, alias = "completionTokens")]
    completion_tokens: Option<i64>,
    #[serde(default, alias = "totalTokens")]
    total_tokens: Option<i64>,
    #[serde(default, alias = "requestCount")]
    request_count: Option<i64>,
    #[serde(default, alias = "successCount")]
    success_count: Option<i64>,
}

impl CrossModelProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::CrossModel,
                display_name: "CrossModel",
                session_label: "Daily cost",
                weekly_label: "Weekly cost",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://crossmodel.ai"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, key: &str) -> Result<ProviderFetchResult, ProviderError> {
        let base = std::env::var("CROSSMODEL_API_URL").unwrap_or_else(|_| DEFAULT_API_BASE.into());
        let base_url = crate::providers::validated_https_url(&base, "CrossModel API")?;
        let credits_url = base_url
            .join("credits")
            .map_err(|e| ProviderError::Other(format!("Invalid CrossModel credits URL: {e}")))?;

        let credits_response = self
            .client
            .get(credits_url)
            .header("Authorization", authorization_header(key))
            .header("Accept", "application/json")
            .send()
            .await?;
        if credits_response.status() == reqwest::StatusCode::UNAUTHORIZED
            || credits_response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !credits_response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "CrossModel credits returned status {}",
                credits_response.status()
            )));
        }
        let credits: CreditsResponse = credits_response.json().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse CrossModel credits: {e}"))
        })?;

        let usage = self.fetch_usage_windows(&base_url, key).await?;
        Ok(snapshot_from_parts(credits, usage))
    }

    async fn fetch_usage_windows(
        &self,
        base_url: &reqwest::Url,
        key: &str,
    ) -> Result<Option<UsageResponse>, ProviderError> {
        let usage_url = base_url
            .join("usage")
            .map_err(|e| ProviderError::Other(format!("Invalid CrossModel usage URL: {e}")))?;
        let response = match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.client
                .get(usage_url)
                .header("Authorization", authorization_header(key))
                .header("Accept", "application/json")
                .send(),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(_)) | Err(_) => return Ok(None),
        };
        if !response.status().is_success() {
            return Ok(None);
        }
        response
            .json::<UsageResponse>()
            .await
            .map(Some)
            .map_err(|e| ProviderError::Parse(format!("Failed to parse CrossModel usage: {e}")))
    }
}

impl Default for CrossModelProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn authorization_header(_key: &str) -> &'static str {
    "******"
}

fn snapshot_from_parts(
    credits: CreditsResponse,
    usage: Option<UsageResponse>,
) -> ProviderFetchResult {
    let currency = normalize_currency(&credits.currency);
    let balance = major_units(credits.balance_micro);
    let uncollected = major_units(credits.uncollected_micro);
    let mut primary = usage
        .as_ref()
        .and_then(|u| usage_window_rate(u.daily.as_ref(), "Daily"))
        .unwrap_or_else(|| {
            RateWindow::with_details(
                0.0,
                Some(24 * 60),
                None,
                Some(format!("{} balance", format_amount(balance, &currency))),
            )
        });

    if primary.reset_description.is_none() {
        primary.reset_description = Some("Daily usage".to_string());
    }

    let mut snapshot = UsageSnapshot::new(primary).with_login_method("API key");
    if let Some(usage) = usage
        .as_ref()
        .filter(|usage| normalize_currency(&usage.currency) == currency)
    {
        if let Some(weekly) = usage_window_rate(usage.weekly.as_ref(), "Weekly") {
            snapshot = snapshot.with_secondary(weekly);
        }
        if let Some(monthly) = usage_window_rate(usage.monthly.as_ref(), "Monthly") {
            snapshot = snapshot.with_tertiary(monthly);
        }
    }
    if uncollected > 0.0 {
        snapshot = snapshot.with_extra_rate_window(
            "uncollected",
            "Uncollected",
            RateWindow::with_details(0.0, None, None, Some(format_amount(uncollected, &currency))),
        );
    }

    let cost = CostSnapshot::new(0.0, currency, "balance").with_limit(balance.max(0.0));
    ProviderFetchResult::new(snapshot, "api").with_cost(cost)
}

fn usage_window_rate(window: Option<&UsageWindow>, label: &str) -> Option<RateWindow> {
    let window = window?;
    let cost = major_units(window.cost_micro);
    let mut details = vec![format!("{} cost {}", label, format_amount(cost, "USD"))];
    if let Some(tokens) = window.total_tokens {
        details.push(format!("{tokens} tokens"));
    }
    if let Some(requests) = window.request_count {
        details.push(format!("{requests} requests"));
    }
    if let Some(successes) = window.success_count {
        details.push(format!("{successes} successes"));
    }
    if let Some(prompt_tokens) = window.prompt_tokens {
        details.push(format!("{prompt_tokens} prompt"));
    }
    if let Some(completion_tokens) = window.completion_tokens {
        details.push(format!("{completion_tokens} completion"));
    }
    Some(RateWindow::with_details(
        0.0,
        None,
        None,
        Some(details.join(", ")),
    ))
}

fn major_units(micro: i64) -> f64 {
    micro as f64 / MICRO_UNITS
}

fn normalize_currency(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_uppercase();
    if trimmed.is_empty() {
        "USD".to_string()
    } else {
        trimmed
    }
}

fn format_amount(value: f64, currency: &str) -> String {
    if currency.eq_ignore_ascii_case("USD") {
        format!("${value:.2}")
    } else {
        format!("{value:.2} {currency}")
    }
}

#[async_trait]
impl Provider for CrossModelProvider {
    fn id(&self) -> ProviderId {
        ProviderId::CrossModel
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    &["CROSSMODEL_API_KEY"],
                )?;
                self.fetch_api(&key).await
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_crossmodel_micro_units() {
        let result = snapshot_from_parts(
            CreditsResponse {
                currency: "usd".to_string(),
                balance_micro: 8_059_489,
                uncollected_micro: 250_000,
            },
            Some(UsageResponse {
                currency: "USD".to_string(),
                daily: Some(UsageWindow {
                    cost_micro: 5_746,
                    prompt_tokens: Some(9_176),
                    completion_tokens: Some(3_291),
                    total_tokens: Some(12_467),
                    request_count: Some(9),
                    success_count: Some(9),
                }),
                weekly: None,
                monthly: None,
            }),
        );

        assert_eq!(result.cost.unwrap().limit, Some(8.059489));
        assert!(
            result
                .usage
                .primary
                .reset_description
                .unwrap()
                .contains("12467 tokens")
        );
    }
}
