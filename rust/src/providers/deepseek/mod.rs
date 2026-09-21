//! DeepSeek provider implementation.
//!
//! Fetches API account balance from DeepSeek's `/user/balance` endpoint.

use async_trait::async_trait;
use reqwest::Client;
use std::collections::{HashMap, HashSet};

use serde::Deserialize;

pub mod pricing;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com";
const DEEPSEEK_CREDENTIAL_TARGET: &str = "codexbar-deepseek";

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    #[serde(default)]
    is_available: bool,
    #[serde(default)]
    balance_infos: Vec<BalanceInfo>,
}

#[derive(Debug, Deserialize, Clone)]
struct BalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Deserialize)]
struct UsageEnvelope<T> {
    code: Option<FlexibleI64>,
    msg: Option<String>,
    data: Option<UsageEnvelopeData<T>>,
}

#[derive(Debug, Deserialize)]
struct UsageEnvelopeData<T> {
    biz_code: Option<FlexibleI64>,
    biz_data: Option<T>,
}

#[derive(Debug, Deserialize, Default)]
struct DeepSeekAmountData {
    #[serde(default)]
    total: Vec<DeepSeekAmountItem>,
    #[serde(default)]
    days: Vec<DeepSeekAmountDay>,
}

#[derive(Debug, Deserialize, Default)]
struct DeepSeekAmountDay {
    #[serde(default)]
    date: String,
    #[serde(default)]
    models: Vec<DeepSeekAmountItem>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct DeepSeekAmountItem {
    #[serde(default)]
    model: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    amount: FlexibleF64,
}

#[derive(Debug, Deserialize, Default)]
struct DeepSeekCostData {
    #[serde(default)]
    total: Vec<DeepSeekCostItem>,
    #[serde(default)]
    days: Vec<DeepSeekCostDay>,
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct DeepSeekCostDay {
    #[serde(default)]
    date: String,
    #[serde(default)]
    models: Vec<DeepSeekCostItem>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct DeepSeekCostItem {
    #[serde(default)]
    model: String,
    #[serde(default)]
    category: String,
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    cost: Option<FlexibleF64>,
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
struct FlexibleF64(#[serde(deserialize_with = "deserialize_f64")] f64);

#[derive(Debug, Deserialize, Clone, Copy)]
struct FlexibleI64(#[serde(deserialize_with = "deserialize_i64")] i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DeepSeekUsagePeriod {
    #[default]
    CurrentMonth,
}

impl DeepSeekUsagePeriod {
    fn label(self) -> &'static str {
        match self {
            Self::CurrentMonth => "Current month",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DeepSeekModelCost {
    model: String,
    cost: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct DeepSeekUsageSummary {
    today_tokens: f64,
    month_tokens: f64,
    today_requests: f64,
    month_requests: f64,
    today_cost: f64,
    month_cost: f64,
    top_model: Option<String>,
    category_tokens: Vec<(String, f64)>,
    model_costs: Vec<DeepSeekModelCost>,
    currency: String,
    period: DeepSeekUsagePeriod,
}

pub struct DeepSeekProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl DeepSeekProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::DeepSeek,
                display_name: "DeepSeek",
                session_label: "Balance",
                weekly_label: "Balance",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://platform.deepseek.com/usage"),
                status_page_url: Some("https://status.deepseek.com"),
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn get_api_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        if let Some(key) = api_key
            && !key.trim().is_empty()
        {
            return Ok(key.trim().to_string());
        }

        if let Ok(entry) = keyring::Entry::new(DEEPSEEK_CREDENTIAL_TARGET, "api_key")
            && let Ok(token) = entry.get_password()
            && !token.trim().is_empty()
        {
            return Ok(token);
        }

        for env in ["DEEPSEEK_API_KEY", "DEEPSEEK_KEY"] {
            if let Ok(token) = std::env::var(env)
                && !token.trim().is_empty()
            {
                return Ok(token);
            }
        }

        Err(ProviderError::NotInstalled(
            "DeepSeek API key not found. Set it in Preferences → Providers, DEEPSEEK_API_KEY, or DEEPSEEK_KEY."
                .to_string(),
        ))
    }

    async fn fetch_usage_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let api_key = Self::get_api_key(ctx.api_key.as_deref())?;

        let resp = self
            .client
            .get(format!("{DEEPSEEK_API_BASE}/user/balance"))
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Accept", "application/json")
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }
        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "DeepSeek API returned status {}",
                resp.status()
            )));
        }

