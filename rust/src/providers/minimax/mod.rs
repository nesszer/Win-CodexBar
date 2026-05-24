//! MiniMax provider implementation
//!
//! Fetches usage data from MiniMax AI API
//! MiniMax stores API keys locally or in environment

mod local_storage;

// Re-exports for local storage import
#[allow(unused_imports)]
pub use local_storage::{ImportError, MiniMaxLocalStorageImporter, MiniMaxSession};

use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const BILLING_HISTORY_URL: &str = "https://platform.minimaxi.com/account/amount";

#[derive(Debug, Deserialize)]
struct MiniMaxBillingHistoryPayload {
    #[serde(default)]
    base_resp: Option<MiniMaxBaseResponse>,
    #[serde(default)]
    charge_records: Vec<MiniMaxBillingRecord>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxBaseResponse {
    status_code: Option<i64>,
    status_msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxBillingRecord {
    consume_token: Option<serde_json::Value>,
    consume_input_token: Option<serde_json::Value>,
    consume_output_token: Option<serde_json::Value>,
    consume_cash: Option<serde_json::Value>,
    consume_cash_after_voucher: Option<serde_json::Value>,
    created_at: Option<serde_json::Value>,
    ymd: Option<String>,
    consume_time: Option<String>,
    method: Option<String>,
    model: Option<String>,
    result: Option<serde_json::Value>,
    status: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct MiniMaxBillingSummary {
    today_tokens: i64,
    last_30_days_tokens: i64,
    today_cash: Option<f64>,
    last_30_days_cash: Option<f64>,
    top_methods: Vec<MiniMaxBillingBreakdown>,
    top_models: Vec<MiniMaxBillingBreakdown>,
}

#[derive(Debug, Clone)]
struct MiniMaxBillingBreakdown {
    name: String,
    tokens: i64,
    cash: Option<f64>,
}

/// MiniMax API region
#[derive(Debug, Clone, Copy, PartialEq)]
enum MiniMaxRegion {
    Global,
    ChinaMainland,
}

/// MiniMax provider
pub struct MiniMaxProvider {
    metadata: ProviderMetadata,
}

impl MiniMaxProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::MiniMax,
                display_name: "MiniMax",
                session_label: "Usage",
                weekly_label: "Monthly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://platform.minimaxi.com/user-center"),
                status_page_url: None,
            },
        }
    }

    /// Get MiniMax config directory
    fn get_minimax_config_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            dirs::config_dir().map(|p| p.join("minimax"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            dirs::home_dir().map(|p| p.join(".minimax"))
        }
    }

    /// Read MiniMax API key
    async fn read_api_key(&self) -> Result<(String, String), ProviderError> {
        // Check environment variables first
        if let (Ok(group_id), Ok(api_key)) = (
            std::env::var("MINIMAX_GROUP_ID"),
            std::env::var("MINIMAX_API_KEY"),
        ) {
            return Ok((group_id, api_key));
        }

        // Check config file
        let config_path = Self::get_minimax_config_path()
            .ok_or_else(|| ProviderError::NotInstalled("MiniMax config not found".to_string()))?;

        let config_file = config_path.join("config.json");
        if config_file.exists() {
            let content = tokio::fs::read_to_string(&config_file)
                .await
                .map_err(|e| ProviderError::Other(e.to_string()))?;

            let json: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| ProviderError::Parse(e.to_string()))?;

            let group_id = json
                .get("group_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let api_key = json
                .get("api_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let (Some(gid), Some(key)) = (group_id, api_key) {
                return Ok((gid, key));
            }
        }

        Err(ProviderError::AuthRequired)
    }

    /// API base URLs for different regions
    fn api_base_url(region: MiniMaxRegion) -> &'static str {
        match region {
            MiniMaxRegion::Global => "https://api.minimax.io",
            MiniMaxRegion::ChinaMainland => "https://api.minimaxi.com",
        }
    }

    /// Fetch usage via MiniMax API with region fallback
    async fn fetch_via_web(&self) -> Result<ProviderFetchResult, ProviderError> {
        let (group_id, api_key) = self.read_api_key().await?;

        // Try global endpoint first, fall back to China mainland on 401/403
        match self
            .fetch_from_region(&group_id, &api_key, MiniMaxRegion::Global)
            .await
        {
            Ok(result) => Ok(result),
            Err(ProviderError::AuthRequired) => {
                // Retry with China mainland endpoint
                self.fetch_from_region(&group_id, &api_key, MiniMaxRegion::ChinaMainland)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    /// Fetch from a specific region endpoint
    async fn fetch_from_region(
        &self,
        group_id: &str,
        api_key: &str,
        region: MiniMaxRegion,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let base_url = Self::api_base_url(region);
        let resp = client
            .get(format!(
                "{}/v1/billing/usage?group_id={}",
                base_url, group_id
            ))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("MM-API-Source", "CodexBar")
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }

        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "MiniMax API returned status {}",
                resp.status()
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let usage = self.parse_usage_response(&json)?;
        Ok(self.result_with_optional_billing(usage, "api", &json))
    }

    fn parse_usage_response(
        &self,
        json: &serde_json::Value,
    ) -> Result<UsageSnapshot, ProviderError> {
        // Parse MiniMax billing response
        let base_resp = json.get("base_resp");
        if let Some(base) = base_resp {
            let status_code = base
                .get("status_code")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1);
            if status_code != 0 {
                return Err(ProviderError::Parse(
                    base.get("status_msg")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string(),
                ));
            }
        }

        let used_credits = json
            .get("used_amount")
            .or_else(|| json.get("total_amount"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let credit_limit = json
            .get("total_quota")
            .or_else(|| json.get("quota"))
            .and_then(|v| v.as_f64())
            .unwrap_or(100.0);

        let used_percent = if credit_limit > 0.0 {
            (used_credits / credit_limit) * 100.0
        } else {
            0.0
        };

        let plan = json
            .get("plan_name")
            .or_else(|| json.get("current_plan_title"))
            .or_else(|| json.get("current_subscribe_title"))
            .or_else(|| json.get("combo_title"))
            .or_else(|| json.pointer("/current_combo_card/title"))
            .or_else(|| json.get("plan_type"))
            .or_else(|| json.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("MiniMax");

        let usage = UsageSnapshot::new(RateWindow::new(used_percent)).with_login_method(plan);

        Ok(usage)
    }

    async fn fetch_billing_with_cookie(
        &self,
        cookie_header: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let response = client
            .get(BILLING_HISTORY_URL)
            .query(&[("page", "1"), ("limit", "100")])
            .header("Cookie", cookie_header)
            .header("Accept", "application/json, text/plain, */*")
            .header("X-Requested-With", "XMLHttpRequest")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "MiniMax billing returned status {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse MiniMax billing: {e}")))?;
        let summary = parse_billing_summary(&json)?;
        Ok(result_from_billing_summary(summary, "web-billing"))
    }

    fn result_with_optional_billing(
        &self,
        usage: UsageSnapshot,
        source_label: &str,
        json: &serde_json::Value,
    ) -> ProviderFetchResult {
        let Ok(summary) = parse_billing_summary(json) else {
            return ProviderFetchResult::new(usage, source_label);
        };
        attach_billing_summary(ProviderFetchResult::new(usage, source_label), summary)
    }

    /// Probe for MiniMax installation (credentials check)
    async fn probe_cli(&self) -> Result<UsageSnapshot, ProviderError> {
        // Check if API key is configured
        let has_env_vars = std::env::var("MINIMAX_API_KEY").is_ok();
        let has_config = Self::get_minimax_config_path()
            .map(|p| p.join("config.json").exists())
            .unwrap_or(false);

        if has_env_vars || has_config {
            let usage =
                UsageSnapshot::new(RateWindow::new(0.0)).with_login_method("MiniMax (configured)");
            Ok(usage)
        } else {
            Err(ProviderError::NotInstalled(
                "MiniMax API not configured. Set MINIMAX_API_KEY and MINIMAX_GROUP_ID environment variables".to_string()
            ))
        }
    }
}

