//! Kilo provider implementation
//!
//! Fetches usage data from Kilo tRPC API using either:
//! - KILO_API_KEY (API mode)
//! - ~/.local/share/kilo/auth.json kilo.access token (CLI mode)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const KILO_API_BASE: &str = "https://app.kilo.ai/api/trpc";
const KILO_API_ENV_KEY: &str = "KILO_API_KEY";
const PROCEDURES: [&str; 3] = [
    "user.getCreditBlocks",
    "kiloPass.getState",
    "user.getAutoTopUpPaymentMethod",
];

pub struct KiloProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl KiloProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Kilo,
                display_name: "Kilo",
                session_label: "Credits",
                weekly_label: "Kilo Pass",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://app.kilo.ai/account/usage"),
                status_page_url: None,
            },
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_auto(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        if let Some(token) = self.resolve_api_token(ctx) {
            match self.fetch_with_token(&token, "api").await {
                Ok(result) => return Ok(result),
                Err(ProviderError::AuthRequired) => {
                    tracing::debug!("Kilo API token unauthorized, falling back to CLI session");
                }
                Err(other) => return Err(other),
            }
        }

        let cli_token = self.read_cli_token()?;
        self.fetch_with_token(&cli_token, "cli").await
    }

    fn resolve_api_token(&self, ctx: &FetchContext) -> Option<String> {
        if let Some(api_key) = ctx.api_key.as_deref().and_then(clean_value) {
            return Some(api_key);
        }
        std::env::var(KILO_API_ENV_KEY)
            .ok()
            .and_then(|v| clean_value(&v))
    }

    fn read_cli_token(&self) -> Result<String, ProviderError> {
        let path = kilo_auth_file_path();
        if !path.exists() {
            return Err(ProviderError::Other(format!(
                "Kilo CLI session not found at {}. Run `kilo login`.",
                path.display()
            )));
        }

        let data = std::fs::read(&path).map_err(|_| {
            ProviderError::Other(format!(
                "Kilo CLI session unreadable at {}. Check file permissions.",
                path.display()
            ))
        })?;

        parse_cli_auth_token(&data).ok_or_else(|| {
            ProviderError::Other(format!(
                "Kilo CLI session invalid at {}. Run `kilo login` again.",
                path.display()
            ))
        })
    }

    async fn fetch_with_token(
        &self,
        token: &str,
        source_label: &'static str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let url = make_batch_url().map_err(|e| ProviderError::Parse(e.to_string()))?;
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await?;

        match response.status().as_u16() {
            200 => {}
            401 | 403 => return Err(ProviderError::AuthRequired),
            404 => {
                return Err(ProviderError::Other(
                    "Kilo API endpoint not found (404).".to_string(),
                ))
            }
            500..=599 => {
                return Err(ProviderError::Other(format!(
                    "Kilo API unavailable (HTTP {}).",
                    response.status().as_u16()
                )))
            }
            _ => {
                return Err(ProviderError::Other(format!(
                    "Kilo API request failed (HTTP {}).",
                    response.status().as_u16()
                )))
            }
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        let usage = parse_snapshot(&payload)?;
        Ok(ProviderFetchResult::new(usage, source_label))
    }
}

impl Default for KiloProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for KiloProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Kilo
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Kilo usage");
        match ctx.source_mode {
            SourceMode::Auto => self.fetch_auto(ctx).await,
            SourceMode::OAuth => {
                let token = self.resolve_api_token(ctx).ok_or_else(|| {
                    ProviderError::Other(
                        "Kilo API credentials missing. Set KILO_API_KEY.".to_string(),
                    )
                })?;
                self.fetch_with_token(&token, "api").await
            }
            SourceMode::Cli => {
                let token = self.read_cli_token()?;
                self.fetch_with_token(&token, "cli").await
            }
            SourceMode::Web => Err(ProviderError::UnsupportedSource(SourceMode::Web)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth, SourceMode::Cli]
    }

    fn supports_web(&self) -> bool {
        false
    }

    fn supports_cli(&self) -> bool {
        true
    }
}

fn kilo_auth_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("kilo")
        .join("auth.json")
}

fn parse_cli_auth_token(data: &[u8]) -> Option<String> {
    let root: Value = serde_json::from_slice(data).ok()?;
    root.get("kilo")
        .and_then(|k| k.get("access"))
        .and_then(|v| v.as_str())
        .and_then(clean_value)
}

