//! Langdock personal usage from the signed-in web app.
//!
//! Ported from upstream CodexBar v0.73.0 (`Resources/Plugins/langdock.js`,
//! `Providers/Langdock/LangdockProviderDescriptor.swift`). Langdock has no
//! API key: the session cookie `auth_token` from app.langdock.com reads the
//! tRPC `usageSettings.getPersonalUsage` procedure. Upstream reads one Edge
//! profile chosen in Settings; Windows uses an imported or pasted Cookie
//! header, else the first detected browser whose langdock.com cookies hold
//! `auth_token`. Response bodies are never echoed in errors.

mod model;
#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, COOKIE, REFERER},
};

use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderError, ProviderFetchResult,
    ProviderId, ProviderMetadata, ProviderStateKind, SourceMode,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

/// Usage history cannot back a limit reset detector (upstream excludes this
/// provider from plan-utilization history).
pub(crate) const LIMIT_RESET_POLICY: crate::core::ProviderLimitResetPolicy =
    crate::core::ProviderLimitResetPolicy::UNSUPPORTED;

const BASE_URL: &str = "https://app.langdock.com";
const USAGE_PATH: &str = "/api/trpc/usageSettings.getPersonalUsage";
/// `encodeURIComponent(JSON.stringify({0: {json: null, meta: {values:
/// ["undefined"], v: 1}}}))`, the batch input upstream sends.
const USAGE_INPUT: &str = "%7B%220%22%3A%7B%22json%22%3Anull%2C%22meta%22%3A%7B%22values%22%3A%5B%22undefined%22%5D%2C%22v%22%3A1%7D%7D%7D";
const USAGE_REFERER: &str = "https://app.langdock.com/settings/account/usage";
const DASHBOARD_URL: &str = "https://app.langdock.com/settings/account/usage";
/// Upstream's parent cookie domain; browser lookups match its subdomains, so
/// it also covers cookies set on app.langdock.com.
const COOKIE_DOMAIN: &str = "langdock.com";
const SESSION_COOKIE: &str = "auth_token";
const SIGN_IN_URL: &str = "https://app.langdock.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

const SESSION_EXPIRED: &str = "The selected browser profile is no longer signed in to Langdock.";
const MISSING_SESSION: &str = "No Langdock session was found in the selected browser. Sign in at app.langdock.com or paste a Cookie header in Settings.";
const COOKIES_OFF: &str = "Langdock reads usage from a browser session. Turn on browser cookies or paste a Cookie header in Settings.";

/// Every detected browser's `(browser name, Cookie header)` for langdock.com.
type BrowserCookieHeaders = dyn Fn() -> Result<Vec<(String, String)>, ProviderError> + Sync;

pub struct LangdockProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
    base_url: String,
}

impl LangdockProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .cookie_store(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self::with_parts(BASE_URL, client)
    }

    #[cfg(test)]
    fn with_client(base_url: &str, client: Client) -> Self {
        Self::with_parts(base_url.trim_end_matches('/'), Some(client))
    }

    fn with_parts(base_url: &str, client: Option<Client>) -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Langdock,
                display_name: "Langdock",
                session_label: "Session",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(DASHBOARD_URL),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client,
            base_url: base_url.to_owned(),
        }
    }

    async fn fetch_routed(
        &self,
        ctx: &FetchContext,
        browser: &BrowserCookieHeaders,
    ) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => self.fetch_web(ctx, browser).await,
            // The shell maps an Off cookie source to Cli for web-only
            // providers, so explain the setting instead of the source mode.
            SourceMode::Cli => Err(ProviderError::Other(COOKIES_OFF.into())),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    async fn fetch_web(
        &self,
        ctx: &FetchContext,
        browser: &BrowserCookieHeaders,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let cookie = session_cookie(ctx, browser)?;
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure Langdock HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let url = format!("{}{USAGE_PATH}?batch=1&input={USAGE_INPUT}", self.base_url);
        let response = client
            .get(url)
            .header(ACCEPT, "application/json")
            .header(REFERER, USAGE_REFERER)
            .header(COOKIE, cookie)
            .timeout(timeout)
            .send()
            .await?;
        check_status(response.status())?;
        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => model::unexpected_response(),
                BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        model::parse_personal_usage(&body)
    }
}

impl Default for LangdockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LangdockProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Langdock
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        self.fetch_routed(ctx, &|| {
            crate::providers::browser_cookie_headers_for_domain(COOKIE_DOMAIN)
        })
        .await
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    /// Manual means a pasted or imported header only, as for upstream's
    /// other cookie providers, so an empty one must not read a browser.
    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }

    /// Upstream requires `auth_token` when it picks a browser session, so the
    /// provider, not the shell's first-browser lookup, chooses the browser.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            // Upstream classifies a 401 and tRPC UNAUTHORIZED as
            // `authenticationExpired`.
            ProviderError::Other(message) if message == SESSION_EXPIRED => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

/// The imported or pasted Cookie header alone, else the first detected
/// browser whose langdock.com cookies hold `auth_token` (upstream
/// `requiredCookies`). An empty Manual source never reads a browser.
fn session_cookie(
    ctx: &FetchContext,
    browser: &BrowserCookieHeaders,
) -> Result<String, ProviderError> {
    let header = match ctx.manual_cookie_header.as_deref() {
        Some(header) => crate::providers::normalize_cookie_header(header),
        None if ctx.manual_cookie_missing => None,
        None => match browser() {
            Ok(candidates) => candidates.iter().find_map(|(_, header)| {
                crate::providers::normalize_cookie_header(header)
                    .filter(|header| has_session_cookie(header))
            }),
            Err(error) => {
                tracing::debug!(%error, "Langdock browser session is unavailable");
                None
            }
        },
    };
    header
        .filter(|header| has_session_cookie(header))
        .ok_or_else(|| ProviderError::BrowserSignInRequired {
            message: MISSING_SESSION.into(),
            sign_in_url: SIGN_IN_URL.into(),
        })
}

fn has_session_cookie(header: &str) -> bool {
    !crate::providers::cookie_values(header, SESSION_COOKIE).is_empty()
}

/// Upstream's HTTP status taxonomy; 2xx continues to the body.
fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    match status.as_u16() {
        200..=299 => Ok(()),
        401 => Err(model::denied("UNAUTHORIZED")),
        403 => Err(model::denied("FORBIDDEN")),
        429 => Err(model::denied("TOO_MANY_REQUESTS")),
        500.. => Err(model::denied("INTERNAL_SERVER_ERROR")),
        code => Err(ProviderError::Other(format!(
            "Langdock usage request failed with HTTP {code}."
        ))),
    }
}