fn parse_billing_summary(json: &serde_json::Value) -> Result<MiniMaxBillingSummary, ProviderError> {
    let payload: MiniMaxBillingHistoryPayload = serde_json::from_value(json.clone())
        .map_err(|e| ProviderError::Parse(format!("Failed to parse MiniMax billing: {e}")))?;
    if let Some(base) = payload.base_resp
        && let Some(status) = base.status_code
        && status != 0
    {
        return Err(ProviderError::Other(
            base.status_msg
                .unwrap_or_else(|| format!("MiniMax billing status {status}")),
        ));
    }
    if payload.charge_records.is_empty() {
        return Err(ProviderError::Parse(
            "MiniMax billing records not present".to_string(),
        ));
    }
    Ok(aggregate_billing(&payload.charge_records, Utc::now()))
}

fn aggregate_billing(
    records: &[MiniMaxBillingRecord],
    now: DateTime<Utc>,
) -> MiniMaxBillingSummary {
    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let window_start = today_start - Duration::days(29);
    let mut today_tokens = 0;
    let mut last_30_days_tokens = 0;
    let mut today_cash = 0.0;
    let mut today_has_cash = false;
    let mut last_30_days_cash = 0.0;
    let mut last_30_has_cash = false;
    let mut method_totals: HashMap<String, (i64, f64, bool)> = HashMap::new();
    let mut model_totals: HashMap<String, (i64, f64, bool)> = HashMap::new();

    for record in records {
        if !billing_record_succeeded(record) {
            continue;
        }
        let Some(date) = record_date(record) else {
            continue;
        };
        if date < window_start || date > now {
            continue;
        }
        let tokens = record_token_count(record);
        let cash = record_cash(record);
        last_30_days_tokens += tokens;
        if let Some(cash) = cash {
            last_30_days_cash += cash;
            last_30_has_cash = true;
        }
        if date >= today_start {
            today_tokens += tokens;
            if let Some(cash) = cash {
                today_cash += cash;
                today_has_cash = true;
            }
        }
        add_breakdown(&mut method_totals, record.method.as_deref(), tokens, cash);
        add_breakdown(&mut model_totals, record.model.as_deref(), tokens, cash);
    }

    MiniMaxBillingSummary {
        today_tokens,
        last_30_days_tokens,
        today_cash: today_has_cash.then_some(today_cash),
        last_30_days_cash: last_30_has_cash.then_some(last_30_days_cash),
        top_methods: top_breakdowns(method_totals),
        top_models: top_breakdowns(model_totals),
    }
}

