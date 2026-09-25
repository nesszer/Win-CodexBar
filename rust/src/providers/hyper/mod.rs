//! Charm Hyper Hypercredit balance provider.

use async_trait::async_trait;
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::Deserialize;
use std::time::Duration;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const CREDITS_URL: &str = "https://hyper.charm.land/v1/credits";
const COOKIE_DOMAIN: &str = "hyper.charm.land";
const CREDENTIAL_TARGET: &str = "codexbar-hyper";
const API_KEY_ENV: &[&str] = &["HYPER_API_KEY"];
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const API_TIMEOUT: Duration = Duration::from_secs(10);

pub struct HyperProvider {
    metadata: ProviderMetadata,
    client: Client,
    credits_url: String,
}

impl HyperProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .redirect(Policy::none())
            .timeout(API_TIMEOUT)
            .build()
            .expect("Charm Hyper HTTP client configuration is valid");
        Self::with_client(CREDITS_URL, client)
    }

    fn with_client(credits_url: impl Into<String>, client: Client) -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Hyper,
                display_name: "Charm Hyper",
                session_label: "Balance",
                weekly_label: "Balance",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://hyper.charm.land"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client,
            credits_url: credits_url.into(),
        }
    }

    async fn fetch_balance(
        &self,
        key: Option<&str>,
        cookie: Option<&str>,
        source: &'static str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let mut request = self
            .client
            .get(&self.credits_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(if cookie.is_some() {
                SESSION_TIMEOUT
            } else {
                API_TIMEOUT
            });
        if let Some(cookie) = cookie {
            request = request.header(reqwest::header::COOKIE, cookie);
        } else if let Some(key) = key {
            request = request.bearer_auth(key);
        }

        let response = request.send().await?;
        let status = response.status();
        if cookie.is_some() && is_rejected_session(status, response.headers()) {
            return Err(ProviderError::AuthRequired);
        }
        if status != StatusCode::OK {
            return Err(status_error(status));
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::Read(error) => ProviderError::Network(error),
                BoundedBodyError::TooLarge => ProviderError::Parse(format!(
                    "Charm Hyper response exceeded {MAX_RESPONSE_BYTES} bytes."
                )),
            })?;
        let balance = parse_balance(&body)?;
        let detail = ProviderDisplayDetail::new(
            "hypercredits",
            "Hypercredits",
            format!("{} HC", format_balance(balance)),
        );
        let usage = UsageSnapshot::new(RateWindow::informational("Hypercredit balance"))
            .with_login_method(if source == "web" {
                "Browser session"
            } else {
                "API key"
            });
        let result = ProviderFetchResult::new(usage, source).with_display_detail(detail);
        Ok(result)
    }

    async fn fetch_web(
        &self,
        ctx: &FetchContext,
        key: Option<&str>,
        auto_mode: bool,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let manual_cookie = ctx
            .manual_cookie_header
            .as_deref()
            .and_then(crate::providers::normalize_cookie_header);
        let cookie = if manual_cookie.is_some() {
            manual_cookie
        } else if ctx.manual_cookie_header.is_some() || ctx.manual_cookie_missing {
            None
        } else {
            match crate::providers::browser_cookie_header(&[COOKIE_DOMAIN]) {
                Ok(header) => crate::providers::normalize_cookie_header(&header),
                Err(ProviderError::NoCookies) => None,
                Err(_) if auto_mode && key.is_some() => {
                    tracing::debug!("Charm Hyper browser cookie lookup failed; trying API key");
                    None
                }
                Err(_) => None,
            }
        };

        if let Some(cookie) = cookie.as_deref() {
            match self.fetch_balance(None, Some(cookie), "web").await {
                Ok(result) => return Ok(result),
                Err(ProviderError::AuthRequired) if auto_mode && key.is_some() => {
                    tracing::debug!("Charm Hyper browser session was rejected; trying API key");
                }
                Err(ProviderError::Network(_)) if auto_mode && key.is_some() => {
                    tracing::debug!("Charm Hyper session request failed; trying API key");
                }
                Err(_) if auto_mode && key.is_some() => {
                    // Upstream Auto retries with the API key after any
                    // non-session response; an HTTP 429/5xx from the browser
                    // lane must not hide a usable configured API credential.
                    tracing::debug!("Charm Hyper browser response failed; trying API key");
                }
                Err(error) => return Err(error),
            }
        }

        if auto_mode {
            if let Some(key) = key {
                return self.fetch_balance(Some(key), None, "api").await;
            }
            if cookie.is_some() {
                return Err(ProviderError::AuthRequired);
            }
            return Err(missing_credential());
        }

        Err(if cookie.is_some() {
            ProviderError::AuthRequired
        } else {
            missing_credential()
        })
    }
}

impl Default for HyperProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for HyperProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Hyper
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            // This repository's SourceMode::OAuth is the documented API-key
            // lane for non-OAuth providers; upstream calls this source "api".
            SourceMode::OAuth => {
                let key = resolve_key(ctx)?;
                self.fetch_balance(Some(&key), None, "api").await
            }
            SourceMode::Web => self.fetch_web(ctx, None, false).await,
            SourceMode::Auto => {
                let key = resolve_key(ctx).ok();
                self.fetch_web(ctx, key.as_deref(), true).await
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::OAuth]
    }

    fn supports_web(&self) -> bool {
        true
    }
}

fn resolve_key(ctx: &FetchContext) -> Result<String, ProviderError> {
    crate::providers::resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, API_KEY_ENV)
}

fn is_rejected_session(status: StatusCode, headers: &reqwest::header::HeaderMap) -> bool {
    status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || status.is_redirection()
        || (status == StatusCode::OK
            && headers
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.to_ascii_lowercase().contains("text/html")))
}

fn status_error(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::AuthRequired,
        StatusCode::FORBIDDEN => {
            ProviderError::Other("Charm Hyper API key cannot access credits (HTTP 403).".into())
        }
        StatusCode::TOO_MANY_REQUESTS => {
            ProviderError::Other("Charm Hyper credits requests are rate limited.".into())
        }
        status if status.is_server_error() => ProviderError::Other(format!(
            "Charm Hyper credits service is unavailable (HTTP {status})."
        )),
        status => ProviderError::Other(format!("Charm Hyper API error: HTTP {status}.")),
    }
}

#[derive(Debug, Deserialize)]
struct CreditsResponse {
    balance: f64,
}

fn parse_balance(body: &[u8]) -> Result<f64, ProviderError> {
    let response: CreditsResponse = serde_json::from_slice(body).map_err(|_| {
        ProviderError::Parse("Charm Hyper credits response is not valid JSON.".into())
    })?;
    if !response.balance.is_finite() || response.balance < 0.0 {
        return Err(ProviderError::Parse(
            "Charm Hyper balance must be a non-negative number.".into(),
        ));
    }
    Ok(response.balance)
}

fn format_balance(balance: f64) -> String {
    let rounded = if balance <= f64::MAX / 100.0 {
        (balance * 100.0).round() / 100.0
    } else {
        balance
    };
    let formatted = format!("{rounded:.2}");
    let (integer, fraction) = formatted
        .split_once('.')
        .unwrap_or((formatted.as_str(), ""));
    let fraction = fraction.trim_end_matches('0');
    let mut grouped = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, digit) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let integer = grouped.chars().rev().collect::<String>();
    if fraction.is_empty() {
        integer
    } else {
        format!("{integer}.{fraction}")
    }
}

fn missing_credential() -> ProviderError {
    ProviderError::NotInstalled(
        "Sign in to hyper.charm.land or configure a Charm Hyper API key.".into(),
    )
}

#[cfg(test)]
mod tests;
