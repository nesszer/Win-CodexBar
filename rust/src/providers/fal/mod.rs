//! fal account credit balance.
//!
//! Ported from upstream CodexBar v0.75.0 (`Resources/Plugins/fal.js`,
//! `Providers/Fal/FalProviderDescriptor.swift`). Reads
//! `GET https://api.fal.ai/v1/account/billing?expand=credits` with the API key
//! as `Authorization: Key <key>`. The key needs the `billing:usage:read`
//! permission (BILLING, READONLY, or FULL preset). Response bodies are never
//! echoed in errors and the key is never logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, AUTHORIZATION},
};
use serde_json::{Map, Value};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot, format_currency,
};
use crate::providers::{BoundedBodyError, read_bounded_response, resolve_api_key};

const BILLING_URL: &str = "https://api.fal.ai/v1/account/billing?expand=credits";
const CREDENTIAL_TARGET: &str = "codexbar-fal";
pub const API_KEY_ENV: &str = "FAL_KEY";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const SECTION_TITLE: &str = "Account balance";
const LOGIN_METHOD: &str = "API key";
const CREDIT_BALANCE_ID: &str = "credit-balance";

pub struct FalProvider {
    client: Client,
    billing_url: String,
}

impl FalProvider {
    pub fn new() -> Self {
        Self::with_billing_url(BILLING_URL)
    }

    fn with_billing_url(billing_url: &str) -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            billing_url: billing_url.to_owned(),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let key = resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, &[API_KEY_ENV])?;

        let response = self
            .client
            .get(&self.billing_url)
            .header(AUTHORIZATION, format!("Key {key}"))
            .header(ACCEPT, "application/json")
            .send()
            .await?;

        let status = response.status();
        match status {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => {
                return Err(ProviderError::Other(
                    "fal rejected the API key. Check or replace it in Settings.".into(),
                ));
            }
            StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(
                    "fal rejected the key: billing needs the billing:usage:read permission (BILLING, READONLY or FULL preset).".into(),
                ));
            }
            _ => {
                return Err(ProviderError::Other(format!(
                    "fal returned HTTP {}.",
                    status.as_u16()
                )));
            }
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::Read(error) => ProviderError::Network(error),
                BoundedBodyError::TooLarge => unrecognized(),
            })?;
        parse_billing(&body)
    }
}

impl Default for FalProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for FalProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Fal
    }

    fn metadata(&self) -> &ProviderMetadata {
        &FAL_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_impl(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn unrecognized() -> ProviderError {
    ProviderError::Parse("fal returned an unrecognized billing response.".into())
}

/// Upstream: a parsable object is required; a successful response without a
/// `credits` object cannot establish a balance and is reported as unavailable.
fn parse_billing(body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let root: Value = serde_json::from_slice(body)
        .map_err(|_| ProviderError::Parse("fal returned a non-JSON billing response.".into()))?;
    let root = root.as_object().ok_or_else(unrecognized)?;
    let credits = match root.get("credits") {
        Some(Value::Object(credits)) => credits,
        _ => {
            return Err(ProviderError::Other(
                "fal did not return the credit balance this time. Try Refresh in a moment.".into(),
            ));
        }
    };

    let balance = credit_balance(credits.get("current_balance"))?;
    let currency = currency(credits);
    let organization = account_name(root);

    let mut usage = UsageSnapshot::new(RateWindow::informational(SECTION_TITLE).hidden_on_card())
        .with_login_method(LOGIN_METHOD);
    if let Some(organization) = organization {
        usage = usage.with_organization(organization);
    }

    let row = ProviderDisplayDetail::new(
        CREDIT_BALANCE_ID,
        "Credit balance",
        format_currency(balance, &currency),
    )
    .and_then(|detail| detail.with_section_title(SECTION_TITLE));
    Ok(ProviderFetchResult::new(usage, "api").with_display_detail(row))
}

/// Upstream accepts a finite number, or a string of plain decimal digits with
/// an optional leading minus and surrounding whitespace.
fn credit_balance(raw: Option<&Value>) -> Result<f64, ProviderError> {
    let parsed = match raw {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) if is_plain_decimal(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed.filter(|value| value.is_finite()).ok_or_else(|| {
        ProviderError::Parse(format!(
            "fal returned an unrecognized credit balance ({}).",
            value_kind(raw)
        ))
    })
}

fn is_plain_decimal(text: &str) -> bool {
    let unsigned = text.trim().strip_prefix('-').unwrap_or(text.trim());
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    digits(whole) && fraction.is_none_or(digits)
}

fn value_kind(raw: Option<&Value>) -> &'static str {
    match raw {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

/// Upstream uses the reported ISO code only when it is three uppercase letters.
fn currency(credits: &Map<String, Value>) -> String {
    credits
        .get("currency")
        .and_then(Value::as_str)
        .filter(|code| code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()))
        .unwrap_or("USD")
        .to_string()
}

fn account_name(root: &Map<String, Value>) -> Option<String> {
    root.get("username")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

const FAL_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Fal,
    display_name: "fal",
    session_label: "Credit balance",
    weekly_label: "Credit balance",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://fal.ai/dashboard/billing"),
    status_page_url: None,
    tertiary_label_key: None,
};
