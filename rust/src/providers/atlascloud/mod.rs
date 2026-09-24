//! Atlas Cloud account balance provider.

use async_trait::async_trait;
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::Deserialize;
use std::time::Duration;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const BALANCE_URL: &str = "https://api.atlascloud.ai/public/v1/balance";
const CREDENTIAL_TARGET: &str = "codexbar-atlascloud";
const API_KEY_ENV: &str = "ATLASCLOUD_API_KEY";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AtlasCloudProvider {
    metadata: ProviderMetadata,
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
            metadata: ProviderMetadata {
                id: ProviderId::AtlasCloud,
                display_name: "Atlas Cloud",
                session_label: "Balance",
                weekly_label: "Balance",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://atlascloud.ai/dashboard"),
                status_page_url: None,
                tertiary_label_key: None,
            },
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
        let detail = ProviderDisplayDetail::new("atlascloud-available", "Available", balance);
        Ok(ProviderFetchResult::new(
            UsageSnapshot::new(RateWindow::informational("Account balance")),
            "api",
        )
        .with_display_detail(detail))
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

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
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

fn parse_balance(body: &str) -> Result<String, ProviderError> {
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
    Ok(amount.to_owned())
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