fn billing_record_succeeded(record: &MiniMaxBillingRecord) -> bool {
    let status =
        scalar_string(record.result.as_ref()).or_else(|| scalar_string(record.status.as_ref()));
    match status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => true,
        Some(value) => value.eq_ignore_ascii_case("success"),
    }
}

fn result_from_billing_summary(
    summary: MiniMaxBillingSummary,
    source_label: &str,
) -> ProviderFetchResult {
    let usage = UsageSnapshot::new(RateWindow::with_details(
        0.0,
        None,
        None,
        Some(format!(
            "{} tokens today",
            format_count(summary.today_tokens)
        )),
    ))
    .with_secondary(RateWindow::with_details(
        0.0,
        None,
        None,
        Some(format!(
            "{} tokens over last 30 days",
            format_count(summary.last_30_days_tokens)
        )),
    ))
    .with_login_method("MiniMax billing");
    attach_billing_summary(ProviderFetchResult::new(usage, source_label), summary)
}

fn attach_billing_summary(
    mut result: ProviderFetchResult,
    summary: MiniMaxBillingSummary,
) -> ProviderFetchResult {
    result.usage = result.usage.with_extra_rate_window(
        "billing-tokens-today",
        "Tokens today",
        RateWindow::with_details(0.0, None, None, Some(format_count(summary.today_tokens))),
    );
    result.usage = result.usage.with_extra_rate_window(
        "billing-tokens-30d",
        "Tokens (30 days)",
        RateWindow::with_details(
            0.0,
            None,
            None,
            Some(format_count(summary.last_30_days_tokens)),
        ),
    );
    if let Some(cash) = summary.today_cash {
        result.usage = result.usage.with_extra_rate_window(
            "billing-cash-today",
            "Spend today",
            RateWindow::with_details(0.0, None, None, Some(format!("${cash:.2}"))),
        );
    }
    if let Some(cash) = summary.last_30_days_cash {
        result.usage = result.usage.with_extra_rate_window(
            "billing-cash-30d",
            "Spend (30 days)",
            RateWindow::with_details(0.0, None, None, Some(format!("${cash:.2}"))),
        );
        result.cost = Some(CostSnapshot::new(cash, "USD", "Last 30 days"));
    }
    for (idx, item) in summary.top_methods.iter().enumerate() {
        result.usage = result.usage.with_extra_rate_window(
            format!("billing-method-{idx}"),
            format!("Method: {}", item.name),
            RateWindow::with_details(0.0, None, None, Some(breakdown_description(item))),
        );
    }
    for (idx, item) in summary.top_models.iter().enumerate() {
        result.usage = result.usage.with_extra_rate_window(
            format!("billing-model-{idx}"),
            format!("Model: {}", item.name),
            RateWindow::with_details(0.0, None, None, Some(breakdown_description(item))),
        );
    }
    result
}

fn add_breakdown(
    totals: &mut HashMap<String, (i64, f64, bool)>,
    raw_name: Option<&str>,
    tokens: i64,
    cash: Option<f64>,
) {
    let Some(name) = raw_name.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let total = totals.entry(name.to_string()).or_default();
    total.0 += tokens;
    if let Some(cash) = cash {
        total.1 += cash;
        total.2 = true;
    }
}

