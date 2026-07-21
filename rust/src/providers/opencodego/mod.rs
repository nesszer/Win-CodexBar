//! OpenCode Go provider implementation
//!
//! Separate workspace surface that shares the `opencode.ai` cookie domain with
//! the OpenCode provider. Resolves the workspace ID, then scrapes the `/go`
//! usage page for rolling/weekly/monthly windows.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use uuid::Uuid;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const BASE_URL: &str = "https://opencode.ai";
const SERVER_URL: &str = "https://opencode.ai/_server";
const WORKSPACES_SERVER_ID: &str =
    "def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub struct OpenCodeGoProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl OpenCodeGoProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::OpenCodeGo,
                display_name: "OpenCode Go",
                session_label: "Rolling",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://opencode.ai"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn workspace_id_from_context(workspace_id: Option<&str>) -> Option<&str> {
        workspace_id.filter(|id| !id.is_empty())
    }

    async fn fetch_workspace_id(&self, cookie_header: &str) -> Result<String, ProviderError> {
        let url = format!("{}?id={}", SERVER_URL, WORKSPACES_SERVER_ID);
        let response = self
            .client
            .get(&url)
            .header("Cookie", cookie_header)
            .header("X-Server-Id", WORKSPACES_SERVER_ID)
            .header("X-Server-Instance", format!("server-fn:{}", Uuid::new_v4()))
            .header("User-Agent", USER_AGENT)
            .header("Origin", BASE_URL)
            .header("Referer", BASE_URL)
            .header(
                "Accept",
                "text/javascript, application/json;q=0.9, */*;q=0.8",
            )
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!(
                "OpenCode workspace API returned {}",
                status
            )));
        }

        let text = response.text().await?;
        if Self::looks_signed_out(&text) {
            return Err(ProviderError::AuthRequired);
        }

        let ids = Self::parse_workspace_ids(&text);
        ids.into_iter()
            .next()
            .ok_or_else(|| ProviderError::Parse("No workspace ID found".to_string()))
    }

    async fn fetch_usage_page(
        &self,
        workspace_id: &str,
        cookie_header: &str,
    ) -> Result<String, ProviderError> {
        let url = format!("{}/workspace/{}/go", BASE_URL, workspace_id);
        let response = self
            .client
            .get(&url)
            .header("Cookie", cookie_header)
            .header("User-Agent", USER_AGENT)
            .header("Referer", BASE_URL)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!(
                "OpenCode Go usage page returned {}",
                status
            )));
        }

        let text = response.text().await?;
        if Self::looks_signed_out(&text) {
            return Err(ProviderError::AuthRequired);
        }
        Ok(text)
    }

    fn parse_usage_text(text: &str) -> Result<UsageSnapshot, ProviderError> {
        let now = Utc::now();

        let rolling = Self::extract_window(text, &["rollingUsage", "rolling_usage", "rolling"])
            .ok_or_else(|| ProviderError::Parse("Missing rolling usage window".to_string()))?;
        let weekly = Self::extract_window(text, &["weeklyUsage", "weekly_usage", "weekly"]);
        let monthly = Self::extract_window(text, &["monthlyUsage", "monthly_usage", "monthly"]);

        let primary = RateWindow::with_details(
            rolling.0,
            Some(300),
            Some(now + chrono::Duration::seconds(rolling.1)),
            None,
        );
        let mut snap = UsageSnapshot::new(primary).with_login_method("OpenCode Go");

        if let Some((pct, reset)) = weekly {
            snap = snap.with_secondary(RateWindow::with_details(
                pct,
                Some(10080),
                Some(now + chrono::Duration::seconds(reset)),
                None,
            ));
        }

        if let Some((pct, reset)) = monthly {
            snap = snap.with_tertiary(RateWindow::with_details(
                pct,
                Some(43200),
                Some(now + chrono::Duration::seconds(reset)),
                None,
            ));
        }

        if let Some(renews_at) = Self::extract_renewal(text) {
            snap = snap.with_extra_rate_window(
                "renewal",
                "Renews",
                RateWindow::with_details(0.0, None, Some(renews_at), None),
            );
        }

        Ok(snap)
    }

    /// Extract `(percent, resetInSec)` for a usage block by name.
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

            let percent = Self::extract_number(&percent_pattern, text);
            if let Some(p) = percent {
                let reset = Self::extract_number(&reset_pattern, text)
                    .map(|n| n as i64)
                    .unwrap_or(0);
                // Regex path only matches direct percent field names — fraction
                // heuristic is safe here (upstream #2331). used/limit computed
                // percents must not use this path without a separate gate.
                let p = if (0.0..=1.0).contains(&p) { p * 100.0 } else { p };
                return Some((p.clamp(0.0, 100.0), reset.max(0)));
            }

            // Computed used/limit (already 0..100) — do not apply fraction *100.
            let used_pattern = format!(
                r#"{}[^}}]*?(?:used|usage|consumed)\s*[:=]\s*([0-9]+(?:\.[0-9]+)?)"#,
                name
            );
            let limit_pattern = format!(
                r#"{}[^}}]*?(?:limit|total|allowance)\s*[:=]\s*([0-9]+(?:\.[0-9]+)?)"#,
                name
            );
            if let (Some(used), Some(limit)) = (
                Self::extract_number(&used_pattern, text),
                Self::extract_number(&limit_pattern, text),
            ) {
                if limit > 0.0 {
                    let reset = Self::extract_number(&reset_pattern, text)
                        .map(|n| n as i64)
                        .unwrap_or(0);
                    let p = (used / limit) * 100.0;
                    return Some((p.clamp(0.0, 100.0), reset.max(0)));
                }
            }
        }
        None
    }

    fn extract_number(pattern: &str, text: &str) -> Option<f64> {
        let re = regex_lite::Regex::new(pattern).ok()?;
        re.captures(text)?.get(1)?.as_str().parse().ok()
    }

    fn extract_renewal(text: &str) -> Option<DateTime<Utc>> {
        let re = regex_lite::Regex::new(
            r#"(?:"renewAt"|"renew_at"|renewAt|renew_at)\s*[:=]\s*"?([^",}\s]+)"?"#,
        )
        .ok()?;
        let raw = re.captures(text)?.get(1)?.as_str();
        Self::date_from_text(raw)
    }

    fn date_from_text(raw: &str) -> Option<DateTime<Utc>> {
        let text = raw.trim();
        if text.is_empty() {
            return None;
        }
        if let Ok(number) = text.parse::<f64>() {
            return Self::date_from_timestamp(number);
        }
        DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }

    fn date_from_timestamp(number: f64) -> Option<DateTime<Utc>> {
        if !number.is_finite() || number <= 0.0 {
            return None;
        }
        let seconds = if number > 10_000_000_000.0 {
            number / 1000.0
        } else {
            number
        };
        DateTime::<Utc>::from_timestamp(seconds as i64, 0)
    }

    fn parse_workspace_ids(text: &str) -> Vec<String> {
        let pattern = r#"(wrk_[A-Za-z0-9_-]+)"#;
        let re = match regex_lite::Regex::new(pattern) {
            Ok(r) => r,
            Err(_) => return vec![],
        };
        let mut seen = Vec::new();
        for caps in re.captures_iter(text) {
            if let Some(m) = caps.get(1) {
                let s = m.as_str().to_string();
                if !seen.contains(&s) {
                    seen.push(s);
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

    fn parse_zen_balance(text: &str) -> Option<f64> {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
            && let Some(value) = Self::find_balance_value(&json)
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
                        .filter(|c| c.is_ascii_alphanumeric())
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
                    if let Some(found) = Self::find_balance_value(value) {
                        return Some(found);
                    }
                }
                None
            }
            serde_json::Value::Array(items) => items.iter().find_map(Self::find_balance_value),
            _ => None,
        }
    }

    async fn fetch_with_cookies(
        &self,
        cookie_header: &str,
        workspace_id_override: Option<&str>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let workspace_id = match Self::workspace_id_from_context(workspace_id_override) {
            Some(workspace_id) => workspace_id.to_string(),
            None => self.fetch_workspace_id(cookie_header).await?,
        };
        let page = self.fetch_usage_page(&workspace_id, cookie_header).await?;
        let mut usage = Self::parse_usage_text(&page)?;
        let balance = Self::parse_zen_balance(&page);
        if let Some(balance) = balance {
            usage = usage.with_extra_rate_window(
                "zen-balance",
                "Zen balance",
                RateWindow::with_details(0.0, None, None, Some(format!("${balance:.2}"))),
            );
        }
        let mut result = ProviderFetchResult::new(usage, "web");
        if let Some(balance) = balance {
            result = result.with_cost(CostSnapshot::new(balance, "USD", "Zen balance"));
        }
        Ok(result)
    }
}

