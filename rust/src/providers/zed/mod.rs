//! Zed provider: editor credential lane (default) and opt-in browser billing.

mod snapshot;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};

use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderError, ProviderFetchResult,
    ProviderId, ProviderStateKind, SourceMode,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const CREDENTIAL_TARGET: &str = "codexbar-zed";
const DEFAULT_URL: &str = "https://cloud.zed.dev/client/users/me";
const BILLING_URL: &str = "https://cloud.zed.dev/frontend/billing/usage";
const COOKIE_DOMAIN: &str = "zed.dev";
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const SESSION_EXPIRED: &str =
    "Zed browser session expired. Sign in to zed.dev in Chrome or update the Cookie header.";
const MISSING_SESSION: &str =
    "Sign in to zed.dev in a supported browser or paste a Cookie header to read token spend.";
const COOKIES_DISABLED: &str =
    "Enable Zed browser cookies or paste a Cookie header to read token spend.";

pub struct ZedProvider {
    client: Client,
    billing_url: String,
}

impl ZedProvider {
    pub fn new() -> Self {
        Self::with_client(
            BILLING_URL,
            crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        )
    }

    fn with_client(billing_url: impl Into<String>, client: Client) -> Self {
        Self {
            client,
            billing_url: billing_url.into(),
        }
    }

    async fn fetch_editor(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &["ZED_API_KEY", "ZED_CREDENTIALS"],
        )?;
        let url = ctx.workspace_id.as_deref().unwrap_or(DEFAULT_URL);
        let request = self
            .client
            .get(url)
            .header(reqwest::header::AUTHORIZATION, key.trim())
            .header(reqwest::header::ACCEPT, "application/json");
        let body = read_body(request, false).await?;
        snapshot::editor_result(&body, chrono::Utc::now())
    }

    async fn fetch_web(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let cookie = web_cookie(ctx)?;
        let request = self
            .client
            .get(&self.billing_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::COOKIE, cookie);
        let body = read_body(request, true).await?;
        snapshot::web_result(&body)
    }
}

impl Default for ZedProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ZedProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Zed
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            // The editor credential is the default lane. Browser billing is
            // opt-in (Web only) and the two lanes are never combined.
            SourceMode::Auto | SourceMode::OAuth => self.fetch_editor(ctx).await,
            SourceMode::Web => self.fetch_web(ctx).await,
            // The shell maps a disabled cookie source to `Cli`; it must not
            // import cookies or send a request.
            SourceMode::Cli => Err(ProviderError::NotInstalled(COOKIES_DISABLED.into())),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::OAuth]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn web_is_opt_in(&self) -> bool {
        true
    }

    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == SESSION_EXPIRED => {
                ProviderStateKind::ExpiredSession
            }
            other => other.state_kind(),
        }
    }
}

async fn read_body(request: reqwest::RequestBuilder, web: bool) -> Result<Vec<u8>, ProviderError> {
    let response = request.send().await?;
    let status = response.status();
    if status != StatusCode::OK {
        return Err(status_error(status, web));
    }
    read_bounded_response(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::Read(error) => ProviderError::Network(error),
            BoundedBodyError::TooLarge => ProviderError::Parse(format!(
                "Zed usage response exceeded {MAX_RESPONSE_BYTES} bytes."
            )),
        })
}

/// Cookie for the browser lane: a pasted header, else the browser session.
/// A manual source with no usable header never falls back to the browser.
fn web_cookie(ctx: &FetchContext) -> Result<String, ProviderError> {
    let missing = || ProviderError::NotInstalled(MISSING_SESSION.into());
    if let Some(raw) = ctx.manual_cookie_header.as_deref() {
        return crate::providers::normalize_cookie_header(raw).ok_or_else(missing);
    }
    if ctx.manual_cookie_missing {
        return Err(missing());
    }
    match crate::providers::browser_cookie_header(&[COOKIE_DOMAIN]) {
        Ok(header) => crate::providers::normalize_cookie_header(&header).ok_or_else(missing),
        Err(ProviderError::NoCookies) => Err(missing()),
        Err(error) => Err(error),
    }
}

fn status_error(status: StatusCode, web: bool) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if web => {
            ProviderError::Other(SESSION_EXPIRED.into())
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::AuthRequired,
        StatusCode::TOO_MANY_REQUESTS => {
            ProviderError::Other("Zed usage requests are rate limited.".into())
        }
        status if status.is_server_error() => ProviderError::Other(format!(
            "Zed cloud API is unavailable (HTTP {}).",
            status.as_u16()
        )),
        status => ProviderError::Other(format!("Zed cloud API returned HTTP {}.", status.as_u16())),
    }
}

#[cfg(test)]
mod tests;
