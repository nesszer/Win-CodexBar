//! Manus provider implementation.
//!
//! Fetches credit balance using a Manus browser session token.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const MANUS_CREDITS_URL: &str = "https://api.manus.im/user.v1.UserService/GetAvailableCredits";

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ManusCreditsResponse {
    #[serde(default)]
    total_credits: f64,
    #[serde(default)]
    free_credits: f64,
    #[serde(default)]
    periodic_credits: f64,
    #[serde(default)]
    addon_credits: f64,
    #[serde(default)]
    refresh_credits: f64,
    #[serde(default)]
    max_refresh_credits: f64,
    #[serde(default)]
    pro_monthly_credits: f64,
    #[serde(default)]
    event_credits: f64,
    #[serde(default)]
    next_refresh_time: Option<DateTime<Utc>>,
    #[serde(default)]
    refresh_interval: Option<String>,
}

pub struct ManusProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ManusProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Manus,
                display_name: "Manus",
                session_label: "Credits",
                weekly_label: "Refresh",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://manus.im"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_web(&self, raw_cookie: &str) -> Result<UsageSnapshot, ProviderError> {
        let token = session_token(raw_cookie).ok_or(ProviderError::NoCookies)?;
        let response = self
            .client
            .post(MANUS_CREDITS_URL)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", "https://manus.im")
            .header("Referer", "https://manus.im/")
            .header("Connect-Protocol-Version", "1")
            .body("{}")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Manus API returned status {}",
                response.status()
            )));
        }

        let body: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Manus response: {e}")))?;
        let credits = parse_credits(body)?;
        Ok(snapshot_from_credits(&credits))
    }
}

fn session_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains('=') && !trimmed.contains(';') {
        return Some(trimmed.to_string());
    }
    for chunk in trimmed.split(';') {
        let (name, value) = chunk.trim().split_once('=')?;
        if name.trim().eq_ignore_ascii_case("session_id") && !value.trim().is_empty() {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn parse_credits(body: Value) -> Result<ManusCreditsResponse, ProviderError> {
    for key in ["data", "result", "response", "availableCredits"] {
        if let Some(value) = body.get(key)
            && value.is_object()
        {
            return serde_json::from_value(value.clone())
                .map_err(|e| ProviderError::Parse(format!("Failed to parse Manus credits: {e}")));
        }
    }
    if !body.is_object() {
        return Err(ProviderError::Parse(
            "Manus response was not an object".into(),
        ));
    }
    serde_json::from_value(body)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Manus credits: {e}")))
}

fn snapshot_from_credits(credits: &ManusCreditsResponse) -> UsageSnapshot {
    let primary = if credits.pro_monthly_credits > 0.0 {
        let used = (credits.pro_monthly_credits - credits.periodic_credits).max(0.0);
        let percent = used / credits.pro_monthly_credits * 100.0;
        RateWindow::with_details(
            percent,
            None,
            None,
            Some(format!(
                "{:.0} total credits ({:.0} free)",
                credits.total_credits, credits.free_credits
            )),
        )
    } else {
        RateWindow::with_details(
            0.0,
            None,
            None,
            Some(format!("{:.0} credits available", credits.total_credits)),
        )
    };

    let mut snapshot = UsageSnapshot::new(primary).with_login_method(format!(
        "Balance: {:.0} credits",
        credits.total_credits.round()
    ));

    if credits.max_refresh_credits > 0.0 {
        let used = (credits.max_refresh_credits - credits.refresh_credits).max(0.0);
        let mut secondary = RateWindow::with_details(
            used / credits.max_refresh_credits * 100.0,
            None,
            credits.next_refresh_time,
            Some(format!(
                "{:.0}/{:.0} refresh credits{}",
                credits.refresh_credits,
                credits.max_refresh_credits,
                credits
                    .refresh_interval
                    .as_deref()
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default()
            )),
        );
        if !secondary.used_percent.is_finite() {
            secondary.used_percent = 0.0;
        }
        snapshot = snapshot.with_secondary(secondary);
    }

    if credits.addon_credits > 0.0 {
        let mut addon = RateWindow::new(0.0);
        addon.reset_description = Some(format!("{:.0} add-on credits", credits.addon_credits));
        snapshot = snapshot.with_extra_rate_window("addon", "Add-on credits", addon);
    }
    if credits.event_credits > 0.0 {
        let mut event = RateWindow::new(0.0);
        event.reset_description = Some(format!("{:.0} event credits", credits.event_credits));
        snapshot = snapshot.with_extra_rate_window("event", "Event credits", event);
    }
    snapshot
}

impl Default for ManusProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ManusProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Manus
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let cookie = match ctx.manual_cookie_header.as_deref() {
                    Some(cookie) => cookie.to_string(),
                    None => crate::providers::browser_cookie_header(&["manus.im"])?,
                };
                Ok(ProviderFetchResult::new(
                    self.fetch_web(&cookie).await?,
                    "web",
                ))
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

    #[test]
    fn manus_extracts_session_id_from_cookie_header() {
        assert_eq!(
            session_token("foo=bar; session_id=abc123; other=1").as_deref(),
            Some("abc123")
        );
    }
}
