//! Command Code provider implementation.
//!
//! Uses a browser session cookie to fetch monthly and purchased credit balances.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const COMMAND_CODE_API_BASE: &str = "https://api.commandcode.ai";
const COMMAND_CODE_CREDITS_PATH: &str = "/internal/billing/credits";
const COMMAND_CODE_SUBSCRIPTIONS_PATH: &str = "/internal/billing/subscriptions";

pub struct CommandCodeProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl CommandCodeProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::CommandCode,
                display_name: "Command Code",
                session_label: "Credits",
                weekly_label: "Monthly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://commandcode.ai"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_web(&self, cookie_header: &str) -> Result<ProviderFetchResult, ProviderError> {
        let cookie_header =
            normalize_cookie_header(cookie_header).ok_or_else(|| ProviderError::NoCookies)?;
        let credits = self
            .get_json(
                &format!("{COMMAND_CODE_API_BASE}{COMMAND_CODE_CREDITS_PATH}"),
                &cookie_header,
            )
            .await?;
        let subscription = self
            .get_json(
                &format!("{COMMAND_CODE_API_BASE}{COMMAND_CODE_SUBSCRIPTIONS_PATH}"),
                &cookie_header,
            )
            .await
            .ok();
        result_from_payloads(&credits, subscription.as_ref())
    }

    async fn get_json(&self, url: &str, cookie_header: &str) -> Result<Value, ProviderError> {
        let response = self
            .client
            .get(url)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", "https://commandcode.ai")
            .header("Referer", "https://commandcode.ai/")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Command Code API returned status {}",
                response.status()
            )));
        }
        response.json::<Value>().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse Command Code response: {e}"))
        })
    }
}

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains('=') && !trimmed.contains(';') {
        return Some(format!("__Secure-better-auth.session_token={trimmed}"));
    }

    let supported = [
        "__Host-better-auth.session_token",
        "__Secure-better-auth.session_token",
        "better-auth.session_token",
    ];
    for chunk in trimmed.split(';') {
        let (name, value) = chunk.trim().split_once('=')?;
        if supported
            .iter()
            .any(|expected| expected.eq_ignore_ascii_case(name.trim()))
            && !value.trim().is_empty()
        {
            return Some(format!("{}={}", name.trim(), value.trim()));
        }
    }
    None
}

fn result_from_payloads(
    credits_payload: &Value,
    subscription_payload: Option<&Value>,
) -> Result<ProviderFetchResult, ProviderError> {
    let credits = credits_payload
        .get("credits")
        .ok_or_else(|| ProviderError::Parse("Command Code credits object missing".into()))?;
    let monthly = number(credits.get("monthlyCredits"))
        .ok_or_else(|| ProviderError::Parse("Command Code monthlyCredits missing".into()))?;
    let purchased = number(credits.get("purchasedCredits")).unwrap_or(0.0);
    let premium = number(credits.get("premiumMonthlyCredits")).unwrap_or(0.0);
    let open_source = number(credits.get("opensourceMonthlyCredits")).unwrap_or(0.0);
    let total_monthly = premium + open_source;
    let used_percent = if total_monthly > 0.0 {
        ((total_monthly - monthly).max(0.0) / total_monthly * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let period_end = subscription_payload
        .and_then(|root| root.get("data"))
        .and_then(|data| data.get("currentPeriodEnd"))
        .and_then(|value| value.as_str())
        .and_then(parse_datetime);
    let plan = subscription_payload
        .and_then(|root| root.get("data"))
        .and_then(|data| data.get("planId"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty());

    let mut primary = RateWindow::with_details(
        used_percent,
        None,
        period_end,
        Some(format!("{monthly:.2} monthly credits remaining")),
    );
    if !primary.used_percent.is_finite() {
        primary.used_percent = 0.0;
    }
    let mut secondary = RateWindow::new(0.0);
    secondary.reset_description = Some(format!("{purchased:.2} purchased credits"));

    let mut snapshot = UsageSnapshot::new(primary).with_secondary(secondary);
    if let Some(plan) = plan {
        snapshot = snapshot.with_login_method(plan.to_string());
    }
    let cost = CostSnapshot::new((total_monthly - monthly).max(0.0), "USD", "monthly credits")
        .with_limit(total_monthly.max(0.0));
    Ok(ProviderFetchResult::new(snapshot, "web").with_cost(cost))
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

impl Default for CommandCodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CommandCodeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::CommandCode
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let cookie = match ctx.manual_cookie_header.as_deref() {
                    Some(cookie) => cookie.to_string(),
                    None => crate::providers::browser_cookie_header(&["commandcode.ai"])?,
                };
                self.fetch_web(&cookie).await
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_code_accepts_bare_session_token() {
        assert_eq!(
            normalize_cookie_header("abc123").as_deref(),
            Some("__Secure-better-auth.session_token=abc123")
        );
    }

    #[test]
    fn command_code_result_uses_monthly_credits() {
        let result = result_from_payloads(
            &json!({"credits":{"monthlyCredits":25,"purchasedCredits":2,"premiumMonthlyCredits":100}}),
            None,
        )
        .unwrap();
        assert_eq!(result.usage.primary.used_percent, 75.0);
    }
}
