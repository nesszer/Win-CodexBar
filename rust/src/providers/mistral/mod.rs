//! Mistral provider implementation
//!
//! Fetches monthly spend from the Mistral admin billing API using browser
//! cookies or a manual Cookie header.

use async_trait::async_trait;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

mod pricing;
mod subscription;
mod token_math;

use pricing::{MistralPrice, PriceIndex};
use subscription::{SubscriptionBudget, SubscriptionBudgets};

use crate::core::{
    CostSnapshot, FetchContext, NamedRateWindow, Provider, ProviderError, ProviderFetchResult,
    ProviderId, RateWindow, SourceMode, UsageSnapshot,
};

const BASE_URL: &str = "https://admin.mistral.ai";
const COOKIE_DOMAINS: [&str; 3] = ["admin.mistral.ai", "mistral.ai", "auth.mistral.ai"];
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const CLIENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Optional subscription-page enrichment joins on a fast deadline so a slow
/// `/subscription` render can never stall the refresh; degraded enrichment is
/// logged and skipped, never fatal.
const SUBSCRIPTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
/// Extra rate window holding the Vibe monthly plan allowance. The Monthly Plan
/// menu bar metric selects it (upstream 0.70.0 #4072).
pub const MONTHLY_PLAN_WINDOW_ID: &str = "mistral-monthly-plan";
/// Picker label for the primary lane, the included API allowance (upstream
/// `menuBarLayoutPrimaryLabel`).
const INCLUDED_API_LABEL: &str = "Included API";

#[derive(Debug, Deserialize)]
struct BillingResponse {
    completion: Option<ModelUsageCategory>,
    /// Le Chat consumption; counted like `completion`.
    chat: Option<ModelUsageCategory>,
    /// Vibe Code consumption; its `completion` counts like API completions.
    #[serde(rename = "vibe_code")]
    vibe_code: Option<VibeCodeCategory>,
    ocr: Option<ModelUsageCategory>,
    connectors: Option<ModelUsageCategory>,
    audio: Option<ModelUsageCategory>,
    #[serde(rename = "libraries_api")]
    libraries_api: Option<LibrariesUsageCategory>,
    #[serde(rename = "fine_tuning")]
    fine_tuning: Option<FineTuningCategory>,
    #[serde(rename = "start_date")]
    start_date: Option<String>,
    #[serde(rename = "end_date")]
    end_date: Option<String>,
    currency: Option<String>,
    #[serde(rename = "currency_symbol")]
    currency_symbol: Option<String>,
    prices: Option<Vec<MistralPrice>>,
}

#[derive(Debug, Deserialize)]
struct ModelUsageCategory {
    models: Option<HashMap<String, ModelUsageData>>,
}

#[derive(Debug, Deserialize)]
struct VibeCodeCategory {
    completion: Option<ModelUsageCategory>,
}

#[derive(Debug, Deserialize)]
struct LibrariesUsageCategory {
    pages: Option<ModelUsageCategory>,
    tokens: Option<ModelUsageCategory>,
}

#[derive(Debug, Deserialize)]
struct FineTuningCategory {
    training: Option<HashMap<String, ModelUsageData>>,
    storage: Option<HashMap<String, ModelUsageData>>,
}

#[derive(Debug, Deserialize)]
struct ModelUsageData {
    input: Option<Vec<UsageEntry>>,
    output: Option<Vec<UsageEntry>>,
    cached: Option<Vec<UsageEntry>>,
}

#[derive(Debug, Deserialize)]
struct UsageEntry {
    #[serde(rename = "event_type")]
    event_type: Option<String>,
    #[serde(rename = "billing_metric")]
    billing_metric: Option<String>,
    #[serde(rename = "billing_group")]
    billing_group: Option<String>,
    #[serde(rename = "api_zone")]
    api_zone: Option<String>,
    #[serde(rename = "service_tier")]
    service_tier: Option<String>,
    value: Option<i64>,
    #[serde(rename = "value_paid")]
    value_paid: Option<i64>,
}

#[derive(Debug)]
struct MistralUsageSummary {
    total_cost: f64,
    currency: String,
    currency_symbol: String,
    total_input_tokens: i64,
    total_output_tokens: i64,
    total_cached_tokens: i64,
    model_count: usize,
    end_date: Option<DateTime<Utc>>,
}

pub struct MistralProvider {
    client: Client,
}

#[derive(Debug, Default)]
struct TokenCounts {
    input: i64,
    output: i64,
    cached: i64,
}