impl Default for OpenCodeGoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OpenCodeGoProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenCodeGo
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching OpenCode Go usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                if let Some(ref cookie_header) = ctx.manual_cookie_header {
                    return self
                        .fetch_with_cookies(cookie_header, ctx.workspace_id.as_deref())
                        .await;
                }

                match crate::providers::browser_cookie_header(&["opencode.ai"]) {
                    Ok(cookie_header) => match self
                        .fetch_with_cookies(&cookie_header, ctx.workspace_id.as_deref())
                        .await
                    {
                        Ok(result) => return Ok(result),
                        Err(ProviderError::AuthRequired) => {}
                        Err(e) => return Err(e),
                    },
                    Err(ProviderError::NoCookies) => {}
                    Err(e) => return Err(e),
                }

                Err(ProviderError::AuthRequired)
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

    fn supports_cli(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspace_ids() {
        let text = r#"{ id: "wrk_abc123", name: "x" } { id: "wrk_def456" }"#;
        let ids = OpenCodeGoProvider::parse_workspace_ids(text);
        assert_eq!(
            ids,
            vec!["wrk_abc123".to_string(), "wrk_def456".to_string()]
        );
    }

    #[test]
    fn uses_context_workspace_id_before_discovery() {
        assert_eq!(
            OpenCodeGoProvider::workspace_id_from_context(Some("wrk_override")),
            Some("wrk_override")
        );
        assert_eq!(
            OpenCodeGoProvider::workspace_id_from_context(Some("")),
            None
        );
    }

    #[test]
    fn sub_one_percent_computed_used_limit_is_not_rescaled() {
        let text = r#"
            rollingUsage: { used: 1, limit: 100, resetInSec: 600 }
            weeklyUsage: { used: 1, limit: 200, resetInSec: 86400 }
        "#;
        let snap = OpenCodeGoProvider::parse_usage_text(text).unwrap();
        assert!((snap.primary.used_percent - 1.0).abs() < 0.001);
        assert!((snap.secondary.as_ref().unwrap().used_percent - 0.5).abs() < 0.001);
    }

    #[test]
    fn parses_usage_blocks() {
        let text = r#"
            rollingUsage: { usagePercent: 42.5, resetInSec: 3600 }
            weeklyUsage: { usagePercent: 0.13, resetInSec: 86400 }
            monthlyUsage: { usagePercent: 7, resetInSec: 2592000 }
        "#;
        let snap = OpenCodeGoProvider::parse_usage_text(text).unwrap();
        assert!((snap.primary.used_percent - 42.5).abs() < 0.001);
        let secondary = snap.secondary.expect("weekly");
        // usagePercent: 0.13 is a direct fraction → 13%
        assert!((secondary.used_percent - 13.0).abs() < 0.001);
        let tertiary = snap.tertiary.expect("monthly");
        assert!((tertiary.used_percent - 7.0).abs() < 0.001);
    }

    #[test]
    fn parses_renewal_window() {
        let text = r#"
            rollingUsage: { usagePercent: 42.5, resetInSec: 3600 }
            weeklyUsage: { usagePercent: 50, resetInSec: 86400 }
            renewAt: "2026-06-01T12:00:00Z"
        "#;
        let snap = OpenCodeGoProvider::parse_usage_text(text).unwrap();
        let renewal = snap
            .extra_rate_windows
            .iter()
            .find(|window| window.id == "renewal")
            .expect("renewal window");
        assert_eq!(renewal.title, "Renews");
        assert_eq!(
            renewal.window.resets_at.unwrap().to_rfc3339(),
            "2026-06-01T12:00:00+00:00"
        );
    }
}
