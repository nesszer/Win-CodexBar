//! ai& (aiand) provider — spend-only API-key usage from request logs.
//!
//! Ported from steipete/CodexBar `AiAndUsageFetcher` (upstream 0.45 #2256):
//! `GET https://api.aiand.com/logs?range=30days&limit=100` with Bearer auth,
//! paginating up to 10 pages via `after` / `after_id` cursors.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const LOGS_URL: &str = "https://api.aiand.com/logs";
const PAGE_LIMIT: u32 = 100;
const MAX_PAGES: usize = 10;
const CREDENTIAL_TARGET: &str = "codexbar-aiand";
const ENV_KEYS: &[&str] = &["AIAND_API_KEY"];

#[derive(Debug, Deserialize, Clone)]
struct LogsEnvelope {
    #[serde(default)]
    data: Vec<LogRow>,
    #[serde(default)]
    has_more: Option<bool>,
    #[serde(default)]
    next_after: Option<String>,
    #[serde(default)]
    next_after_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct LogRow {
    cost: Option<String>,
    currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct AiAndSpend {
    amount: f64,
    currency_code: String,
}

#[derive(Debug, Clone, PartialEq)]
struct AiAndSnapshot {
    last_30_days_spend: Option<AiAndSpend>,
    /// False when the page cap was hit before the end of the 30-day window.
    is_complete: bool,
}

impl AiAndSnapshot {
    fn summarize(rows: &[LogRow], is_complete: bool) -> Self {
        // Rows arrive newest-first; the newest priced row decides display currency.
        let mut currency: Option<String> = None;
        let mut total = 0.0_f64;
        for row in rows {
            let Some(raw_cost) = row.cost.as_deref() else {
                continue;
            };
            let Ok(cost) = raw_cost.trim().parse::<f64>() else {
                continue;
            };
            if !cost.is_finite() {
                continue;
            }
            let Some(row_currency) = row
                .currency
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
            else {
                continue;
            };
            if currency.is_none() {
                currency = Some(row_currency.clone());
            }
            if currency.as_deref() != Some(row_currency.as_str()) {
                continue;
            }
            total += cost;
        }
        Self {
            last_30_days_spend: currency.map(|code| AiAndSpend {
                amount: total,
                currency_code: code.to_ascii_uppercase(),
            }),
            is_complete,
        }
    }

    fn period_label(&self) -> &'static str {
        if self.is_complete {
            "Last 30 days"
        } else {
            "Last 30 days (partial)"
        }
    }

    fn to_usage_snapshot(&self) -> UsageSnapshot {
        let detail = match &self.last_30_days_spend {
            Some(spend) => format!(
                "{} {:.2} · {}",
                spend.currency_code,
                spend.amount,
                self.period_label()
            ),
            None => format!("No priced requests · {}", self.period_label()),
        };
        // UsageSnapshot requires a primary window; spend is informational only.
        let primary = RateWindow::informational(detail.clone());
        UsageSnapshot::new(primary).with_login_method(detail)
    }

    fn to_cost_snapshot(&self) -> Option<CostSnapshot> {
        self.last_30_days_spend.as_ref().map(|spend| {
            CostSnapshot::new(
                spend.amount,
                spend.currency_code.clone(),
                self.period_label(),
            )
        })
    }
}

pub struct AiAndProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl AiAndProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::AiAnd,
                display_name: "ai&",
                session_label: "Spend",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://console.aiand.com"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn resolve_api_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        let raw = crate::providers::resolve_api_key(api_key, CREDENTIAL_TARGET, ENV_KEYS)?;
        clean_api_key(&raw).ok_or_else(|| {
            ProviderError::NotInstalled(
                "Missing ai& API key. Add one in Settings or set AIAND_API_KEY.".to_string(),
            )
        })
    }

    async fn fetch_usage_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let api_key = Self::resolve_api_key(ctx.api_key.as_deref())?;
        let snapshot = self.fetch_logs_snapshot(&api_key).await?;
        let mut result = ProviderFetchResult::new(snapshot.to_usage_snapshot(), "api");
        if let Some(cost) = snapshot.to_cost_snapshot() {
            result = result.with_cost(cost);
        }
        Ok(result)
    }

    async fn fetch_logs_snapshot(&self, api_key: &str) -> Result<AiAndSnapshot, ProviderError> {
        let mut rows = Vec::new();
        let mut after: Option<String> = None;
        let mut after_id: Option<String> = None;
        let mut is_complete = false;

        for _ in 0..MAX_PAGES {
            let page = self
                .fetch_logs_page(api_key, after.as_deref(), after_id.as_deref())
                .await?;
            rows.extend(page.data);
            if !page.has_more.unwrap_or(false) {
                is_complete = true;
                break;
            }
            match (page.next_after, page.next_after_id) {
                (Some(next_after), Some(next_after_id)) => {
                    after = Some(next_after);
                    after_id = Some(next_after_id);
                }
                // Server reports more rows but omitted a cursor; after alone is unsafe.
                _ => break,
            }
        }

        Ok(AiAndSnapshot::summarize(&rows, is_complete))
    }

    async fn fetch_logs_page(
        &self,
        api_key: &str,
        after: Option<&str>,
        after_id: Option<&str>,
    ) -> Result<LogsEnvelope, ProviderError> {
        let mut req = self
            .client
            .get(LOGS_URL)
            .query(&[("range", "30days"), ("limit", &PAGE_LIMIT.to_string())])
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Accept", "application/json");
        if let Some(after) = after {
            req = req.query(&[("after", after)]);
        }
        if let Some(after_id) = after_id {
            req = req.query(&[("after_id", after_id)]);
        }

        let resp = req.send().await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Other(
                "ai& rejected the API key. Create a new key at console.aiand.com and update Settings."
                    .to_string(),
            ));
        }
        if status == reqwest::StatusCode::PAYMENT_REQUIRED {
            return Err(ProviderError::Other(
                "ai& reports the organization is out of credits. Top up at console.aiand.com."
                    .to_string(),
            ));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::Other(
                "ai& rate limit exceeded. Usage will refresh on the next cycle.".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "ai& logs API returned HTTP {status}."
            )));
        }

        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Could not parse ai& usage: {e}")))
    }
}