#[derive(Clone, Copy)]
enum TokenKind {
    Input,
    Output,
    Cached,
}

#[derive(Clone, Copy)]
enum AggregationMode {
    CostOnly,
    CostAndTokens,
}

enum ModelAggregation {
    Cost(f64),
    CostAndTokens { tokens: TokenCounts, cost: f64 },
}

impl TokenCounts {
    fn add_lane(&mut self, units: i64, kind: TokenKind) -> Result<(), ProviderError> {
        let lane = match kind {
            TokenKind::Input => &mut self.input,
            TokenKind::Output => &mut self.output,
            TokenKind::Cached => &mut self.cached,
        };
        *lane = lane.checked_add(units).ok_or_else(|| {
            ProviderError::Parse("Mistral token count exceeds supported range".into())
        })?;
        Ok(())
    }

    fn add(&mut self, other: &Self) -> Result<(), ProviderError> {
        self.add_lane(other.input, TokenKind::Input)?;
        self.add_lane(other.output, TokenKind::Output)?;
        self.add_lane(other.cached, TokenKind::Cached)
    }

    fn total(&self) -> Result<i64, ProviderError> {
        token_math::checked_total(self.input, self.cached, self.output).ok_or_else(|| {
            ProviderError::Parse("Mistral token count exceeds supported range".into())
        })
    }
}

