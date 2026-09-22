use chrono::Utc;
use reqwest::Client;
use std::time::Duration;
use uuid::Uuid;

use crate::core::{ProviderError, RateWindow, UsageSnapshot};

use super::{BASE_URL, USER_AGENT};

const SERVER_URL: &str = "https://opencode.ai/_server";
const WORKSPACES_SERVER_ID: &str =
    "def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f";
const BILLING_SERVER_ID: &str = "c83b78a614689c38ebee981f9b39a8b377716db85c1fd7dbab604adc02d3313d";

/// Authenticated legacy transport. Workspace discovery stays inside this
/// session so Console workspace IDs never cross into the independent legacy
/// cookie session.
pub(super) struct LegacySession<'a> {
    client: &'a Client,
    cookie_header: &'a str,
}

/// Parsed legacy usage response returned to the provider orchestrator.
pub(super) struct LegacyUsage {
    pub(super) usage: UsageSnapshot,
    pub(super) embedded_balance: Option<f64>,
}

/// Whether a failed Console request is eligible for the legacy route.
pub(super) fn can_recover(cookie_header: &str, error: &ProviderError) -> bool {
    if !super::console::has_legacy_cookie(cookie_header) {
        return false;
    }
    matches!(
        error,
        ProviderError::AuthRequired
            | ProviderError::Parse(_)
            | ProviderError::Other(_)
            | ProviderError::Timeout
    ) || error.is_transport_failure()
}

/// Resolve competing route failures without hiding a useful Console error.
pub(super) fn select_result<T>(
    cookie_header: &str,
    console_error: ProviderError,
    legacy_result: Result<T, ProviderError>,
) -> Result<T, ProviderError> {
    match legacy_result {
        Ok(value) => Ok(value),
        Err(_legacy_error)
            if super::console::has_console_cookie(cookie_header)
                && !matches!(console_error, ProviderError::AuthRequired) =>
        {
            Err(console_error)
        }
        Err(legacy_error) => Err(legacy_error),
    }
}

impl<'a> LegacySession<'a> {
    pub(super) fn new(client: &'a Client, cookie_header: &'a str) -> Self {
        Self {
            client,
            cookie_header,
        }
    }

    /// Discover the workspace owned by this legacy cookie session.
    pub(super) async fn discover_workspace_id(&self) -> Result<String, ProviderError> {
        let text = self
            .fetch_server_text(WORKSPACES_SERVER_ID, None, BASE_URL, None, "workspace API")
            .await?;
        parse_workspace_ids(&text)
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::Parse("No workspace ID found".to_string()))
    }

    /// Execute the complete legacy usage route, including route-local
    /// workspace discovery.
    pub(super) async fn fetch_usage(&self) -> Result<LegacyUsage, ProviderError> {
        let workspace_id = self.discover_workspace_id().await?;
        let url = format!("{BASE_URL}/workspace/{workspace_id}/go");
        let page = self.fetch_page_text(&url, None, "usage page").await?;
        Ok(LegacyUsage {
            usage: parse_usage_text(&page)?,
            embedded_balance: parse_zen_balance(&page),
        })
    }

    /// Execute the complete legacy balance route with a workspace discovered
    /// from the legacy cookie rather than the Console session.
    pub(super) async fn fetch_balance(
        &self,
        timeout: Duration,
    ) -> Result<Option<f64>, ProviderError> {
        let workspace_id = self.discover_workspace_id().await?;
        let referer = format!("{BASE_URL}/workspace/{workspace_id}");
        let page = self
            .fetch_page_text(&referer, Some(timeout), "Zen dashboard page")
            .await?;
        if let Some(balance) = parse_zen_balance(&page) {
            return Ok(Some(balance));
        }

        let args = serde_json::json!([workspace_id]).to_string();
        let billing = self
            .fetch_server_text(
                BILLING_SERVER_ID,
                Some(&args),
                &referer,
                Some(timeout),
                "billing API",
            )
            .await?;
        Ok(parse_billing_server_balance(&billing))
    }

    async fn fetch_page_text(
        &self,
        url: &str,
        timeout: Option<Duration>,
        what: &str,
    ) -> Result<String, ProviderError> {
        let mut request = self
            .client
            .get(url)
            .header("Cookie", self.cookie_header)
            .header("User-Agent", USER_AGENT)
            .header("Referer", BASE_URL)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            );
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!(
                "OpenCode Go {what} returned {status}"
            )));
        }
        let text = response.text().await?;
        if looks_signed_out(&text) {
            return Err(ProviderError::AuthRequired);
        }
        Ok(text)
    }

    async fn fetch_server_text(
        &self,
        server_id: &str,
        args: Option<&str>,
        referer: &str,
        timeout: Option<Duration>,
        what: &str,
    ) -> Result<String, ProviderError> {
        let mut url = reqwest::Url::parse(SERVER_URL).map_err(|error| {
            ProviderError::Parse(format!("Invalid OpenCode server URL: {error}"))
        })?;
        url.query_pairs_mut().append_pair("id", server_id);
        if let Some(args) = args {
            url.query_pairs_mut().append_pair("args", args);
        }
        let mut request = self
            .client
            .get(url)
            .header("Cookie", self.cookie_header)
            .header("X-Server-Id", server_id)
            .header("X-Server-Instance", format!("server-fn:{}", Uuid::new_v4()))
            .header("User-Agent", USER_AGENT)
            .header("Origin", BASE_URL)
            .header("Referer", referer)
            .header(
                "Accept",
                "text/javascript, application/json;q=0.9, */*;q=0.8",
            );
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!(
                "OpenCode Go {what} returned {status}"
            )));
        }
        let text = response.text().await?;
        if looks_signed_out(&text) {
            return Err(ProviderError::AuthRequired);
        }
        Ok(text)
    }
}

