//! abliteration.ai prepaid USD credits.
//!
//! Ported from upstream CodexBar (`Resources/Plugins/abliteration.ts`,
//! `Providers/Abliteration/AbliterationProviderDescriptor.swift`, upstream
//! #4756). One non-billable `GET https://api.abliteration.ai/v1/credits` per
//! refresh with the API key as a bearer token. Inference endpoints are never
//! called. Response bodies are never echoed in errors and the key is never
//! logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode, header::ACCEPT};
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response, resolve_api_key};

const API_BASE: &str = "https://api.abliteration.ai";
const CREDITS_PATH: &str = "/v1/credits";
const CREDENTIAL_TARGET: &str = "codexbar-abliteration";
pub const API_KEY_ENV: &str = "ABLITERATION_API_KEY";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const LOGIN_METHOD: &str = "API key";
const SECTION: &str = "Credits";

const CHECK_KEY: &str = "Check your API key in Settings.";
const MISSING_KEY: &str = "Set an abliteration.ai API key in Settings or ABLITERATION_API_KEY.";
const RATE_LIMITED: &str = "abliteration.ai is rate limiting requests. Try again later.";
const REQUEST_FAILED: &str = "abliteration.ai request failed. Try refreshing later.";
const UNRECOGNIZED: &str =
    "abliteration.ai returned an unrecognized response. Update CodexBar and try again.";

pub struct AbliterationProvider {
    client: Client,
    base_url: String,
}

impl AbliterationProvider {
    pub fn new() -> Self {
        Self::with_base_url(API_BASE)
    }

    fn with_base_url(base_url: &str) -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let key = resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, &[API_KEY_ENV])
            .map_err(|_| ProviderError::NotInstalled(MISSING_KEY.into()))?;
        let response = self
            .client
            .get(format!("{}{CREDITS_PATH}", self.base_url))
            .bearer_auth(key.trim())
            .header(ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        if status != StatusCode::OK {
            return Err(status_error(status));
        }
        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::Read(error) => ProviderError::Network(error),
                BoundedBodyError::TooLarge => unrecognized(),
            })?;
        let credits = parse_credits(&body)?;
        Ok(build_result(credits))
    }
}

impl Default for AbliterationProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AbliterationProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Abliteration
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn status_error(status: StatusCode) -> ProviderError {
    match status.as_u16() {
        401 | 403 => ProviderError::Other(CHECK_KEY.into()),
        429 => ProviderError::Other(RATE_LIMITED.into()),
        _ => ProviderError::Other(REQUEST_FAILED.into()),
    }
}

fn unrecognized() -> ProviderError {
    ProviderError::Parse(UNRECOGNIZED.into())
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Credits {
    total: f64,
    used: f64,
}

/// `{"data":{"total_credits":<usd>,"total_usage":<usd>}}`. Both counters must
/// be finite, non-negative JSON numbers; anything else fails closed.
fn parse_credits(body: &[u8]) -> Result<Credits, ProviderError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| unrecognized())?;
    let data = value
        .as_object()
        .and_then(|object| object.get("data"))
        .and_then(Value::as_object)
        .ok_or_else(unrecognized)?;
    let money = |field: &str| {
        data.get(field)
            .filter(|value| value.is_number())
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(unrecognized)
    };
    Ok(Credits {
        total: money("total_credits")?,
        used: money("total_usage")?,
    })
}

fn row(id: &str, title: &str, value: String) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(id, title, value).and_then(|row| row.with_section_title(SECTION))
}

fn build_result(credits: Credits) -> ProviderFetchResult {
    let Credits { total, used } = credits;
    // No reset date or pace is inferred: the counters are cumulative. Zero
    // total credits keep the dollar rows without inventing a percentage.
    let primary = if total > 0.0 {
        RateWindow::with_details(
            (used / total * 100.0).clamp(0.0, 100.0),
            None,
            None,
            Some(format!("{} / {}", format::usd(used), format::usd(total))),
        )
        .with_description_as_detail()
    } else {
        RateWindow::informational(format!("{} / {}", format::usd(used), format::usd(total)))
            .hidden_on_card()
    };
    let usage = UsageSnapshot::new(primary).with_login_method(LOGIN_METHOD);
    let rows = [
        row("balance", "Balance", format::usd((total - used).max(0.0))),
        row("credits-used", "Credits used", format::usd(used)),
        row("credits", "Total credits", format::usd(total)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    ProviderFetchResult::new(usage, "api").with_display_details(rows)
}
