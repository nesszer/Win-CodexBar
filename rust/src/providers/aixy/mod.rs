//! Aixy AI gateway: the API key's own usage and applicable budget balances.
//!
//! Ported from upstream CodexBar v0.67.0. The key is sent only as a Bearer
//! token to `GET {base}/v1/usage`; the configured base URL is validated
//! before the credential is resolved, redirects are never followed, and
//! response bodies are never echoed in errors.

mod model;
mod present;
#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, SourceMode,
};

const CREDENTIAL_TARGET: &str = "codexbar-aixy";
const API_KEY_ENV: &str = "AIXY_API_KEY";
const BASE_URL_ENV: &str = "AIXY_BASE_URL";
const DEFAULT_BASE_URL: &str = "https://api.aixy-gateway.com";
const USAGE_PATH: &str = "/v1/usage";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub struct AixyProvider {
    client: Option<Client>,
}

impl AixyProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                // The Bearer credential must never follow a gateway redirect
                // to another origin.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok(),
        }
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        // Validate the endpoint before touching the keyring or environment so
        // a malformed or public plain-HTTP gateway never receives the key.
        let base = ctx
            .gateway_url
            .clone()
            .filter(|url| !url.trim().is_empty())
            .or_else(|| std::env::var(BASE_URL_ENV).ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let url = usage_url(&base)?;
        let api_key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure Aixy HTTP client.".into())
        })?;

        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let response = client
            .get(url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .timeout(timeout)
            .send()
            .await?;
        check_status(response.status())?;

        let bytes = crate::providers::read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                crate::providers::BoundedBodyError::TooLarge => {
                    ProviderError::Parse("Aixy returned an oversized usage response.".into())
                }
                crate::providers::BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|_| ProviderError::Parse("Aixy returned non-UTF-8 usage data.".into()))?;
        let usage = model::parse_key_usage(body)?;
        Ok(present::build_result(usage))
    }
}

impl Default for AixyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AixyProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Aixy
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx).await,
            source => Err(ProviderError::UnsupportedSource(source)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// Classify a non-success status without reading or echoing the body.
fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    match status {
        StatusCode::UNAUTHORIZED => Err(ProviderError::AuthRequired),
        StatusCode::FORBIDDEN => Err(ProviderError::Other(
            "Aixy denied access to this key's usage.".into(),
        )),
        StatusCode::NOT_FOUND => Err(ProviderError::Other(
            "This Aixy installation does not provide the usage endpoint. Update the gateway or check its Base URL."
                .into(),
        )),
        StatusCode::TOO_MANY_REQUESTS => Err(ProviderError::Other(format!(
            "Aixy usage request was rate limited (HTTP {status})."
        ))),
        status if status.is_server_error() => Err(ProviderError::Other(format!(
            "Aixy usage service is unavailable (HTTP {status})."
        ))),
        status if !status.is_success() => Err(ProviderError::Other(format!(
            "Aixy usage request failed (HTTP {status})."
        ))),
        _ => Ok(()),
    }
}

/// Build the usage endpoint from a configured base URL.
///
/// Trailing slashes and a trailing `/v1` are removed, any path prefix is kept,
/// and a scheme-less value is treated as HTTPS. Plain HTTP is accepted only
/// for localhost, private-network and `.local` hosts.
fn usage_url(raw: &str) -> Result<Url, ProviderError> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err(ProviderError::Other("Aixy Base URL is empty.".into()));
    }
    if raw.contains(['?', '#']) {
        return Err(ProviderError::Other(
            "Aixy Base URL must not contain a query or fragment.".into(),
        ));
    }
    let candidate = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    let mut url = Url::parse(&candidate)
        .map_err(|_| ProviderError::Other("Aixy Base URL is invalid.".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ProviderError::Other("Aixy Base URL must include a host.".into()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProviderError::Other(
            "Aixy Base URL must not contain embedded credentials.".into(),
        ));
    }
    let allowed = match url.scheme() {
        "https" => true,
        "http" => crate::providers::is_private_network_host(host) || host.ends_with(".local"),
        _ => false,
    };
    if !allowed {
        return Err(ProviderError::Other(
            "Aixy Base URL must use HTTPS; HTTP is allowed only for localhost, private-network or .local hosts."
                .into(),
        ));
    }

    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/v1").unwrap_or(path);
    let path = format!("{path}{USAGE_PATH}");
    url.set_path(&path);
    Ok(url)
}

/// Validate a configured Base URL before saving it to settings. An empty value
/// is valid and selects the hosted gateway.
pub fn validate_gateway_url(raw: &str) -> Result<(), ProviderError> {
    if raw.trim().is_empty() {
        return Ok(());
    }
    usage_url(raw).map(|_| ())
}