fn clean_value(raw: &str) -> Option<String> {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return None;
    }

    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value.remove(0);
        value.pop();
        value = value.trim().to_string();
    }

    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn make_batch_url() -> anyhow::Result<Url> {
    let mut url = Url::parse(&format!(
        "{}/{}",
        KILO_API_BASE.trim_end_matches('/'),
        PROCEDURES.join(",")
    ))?;

    let input = json!({
        "0": { "json": null },
        "1": { "json": null },
        "2": { "json": null }
    });

    url.query_pairs_mut()
        .append_pair("batch", "1")
        .append_pair("input", &input.to_string());
    Ok(url)
}

fn parse_snapshot(root: &Value) -> Result<UsageSnapshot, ProviderError> {
    let entries = response_entries_by_index(root)?;

    let mut payload_by_proc: HashMap<&str, &Value> = HashMap::new();
    for (idx, procedure) in PROCEDURES.iter().enumerate() {
        if let Some(entry) = entries.get(&idx) {
            if let Some(err) = trpc_error(entry) {
                if *procedure == "user.getAutoTopUpPaymentMethod" {
                    continue;
                }
                return Err(err);
            }
            if let Some(payload) = result_payload(entry) {
                payload_by_proc.insert(*procedure, payload);
            }
        }
    }

    let (credits_used, credits_total) =
        credit_fields(payload_by_proc.get("user.getCreditBlocks").copied());
    let pass = pass_fields(payload_by_proc.get("kiloPass.getState").copied());
    let plan_name = plan_name(payload_by_proc.get("kiloPass.getState").copied());
    let (auto_top_up_enabled, auto_top_up_method) = auto_top_up_state(
        payload_by_proc.get("user.getCreditBlocks").copied(),
        payload_by_proc
            .get("user.getAutoTopUpPaymentMethod")
            .copied(),
    );

    let mut primary = RateWindow::new(0.0);
    if let (Some(used), Some(total)) = (credits_used, credits_total) {
        let used_percent = if total > 0.0 {
            ((used / total) * 100.0).clamp(0.0, 100.0)
        } else {
            100.0
        };
        primary = RateWindow::with_details(
            used_percent,
            None,
            None,
            Some(format!("{}/{} credits", compact(used), compact(total))),
        );
    }

    let mut usage = UsageSnapshot::new(primary);
    if let Some(pass_window) = pass {
        usage = usage.with_secondary(pass_window);
    }

    if let Some(login_method) = login_method(plan_name, auto_top_up_enabled, auto_top_up_method) {
        usage = usage.with_login_method(login_method);
    }

    Ok(usage)
}

fn response_entries_by_index<'a>(
    root: &'a Value,
) -> Result<HashMap<usize, &'a Value>, ProviderError> {
    if let Some(entries) = root.as_array() {
        return Ok(entries.iter().enumerate().take(PROCEDURES.len()).collect());
    }

    if let Some(dict) = root.as_object() {
        if dict.contains_key("result") || dict.contains_key("error") {
            let mut out = HashMap::new();
            out.insert(0, root);
            return Ok(out);
        }

        let mut indexed = HashMap::new();
        for (k, v) in dict {
            if let Ok(index) = k.parse::<usize>() {
                if index < PROCEDURES.len() {
                    indexed.insert(index, v);
                }
            }
        }

        if !indexed.is_empty() {
            return Ok(indexed);
        }
    }

    Err(ProviderError::Parse(
        "Unexpected Kilo tRPC batch response shape".to_string(),
    ))
}

fn trpc_error(entry: &Value) -> Option<ProviderError> {
    let err = entry.get("error")?;
    let code = string_path(err, &["json", "data", "code"])
        .or_else(|| string_path(err, &["data", "code"]))
        .or_else(|| string_path(err, &["code"]));
    let msg = string_path(err, &["json", "message"]).or_else(|| string_path(err, &["message"]));
    let combined = format!(
        "{} {}",
        code.unwrap_or_default().to_lowercase(),
        msg.unwrap_or_default().to_lowercase()
    );

    if combined.contains("unauthorized") || combined.contains("forbidden") {
        return Some(ProviderError::AuthRequired);
    }
    if combined.contains("not_found") || combined.contains("not found") {
        return Some(ProviderError::Other(
            "Kilo API endpoint/procedure not found.".to_string(),
        ));
    }

    Some(ProviderError::Parse("Kilo tRPC error payload".to_string()))
}

