use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use crate::core::{ProviderError, RateWindow, UsageSnapshot};

use super::USER_AGENT;

const CONSOLE_WORKSPACES_URL: &str = "https://opencode.ai/console/api/orgs";
const CONSOLE_GO_STATUS_URL: &str = "https://opencode.ai/console/api/go/status";
const CONSOLE_BILLING_STATUS_URL: &str = "https://opencode.ai/console/api/billing/status";
const CONSOLE_WORKSPACE_HEADER: &str = "x-org-id";
const BILLING_SCALE: f64 = 100_000_000.0;

pub(super) enum ConsoleUsage {
    Snapshot(Box<UsageSnapshot>),
    NoSubscription,
}

pub(super) fn normalize_workspace_id(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if is_workspace_id(raw) {
        return Some(raw.to_string());
    }
    let url = reqwest::Url::parse(raw).ok()?;
    if url.scheme() != "https" || url.host_str() != Some("opencode.ai") {
        return None;
    }
    let segments: Vec<_> = url.path_segments()?.collect();
    let candidate = segments
        .windows(2)
        .find_map(|parts| matches!(parts[0], "console" | "workspace").then_some(parts[1]))?;
    is_workspace_id(candidate).then(|| candidate.to_string())
}

fn is_workspace_id(value: &str) -> bool {
    let Some(suffix) = value
        .strip_prefix("wrk_")
        .or_else(|| value.strip_prefix("org_"))
    else {
        return false;
    };
    !suffix.is_empty()
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub(super) async fn fetch_workspace_id(
    client: &Client,
    cookie_header: &str,
    timeout: Duration,
) -> Result<String, ProviderError> {
    let text = fetch_text(
        client,
        CONSOLE_WORKSPACES_URL,
        None,
        cookie_header,
        timeout,
        "workspace list",
    )
    .await?;
    parse_workspace_ids(&text)
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::Parse("Missing OpenCode Console workspace ID".to_string()))
}

pub(super) async fn fetch_usage(
    client: &Client,
    workspace_id: &str,
    cookie_header: &str,
    timeout: Duration,
) -> Result<ConsoleUsage, ProviderError> {
    let text = fetch_text(
        client,
        CONSOLE_GO_STATUS_URL,
        Some(workspace_id),
        cookie_header,
        timeout,
        "Go status",
    )
    .await?;
    parse_usage(&text, Utc::now())
}

pub(super) async fn fetch_balance(
    client: &Client,
    workspace_id: &str,
    cookie_header: &str,
    timeout: Duration,
) -> Result<Option<f64>, ProviderError> {
    let text = fetch_text(
        client,
        CONSOLE_BILLING_STATUS_URL,
        Some(workspace_id),
        cookie_header,
        timeout,
        "billing status",
    )
    .await?;
    parse_balance(&text)
}

async fn fetch_text(
    client: &Client,
    url: &str,
    workspace_id: Option<&str>,
    cookie_header: &str,
    timeout: Duration,
    what: &str,
) -> Result<String, ProviderError> {
    let mut request = client
        .get(url)
        .timeout(timeout)
        .header("Cookie", cookie_header)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json");
    if let Some(workspace_id) = workspace_id {
        request = request.header(CONSOLE_WORKSPACE_HEADER, workspace_id);
    }
    let response = request.send().await?;
    let status = response.status();
    if status.as_u16() == 401 {
        return Err(ProviderError::AuthRequired);
    }
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "OpenCode Console {what} returned {status}"
        )));
    }
    response.text().await.map_err(ProviderError::Network)
}

fn parse_workspace_ids(text: &str) -> Vec<String> {
    let Ok(Value::Array(rows)) = serde_json::from_str(text) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| row.get("id")?.as_str())
        .filter(|id| is_workspace_id(id))
        .map(str::to_string)
        .collect()
}

