//! TypeSafe console billing provider.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde_json::Value;
use std::time::Duration;

use super::{BoundedBodyError, read_bounded_response};
use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, RateWindow, SourceMode, UsageSnapshot,
};

pub(crate) const BILLING_URL: &str = "https://console.typesafe.ai/settings/billing";
const ORIGIN: &str = "https://console.typesafe.ai";
const MAX_CHUNKS: usize = 60;
const CHUNK_SCAN_CONCURRENCY: usize = 6;
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq)]
struct Credit {
    amount: f64,
    remaining: f64,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
struct Billing {
    spent: f64,
    balance: f64,
    cycle_label: Option<String>,
    plan: Option<String>,
    credits: Vec<Credit>,
}

pub struct TypeSafeProvider {
    client: Client,
}

impl TypeSafeProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_web(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let cookie = match ctx.manual_cookie_header.as_deref() {
            Some(raw) => crate::providers::normalize_cookie_header(raw).ok_or_else(|| {
                ProviderError::Other(
                    "TypeSafe needs a nonempty Cookie header from the billing page.".into(),
                )
            })?,
            None => {
                crate::providers::browser_cookie_header(&["console.typesafe.ai", "typesafe.ai"])?
            }
        };
        let page = self.get(BILLING_URL, &cookie, "text/html").await?;
        let action_id = self.discover_action(&cookie, &page).await?;
        let response = match self.post_action(&cookie, &action_id).await? {
            Some(response) => response,
            None => {
                let refreshed_page = self.get(BILLING_URL, &cookie, "text/html").await?;
                let refreshed_action = self.discover_action(&cookie, &refreshed_page).await?;
                self.post_action(&cookie, &refreshed_action)
                    .await?
                    .ok_or_else(|| parse_failure("server action stayed stale after rediscovery"))?
            }
        };
        let billing = parse_rsc_billing(&response)?;
        Ok(build_result(billing))
    }

    async fn discover_action(&self, cookie: &str, page: &str) -> Result<String, ProviderError> {
        let mut chunks = stream::iter(extract_chunk_urls(page))
            .map(|url| async move { self.get(&url, cookie, "application/javascript").await })
            .buffer_unordered(CHUNK_SCAN_CONCURRENCY);
        let mut first_error = None;
        while let Some(result) = chunks.next().await {
            match result {
                Ok(chunk) => {
                    if let Some(found) = find_action_id(&chunk) {
                        return Ok(found);
                    }
                }
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Err(parse_failure("action id not found"))
    }

    async fn get(&self, url: &str, cookie: &str, accept: &str) -> Result<String, ProviderError> {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            let response = self
                .client
                .get(url)
                .header("Cookie", cookie)
                .header("Accept", accept)
                .send()
                .await?;
            read_response(response).await
        })
        .await
        .map_err(|_| ProviderError::Timeout)?
    }

    async fn post_action(
        &self,
        cookie: &str,
        action_id: &str,
    ) -> Result<Option<String>, ProviderError> {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            let response = self
                .client
                .post(BILLING_URL)
                .header("Cookie", cookie)
                .header("Origin", ORIGIN)
                .header("Next-Action", action_id)
                .header("Accept", "text/x-component")
                .header("Content-Type", "application/json")
                .body("[]")
                .send()
                .await?;
            if response.status() == StatusCode::NOT_FOUND
                && response
                    .headers()
                    .get("x-nextjs-action-not-found")
                    .and_then(|value| value.to_str().ok())
                    == Some("1")
            {
                return Ok(None);
            }
            read_response(response).await.map(Some)
        })
        .await
        .map_err(|_| ProviderError::Timeout)?
    }
}

impl Default for TypeSafeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for TypeSafeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::TypeSafe
    }
    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => self.fetch_web(ctx).await,
            SourceMode::OAuth | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }
    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }
    fn supports_web(&self) -> bool {
        true
    }
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }
}

async fn read_response(response: reqwest::Response) -> Result<String, ProviderError> {
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || status.is_redirection()
    {
        return Err(ProviderError::AuthRequired);
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::Other("TypeSafe rate limit reached.".into()));
    }
    if status == StatusCode::REQUEST_TIMEOUT || status.is_server_error() {
        return Err(ProviderError::Other(
            "TypeSafe billing is unavailable.".into(),
        ));
    }
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "TypeSafe returned HTTP {status}."
        )));
    }
    let body = read_bounded_response(response, MAX_BODY_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::TooLarge => parse_failure("response too large"),
            BoundedBodyError::Read(error) => ProviderError::Network(error),
        })?;
    let body = String::from_utf8(body).map_err(|_| parse_failure("response was not UTF-8"))?;
    if body.contains("\\\"(auth)\\\",{\\\"children\\\":[\\\"login\\\"") {
        return Err(ProviderError::AuthRequired);
    }
    Ok(body)
}