fn result_payload<'a>(entry: &'a Value) -> Option<&'a Value> {
    let result = entry.get("result")?;

    if let Some(data_obj) = result.get("data").and_then(Value::as_object) {
        if let Some(json_payload) = data_obj.get("json") {
            if json_payload.is_null() {
                return None;
            }
            return Some(json_payload);
        }
        return Some(result.get("data")?);
    }

    let json_payload = result.get("json")?;
    if json_payload.is_null() {
        None
    } else {
        Some(json_payload)
    }
}

fn credit_fields(payload: Option<&Value>) -> (Option<f64>, Option<f64>) {
    let Some(payload) = payload else {
        return (None, None);
    };

    if let Some(blocks) = find_array(payload, "creditBlocks") {
        let mut total = 0.0;
        let mut remaining = 0.0;
        let mut saw_total = false;
        let mut saw_remaining = false;

        for item in blocks {
            if let Some(obj) = item.as_object() {
                if let Some(amount) = obj.get("amount_mUsd").and_then(as_f64) {
                    total += amount / 1_000_000.0;
                    saw_total = true;
                }
                if let Some(balance) = obj.get("balance_mUsd").and_then(as_f64) {
                    remaining += balance / 1_000_000.0;
                    saw_remaining = true;
                }
            }
        }

        if saw_total || saw_remaining {
            let total = if saw_total {
                Some(total.max(0.0))
            } else {
                None
            };
            let remaining = if saw_remaining {
                Some(remaining.max(0.0))
            } else {
                None
            };
            let used = match (total, remaining) {
                (Some(t), Some(r)) => Some((t - r).max(0.0)),
                _ => None,
            };
            return (used, total);
        }
    }

    if let Some(balance_musd) = find_number(payload, &["totalBalance_mUsd"]) {
        let total = (balance_musd / 1_000_000.0).max(0.0);
        return (Some(0.0), Some(total));
    }

    let used = find_number(
        payload,
        &["used", "usedCredits", "creditsUsed", "consumed", "spent"],
    );
    let mut total = find_number(payload, &["total", "totalCredits", "creditsTotal", "limit"]);
    let remaining = find_number(
        payload,
        &["remaining", "remainingCredits", "creditsRemaining"],
    );
    if total.is_none() {
        if let (Some(u), Some(r)) = (used, remaining) {
            total = Some(u + r);
        }
    }

    (used, total)
}

fn pass_fields(payload: Option<&Value>) -> Option<RateWindow> {
    let payload = payload?;
    let subscription = find_object(payload, "subscription").unwrap_or(payload);

    let mut used = subscription.get("currentPeriodUsageUsd").and_then(as_f64);
    let mut total = subscription
        .get("currentPeriodBaseCreditsUsd")
        .and_then(as_f64);
    let mut bonus = subscription
        .get("currentPeriodBonusCreditsUsd")
        .and_then(as_f64)
        .unwrap_or(0.0)
        .max(0.0);

    if used.is_none() && total.is_none() {
        used = find_money(payload, &["used", "spent", "consumed", "creditsUsed"]);
        total = find_money(payload, &["total", "limit", "creditsTotal", "totalCredits"]);
        bonus = find_money(payload, &["bonus", "bonusAmount", "bonusCredits"]).unwrap_or(0.0);
    }

    let total = total.map(|t| (t + bonus).max(0.0))?;
    let used = used.unwrap_or(0.0).max(0.0);
    let used_percent = if total > 0.0 {
        ((used / total) * 100.0).clamp(0.0, 100.0)
    } else {
        100.0
    };

    let resets_at = subscription
        .get("nextBillingAt")
        .and_then(parse_datetime)
        .or_else(|| subscription.get("nextRenewalAt").and_then(parse_datetime))
        .or_else(|| subscription.get("renewsAt").and_then(parse_datetime))
        .or_else(|| subscription.get("renewAt").and_then(parse_datetime));

    let base = (total - bonus).max(0.0);
    let mut detail = format!("${:.2} / ${:.2}", used, base);
    if bonus > 0.0 {
        detail.push_str(&format!(" (+ ${:.2} bonus)", bonus));
    }

    Some(RateWindow::with_details(
        used_percent,
        None,
        resets_at,
        Some(detail),
    ))
}