        let balance: BalanceResponse = resp.json().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse DeepSeek balance response: {e}"))
        })?;
        let mut usage = Self::snapshot_from_balance(balance);
        let mut result = ProviderFetchResult::new(usage.clone(), "api");

        if let Ok(summary) = self.fetch_usage_summary(&api_key).await {
            usage = Self::apply_usage_summary(usage, &summary);
            result = ProviderFetchResult::new(usage, "api");
            if summary.month_cost > 0.0 || !summary.model_costs.is_empty() {
                let mut cost = CostSnapshot::new(
                    summary.month_cost,
                    &summary.currency,
                    summary.period.label(),
                );
                if let Some(symbol) = currency_symbol_for_cost(&summary.currency) {
                    cost = cost.with_currency_symbol(symbol);
                }
                result = result.with_cost(cost);
            }
        }

        Ok(result)
    }

    async fn fetch_usage_summary(
        &self,
        api_key: &str,
    ) -> Result<DeepSeekUsageSummary, ProviderError> {
        let amount: UsageEnvelope<DeepSeekAmountData> = self
            .fetch_usage_endpoint("/api/v0/usage/amount", api_key, "amount")
            .await?;
        let cost: UsageEnvelope<DeepSeekCostData> = self
            .fetch_usage_endpoint("/api/v0/usage/cost", api_key, "cost")
            .await?;
        DeepSeekUsageSummary::from_payloads(amount, cost)
    }

    async fn fetch_usage_endpoint<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        api_key: &str,
        label: &str,
    ) -> Result<T, ProviderError> {
        let resp = self
            .client
            .get(format!("https://platform.deepseek.com{path}"))
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Accept", "application/json")
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }
        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "DeepSeek usage {label} returned status {}",
                resp.status()
            )));
        }
        resp.json().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse DeepSeek usage {label}: {e}"))
        })
    }

    fn snapshot_from_balance(balance: BalanceResponse) -> UsageSnapshot {
        let selected = select_balance_info(&balance.balance_infos).cloned();

        let Some(info) = selected else {
            let mut window = RateWindow::new(100.0);
            window.reset_description = Some("No balance information returned".to_string());
            return UsageSnapshot::new(window).with_login_method("Balance unavailable");
        };

        let total = parse_money(&info.total_balance);
        let granted = parse_money(&info.granted_balance);
        let topped_up = parse_money(&info.topped_up_balance);
        let symbol = currency_symbol(&info.currency);

        let mut window = RateWindow::new(if !balance.is_available || total <= 0.0 {
            100.0
        } else {
            0.0
        });

        window.reset_description = if !balance.is_available {
            Some("Balance unavailable for API calls".to_string())
        } else if total <= 0.0 {
            Some(format!(
                "{symbol}0.00 — add credits at platform.deepseek.com"
            ))
        } else {
            Some(format!(
                "{symbol}{total:.2} (Paid: {symbol}{topped_up:.2} / Granted: {symbol}{granted:.2})"
            ))
        };

        UsageSnapshot::new(window).with_login_method(format!(
            "{} balance: {symbol}{total:.2}",
            info.currency.to_uppercase()
        ))
    }

    fn apply_usage_summary(
        mut usage: UsageSnapshot,
        summary: &DeepSeekUsageSummary,
    ) -> UsageSnapshot {
        if summary.today_tokens > 0.0 {
            usage = usage.with_extra_rate_window(
                "tokens-today",
                "Tokens today",
                RateWindow::with_details(
                    0.0,
                    Some(24 * 60),
                    None,
                    Some(format_count(summary.today_tokens)),
                ),
            );
        }
        if summary.month_tokens > 0.0 {
            usage = usage.with_extra_rate_window(
                "tokens-month",
                "Tokens (month)",
                RateWindow::with_details(0.0, None, None, Some(format_count(summary.month_tokens))),
            );
        }
        if summary.today_requests > 0.0 {
            usage = usage.with_extra_rate_window(
                "requests-today",
                "Requests today",
                RateWindow::with_details(
                    0.0,
                    Some(24 * 60),
                    None,
                    Some(format_count(summary.today_requests)),
                ),
            );
        }
        if summary.month_requests > 0.0 {
            usage = usage.with_extra_rate_window(
                "requests-month",
                "Requests (month)",
                RateWindow::with_details(
                    0.0,
                    None,
                    None,
                    Some(format_count(summary.month_requests)),
                ),
            );
        }
        if let Some(model) = summary.top_model.as_deref() {
            usage = usage.with_extra_rate_window(
                "top-model",
                "Top model",
                RateWindow::with_details(0.0, None, None, Some(model.to_string())),
            );
        }
        for (idx, (category, tokens)) in summary.category_tokens.iter().take(3).enumerate() {
            usage = usage.with_extra_rate_window(
                format!("category-{idx}"),
                format!("Category: {}", category_label(category)),
                RateWindow::with_details(0.0, None, None, Some(format_count(*tokens))),
            );
        }
        for (idx, model_cost) in summary.model_costs.iter().enumerate() {
            usage = usage.with_extra_rate_window(
                format!("deepseek-model-cost-{idx}"),
                format!("Spend: {}", model_cost.model),
                RateWindow::informational(format!(
                    "{} · {}",
                    format_currency_amount(model_cost.cost, &summary.currency),
                    summary.period.label()
                )),
            );
        }
        usage
    }
}

