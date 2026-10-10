//! MiniMax billing-history summary: parse, aggregate and attach as extra windows.

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Deserialize;
use std::collections::HashMap;

use super::{format_count, scalar_string, value_i64};
use crate::core::{CostSnapshot, ProviderError, ProviderFetchResult, RateWindow};
use crate::providers::json;

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
pub(super) struct MiniMaxBillingRecord {
    pub(super) consume_token: Option<serde_json::Value>,
    pub(super) consume_input_token: Option<serde_json::Value>,
    pub(super) consume_output_token: Option<serde_json::Value>,
    pub(super) consume_cash: Option<serde_json::Value>,
    pub(super) consume_cash_after_voucher: Option<serde_json::Value>,
    pub(super) created_at: Option<serde_json::Value>,
    pub(super) ymd: Option<String>,
    pub(super) consume_time: Option<String>,
    pub(super) method: Option<String>,
    pub(super) model: Option<String>,
    pub(super) result: Option<serde_json::Value>,
    pub(super) status: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub(super) struct MiniMaxBillingSummary {
    pub(super) today_tokens: i64,
    pub(super) last_30_days_tokens: i64,
    pub(super) today_cash: Option<f64>,
    pub(super) last_30_days_cash: Option<f64>,
    pub(super) top_methods: Vec<MiniMaxBillingBreakdown>,
    pub(super) top_models: Vec<MiniMaxBillingBreakdown>,
}

#[derive(Debug, Clone)]
pub(super) struct MiniMaxBillingBreakdown {
    pub(super) name: String,
    pub(super) tokens: i64,
    pub(super) cash: Option<f64>,
}

pub(super) fn parse_billing_summary(
    json: &serde_json::Value,
) -> Result<MiniMaxBillingSummary, ProviderError> {
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

pub(super) fn aggregate_billing(
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

pub(super) fn attach_billing_summary(
    mut result: ProviderFetchResult,
    summary: MiniMaxBillingSummary,
) -> ProviderFetchResult {
    let mut rows = vec![
        (
            "billing-tokens-today".to_string(),
            "Tokens today".to_string(),
            format_count(summary.today_tokens),
        ),
        (
            "billing-tokens-30d".to_string(),
            "Tokens (30 days)".to_string(),
            format_count(summary.last_30_days_tokens),
        ),
    ];
    if let Some(cash) = summary.today_cash {
        rows.push((
            "billing-cash-today".to_string(),
            "Spend today".to_string(),
            format!("${cash:.2}"),
        ));
    }
    if let Some(cash) = summary.last_30_days_cash {
        rows.push((
            "billing-cash-30d".to_string(),
            "Spend (30 days)".to_string(),
            format!("${cash:.2}"),
        ));
        result.cost = Some(CostSnapshot::new(cash, "USD", "Last 30 days"));
    }
    for (kind, label, items) in [
        ("method", "Method", &summary.top_methods),
        ("model", "Model", &summary.top_models),
    ] {
        for (idx, item) in items.iter().enumerate() {
            rows.push((
                format!("billing-{kind}-{idx}"),
                format!("{label}: {}", item.name),
                breakdown_description(item),
            ));
        }
    }
    for (id, title, detail) in rows {
        result.usage = result.usage.with_extra_rate_window(
            id,
            title,
            RateWindow::with_details(0.0, None, None, Some(detail)),
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
    json::lenient_f64(record.consume_cash_after_voucher.as_ref())
        .or_else(|| json::lenient_f64(record.consume_cash.as_ref()))
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