fn extract_chunk_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = html;
    while let Some(index) = rest.find("src=") {
        rest = &rest[index + 4..];
        let Some(quote) = rest
            .chars()
            .next()
            .filter(|value| matches!(value, '\'' | '"'))
        else {
            continue;
        };
        rest = &rest[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        let source = &rest[..end];
        rest = &rest[end + quote.len_utf8()..];
        let normalized = if source.starts_with('/') {
            format!("{ORIGIN}{source}")
        } else {
            source.to_string()
        };
        if normalized.starts_with(&format!("{ORIGIN}/"))
            && normalized
                .split('?')
                .next()
                .is_some_and(|path| path.ends_with(".js"))
            && !urls.contains(&normalized)
        {
            urls.push(normalized);
        }
        if urls.len() >= MAX_CHUNKS {
            break;
        }
    }
    urls
}

fn find_action_id(chunk: &str) -> Option<String> {
    let marker = chunk.find("getBillingOverviewResult")?;
    let start = (marker.saturating_sub(200)..=marker)
        .find(|index| chunk.is_char_boundary(*index))
        .unwrap_or(marker);
    let prefix = &chunk[start..marker];
    prefix.split('"').rev().find_map(|candidate| {
        (candidate.len() >= 40 && candidate.chars().all(|ch| ch.is_ascii_hexdigit()))
            .then(|| candidate.to_string())
    })
}

fn parse_rsc_billing(body: &str) -> Result<Billing, ProviderError> {
    let result = body
        .lines()
        .find_map(|line| {
            let (_, json) = line.split_once(':')?;
            let value: Value = serde_json::from_str(json).ok()?;
            value.as_object()?.contains_key("ok").then_some(value)
        })
        .ok_or_else(|| parse_failure("missing result"))?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(ProviderError::Other(
            "TypeSafe billing request failed.".into(),
        ));
    }
    let billing = result
        .get("data")
        .and_then(|value| value.get("billing"))
        .and_then(Value::as_object)
        .ok_or_else(|| parse_failure("missing billing"))?;
    let spent = finite_nonnegative(billing.get("spent"), "spent")?;
    let balance = finite_nonnegative(billing.get("balance"), "balance")?;
    let cycle_label = clean_text(billing.get("cycleLabel"));
    let plan = clean_text(billing.get("plan"));
    let credits = billing
        .get("credits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let object = item.as_object()?;
            let amount = finite_nonnegative(object.get("amount"), "credit amount").ok()?;
            let remaining = finite_nonnegative(object.get("remaining"), "credit remaining").ok()?;
            if remaining <= 0.0 {
                return None;
            }
            let expires_at = object
                .get("expiresAt")
                .and_then(Value::as_str)
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())?
                .with_timezone(&Utc);
            if expires_at <= Utc::now() {
                return None;
            }
            Some(Credit {
                amount,
                remaining,
                expires_at,
            })
        })
        .collect();
    Ok(Billing {
        spent,
        balance,
        cycle_label,
        plan,
        credits,
    })
}