fn parse_usage(text: &str, _now: DateTime<Utc>) -> Result<ConsoleUsage, ProviderError> {
    let root: Value = serde_json::from_str(text)
        .map_err(|_| ProviderError::Parse("Invalid OpenCode Console usage payload".to_string()))?;
    if root.is_null() || root.get("access").is_some_and(Value::is_null) {
        return Ok(ConsoleUsage::NoSubscription);
    }
    let access = root
        .get("access")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ProviderError::Parse("Invalid OpenCode Console usage payload".to_string())
        })?;
    let meters = access
        .get("meters")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ProviderError::Parse("Invalid OpenCode Console usage payload".to_string())
        })?;
    let rolling = meters.get("fiveHour").ok_or_else(|| {
        ProviderError::Parse("Invalid OpenCode Console usage payload".to_string())
    })?;
    let ends_at = access.get("endsAt").and_then(date_value);
    let primary = meter_window(rolling, Some(300), None)?;
    let mut snapshot = UsageSnapshot::new(primary).with_login_method("OpenCode Go");

    if let Some(weekly) = meters.get("week") {
        snapshot = snapshot.with_secondary(meter_window(weekly, Some(10_080), None)?);
    }
    if let Some(monthly) = meters.get("month") {
        let resets_at = monthly.get("resetsAt").and_then(date_value).or(ends_at);
        let minutes = RateWindow::monthly_window_minutes(resets_at).or(Some(43_200));
        snapshot = snapshot.with_tertiary(meter_window(monthly, minutes, resets_at)?);
    }
    if let Some(ends_at) = ends_at {
        snapshot = snapshot.with_extra_rate_window(
            "renewal",
            "Renews",
            RateWindow::with_details(0.0, None, Some(ends_at), None),
        );
    }
    Ok(ConsoleUsage::Snapshot(Box::new(snapshot)))
}

fn meter_window(
    meter: &Value,
    window_minutes: Option<u32>,
    reset_override: Option<DateTime<Utc>>,
) -> Result<RateWindow, ProviderError> {
    let object = meter
        .as_object()
        .ok_or_else(|| ProviderError::Parse("Invalid OpenCode Console usage meter".to_string()))?;
    let used = numeric_value(object.get("usedMicroCents"))
        .ok_or_else(|| ProviderError::Parse("Missing OpenCode Console usage amount".to_string()))?;
    let limit = numeric_value(object.get("limitMicroCents"))
        .filter(|value| *value > 0.0)
        .ok_or_else(|| ProviderError::Parse("Missing OpenCode Console usage limit".to_string()))?;
    let resets_at = reset_override.or_else(|| object.get("resetsAt").and_then(date_value));
    Ok(RateWindow::with_details(
        ((used / limit) * 100.0).clamp(0.0, 100.0),
        window_minutes,
        resets_at,
        None,
    ))
}

fn parse_balance(text: &str) -> Result<Option<f64>, ProviderError> {
    let root: Value = serde_json::from_str(text).map_err(|_| {
        ProviderError::Parse("Invalid OpenCode Console billing payload".to_string())
    })?;
    let billing_mode = root
        .get("billingMode")
        .and_then(Value::as_str)
        .filter(|mode| matches!(*mode, "prepaid" | "legacy" | "seat" | "credit"))
        .ok_or_else(|| {
            ProviderError::Parse("Invalid OpenCode Console billing payload".to_string())
        })?;
    let mode = root
        .get("mode")
        .and_then(Value::as_str)
        .filter(|mode| matches!(*mode, "pay-as-you-go" | "invoiceable"))
        .ok_or_else(|| {
            ProviderError::Parse("Invalid OpenCode Console billing payload".to_string())
        })?;
    if billing_mode != "prepaid" || mode != "pay-as-you-go" {
        return Ok(None);
    }
    let raw = root
        .get("balanceMicroCents")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::Parse("Missing OpenCode Console balance".to_string()))?;
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProviderError::Parse(
            "Invalid OpenCode Console balance".to_string(),
        ));
    }
    let balance = raw
        .parse::<f64>()
        .map_err(|_| ProviderError::Parse("Invalid OpenCode Console balance".to_string()))?;
    if !balance.is_finite() {
        return Err(ProviderError::Parse(
            "Invalid OpenCode Console balance".to_string(),
        ));
    }
    Ok(Some(balance / BILLING_SCALE))
}