impl DeepSeekUsageSummary {
    fn from_payloads(
        amount: UsageEnvelope<DeepSeekAmountData>,
        cost: UsageEnvelope<DeepSeekCostData>,
    ) -> Result<Self, ProviderError> {
        validate_usage_envelope(&amount, "amount")?;
        validate_usage_envelope(&cost, "cost")?;

        let amount = amount
            .data
            .and_then(|data| data.biz_data)
            .ok_or_else(|| ProviderError::Parse("Missing DeepSeek amount data".to_string()))?;
        let cost = cost
            .data
            .and_then(|data| data.biz_data)
            .ok_or_else(|| ProviderError::Parse("Missing DeepSeek cost data".to_string()))?;

        let today = latest_amount_day(&amount.days);
        let today_cost_day = latest_cost_day(&cost.days);
        let mut model_tokens: HashMap<String, f64> = HashMap::new();
        let mut category_tokens: HashMap<String, f64> = HashMap::new();

        for item in &amount.total {
            if !item.model.trim().is_empty() {
                *model_tokens.entry(item.model.clone()).or_default() += item.amount.0;
            }
            if !item.category.trim().is_empty() {
                *category_tokens.entry(item.category.clone()).or_default() += item.amount.0;
            }
        }

        let top_model = model_tokens
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(model, _)| model);
        let mut category_tokens = category_tokens.into_iter().collect::<Vec<_>>();
        category_tokens.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut model_cost_totals = ModelCostTotals::default();
        for item in &cost.total {
            let model = item.model.trim();
            if model.is_empty() || is_request_category(&item.category) {
                continue;
            }
            let amount = is_known_cost_category(&item.category)
                .then(|| item.cost.map(|cost| cost.0))
                .flatten();
            model_cost_totals.add(amount, model);
        }

        Ok(Self {
            today_tokens: today.map(sum_non_request_amount).unwrap_or_default(),
            month_tokens: amount
                .total
                .iter()
                .filter(|item| !is_request_category(&item.category))
                .map(|item| item.amount.0)
                .sum(),
            today_requests: today.map(sum_request_amount).unwrap_or_default(),
            month_requests: amount
                .total
                .iter()
                .filter(|item| is_request_category(&item.category))
                .map(|item| item.amount.0)
                .sum(),
            today_cost: today_cost_day.map(sum_cost).unwrap_or_default(),
            month_cost: sum_cost(&cost.total),
            top_model,
            category_tokens,
            model_costs: model_cost_totals.values(),
            currency: cost
                .currency
                .as_deref()
                .map(str::trim)
                .filter(|currency| !currency.is_empty())
                .unwrap_or("USD")
                .to_string(),
            period: DeepSeekUsagePeriod::CurrentMonth,
        })
    }
}