fn top_breakdowns(totals: HashMap<String, (i64, f64, bool)>) -> Vec<MiniMaxBillingBreakdown> {
    let mut items: Vec<_> = totals
        .into_iter()
        .map(|(name, (tokens, cash, has_cash))| MiniMaxBillingBreakdown {
            name,
            tokens,
            cash: has_cash.then_some(cash),
        })
        .collect();
    items.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.name.cmp(&b.name)));
    items.truncate(3);
    items
}

fn breakdown_description(item: &MiniMaxBillingBreakdown) -> String {
    match item.cash {
        Some(cash) => format!("{} tokens / ${cash:.2}", format_count(item.tokens)),
        None => format!("{} tokens", format_count(item.tokens)),
    }
}

fn record_token_count(record: &MiniMaxBillingRecord) -> i64 {
    let direct = value_i64(record.consume_token.as_ref()).unwrap_or(0);
    if direct > 0 {
        return direct;
    }
    value_i64(record.consume_input_token.as_ref()).unwrap_or(0)
        + value_i64(record.consume_output_token.as_ref()).unwrap_or(0)
}

fn record_cash(record: &MiniMaxBillingRecord) -> Option<f64> {
    value_f64(record.consume_cash_after_voucher.as_ref())
        .or_else(|| value_f64(record.consume_cash.as_ref()))
}

fn record_date(record: &MiniMaxBillingRecord) -> Option<DateTime<Utc>> {
    if let Some(created_at) = value_i64(record.created_at.as_ref()) {
        let seconds = if created_at > 1_000_000_000_000 {
            created_at / 1000
        } else {
            created_at
        };
        return Utc.timestamp_opt(seconds, 0).single();
    }
    if let Some(ymd) = record.ymd.as_deref() {
        for format in ["%Y-%m-%d", "%Y%m%d", "%Y/%m/%d"] {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(ymd.trim(), format) {
                return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
            }
        }
    }
    if let Some(text) = record.consume_time.as_deref() {
        for format in ["%Y-%m-%d %H:%M:%S", "%Y/%m/%d %H:%M:%S"] {
            if let Ok(date) = chrono::NaiveDateTime::parse_from_str(text.trim(), format) {
                return Some(date.and_utc());
            }
        }
        if let Ok(date) = DateTime::parse_from_rfc3339(text.trim()) {
            return Some(date.with_timezone(&Utc));
        }
    }
    None
}