fn parse_workspace_ids(text: &str) -> Vec<String> {
    let Ok(re) = regex_lite::Regex::new(r#"(wrk_[A-Za-z0-9_-]+)"#) else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    for captures in re.captures_iter(text) {
        if let Some(value) = captures.get(1) {
            let value = value.as_str().to_string();
            if !seen.contains(&value) {
                seen.push(value);
            }
        }
    }
    seen
}

fn looks_signed_out(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("auth/authorize")
        || lower.contains("\"signin\"")
        || lower.contains("please sign in")
}

fn parse_usage_text(text: &str) -> Result<UsageSnapshot, ProviderError> {
    let now = Utc::now();
    let rolling = extract_window(text, &["rollingUsage", "rolling_usage", "rolling"])
        .ok_or_else(|| ProviderError::Parse("Missing rolling usage window".to_string()))?;
    let weekly = extract_window(text, &["weeklyUsage", "weekly_usage", "weekly"]);
    let monthly = extract_window(text, &["monthlyUsage", "monthly_usage", "monthly"]);

    let primary = RateWindow::with_details(
        rolling.0,
        Some(300),
        Some(now + chrono::Duration::seconds(rolling.1)),
        None,
    );
    let mut snapshot = UsageSnapshot::new(primary).with_login_method("OpenCode Go");
    if let Some((percent, reset)) = weekly {
        snapshot = snapshot.with_secondary(RateWindow::with_details(
            percent,
            Some(10080),
            Some(now + chrono::Duration::seconds(reset)),
            None,
        ));
    }
    if let Some((percent, reset)) = monthly {
        let resets_at = now + chrono::Duration::seconds(reset);
        snapshot = snapshot.with_tertiary(RateWindow::with_details(
            percent,
            RateWindow::monthly_window_minutes(Some(resets_at)).or(Some(43200)),
            Some(resets_at),
            None,
        ));
    }
    if let Some(renews_at) = super::super::extract_renewal(text) {
        snapshot = snapshot.with_extra_rate_window(
            "renewal",
            "Renews",
            RateWindow::with_details(0.0, None, Some(renews_at), None),
        );
    }
    Ok(snapshot)
}