impl Default for AiAndProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AiAndProvider {
    fn id(&self) -> ProviderId {
        ProviderId::AiAnd
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

fn clean_api_key(raw: &str) -> Option<String> {
    let mut value = raw.trim().to_string();
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value = value[1..value.len() - 1].trim().to_string();
    }
    if let Some(stripped) = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
    {
        value = stripped.trim().to_string();
    }
    (!value.is_empty()).then_some(value)
}

/// Parse fixture page JSON without network (unit tests).
fn summarize_pages_for_testing(pages: &[&str]) -> Result<AiAndSnapshot, ProviderError> {
    let mut rows = Vec::new();
    let mut is_complete = false;
    let mut page_count = 0usize;
    for page_json in pages {
        page_count += 1;
        let page: LogsEnvelope = serde_json::from_str(page_json)
            .map_err(|e| ProviderError::Parse(format!("Could not parse ai& usage: {e}")))?;
        rows.extend(page.data);
        if !page.has_more.unwrap_or(false) {
            is_complete = true;
            break;
        }
        if page.next_after.is_none() || page.next_after_id.is_none() {
            break;
        }
        if page_count >= MAX_PAGES {
            break;
        }
    }
    // Hitting MAX_PAGES without has_more=false leaves is_complete false.
    Ok(AiAndSnapshot::summarize(&rows, is_complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        costs: &[(&str, &str)],
        has_more: bool,
        next_after: Option<&str>,
        next_after_id: Option<&str>,
    ) -> String {
        let rows: Vec<String> = costs
            .iter()
            .map(|(cost, currency)| format!(r#"{{"cost":"{cost}","currency":"{currency}"}}"#))
            .collect();
        let next_after_json = match next_after {
            Some(v) => format!("\"{v}\""),
            None => "null".to_string(),
        };
        let next_after_id_json = match next_after_id {
            Some(v) => format!("\"{v}\""),
            None => "null".to_string(),
        };
        format!(
            r#"{{
              "data": [{}],
              "has_more": {},
              "next_after": {},
              "next_after_id": {}
            }}"#,
            rows.join(","),
            has_more,
            next_after_json,
            next_after_id_json
        )
    }

    #[test]
    fn sums_same_currency_rows_and_marks_complete() {
        let p1 = page(
            &[("1.50", "usd"), ("2.25", "USD"), ("0.10", "eur")],
            false,
            None,
            None,
        );
        let snapshot = summarize_pages_for_testing(&[&p1]).unwrap();
        let spend = snapshot.last_30_days_spend.as_ref().unwrap();
        assert!((spend.amount - 3.75).abs() < 1e-9);
        assert_eq!(spend.currency_code, "USD");
        assert!(snapshot.is_complete);

        let usage = snapshot.to_usage_snapshot();
        assert!(usage.primary.is_informational);
        assert!(
            usage
                .primary
                .reset_description
                .as_deref()
                .unwrap_or("")
                .contains("Last 30 days")
        );
        let cost = snapshot.to_cost_snapshot().unwrap();
        assert_eq!(cost.period, "Last 30 days");
        assert!((cost.used - 3.75).abs() < 1e-9);
        assert_eq!(cost.currency_code, "USD");
    }

    #[test]
    fn partial_when_page_cap_or_missing_cursor() {
        let p1 = page(
            &[("1.00", "usd")],
            true,
            Some("2026-01-01T00:00:00+00"),
            Some("id-1"),
        );
        let p2 = page(&[("2.00", "usd")], true, None, None);
        let snapshot = summarize_pages_for_testing(&[&p1, &p2]).unwrap();
        assert!(!snapshot.is_complete);
        assert!((snapshot.last_30_days_spend.as_ref().unwrap().amount - 3.00).abs() < 1e-9);
        assert_eq!(
            snapshot.to_cost_snapshot().unwrap().period,
            "Last 30 days (partial)"
        );
    }

    #[test]
    fn empty_window_has_no_cost_snapshot() {
        let p1 = page(&[], false, None, None);
        let snapshot = summarize_pages_for_testing(&[&p1]).unwrap();
        assert!(snapshot.last_30_days_spend.is_none());
        assert!(snapshot.is_complete);
        assert!(snapshot.to_cost_snapshot().is_none());
        assert!(
            snapshot
                .to_usage_snapshot()
                .primary
                .reset_description
                .as_deref()
                .unwrap_or("")
                .contains("No priced")
        );
    }

    #[test]
    fn rejects_malformed_page_json() {
        let err = summarize_pages_for_testing(&["not-json"]).unwrap_err();
        match err {
            ProviderError::Parse(msg) => assert!(msg.contains("ai&")),
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn cleans_quoted_and_bearer_prefixed_keys() {
        assert_eq!(
            clean_api_key("  \"Bearer sk-test\"  ").as_deref(),
            Some("sk-test")
        );
        assert_eq!(clean_api_key("bearer sk-abc").as_deref(), Some("sk-abc"));
        assert_eq!(clean_api_key("   ").as_deref(), None);
    }

    #[test]
    fn metadata_matches_upstream_descriptor() {
        let provider = AiAndProvider::new();
        assert_eq!(provider.id(), ProviderId::AiAnd);
        assert_eq!(provider.metadata().display_name, "ai&");
        assert_eq!(
            provider.metadata().dashboard_url,
            Some("https://console.aiand.com")
        );
        assert!(!provider.metadata().supports_credits);
        assert!(!provider.metadata().default_enabled);
    }
}