impl MistralProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(CLIENT_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn csrf_from_cookie_header(cookie_header: &str) -> Option<&str> {
        cookie_header.split(';').find_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            (name == "csrftoken").then_some(value.trim())
        })
    }

    async fn fetch_with_cookies(
        &self,
        cookie_header: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let now = Utc::now();
        let url = format!(
            "{BASE_URL}/api/billing/v2/usage?month={}&year={}",
            now.month(),
            now.year()
        );

        let mut request = self
            .client
            .get(url)
            .header("Accept", "*/*")
            .header("Cookie", cookie_header)
            .header("Origin", BASE_URL)
            .header("Referer", "https://admin.mistral.ai/organization/usage")
            .header("User-Agent", USER_AGENT);

        if let Some(csrf) = Self::csrf_from_cookie_header(cookie_header) {
            request = request.header("X-CSRFTOKEN", csrf);
        }

        let response = request.send().await?;
        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Other(format!(
                "Mistral API returned {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            )));
        }

        let body = response.text().await?;
        let billing: BillingResponse = serde_json::from_str(&body)
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Mistral usage: {e}")))?;

        let summary = Self::summarize_billing(billing)?;
        let budgets = match self.fetch_subscription_budgets(cookie_header).await {
            Ok(budgets) => Some(budgets),
            Err(error) => {
                tracing::debug!(error = %error, "Mistral subscription allowance enrichment unavailable");
                None
            }
        };
        Ok(Self::build_result(summary, budgets))
    }

    async fn fetch_subscription_budgets(
        &self,
        cookie_header: &str,
    ) -> Result<SubscriptionBudgets, ProviderError> {
        let response = self
            .client
            .get(format!("{BASE_URL}/subscription"))
            .timeout(SUBSCRIPTION_TIMEOUT)
            .header("Accept", "text/html")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Cookie", cookie_header)
            .header("Referer", format!("{BASE_URL}/subscription"))
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;
        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "Mistral subscription API returned {status}"
            )));
        }
        let final_url = response.url();
        if final_url.scheme() != "https" || final_url.host_str() != Some("admin.mistral.ai") {
            return Err(ProviderError::Parse(
                "Mistral subscription response came from an unexpected host".into(),
            ));
        }
        let body = response.text().await?;
        subscription::parse(&body).map_err(ProviderError::Parse)
    }

    fn summarize_billing(billing: BillingResponse) -> Result<MistralUsageSummary, ProviderError> {
        let prices = PriceIndex::new(billing.prices.unwrap_or_default());
        let mut total_cost = 0.0;
        let mut total_tokens = TokenCounts::default();
        let mut model_count = 0;

        // API, Le Chat, and Vibe Code completions share consumed-token and
        // billed-cost accounting.
        let token_categories = [
            billing.completion,
            billing.chat,
            billing.vibe_code.and_then(|vibe| vibe.completion),
        ];
        for models in token_categories
            .into_iter()
            .flatten()
            .filter_map(|category| category.models)
        {
            model_count += models.len();
            for data in models.values() {
                match Self::aggregate_model(data, &prices, AggregationMode::CostAndTokens)? {
                    ModelAggregation::CostAndTokens { tokens, cost } => {
                        total_tokens.add(&tokens)?;
                        Self::accumulate_finite_cost(cost, &mut total_cost);
                    }
                    ModelAggregation::Cost(_) => {
                        unreachable!("token mode returned cost-only result")
                    }
                }
            }
        }

        for category in [billing.ocr, billing.connectors, billing.audio]
            .into_iter()
            .flatten()
        {
            if let Some(models) = category.models {
                for data in models.values() {
                    Self::accumulate_finite_cost(
                        Self::aggregate_cost(data, &prices)?,
                        &mut total_cost,
                    );
                }
            }
        }

        if let Some(libraries) = billing.libraries_api {
            for category in [libraries.pages, libraries.tokens].into_iter().flatten() {
                if let Some(models) = category.models {
                    for data in models.values() {
                        Self::accumulate_finite_cost(
                            Self::aggregate_cost(data, &prices)?,
                            &mut total_cost,
                        );
                    }
                }
            }
        }

        if let Some(fine_tuning) = billing.fine_tuning {
            for models in [fine_tuning.training, fine_tuning.storage]
                .into_iter()
                .flatten()
            {
                for data in models.values() {
                    Self::accumulate_finite_cost(
                        Self::aggregate_cost(data, &prices)?,
                        &mut total_cost,
                    );
                }
            }
        }

        let _ = billing.start_date;

        total_tokens.total()?;

        Ok(MistralUsageSummary {
            total_cost,
            currency: billing.currency.unwrap_or_else(|| "EUR".to_string()),
            currency_symbol: billing.currency_symbol.unwrap_or_else(|| "€".to_string()),
            total_input_tokens: total_tokens.input,
            total_output_tokens: total_tokens.output,
            total_cached_tokens: total_tokens.cached,
            model_count,
            end_date: billing.end_date.as_deref().and_then(Self::parse_date),
        })
    }

    fn build_result(
        summary: MistralUsageSummary,
        budgets: Option<SubscriptionBudgets>,
    ) -> ProviderFetchResult {
        let reset_date = summary.end_date.map(|dt| dt + chrono::Duration::seconds(1));
        let cost_description = if summary.total_cost > 0.0 {
            format!(
                "{}{:.4} this month",
                summary.currency_symbol, summary.total_cost
            )
        } else {
            "No usage this month".to_string()
        };

        // The description carries spend amounts, not reset wording.
        let primary = RateWindow::with_details(0.0, None, reset_date, Some(cost_description))
            .with_description_as_detail();
        let mut usage = UsageSnapshot::new(primary);
        if summary.model_count > 0 {
            usage = usage.with_login_method(format!("{} model(s)", summary.model_count));
        }

        let mut cost = CostSnapshot::new(summary.total_cost, summary.currency, "Monthly");
        cost = cost.with_currency_symbol(summary.currency_symbol);
        if let Some(reset) = reset_date {
            cost = cost.with_resets_at(reset);
        }

        let token_detail = format!(
            "{} input / {} output / {} cached tokens",
            summary.total_input_tokens, summary.total_output_tokens, summary.total_cached_tokens
        );
        usage.primary.reset_description = Some(format!(
            "{} • {}",
            usage.primary.reset_description.clone().unwrap_or_default(),
            token_detail
        ));

        if let Some(budgets) = budgets {
            if let Some(api) = budgets.api {
                usage.primary = Self::budget_window(&api);
                usage.primary_label = Some(INCLUDED_API_LABEL.to_string());
            }
            if let Some(vibe) = budgets.vibe {
                usage.extra_rate_windows.push(NamedRateWindow::new(
                    MONTHLY_PLAN_WINDOW_ID,
                    "Monthly Plan",
                    Self::budget_window(&vibe),
                ));
            }
        }

        ProviderFetchResult::new(usage, "web").with_cost(cost)
    }

    fn budget_window(budget: &SubscriptionBudget) -> RateWindow {
        let used = budget.used_amount();
        let remaining = budget.remaining_amount();
        let description = format!(
            "{used:.2} {currency} / {limit:.2} {currency} · {remaining:.2} {currency} remaining",
            currency = budget.currency,
            limit = budget.limit,
        );
        RateWindow::with_details(
            budget.used_percent,
            None,
            budget.resets_at,
            Some(description),
        )
        .with_description_as_detail()
    }

    fn aggregate_model(
        data: &ModelUsageData,
        prices: &PriceIndex,
        mode: AggregationMode,
    ) -> Result<ModelAggregation, ProviderError> {
        let mut tokens = TokenCounts::default();
        let mut cost = 0.0;
        for (kind, entries) in [
            (TokenKind::Input, data.input.as_deref()),
            (TokenKind::Output, data.output.as_deref()),
            (TokenKind::Cached, data.cached.as_deref()),
        ] {
            for entry in entries.unwrap_or_default() {
                // Tokens count consumed units (plan-covered usage included);
                // spend counts only billed units.
                if matches!(mode, AggregationMode::CostAndTokens) {
                    tokens.add_lane(entry.value.or(entry.value_paid).unwrap_or(0), kind)?;
                }
                // Upstream 0.70.0 (#4076): priced by event type, API zone and
                // service tier, not by metric and group alone.
                if let Some(unit_price) = prices.unit_price(entry) {
                    let billed_units = entry.value_paid.or(entry.value).unwrap_or(0);
                    Self::accumulate_finite_cost(billed_units as f64 * unit_price, &mut cost);
                }
            }
        }
        Ok(match mode {
            AggregationMode::CostOnly => ModelAggregation::Cost(cost),
            AggregationMode::CostAndTokens => ModelAggregation::CostAndTokens { tokens, cost },
        })
    }

    fn aggregate_cost(data: &ModelUsageData, prices: &PriceIndex) -> Result<f64, ProviderError> {
        match Self::aggregate_model(data, prices, AggregationMode::CostOnly)? {
            ModelAggregation::Cost(cost) => Ok(cost),
            ModelAggregation::CostAndTokens { .. } => {
                unreachable!("cost mode returned token-bearing result")
            }
        }
    }

    fn accumulate_finite_cost(cost: f64, total: &mut f64) {
        if cost.is_finite() {
            let updated = *total + cost;
            if updated.is_finite() {
                *total = updated;
            }
        }
    }

    fn parse_date(value: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|| {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .ok()
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .map(|naive| Utc.from_utc_datetime(&naive))
            })
    }
}