fn extract_window(text: &str, names: &[&str]) -> Option<(f64, i64)> {
    for name in names {
        let percent_pattern = format!(
            r#"{}[^}}]*?(?:usagePercent|usedPercent|percentUsed|percent)\s*[:=]\s*([0-9]+(?:\.[0-9]+)?)"#,
            name
        );
        let reset_pattern = format!(
            r#"{}[^}}]*?(?:resetInSec|resetInSeconds|resetSeconds|resetSec)\s*[:=]\s*([0-9]+)"#,
            name
        );
        if let Some(percent) = super::super::extract_number(&percent_pattern, text) {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "resetInSec values are whole-second counts scraped as integral numbers"
            )]
            let reset = super::super::extract_number(&reset_pattern, text)
                .map(|number| number as i64)
                .unwrap_or(0);
            return Some((percent.clamp(0.0, 100.0), reset.max(0)));
        }

        let used_pattern = format!(
            r#"{}[^}}]*?(?:used|usage|consumed)\s*[:=]\s*([0-9]+(?:\.[0-9]+)?)"#,
            name
        );
        let limit_pattern = format!(
            r#"{}[^}}]*?(?:limit|total|allowance)\s*[:=]\s*([0-9]+(?:\.[0-9]+)?)"#,
            name
        );
        if let (Some(used), Some(limit)) = (
            super::super::extract_number(&used_pattern, text),
            super::super::extract_number(&limit_pattern, text),
        ) && limit > 0.0
        {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "resetInSec values are whole-second counts scraped as integral numbers"
            )]
            let reset = super::super::extract_number(&reset_pattern, text)
                .map(|number| number as i64)
                .unwrap_or(0);
            return Some((((used / limit) * 100.0).clamp(0.0, 100.0), reset.max(0)));
        }
    }
    None
}

fn parse_zen_balance(text: &str) -> Option<f64> {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(value) = find_balance_value(&json)
    {
        return Some(value);
    }
    let patterns = [
        r#"(?i)(?:current\s+balance|zen\s+balance|現在の残高)[^$]{0,80}\$\s*([0-9][0-9,]*(?:\.[0-9]+)?)"#,
        r#"(?i)(?:balance|残高)[\s\S]{0,120}?\$\s*([0-9][0-9,]*(?:\.[0-9]+)?)"#,
    ];
    patterns.iter().find_map(|pattern| {
        let re = regex_lite::Regex::new(pattern).ok()?;
        let raw = re.captures(text)?.get(1)?.as_str().replace(',', "");
        raw.parse::<f64>().ok()
    })
}

fn find_balance_value(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let normalized: String = key
                    .to_lowercase()
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .collect();
                if matches!(
                    normalized.as_str(),
                    "zenbalance"
                        | "zencurrentbalance"
                        | "currentbalance"
                        | "currentbalanceusd"
                        | "balanceusd"
                        | "usdbalance"
                ) {
                    if let Some(number) = value.as_f64() {
                        return Some(number);
                    }
                    if let Some(text) = value.as_str()
                        && let Ok(number) = text.trim().replace(',', "").parse()
                    {
                        return Some(number);
                    }
                }
                if let Some(found) = find_balance_value(value) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_balance_value),
        _ => None,
    }
}

fn parse_billing_server_balance(text: &str) -> Option<f64> {
    const BILLING_SCALE: f64 = 100_000_000.0;
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(raw) = find_raw_billing_balance(&json)
    {
        return Some(raw / BILLING_SCALE);
    }
    let customer_re = regex_lite::Regex::new(
        r#"(?:\"customerID\"|customerID)\s*:\s*(?:\$R\[\d+\]\s*=\s*)?\"[^\"]+\""#,
    )
    .ok()?;
    customer_re.find(text)?;
    let balance_re = regex_lite::Regex::new(
        r#"(?:\"balance\"|balance)\s*:\s*(?:\$R\[\d+\]\s*=\s*)?(-?[0-9]+(?:\.[0-9]+)?)"#,
    )
    .ok()?;
    let raw: f64 = balance_re
        .captures(text)?
        .get(1)?
        .as_str()
        .replace(',', "")
        .parse()
        .ok()?;
    Some(raw / BILLING_SCALE)
}

fn find_raw_billing_balance(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(balance) = map.get("balance") {
                let customer_ok = map
                    .get("customerID")
                    .and_then(|value| value.as_str())
                    .is_some_and(|id| !id.is_empty());
                if !customer_ok {
                    return None;
                }
                return billing_numeric_value(balance);
            }
            map.values().find_map(find_raw_billing_balance)
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_raw_billing_balance),
        _ => None,
    }
}