fn plan_name(payload: Option<&Value>) -> Option<String> {
    let payload = payload?;
    let sub = find_object(payload, "subscription").unwrap_or(payload);

    if let Some(tier) = sub.get("tier").and_then(Value::as_str).map(str::trim) {
        return Some(match tier {
            "tier_19" => "Starter".to_string(),
            "tier_49" => "Pro".to_string(),
            "tier_199" => "Expert".to_string(),
            other if !other.is_empty() => other.to_string(),
            _ => "Kilo Pass".to_string(),
        });
    }

    find_string(
        payload,
        &[
            "planName",
            "tierName",
            "passName",
            "subscriptionName",
            "name",
        ],
    )
}

fn auto_top_up_state(
    credit_payload: Option<&Value>,
    auto_payload: Option<&Value>,
) -> (Option<bool>, Option<String>) {
    let enabled = find_bool(
        auto_payload.unwrap_or(&Value::Null),
        &["enabled", "isEnabled", "active"],
    )
    .or_else(|| find_status_bool(auto_payload.unwrap_or(&Value::Null)))
    .or_else(|| {
        find_bool(
            credit_payload.unwrap_or(&Value::Null),
            &["autoTopUpEnabled"],
        )
    });

    let method = find_string(
        auto_payload.unwrap_or(&Value::Null),
        &["paymentMethod", "paymentMethodType", "method", "cardBrand"],
    );

    (enabled, method)
}

fn login_method(
    plan_name: Option<String>,
    auto_enabled: Option<bool>,
    auto_method: Option<String>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(plan) = plan_name.and_then(|p| clean_value(&p)) {
        parts.push(plan);
    }

    if let Some(enabled) = auto_enabled {
        if enabled {
            if let Some(method) = auto_method.and_then(|m| clean_value(&m)) {
                parts.push(format!("Auto top-up: {}", method));
            } else {
                parts.push("Auto top-up: enabled".to_string());
            }
        } else {
            parts.push("Auto top-up: off".to_string());
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn compact(v: f64) -> String {
    if (v - v.trunc()).abs() < f64::EPSILON {
        format!("{}", v as i64)
    } else {
        format!("{:.2}", v)
    }
}

fn string_path<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cursor = v;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    cursor.as_str()
}

fn find_array<'a>(v: &'a Value, key: &str) -> Option<&'a [Value]> {
    if let Some(arr) = v.get(key).and_then(Value::as_array) {
        return Some(arr.as_slice());
    }
    if let Some(obj) = v.as_object() {
        for value in obj.values() {
            if let Some(found) = find_array(value, key) {
                return Some(found);
            }
        }
    }
    if let Some(arr) = v.as_array() {
        for value in arr {
            if let Some(found) = find_array(value, key) {
                return Some(found);
            }
        }
    }
    None
}

fn find_object<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(obj) = v.get(key) {
        if obj.is_object() {
            return Some(obj);
        }
    }
    if let Some(map) = v.as_object() {
        for value in map.values() {
            if let Some(found) = find_object(value, key) {
                return Some(found);
            }
        }
    }
    if let Some(arr) = v.as_array() {
        for value in arr {
            if let Some(found) = find_object(value, key) {
                return Some(found);
            }
        }
    }
    None
}

fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|n| n as f64))
        .or_else(|| v.as_u64().map(|n| n as f64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

fn find_number(v: &Value, keys: &[&str]) -> Option<f64> {
    if let Some(obj) = v.as_object() {
        for key in keys {
            if let Some(found) = obj.get(*key).and_then(as_f64) {
                return Some(found);
            }
        }
        for value in obj.values() {
            if let Some(found) = find_number(value, keys) {
                return Some(found);
            }
        }
    }
    if let Some(arr) = v.as_array() {
        for value in arr {
            if let Some(found) = find_number(value, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn find_string(v: &Value, keys: &[&str]) -> Option<String> {
    if let Some(obj) = v.as_object() {
        for key in keys {
            if let Some(found) = obj.get(*key).and_then(Value::as_str).and_then(clean_value) {
                return Some(found);
            }
        }
        for value in obj.values() {
            if let Some(found) = find_string(value, keys) {
                return Some(found);
            }
        }
    }
    if let Some(arr) = v.as_array() {
        for value in arr {
            if let Some(found) = find_string(value, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn find_bool(v: &Value, keys: &[&str]) -> Option<bool> {
    if let Some(obj) = v.as_object() {
        for key in keys {
            if let Some(value) = obj.get(*key) {
                if let Some(found) = parse_bool(value) {
                    return Some(found);
                }
            }
        }
        for value in obj.values() {
            if let Some(found) = find_bool(value, keys) {
                return Some(found);
            }
        }
    }
    if let Some(arr) = v.as_array() {
        for value in arr {
            if let Some(found) = find_bool(value, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn find_status_bool(v: &Value) -> Option<bool> {
    let status = find_string(v, &["status"])?;
    match status.to_lowercase().as_str() {
        "enabled" | "active" | "on" => Some(true),
        "disabled" | "inactive" | "off" | "none" => Some(false),
        _ => None,
    }
}

fn find_money(v: &Value, plain_keys: &[&str]) -> Option<f64> {
    if let Some(cents) = find_number(
        v,
        &["amountCents", "totalCents", "usedCents", "remainingCents"],
    ) {
        return Some(cents / 100.0);
    }
    if let Some(musd) = find_number(
        v,
        &["amount_mUsd", "total_mUsd", "used_mUsd", "remaining_mUsd"],
    ) {
        return Some(musd / 1_000_000.0);
    }
    find_number(v, plain_keys)
}

fn parse_bool(v: &Value) -> Option<bool> {
    v.as_bool().or_else(|| {
        v.as_str()
            .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "enabled" | "on" => Some(true),
                "false" | "0" | "no" | "disabled" | "off" => Some(false),
                _ => None,
            })
    })
}

fn parse_datetime(v: &Value) -> Option<DateTime<Utc>> {
    if let Some(s) = v.as_str() {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s.trim()) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(num) = s.trim().parse::<f64>() {
            return epoch_to_datetime(num);
        }
    }
    if let Some(num) = as_f64(v) {
        return epoch_to_datetime(num);
    }
    None
}

fn epoch_to_datetime(raw: f64) -> Option<DateTime<Utc>> {
    let seconds = if raw.abs() > 10_000_000_000.0 {
        raw / 1000.0
    } else {
        raw
    };
    DateTime::from_timestamp(seconds as i64, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_auth_token() {
        let data = br#"{"kilo":{"access":"  token-123  "}}"#;
        let token = parse_cli_auth_token(data);
        assert_eq!(token.as_deref(), Some("token-123"));
    }

    #[test]
    fn parses_snapshot_with_credit_blocks_and_pass_state() {
        let payload = json!([
          {
            "result": {
              "data": {
                "json": {
                  "creditBlocks": [
                    { "amount_mUsd": 10000000, "balance_mUsd": 2500000 }
                  ]
                }
              }
            }
          },
          {
            "result": {
              "data": {
                "json": {
                  "subscription": {
                    "currentPeriodUsageUsd": 12.5,
                    "currentPeriodBaseCreditsUsd": 20.0,
                    "currentPeriodBonusCreditsUsd": 5.0,
                    "tier": "tier_49",
                    "nextBillingAt": "2030-01-01T00:00:00Z"
                  }
                }
              }
            }
          },
          {
            "result": {
              "data": {
                "json": {
                  "enabled": true,
                  "paymentMethod": "visa"
                }
              }
            }
          }
        ]);

        let usage = parse_snapshot(&payload).expect("snapshot should parse");
        assert!((usage.primary.used_percent - 75.0).abs() < 0.01);
        assert!(usage.secondary.is_some());
        assert_eq!(
            usage.login_method.as_deref(),
            Some("Pro · Auto top-up: visa")
        );
    }

    #[test]
    fn unauthorized_trpc_errors_map_to_auth_required() {
        let payload = json!([
            {
                "error": {
                    "json": {
                        "data": { "code": "UNAUTHORIZED" },
                        "message": "unauthorized"
                    }
                }
            }
        ]);

        let err = parse_snapshot(&payload).expect_err("expected auth error");
        assert!(matches!(err, ProviderError::AuthRequired));
    }
}