fn value_i64(value: Option<&serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(text) => text.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

fn value_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

fn scalar_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn format_count(value: i64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (idx, ch) in raw.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

impl Default for MiniMaxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MiniMaxProvider {
    fn id(&self) -> ProviderId {
        ProviderId::MiniMax
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching MiniMax usage");

        match ctx.source_mode {
            SourceMode::Auto => {
                if let Some(cookie_header) = ctx.manual_cookie_header.as_deref()
                    && let Ok(result) = self.fetch_billing_with_cookie(cookie_header).await
                {
                    return Ok(result);
                }
                if let Ok(result) = self.fetch_via_web().await {
                    return Ok(result);
                }
                let usage = self.probe_cli().await?;
                Ok(ProviderFetchResult::new(usage, "cli"))
            }
            SourceMode::Web => {
                if let Some(cookie_header) = ctx.manual_cookie_header.as_deref() {
                    return self.fetch_billing_with_cookie(cookie_header).await;
                }
                self.fetch_via_web().await
            }
            SourceMode::Cli => {
                let usage = self.probe_cli().await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_billing_history_records() {
        let records = vec![
            MiniMaxBillingRecord {
                consume_token: Some(serde_json::json!(1200)),
                consume_input_token: None,
                consume_output_token: None,
                consume_cash: Some(serde_json::json!("0.42")),
                consume_cash_after_voucher: None,
                created_at: None,
                ymd: Some("2026-12-16".to_string()),
                consume_time: None,
                method: Some("chat".to_string()),
                model: Some("abab6.5".to_string()),
                result: Some(serde_json::json!("SUCCESS")),
                status: None,
            },
            MiniMaxBillingRecord {
                consume_token: None,
                consume_input_token: Some(serde_json::json!(300)),
                consume_output_token: Some(serde_json::json!(500)),
                consume_cash: None,
                consume_cash_after_voucher: Some(serde_json::json!(0.21)),
                created_at: None,
                ymd: Some("2026-12-15".to_string()),
                consume_time: None,
                method: Some("completion".to_string()),
                model: Some("abab6.5".to_string()),
                result: None,
                status: None,
            },
        ];
        let now = Utc.with_ymd_and_hms(2026, 12, 16, 12, 0, 0).unwrap();
        let summary = aggregate_billing(&records, now);
        assert_eq!(summary.last_30_days_tokens, 2000);
        assert!((summary.last_30_days_cash.unwrap() - 0.63).abs() < 0.001);
        assert_eq!(summary.top_models[0].name, "abab6.5");
        assert_eq!(summary.top_models[0].tokens, 2000);
    }

    #[test]
    fn parses_billing_payload_into_result_extras() {
        let json = serde_json::json!({
            "base_resp": { "status_code": 0 },
            "charge_records": [{
                "consume_token": "2500",
                "consume_cash_after_voucher": "1.25",
                "ymd": Utc::now().format("%Y-%m-%d").to_string(),
                "method": "chat",
                "model": "abab6.5"
            }]
        });
        let summary = parse_billing_summary(&json).unwrap();
        let result = result_from_billing_summary(summary, "web-billing");
        assert!(result.cost.is_some());
        assert!(
            result
                .usage
                .extra_rate_windows
                .iter()
                .any(|window| window.id == "billing-tokens-30d")
        );
    }

    #[test]
    fn parses_plan_title_from_coding_plan_fields() {
        let provider = MiniMaxProvider::new();
        for (field, expected) in [
            ("plan_name", "MiniMax Star"),
            ("current_plan_title", "Coding Plan Pro"),
            ("current_subscribe_title", "Max"),
            ("combo_title", "Combo Star"),
        ] {
            let snapshot = provider
                .parse_usage_response(&serde_json::json!({
                    "base_resp": { "status_code": 0 },
                    field: expected,
                    "used_amount": 0,
                    "total_quota": 100
                }))
                .unwrap();
            assert_eq!(snapshot.login_method.as_deref(), Some(expected));
        }

        let snapshot = provider
            .parse_usage_response(&serde_json::json!({
                "base_resp": { "status_code": 0 },
                "current_combo_card": { "title": "Card Title" },
                "used_amount": 0,
                "total_quota": 100
            }))
            .unwrap();
        assert_eq!(snapshot.login_method.as_deref(), Some("Card Title"));
    }

    #[test]
    fn filters_failed_billing_records() {
        let records = vec![
            MiniMaxBillingRecord {
                consume_token: Some(serde_json::json!(1000)),
                consume_input_token: None,
                consume_output_token: None,
                consume_cash: None,
                consume_cash_after_voucher: None,
                created_at: None,
                ymd: Some("2026-05-17".to_string()),
                consume_time: None,
                method: Some("chat".to_string()),
                model: Some("MiniMax-M1".to_string()),
                result: Some(serde_json::json!("SUCCESS")),
                status: None,
            },
            MiniMaxBillingRecord {
                consume_token: Some(serde_json::json!(2000)),
                consume_input_token: None,
                consume_output_token: None,
                consume_cash: None,
                consume_cash_after_voucher: None,
                created_at: None,
                ymd: Some("2026-05-17".to_string()),
                consume_time: None,
                method: Some("chat".to_string()),
                model: Some("MiniMax-M1".to_string()),
                result: Some(serde_json::json!("FAILED")),
                status: None,
            },
            MiniMaxBillingRecord {
                consume_token: Some(serde_json::json!(3000)),
                consume_input_token: None,
                consume_output_token: None,
                consume_cash: None,
                consume_cash_after_voucher: None,
                created_at: None,
                ymd: Some("2026-05-17".to_string()),
                consume_time: None,
                method: Some("audio".to_string()),
                model: Some("speech".to_string()),
                result: None,
                status: None,
            },
            MiniMaxBillingRecord {
                consume_token: Some(serde_json::json!(4000)),
                consume_input_token: None,
                consume_output_token: None,
                consume_cash: None,
                consume_cash_after_voucher: None,
                created_at: None,
                ymd: Some("2026-05-17".to_string()),
                consume_time: None,
                method: Some("video".to_string()),
                model: Some("video".to_string()),
                result: None,
                status: Some(serde_json::json!(0)),
            },
        ];
        let now = Utc.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
        let summary = aggregate_billing(&records, now);
        assert_eq!(summary.today_tokens, 4000);
        assert_eq!(summary.last_30_days_tokens, 4000);
        assert_eq!(summary.top_methods.len(), 2);
        assert_eq!(summary.top_methods[0].name, "audio");
        assert_eq!(summary.top_methods[0].tokens, 3000);
        assert_eq!(summary.top_methods[1].name, "chat");
        assert_eq!(summary.top_methods[1].tokens, 1000);
    }
}