fn billing_numeric_value(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_recovery_requires_an_independent_legacy_cookie() {
        assert!(can_recover(
            "auth=legacy; __Host-console_session=console",
            &ProviderError::AuthRequired
        ));
        assert!(!can_recover(
            "__Host-console_session=console",
            &ProviderError::AuthRequired
        ));
        assert!(!can_recover(
            "auth=legacy",
            &ProviderError::NotInstalled("terminal".to_string())
        ));
    }

    #[test]
    fn failed_legacy_read_preserves_non_auth_console_failure() {
        let error = select_result::<()>(
            "auth=legacy; __Host-console_session=console",
            ProviderError::Other("console unavailable".to_string()),
            Err(ProviderError::AuthRequired),
        )
        .unwrap_err();
        assert!(matches!(error, ProviderError::Other(message) if message == "console unavailable"));

        let error = select_result::<()>(
            "auth=legacy; __Host-console_session=console",
            ProviderError::AuthRequired,
            Err(ProviderError::Parse("legacy payload missing".to_string())),
        )
        .unwrap_err();
        assert!(
            matches!(error, ProviderError::Parse(message) if message == "legacy payload missing")
        );
    }

    #[test]
    fn parses_workspace_ids_without_duplicates() {
        let text = r#"{ id: "wrk_abc123", name: "x" } { id: "wrk_def456" } { id: "wrk_abc123" }"#;
        assert_eq!(
            parse_workspace_ids(text),
            vec!["wrk_abc123".to_string(), "wrk_def456".to_string()]
        );
    }

    #[test]
    fn parses_usage_blocks() {
        let text = r#"
            rollingUsage: { usagePercent: 42.5, resetInSec: 3600 }
            weeklyUsage: { usagePercent: 13, resetInSec: 86400 }
            monthlyUsage: { usagePercent: 7, resetInSec: 2592000 }
        "#;
        let snapshot = parse_usage_text(text).unwrap();
        assert!((snapshot.primary.used_percent - 42.5).abs() < 0.001);
        assert!((snapshot.secondary.unwrap().used_percent - 13.0).abs() < 0.001);
        assert!((snapshot.tertiary.unwrap().used_percent - 7.0).abs() < 0.001);
    }

    #[test]
    fn sub_one_percent_computed_used_limit_is_not_rescaled() {
        let text = r#"
            rollingUsage: { used: 1, limit: 100, resetInSec: 600 }
            weeklyUsage: { used: 1, limit: 200, resetInSec: 86400 }
        "#;
        let snapshot = parse_usage_text(text).unwrap();
        assert!((snapshot.primary.used_percent - 1.0).abs() < 0.001);
        assert!((snapshot.secondary.unwrap().used_percent - 0.5).abs() < 0.001);
    }

    #[test]
    fn direct_percent_one_is_not_rescaled() {
        let text = r#"rollingUsage:$R[34]={status:"ok",resetInSec:13631,usagePercent:1} weeklyUsage:$R[35]={status:"ok",resetInSec:53863,usagePercent:15}"#;
        let snapshot = parse_usage_text(text).unwrap();
        assert!((snapshot.primary.used_percent - 1.0).abs() < 0.001);
        assert!((snapshot.secondary.unwrap().used_percent - 15.0).abs() < 0.001);
    }

    #[test]
    fn parses_renewal_window() {
        let text = r#"
            rollingUsage: { usagePercent: 42.5, resetInSec: 3600 }
            renewAt: "2026-06-01T12:00:00Z"
        "#;
        let snapshot = parse_usage_text(text).unwrap();
        let renewal = snapshot
            .extra_rate_windows
            .iter()
            .find(|window| window.id == "renewal")
            .expect("renewal window");
        assert_eq!(
            renewal.window.resets_at.unwrap().to_rfc3339(),
            "2026-06-01T12:00:00+00:00"
        );
    }

    #[test]
    fn billing_server_balance_needs_customer_marker() {
        assert_eq!(
            parse_billing_server_balance(r#"{"balance": 1500000000, "customerID": "cus_123"}"#),
            Some(15.0)
        );
        assert_eq!(
            parse_billing_server_balance(r#"{"balance": 1500000000}"#),
            None
        );
        assert_eq!(
            parse_billing_server_balance(r#"{"balance": true, "customerID": "cus_123"}"#),
            None
        );
    }

    #[test]
    fn billing_server_balance_handles_nesting_and_rsc_fragments() {
        assert_eq!(
            parse_billing_server_balance(r#"{"balance": "1,000,000,000", "customerID": "cus_1"}"#),
            Some(10.0)
        );
        assert_eq!(
            parse_billing_server_balance(
                r#"{"data": {"rows": [{"balance": 250000000, "customerID": "cus_2"}]}}"#
            ),
            Some(2.5)
        );
        assert_eq!(
            parse_billing_server_balance(r#"customerID:$R[1] = "cus_9"; "balance": -500000000"#),
            Some(-5.0)
        );
        assert_eq!(parse_billing_server_balance(r#"customerID: "cus_9""#), None);
    }
}