fn build_result(billing: Billing) -> ProviderFetchResult {
    let mut usage = UsageSnapshot::new(RateWindow::informational(format!(
        "Balance ${:.2}",
        billing.balance
    )))
    .with_login_method(format!("Balance ${:.2}", billing.balance));
    usage.updated_at = Utc::now();
    let mut cost = CostSnapshot::new(
        billing.spent,
        "USD",
        billing
            .cycle_label
            .clone()
            .unwrap_or_else(|| "Billing cycle".into()),
    )
    .with_balance(billing.balance)
    .always_visible();
    cost.updated_at = usage.updated_at;
    let spent_title = billing
        .cycle_label
        .as_deref()
        .map_or_else(|| "Spent".into(), |cycle| format!("Spent ({cycle})"));
    let mut result = ProviderFetchResult::new(usage, "web")
        .with_non_authoritative_pace()
        .with_cost(cost)
        .with_display_detail(ProviderDisplayDetail::new(
            "spent",
            spent_title,
            format!("${:.2}", billing.spent),
        ));
    if let Some(plan) = billing.plan.as_deref() {
        let label = if plan == "free_plan" {
            "Free".into()
        } else {
            title_case(plan)
        };
        result = result.with_display_detail(ProviderDisplayDetail::new("plan", "Plan", label));
    }
    let available = 24usize.saturating_sub(result.display_details().len());
    let visible = billing
        .credits
        .iter()
        .take(available.saturating_sub(1))
        .collect::<Vec<_>>();
    for (index, credit) in visible.iter().enumerate() {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            format!("credit-{index}"),
            "Credit",
            format!(
                "{:.2} of {:.2}, expires {}",
                credit.remaining,
                credit.amount,
                credit.expires_at.format("%b %d")
            ),
        ));
    }
    if billing.credits.len() > visible.len() {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "additional-credits",
            "Additional credits",
            (billing.credits.len() - visible.len()).to_string(),
        ));
    }
    result
}

fn finite_nonnegative(value: Option<&Value>, field: &str) -> Result<f64, ProviderError> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| parse_failure(field))
}
fn clean_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
fn title_case(value: &str) -> String {
    value
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + &chars.as_str().to_ascii_lowercase()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn parse_failure(field: impl AsRef<str>) -> ProviderError {
    ProviderError::Parse(format!(
        "TypeSafe billing response format changed: {}.",
        field.as_ref()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_only_same_origin_javascript_and_action_id() {
        let urls = extract_chunk_urls(
            r#"<script src="/_next/a.js"></script><script src="https://evil.test/x.js"></script>"#,
        );
        assert_eq!(urls, vec!["https://console.typesafe.ai/_next/a.js"]);
        let id = "a".repeat(40);
        assert_eq!(
            find_action_id(&format!(r#"x("{id}")xxx"getBillingOverviewResult""#)).as_deref(),
            Some(id.as_str())
        );
    }

    #[test]
    fn action_discovery_handles_multibyte_text_at_scan_boundary() {
        let id = "b".repeat(40);
        let chunk = format!(
            "{}\u{00e9}{}x(\"{id}\")getBillingOverviewResult",
            "x".repeat(10),
            "x".repeat(154)
        );
        let marker = chunk.find("getBillingOverviewResult").unwrap();
        assert!(!chunk.is_char_boundary(marker - 200));
        assert_eq!(find_action_id(&chunk).as_deref(), Some(id.as_str()));
    }

    #[test]
    fn parses_billing_result_and_skips_expired_or_empty_credits() {
        let body = r#"1:{"ok":true,"data":{"billing":{"spent":4.5,"balance":10,"cycleLabel":"September","plan":"free_plan","credits":[{"amount":8,"remaining":3,"expiresAt":"2100-01-02T00:00:00Z"},{"amount":1,"remaining":0,"expiresAt":"2100-01-02T00:00:00Z"},{"amount":5,"remaining":2,"expiresAt":"2000-01-02T00:00:00Z"},{"amount":-2,"remaining":1,"expiresAt":"2100-01-02T00:00:00Z"},{"amount":2,"remaining":-1,"expiresAt":"2100-01-02T00:00:00Z"}]}}}"#;
        let parsed = parse_rsc_billing(body).unwrap();
        assert_eq!(parsed.spent, 4.5);
        assert_eq!(parsed.balance, 10.0);
        assert_eq!(parsed.credits.len(), 1);
    }

    #[test]
    fn rejects_negative_spent_and_balance() {
        let negative_spent = r#"1:{"ok":true,"data":{"billing":{"spent":-0.01,"balance":10}}}"#;
        let negative_balance = r#"1:{"ok":true,"data":{"billing":{"spent":0,"balance":-0.01}}}"#;

        assert!(parse_rsc_billing(negative_spent).is_err());
        assert!(parse_rsc_billing(negative_balance).is_err());
    }

    #[test]
    fn accepts_zero_and_positive_monetary_values() {
        assert_eq!(
            finite_nonnegative(Some(&serde_json::json!(0)), "amount").unwrap(),
            0.0
        );
        assert_eq!(
            finite_nonnegative(Some(&serde_json::json!(1.25)), "amount").unwrap(),
            1.25
        );
        assert!(finite_nonnegative(Some(&serde_json::json!(-0.01)), "amount").is_err());
    }
}