impl Default for DeepSeekProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for DeepSeekProvider {
    fn id(&self) -> ProviderId {
        ProviderId::DeepSeek
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
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

fn parse_money(value: &str) -> f64 {
    value.parse::<f64>().unwrap_or(0.0)
}

fn select_balance_info(balance_infos: &[BalanceInfo]) -> Option<&BalanceInfo> {
    balance_infos
        .iter()
        .find(|info| {
            info.currency.eq_ignore_ascii_case("USD") && parse_money(&info.total_balance) > 0.0
        })
        .or_else(|| {
            balance_infos
                .iter()
                .find(|info| parse_money(&info.total_balance) > 0.0)
        })
        .or_else(|| {
            balance_infos
                .iter()
                .find(|info| info.currency.eq_ignore_ascii_case("USD"))
        })
        .or_else(|| balance_infos.first())
}

fn validate_usage_envelope<T>(
    envelope: &UsageEnvelope<T>,
    label: &str,
) -> Result<(), ProviderError> {
    if envelope
        .code
        .map(|code| code.0)
        .is_some_and(|code| code != 0)
    {
        return Err(ProviderError::Other(format!(
            "DeepSeek usage {label} error: {}",
            envelope
                .msg
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        )));
    }
    if envelope
        .data
        .as_ref()
        .and_then(|data| data.biz_code.map(|code| code.0))
        .is_some_and(|code| code != 0)
    {
        return Err(ProviderError::Other(format!(
            "DeepSeek usage {label} business error"
        )));
    }
    Ok(())
}

fn latest_amount_day(days: &[DeepSeekAmountDay]) -> Option<&[DeepSeekAmountItem]> {
    days.iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .map(|day| day.models.as_slice())
}

fn latest_cost_day(days: &[DeepSeekCostDay]) -> Option<&[DeepSeekCostItem]> {
    days.iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .map(|day| day.models.as_slice())
}

fn is_request_category(category: &str) -> bool {
    category.eq_ignore_ascii_case("REQUEST")
}

fn is_known_cost_category(category: &str) -> bool {
    matches!(
        category,
        "PROMPT_CACHE_HIT_TOKEN" | "PROMPT_CACHE_MISS_TOKEN" | "RESPONSE_TOKEN" | "REQUEST"
    )
}

fn sum_non_request_amount(items: &[DeepSeekAmountItem]) -> f64 {
    items
        .iter()
        .filter(|item| !is_request_category(&item.category))
        .map(|item| item.amount.0)
        .sum()
}

fn sum_request_amount(items: &[DeepSeekAmountItem]) -> f64 {
    items
        .iter()
        .filter(|item| is_request_category(&item.category))
        .map(|item| item.amount.0)
        .sum()
}

fn sum_cost(items: &[DeepSeekCostItem]) -> f64 {
    items
        .iter()
        .filter(|item| {
            is_known_cost_category(&item.category) && !is_request_category(&item.category)
        })
        .filter_map(|item| item.cost.map(|cost| cost.0))
        .filter(|cost| cost.is_finite() && *cost >= 0.0)
        .sum()
}

#[derive(Default)]
struct ModelCostTotals {
    totals: HashMap<String, f64>,
    unavailable: HashSet<String>,
}

impl ModelCostTotals {
    fn add(&mut self, amount: Option<f64>, model: &str) {
        let model = model.trim();
        if model.is_empty() || self.unavailable.contains(model) {
            return;
        }

        let Some(amount) = amount.filter(|amount| amount.is_finite() && *amount >= 0.0) else {
            self.unavailable.insert(model.to_string());
            self.totals.remove(model);
            return;
        };

        let total = self.totals.get(model).copied().unwrap_or_default() + amount;
        if total.is_finite() {
            self.totals.insert(model.to_string(), total);
        } else {
            self.unavailable.insert(model.to_string());
            self.totals.remove(model);
        }
    }

    fn values(self) -> Vec<DeepSeekModelCost> {
        let mut values = self
            .totals
            .into_iter()
            .map(|(model, cost)| DeepSeekModelCost { model, cost })
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .cost
                .partial_cmp(&left.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.model.cmp(&right.model))
        });
        values
    }
}

fn category_label(category: &str) -> String {
    match category {
        "PROMPT_CACHE_HIT_TOKEN" => "Prompt cache hit tokens".to_string(),
        "PROMPT_CACHE_MISS_TOKEN" => "Prompt cache miss tokens".to_string(),
        "RESPONSE_TOKEN" => "Response tokens".to_string(),
        "REQUEST" => "Requests".to_string(),
        other => other.replace('_', " ").to_lowercase(),
    }
}

fn format_count(value: f64) -> String {
    let rounded = value.round();
    if (value - rounded).abs() < 0.01 {
        // Guarded above: value is within 0.01 of a whole number, so the
        // fractional part is zero.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "whole-number guard above; fractional part is zero"
        )]
        let raw = (rounded as i64).to_string();
        let mut output = String::new();
        for (idx, ch) in raw.chars().rev().enumerate() {
            if idx > 0 && idx % 3 == 0 {
                output.push(',');
            }
            output.push(ch);
        }
        output.chars().rev().collect()
    } else {
        format!("{value:.2}")
    }
}

fn format_currency_amount(value: f64, currency: &str) -> String {
    match currency.trim().to_uppercase().as_str() {
        "CNY" | "RMB" => format!("¥{value:.4}"),
        "USD" | "" => format!("${value:.4}"),
        code => format!("{code} {value:.4}"),
    }
}

fn currency_symbol_for_cost(currency: &str) -> Option<&'static str> {
    match currency.trim().to_uppercase().as_str() {
        "CNY" | "RMB" => Some("¥"),
        "USD" => Some("$"),
        _ => None,
    }
}

fn parse_flexible_json_number(value: serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(value) => value.replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}

fn deserialize_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    parse_flexible_json_number(value).ok_or_else(|| serde::de::Error::custom("invalid number"))
}

fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<FlexibleF64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(parse_flexible_json_number(value).map(FlexibleF64))
}

fn deserialize_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("invalid integer")),
        serde_json::Value::String(s) => s
            .parse::<i64>()
            .map_err(|_| serde::de::Error::custom("invalid integer string")),
        _ => Ok(0),
    }
}

fn currency_symbol(currency: &str) -> &'static str {
    match currency.to_uppercase().as_str() {
        "CNY" | "RMB" => "¥",
        _ => "$",
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