impl Default for MistralProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MistralProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Mistral
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                if let Some(ref cookie_header) = ctx.manual_cookie_header {
                    return self.fetch_with_cookies(cookie_header).await;
                }

                match crate::providers::browser_cookie_header(&COOKIE_DOMAINS) {
                    Ok(header) => match self.fetch_with_cookies(&header).await {
                        Ok(result) => return Ok(result),
                        Err(ProviderError::AuthRequired) => {}
                        Err(err) => return Err(err),
                    },
                    Err(ProviderError::NoCookies) => {}
                    Err(err) => return Err(err),
                }

                Err(ProviderError::NoCookies)
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn monthly_plan_window_id(&self) -> Option<&'static str> {
        Some(MONTHLY_PLAN_WINDOW_ID)
    }

    fn menu_bar_primary_label(&self) -> Option<&'static str> {
        Some(INCLUDED_API_LABEL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mistral_billing_cost() {
        let billing: BillingResponse = serde_json::from_value(serde_json::json!({
            "currency": "EUR",
            "currency_symbol": "€",
            "end_date": "2026-04-30T00:00:00Z",
            "prices": [
                { "billing_metric": "mistral-large-2411", "billing_group": "input", "price": "0.000002" },
                { "billing_metric": "mistral-large-2411", "billing_group": "output", "price": "0.000006" }
            ],
            "completion": {
                "models": {
                    "mistral-large-latest::mistral-large-2411": {
                        "input": [
                            { "billing_metric": "mistral-large-2411", "billing_group": "input", "value": 1000, "value_paid": 1000 }
                        ],
                        "output": [
                            { "billing_metric": "mistral-large-2411", "billing_group": "output", "value": 500, "value_paid": 500 }
                        ]
                    }
                }
            }
        }))
        .unwrap();

        let summary = MistralProvider::summarize_billing(billing).unwrap();
        assert!((summary.total_cost - 0.005).abs() < 0.000001);
        assert_eq!(summary.model_count, 1);

        let result = MistralProvider::build_result(summary, None);
        assert_eq!(
            result.cost.as_ref().map(|c| c.currency_code.as_str()),
            Some("EUR")
        );
        assert!(
            result
                .usage
                .primary
                .reset_description
                .as_deref()
                .unwrap_or_default()
                .contains("1000 input / 500 output")
        );
    }

    #[test]
    fn monthly_plan_metric_targets_the_published_vibe_window() {
        let provider = MistralProvider::new();

        assert_eq!(
            provider.monthly_plan_window_id(),
            Some("mistral-monthly-plan")
        );
        assert_eq!(provider.menu_bar_primary_label(), Some("Included API"));
    }

    #[test]
    fn attaches_subscription_allowances_without_replacing_billing_cost() {
        let summary = MistralUsageSummary {
            total_cost: 12.5,
            currency: "EUR".to_string(),
            currency_symbol: "€".to_string(),
            total_input_tokens: 100,
            total_output_tokens: 50,
            total_cached_tokens: 0,
            model_count: 1,
            end_date: None,
        };
        let result = MistralProvider::build_result(
            summary,
            Some(SubscriptionBudgets {
                api: Some(SubscriptionBudget {
                    used_percent: 25.0,
                    limit: 100.0,
                    currency: "USD".to_string(),
                    resets_at: None,
                }),
                vibe: Some(SubscriptionBudget {
                    used_percent: 50.0,
                    limit: 20.0,
                    currency: "EUR".to_string(),
                    resets_at: None,
                }),
            }),
        );

        assert_eq!(result.usage.primary.used_percent, 25.0);
        assert_eq!(result.usage.primary_label.as_deref(), Some("Included API"));
        assert_eq!(result.usage.extra_rate_windows.len(), 1);
        assert_eq!(
            result.usage.extra_rate_windows[0].id,
            "mistral-monthly-plan"
        );
        assert_eq!(result.cost.as_ref().map(|cost| cost.used), Some(12.5));
        assert!(result.usage.primary.description_is_detail);
        assert!(
            result.usage.extra_rate_windows[0]
                .window
                .description_is_detail
        );
        assert_eq!(
            result.usage.extra_rate_windows[0]
                .window
                .reset_description
                .as_deref(),
            Some("10.00 EUR / 20.00 EUR · 10.00 EUR remaining")
        );
    }

    #[test]
    fn extracts_csrf_token_from_cookie_header() {
        assert_eq!(
            MistralProvider::csrf_from_cookie_header("foo=bar; csrftoken=abc123; ory_session=x"),
            Some("abc123")
        );
    }

    #[test]
    fn ignores_non_finite_prices() {
        let billing: BillingResponse = serde_json::from_value(serde_json::json!({
            "prices": [
                { "billing_metric": "mistral-large", "billing_group": "input", "price": "1e309" }
            ],
            "completion": {
                "models": {
                    "mistral-large": {
                        "input": [
                            { "billing_metric": "mistral-large", "billing_group": "input", "value": 1000 }
                        ]
                    }
                }
            }
        }))
        .unwrap();

        let summary = MistralProvider::summarize_billing(billing).unwrap();

        assert_eq!(summary.total_cost, 0.0);
    }

    #[test]
    fn preserves_signed_lanes_when_the_checked_total_cancels() {
        let billing: BillingResponse = serde_json::from_value(serde_json::json!({
            "completion": {
                "models": {
                    "fixture": {
                        "input": [{"value": i64::MAX}],
                        "cached": [{"value": -1}],
                        "output": [{"value": 1}]
                    }
                }
            }
        }))
        .unwrap();

        let summary = MistralProvider::summarize_billing(billing).unwrap();
        assert_eq!(summary.total_input_tokens, i64::MAX);
        assert_eq!(summary.total_cached_tokens, -1);
        assert_eq!(summary.total_output_tokens, 1);
    }

    #[test]
    fn rejects_same_lane_token_overflow_during_aggregation() {
        let billing: BillingResponse = serde_json::from_value(serde_json::json!({
            "completion": {
                "models": {
                    "fixture": {
                        "input": [{"value": i64::MAX}, {"value": 1}]
                    }
                }
            }
        }))
        .unwrap();

        assert!(matches!(
            MistralProvider::summarize_billing(billing),
            Err(ProviderError::Parse(message)) if message.contains("token count")
        ));
    }

    /// Model `fixture` priced at 0.25 per billed unit, wrapped for one category.
    fn fixture_billing(category: &str, model: serde_json::Value) -> BillingResponse {
        let models = serde_json::json!({ "models": { "fixture": model } });
        let mut payload = serde_json::json!({
            "prices": [{ "billing_metric": "fixture", "billing_group": "unit", "price": "0.25" }],
        });
        payload[category] = if category == "vibe_code" {
            serde_json::json!({ "completion": models })
        } else {
            models
        };
        serde_json::from_value(payload).unwrap()
    }

    fn fixture_entry(value: Option<i64>, paid: Option<i64>) -> serde_json::Value {
        let mut entry = serde_json::json!({ "billing_metric": "fixture", "billing_group": "unit" });
        if let Some(value) = value {
            entry["value"] = value.into();
        }
        if let Some(paid) = paid {
            entry["value_paid"] = paid.into();
        }
        entry
    }

    #[test]
    fn plan_covered_consumption_counts_tokens_separately_from_billed_spend() {
        for category in ["completion", "chat", "vibe_code"] {
            let billing = fixture_billing(
                category,
                serde_json::json!({
                    "input": [fixture_entry(Some(1000), Some(0))],
                    "output": [fixture_entry(Some(500), Some(200))],
                    "cached": [fixture_entry(Some(300), Some(0))],
                }),
            );
            let summary = MistralProvider::summarize_billing(billing).unwrap();
            assert_eq!(summary.total_input_tokens, 1000, "{category}");
            assert_eq!(summary.total_output_tokens, 500, "{category}");
            assert_eq!(summary.total_cached_tokens, 300, "{category}");
            assert_eq!(summary.model_count, 1, "{category}");
            assert_eq!(summary.total_cost, 50.0, "{category}");
        }
    }

    #[test]
    fn missing_consumed_or_billed_units_use_the_available_count() {
        for entry in [fixture_entry(Some(20), None), fixture_entry(None, Some(20))] {
            let billing = fixture_billing("completion", serde_json::json!({ "input": [entry] }));
            let summary = MistralProvider::summarize_billing(billing).unwrap();
            assert_eq!(summary.total_input_tokens, 20);
            assert_eq!(summary.total_cost, 5.0);
        }
    }

    #[test]
    fn categories_aggregate_together_and_count_each_model_entry_once() {
        let billing: BillingResponse = serde_json::from_value(serde_json::json!({
            "prices": [{ "billing_metric": "fixture", "billing_group": "unit", "price": "0.25" }],
            "completion": { "models": { "a": { "input": [fixture_entry(Some(10), Some(10))] } } },
            "chat": { "models": { "b": { "input": [fixture_entry(Some(20), Some(0))] } } },
            "vibe_code": { "completion": { "models": {
                "c": { "output": [fixture_entry(Some(30), Some(4))] }
            } } },
        }))
        .unwrap();
        let summary = MistralProvider::summarize_billing(billing).unwrap();
        assert_eq!(summary.total_input_tokens, 30);
        assert_eq!(summary.total_output_tokens, 30);
        assert_eq!(summary.model_count, 3);
        assert_eq!(summary.total_cost, 3.5);
    }

    #[test]
    fn cost_only_categories_do_not_sum_token_lanes() {
        let entries = serde_json::json!({
            "input": [fixture_entry(Some(i64::MAX), None), fixture_entry(Some(1), None)]
        });
        let billing: BillingResponse = serde_json::from_value(serde_json::json!({
            "prices": [{ "billing_metric": "fixture", "billing_group": "unit", "price": "0.25" }],
            "ocr": { "models": { "fixture": entries } },
        }))
        .unwrap();
        let summary = MistralProvider::summarize_billing(billing).unwrap();
        assert_eq!(summary.total_input_tokens, 0);
        assert_eq!(summary.model_count, 0);
        assert!(summary.total_cost > 0.0);
    }

    #[test]
    fn paid_zero_does_not_hide_an_unrepresentable_consumed_value() {
        let entry = fixture_entry(Some(i64::MAX), Some(0));
        let billing = fixture_billing(
            "completion",
            serde_json::json!({ "input": [entry.clone(), entry] }),
        );
        assert!(matches!(
            MistralProvider::summarize_billing(billing),
            Err(ProviderError::Parse(message)) if message.contains("token count")
        ));
    }

    #[test]
    fn consumed_totals_stay_checked_across_api_chat_and_vibe_categories() {
        let big = serde_json::json!({ "input": [fixture_entry(Some(i64::MAX), None)] });
        let one = serde_json::json!({ "input": [fixture_entry(Some(1), None)] });
        for mut payload in [
            serde_json::json!({ "chat": { "models": { "fixture": one } } }),
            serde_json::json!({ "vibe_code": { "completion": { "models": { "fixture": one } } } }),
        ] {
            payload["completion"] = serde_json::json!({ "models": { "fixture": big } });
            let billing: BillingResponse = serde_json::from_value(payload).unwrap();
            assert!(matches!(
                MistralProvider::summarize_billing(billing),
                Err(ProviderError::Parse(message)) if message.contains("token count")
            ));
        }
    }
}
