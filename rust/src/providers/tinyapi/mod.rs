//! TinyApi available credit balance provider.
//!
//! Fetches account available credit balance from `GET https://tinyapi.rest/api/user/credits`
//! using signed-in browser session cookies or manual Cookie headers. Tries each browser's
//! cookies in order; skips rejected (401/403) sessions and moves to the next candidate.
//! A pasted or manual Cookie header is the only session tried. Without one, Automatic tries
//! every detected browser holding cookies for tinyapi.rest, in browser order.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/tinyapi.js`).

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::Value;

use super::format;
use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, ProviderStateKind, RateWindow, SourceMode,
    UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

/// Usage history cannot back a limit reset detector (upstream excludes this
/// provider from plan-utilization history).
pub(crate) const LIMIT_RESET_POLICY: crate::core::ProviderLimitResetPolicy =
    crate::core::ProviderLimitResetPolicy::UNSUPPORTED;

const USAGE_URL: &str = "https://tinyapi.rest/api/user/credits";
const COOKIE_DOMAIN: &str = "tinyapi.rest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

const COOKIES_OFF: &str = "TinyApi cookies are disabled.";
const SESSION_EXPIRED: &str =
    "TinyApi session expired. Sign in again or paste a fresh Cookie header.";
const MISSING_SESSION: &str =
    "Sign in to tinyapi.rest in the selected browser. Or set a manual Cookie header.";

/// Every detected browser's Cookie header for tinyapi.rest, in browser order.
type BrowserCookieHeaders = dyn Fn() -> Result<Vec<(String, String)>, ProviderError> + Sync;

pub struct TinyApiProvider {
    client: Option<Client>,
    usage_url: String,
}

impl TinyApiProvider {
    pub fn new() -> Self {
        Self::with_usage_url(USAGE_URL)
    }

    fn with_usage_url(usage_url: &str) -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .cookie_store(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self {
            client,
            usage_url: usage_url.to_owned(),
        }
    }

    /// Tries each candidate session in order. Only an expired session (401/403)
    /// moves on to the next candidate; any other failure ends the fetch.
    async fn fetch_web(
        &self,
        ctx: &FetchContext,
        browser: &BrowserCookieHeaders,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let (sessions, browser_error) = sessions(ctx, browser);
        if sessions.is_empty() {
            return Err(browser_error.unwrap_or_else(missing_session));
        }
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure TinyApi HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let mut last_error = None;
        for cookie_header in sessions {
            match fetch_credits(client, &self.usage_url, &cookie_header, timeout).await {
                Err(error) if is_session_expired(&error) => last_error = Some(error),
                result => return result,
            }
        }
        Err(last_error.unwrap_or_else(missing_session))
    }
}

impl Default for TinyApiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for TinyApiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::TinyApi
    }

    fn metadata(&self) -> &ProviderMetadata {
        &TINYAPI_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                self.fetch_web(ctx, &|| {
                    crate::providers::browser_cookie_headers_for_domain(COOKIE_DOMAIN)
                })
                .await
            }
            // The shell maps an Off cookie source to Cli for web-only
            // providers; upstream reports that cookies are disabled.
            SourceMode::Cli => Err(ProviderError::Other(COOKIES_OFF.into())),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    /// Upstream sends only the pasted header in Manual mode, so an empty
    /// manual source must not fall back to a browser session.
    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }

    /// The shell would import only the first browser's cookies; the provider
    /// reads every browser so an expired session can move to the next one.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        if is_session_expired(error) {
            ProviderStateKind::ExpiredSession
        } else {
            error.state_kind()
        }
    }
}

