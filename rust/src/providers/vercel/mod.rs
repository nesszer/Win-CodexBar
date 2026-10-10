//! Vercel AI Gateway credit balance provider.
//!
//! `GET https://ai-gateway.vercel.sh/v1/credits` with a bearer API key returns
//! the team's remaining balance and lifetime spend as decimal strings
//! (upstream CodexBar 0.66.0, `Resources/Plugins/vercel.js`). The endpoint
//! reports no quota or billing period, so the primary window is informational
//! and no percentage is invented.

use async_trait::async_trait;
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::Deserialize;
use std::time::Duration;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response};

const CREDITS_URL: &str = "https://ai-gateway.vercel.sh/v1/credits";
const CREDENTIAL_TARGET: &str = "codexbar-vercel";
const ENV_KEYS: &[&str] = &["AI_GATEWAY_API_KEY"];
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const SECTION_LABEL: &str = "Team credits";

pub struct VercelProvider {
    client: Client,
    credits_url: String,
}

impl VercelProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .redirect(Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self::with_client(CREDITS_URL, client)
    }

    fn with_client(credits_url: impl Into<String>, client: Client) -> Self {
        Self {
            client,
            credits_url: credits_url.into(),
        }
    }

    async fn fetch_credits(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let api_key =
            crate::providers::resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, ENV_KEYS)?;
        let response = self
            .client
            .get(&self.credits_url)
            .bearer_auth(api_key.trim())
            .header(reqwest::header::ACCEPT, "application/json")
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
                BoundedBodyError::TooLarge => parse_failure(),
            })?;
        Ok(build_result(parse_credits(&body)?))
    }
}

impl Default for VercelProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for VercelProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Vercel
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_credits(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// Wire shape at upstream v0.66.0: both amounts are decimal strings.
#[derive(Debug, Deserialize)]
struct CreditsResponse {
    balance: String,
    total_used: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Credits {
    balance: f64,
    total_used: f64,
}

/// Status mapping keeps the code but never the response body.
fn status_error(status: StatusCode) -> ProviderError {
    let code = status.as_u16();
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::AuthRequired,
        StatusCode::FORBIDDEN => ProviderError::Other(format!(
            "Vercel AI Gateway returned HTTP {code}; the API key lacks permission to read credits."
        )),
        StatusCode::TOO_MANY_REQUESTS => ProviderError::Other(format!(
            "Vercel AI Gateway rate limit reached (HTTP {code})."
        )),
        status if status.is_server_error() => {
            ProviderError::Other(format!("Vercel AI Gateway is unavailable (HTTP {code})."))
        }
        _ => ProviderError::Other(format!("Vercel AI Gateway returned HTTP {code}.")),
    }
}

/// Fixed message: response data (and serde's value-echoing errors) never
/// reach the user-facing error.
fn parse_failure() -> ProviderError {
    ProviderError::Parse("Vercel AI Gateway returned an unrecognized credit balance.".into())
}

fn parse_credits(body: &[u8]) -> Result<Credits, ProviderError> {
    let response: CreditsResponse = serde_json::from_slice(body).map_err(|_| parse_failure())?;
    let balance = parse_amount(&response.balance).ok_or_else(parse_failure)?;
    let total_used = parse_amount(&response.total_used).ok_or_else(parse_failure)?;
    if total_used < 0.0 {
        return Err(parse_failure());
    }
    Ok(Credits {
        balance,
        total_used,
    })
}

/// Accept only `^-?\d+(?:\.\d+)?$` that converts to a finite number.
fn parse_amount(value: &str) -> Option<f64> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    let (integer, fraction) = match digits.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (digits, None),
    };
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(integer) || fraction.is_some_and(|part| !all_digits(part)) {
        return None;
    }
    value
        .parse::<f64>()
        .ok()
        .filter(|amount| amount.is_finite())
}

fn build_result(credits: Credits) -> ProviderFetchResult {
    let usage =
        UsageSnapshot::new(RateWindow::informational(SECTION_LABEL)).with_login_method("API");
    // `with_balance` clamps to >= 0, so a negative balance is left untyped and
    // stays visible only through the signed display row below.
    let mut cost = CostSnapshot::new(credits.total_used, "USD", SECTION_LABEL);
    if credits.balance >= 0.0 {
        cost = cost.with_balance(credits.balance);
    }
    ProviderFetchResult::new(usage, "api")
        .with_cost(cost)
        .with_display_detail(ProviderDisplayDetail::new(
            "vercel-balance",
            "Available balance",
            format::usd_signed(credits.balance),
        ))
        .with_display_detail(ProviderDisplayDetail::new(
            "vercel-lifetime-spend",
            "Lifetime spend",
            format::usd_signed(credits.total_used),
        ))
}

#[cfg(test)]
mod tests;