fn numeric_value(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(text) => text.parse::<f64>().ok().filter(|value| value.is_finite()),
        _ => None,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated Unix timestamps are narrowed only after selecting seconds and nanoseconds"
)]
fn date_value(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(number) = numeric_value(Some(value)) {
        let seconds = if number > 1_000_000_000_000.0 {
            number / 1000.0
        } else {
            number
        };
        let whole = seconds.trunc() as i64;
        let nanos = ((seconds.fract()) * 1_000_000_000.0).round() as u32;
        return Utc.timestamp_opt(whole, nanos).single();
    }
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-20T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn usage_json(five_hour_reset: &str) -> String {
        format!(
            r#"{{"access":{{"endsAt":"2026-10-19T00:00:00Z","meters":{{"fiveHour":{{"resetsAt":{five_hour_reset},"limitMicroCents":"1200000000","usedMicroCents":"300000000"}},"week":{{"resetsAt":"2026-09-21T00:00:00Z","limitMicroCents":"3000000000","usedMicroCents":"1200000000"}},"month":{{"limitMicroCents":"6000000000","usedMicroCents":"600000000"}}}}}}}}"#
        )
    }

    #[test]
    fn parses_console_workspace_ids_and_urls() {
        assert_eq!(
            parse_workspace_ids(r#"[{"id":"wrk_ONE"},{"id":"org_TWO"},{"id":"acc_BAD"}]"#),
            vec!["wrk_ONE", "org_TWO"]
        );
        assert_eq!(
            normalize_workspace_id(Some("https://opencode.ai/console/org_TWO/go")).as_deref(),
            Some("org_TWO")
        );
        assert_eq!(
            normalize_workspace_id(Some("https://opencode.ai/workspace/wrk_ONE/go")).as_deref(),
            Some("wrk_ONE")
        );
        assert_eq!(
            normalize_workspace_id(Some("https://example.com/console/org_TWO/go")),
            None
        );
    }

    #[test]
    fn parses_console_microcent_usage_and_nullable_resets() {
        let ConsoleUsage::Snapshot(snapshot) = parse_usage(&usage_json("null"), now()).unwrap()
        else {
            panic!("expected usage snapshot");
        };
        assert!((snapshot.primary.used_percent - 25.0).abs() < 0.001);
        assert_eq!(snapshot.primary.resets_at, None);
        assert!((snapshot.secondary.unwrap().used_percent - 40.0).abs() < 0.001);
        let monthly = snapshot.tertiary.unwrap();
        assert!((monthly.used_percent - 10.0).abs() < 0.001);
        assert_eq!(
            monthly.resets_at.unwrap().to_rfc3339(),
            "2026-10-19T00:00:00+00:00"
        );
    }

    #[test]
    fn distinguishes_no_subscription_from_malformed_usage() {
        assert!(matches!(
            parse_usage("null", now()),
            Ok(ConsoleUsage::NoSubscription)
        ));
        assert!(matches!(
            parse_usage(r#"{"access":null}"#, now()),
            Ok(ConsoleUsage::NoSubscription)
        ));
        assert!(matches!(
            parse_usage(r#"{"access":{}}"#, now()),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn parses_only_explicit_prepaid_payg_balances() {
        let payload =
            r#"{"billingMode":"prepaid","mode":"pay-as-you-go","balanceMicroCents":"2786781005"}"#;
        let balance = parse_balance(payload).unwrap().unwrap();
        assert!((balance - 27.867_810_05).abs() < 0.000_001);
        assert_eq!(
            parse_balance(r#"{"billingMode":"seat","mode":"pay-as-you-go"}"#).unwrap(),
            None
        );
        assert!(
            parse_balance(
                r#"{"billingMode":"prepaid","mode":"pay-as-you-go","balanceMicroCents":"1.5"}"#
            )
            .is_err()
        );
    }
}