/// Fetch credits from a single cookie header.
async fn fetch_credits(
    client: &Client,
    usage_url: &str,
    cookie_header: &str,
    timeout: Duration,
) -> Result<ProviderFetchResult, ProviderError> {
    let response = client
        .get(usage_url)
        .header(reqwest::header::COOKIE, cookie_header)
        .timeout(timeout)
        .send()
        .await?;

    let status = response.status();
    check_status(status)?;

    let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::TooLarge => invalid_response(),
            BoundedBodyError::Read(error) => ProviderError::from(error),
        })?;

    let root: Value = serde_json::from_slice(&body)
        .map_err(|_| ProviderError::Other("TinyApi credits response is not valid JSON.".into()))?;

    parse_usage(&root)
}

fn is_session_expired(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Other(message) if message == SESSION_EXPIRED)
}

fn missing_session() -> ProviderError {
    ProviderError::BrowserSignInRequired {
        message: MISSING_SESSION.into(),
        sign_in_url: "https://tinyapi.rest".into(),
    }
}

/// Candidate sessions in upstream's order: a pasted or shell-resolved Cookie
/// header is used alone, an empty Manual source offers nothing, and otherwise
/// every browser holding cookies for tinyapi.rest is a candidate.
fn sessions(
    ctx: &FetchContext,
    browser: &BrowserCookieHeaders,
) -> (Vec<String>, Option<ProviderError>) {
    if let Some(header) = ctx.manual_cookie_header.as_deref() {
        let header = crate::providers::normalize_cookie_header(header);
        return (header.into_iter().collect(), None);
    }
    if ctx.manual_cookie_missing {
        return (Vec::new(), None);
    }
    match browser() {
        Ok(headers) => (
            headers
                .iter()
                .filter_map(|(_, header)| crate::providers::normalize_cookie_header(header))
                .collect(),
            None,
        ),
        // No browser holds tinyapi cookies: the sign-in hint is clearer.
        Err(ProviderError::NoCookies) => (Vec::new(), None),
        Err(error) => {
            tracing::debug!(%error, "TinyApi browser sessions are unavailable");
            (Vec::new(), Some(error))
        }
    }
}

/// HTTP status taxonomy; only 200 continues to the body.
fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    let message = match status.as_u16() {
        200 => return Ok(()),
        401 | 403 => SESSION_EXPIRED.to_owned(),
        429 => "TinyApi credits requests are rate limited.".to_owned(),
        code @ 500.. => format!("TinyApi credits service returned HTTP {code}."),
        code => format!("TinyApi credits request returned HTTP {code}."),
    };
    Err(ProviderError::Other(message))
}

fn invalid_response() -> ProviderError {
    ProviderError::Other("TinyApi credits response is missing a valid available balance.".into())
}

pub fn parse_usage(root: &Value) -> Result<ProviderFetchResult, ProviderError> {
    let root = root.as_object().ok_or_else(invalid_response)?;

    // Check success field
    if root.get("success") != Some(&Value::Bool(true)) {
        return Err(invalid_response());
    }

    // Extract available balance
    let data = root
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(invalid_response)?;

    let balance = data
        .get("totalAvailable")
        .and_then(|v| v.as_f64())
        .ok_or_else(invalid_response)?;

    // Validate: must be finite and non-negative
    if !balance.is_finite() || balance < 0.0 {
        return Err(invalid_response());
    }

    // Format balance with up to 2 decimal places
    let balance_text = format::number(balance, 2);
    let value_text = format!("{} credits", balance_text);

    let primary =
        RateWindow::informational(format!("Available credits: {value_text}")).hidden_on_card();
    let usage = UsageSnapshot::new(primary).with_login_method("Browser session");

    let mut result = ProviderFetchResult::new(usage, "web");
    result = result.with_display_detail(
        ProviderDisplayDetail::new("tinyapi-available", "Available credits", value_text)
            .and_then(|d| d.with_section_title("Credits")),
    );
    Ok(result)
}

const TINYAPI_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::TinyApi,
    display_name: "TinyApi",
    session_label: "Credits",
    weekly_label: "Credits",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://tinyapi.rest/dashboard"),
    status_page_url: None,
    tertiary_label_key: None,
};
