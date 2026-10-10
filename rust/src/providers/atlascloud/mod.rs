//! Atlas Cloud account balance provider.

use async_trait::async_trait;
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::Deserialize;
use std::time::Duration;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const BALANCE_URL: &str = "https://api.atlascloud.ai/public/v1/balance";
const CREDENTIAL_TARGET: &str = "codexbar-atlascloud";
const API_KEY_ENV: &str = "ATLASCLOUD_API_KEY";
/// Single canonical console URL, shared with the API-key settings catalog.
pub const DASHBOARD_URL: &str = "https://www.atlascloud.ai/console";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AtlasCloudProvider {
    client: Client,
    balance_url: String,
}

impl AtlasCloudProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .redirect(Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("Atlas Cloud HTTP client configuration is valid");
        Self::with_client(BALANCE_URL, client)
    }

    fn with_client(balance_url: impl Into<String>, client: Client) -> Self {
        Self {
            client,
            balance_url: balance_url.into(),
        }
    }

    async fn fetch_balance(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;

        let response = self
            .client
            .get(&self.balance_url)
            .bearer_auth(&key)
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
                BoundedBodyError::TooLarge => ProviderError::Parse(format!(
                    "Atlas Cloud response exceeded {MAX_RESPONSE_BYTES} bytes."
                )),
            })?;
        let body = std::str::from_utf8(&body).map_err(|error| {
            ProviderError::Parse(format!("Invalid Atlas Cloud response: {error}"))
        })?;
        let balance = parse_balance(body)?;
        Ok(balance_result(balance))
    }
}

/// Typed USD balance for balance formatting, Usage & Spend and currency
/// conversion, plus a signed display row. `CostSnapshot::with_balance` clamps
/// negatives to zero, so the row is what keeps a deficit (`-$1.25`) visible.
fn balance_result(balance: f64) -> ProviderFetchResult {
    let usage =
        UsageSnapshot::new(RateWindow::informational("Account balance")).with_login_method("API");
    let cost = CostSnapshot::new(0.0, "USD", "Atlas Cloud balance").with_balance(balance);
    let detail = ProviderDisplayDetail::new(
        "atlascloud-available",
        "Available balance",
        format_usd(balance),
    );
    ProviderFetchResult::new(usage, "api")
        .with_cost(cost)
        .with_display_detail(detail)
}

/// `$95.50` / `-$1.25`; a negative that rounds to zero shows no sign.
fn format_usd(amount: f64) -> String {
    let magnitude = format!("{:.2}", amount.abs());
    if amount < 0.0 && magnitude != "0.00" {
        format!("-${magnitude}")
    } else {
        format!("${magnitude}")
    }
}

impl Default for AtlasCloudProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AtlasCloudProvider {
    fn id(&self) -> ProviderId {
        ProviderId::AtlasCloud
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_balance(ctx).await,
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
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::AuthRequired,
        StatusCode::FORBIDDEN => ProviderError::Other(
            "Atlas Cloud denied access to the account balance; check API key permissions.".into(),
        ),
        StatusCode::TOO_MANY_REQUESTS => {
            ProviderError::Other("Atlas Cloud rate limit reached.".into())
        }
        status if status.is_server_error() => {
            ProviderError::Other("Atlas Cloud balance service is unavailable.".into())
        }
        status => ProviderError::Other(format!("Atlas Cloud returned HTTP {status}.")),
    }
}

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    object: String,
    scope: String,
    available: AvailableBalance,
}

#[derive(Debug, Deserialize)]
struct AvailableBalance {
    currency: String,
    value: String,
}

fn parse_balance(body: &str) -> Result<f64, ProviderError> {
    let response: BalanceResponse = serde_json::from_str(body)
        .map_err(|error| ProviderError::Parse(format!("Invalid Atlas Cloud response: {error}")))?;
    if response.object != "balance"
        || response.scope != "account"
        || response.available.currency != "usd"
    {
        return Err(parse_failure("unexpected object, scope, or currency"));
    }
    let amount = response.available.value;
    if !is_decimal(&amount) {
        return Err(parse_failure(
            "available.value must be a signed decimal string",
        ));
    }
    let parsed = amount
        .parse::<f64>()
        .map_err(|_| parse_failure("available.value is not a finite number"))?;
    if !parsed.is_finite() {
        return Err(parse_failure("available.value is not a finite number"));
    }
    Ok(parsed)
}

fn is_decimal(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    let mut parts = digits.split('.');
    let Some(integer) = parts.next() else {
        return false;
    };
    let fraction = parts.next();
    parts.next().is_none()
        && !integer.is_empty()
        && integer.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn parse_failure(reason: &str) -> ProviderError {
    ProviderError::Parse(format!("Invalid Atlas Cloud balance response: {reason}."))
}

#[cfg(test)]
mod tests;
