//! Parallel prepaid credit balance from the platform billing console.
//!
//! Ported from upstream CodexBar v0.75.0 (`Resources/Plugins/parallel.ts`,
//! `Providers/Parallel/ParallelProviderDescriptor.swift`). Parallel has no
//! API key for this balance: the signed-in platform.parallel.ai session cookie
//! posts an empty JSON body to the dashboard's billing RPC. Upstream reads
//! Chrome only; this port uses the shared browser order and the explicit
//! cookie-source picker, like X API. A pasted or imported Cookie header is
//! the only session tried in Manual mode. Automatic tries every detected
//! browser holding a session for platform.parallel.ai, in order, and moves to
//! the next one after a signed-out response (upstream `rejectCookie`).
//! Response bodies are never echoed in errors and the cookies are never logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, COOKIE, ORIGIN},
};
use serde_json::Value;

use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, ProviderStateKind, RateWindow, SourceMode,
    UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response};

const BALANCE_URL: &str = "https://platform.parallel.ai/api/rpc/billing/payments/refresh-balance";
const ORIGIN_URL: &str = "https://platform.parallel.ai";
const COOKIE_DOMAIN: &str = "platform.parallel.ai";
const SIGN_IN_URL: &str = "https://platform.parallel.ai/login";
const DASHBOARD_URL: &str = "https://platform.parallel.ai";
/// Upstream's fixed web timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const SECTION_TITLE: &str = "Account balance";

const SESSION_EXPIRED: &str = "Parallel session expired. Sign in to platform.parallel.ai again or paste a fresh Cookie header.";
const MISSING_SESSION: &str =
    "Sign in to platform.parallel.ai in a supported browser, or paste a Cookie header.";
const COOKIES_OFF: &str = "Parallel cookies are disabled.";
const FORMAT_CHANGED: &str = "Parallel billing response format changed.";

/// Every detected browser's Cookie header for the console, in browser order.
type BrowserCookieHeaders = dyn Fn() -> Result<Vec<(String, String)>, ProviderError> + Sync;

pub struct ParallelProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
    balance_url: String,
}

/// One answer from the billing RPC. Signed-out replies carry no body.
enum Reply {
    SignedOut,
    Answer { status: StatusCode, body: Vec<u8> },
}

impl ParallelProvider {
    pub fn new() -> Self {
        Self::with_balance_url(BALANCE_URL)
    }

    fn with_balance_url(balance_url: &str) -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .cookie_store(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Parallel,
                display_name: "Parallel",
                session_label: "Balance",
                weekly_label: "Balance",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(DASHBOARD_URL),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client,
            balance_url: balance_url.to_owned(),
        }
    }

    /// Tries each candidate session in order. Only a signed-out session moves
    /// on to the next candidate; any other failure ends the fetch.
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
            ProviderError::Other("Could not create a secure Parallel HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let mut rejected = false;
        for cookie in &sessions {
            match post_balance(client, &self.balance_url, cookie, timeout).await? {
                Reply::SignedOut => rejected = true,
                Reply::Answer { status, body } => return answer(status, &body),
            }
        }
        if rejected {
            Err(ProviderError::Other(SESSION_EXPIRED.into()))
        } else {
            Err(missing_session())
        }
    }
}

impl Default for ParallelProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ParallelProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Parallel
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
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
        match error {
            ProviderError::Other(message) if message == SESSION_EXPIRED => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

/// Candidate Cookie headers in upstream's order, plus a browser read failure
/// worth reporting when no candidate is usable. A pasted or shell-resolved
/// header is used alone, an empty Manual source offers nothing, and otherwise
/// every browser holding a platform.parallel.ai session is a candidate.
fn sessions(
    ctx: &FetchContext,
    browser: &BrowserCookieHeaders,
) -> (Vec<String>, Option<ProviderError>) {
    if let Some(header) = ctx.manual_cookie_header.as_deref() {
        return (
            crate::providers::normalize_cookie_header(header)
                .into_iter()
                .collect(),
            None,
        );
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
        // No browser holds console cookies: the sign-in hint is clearer.
        Err(ProviderError::NoCookies) => (Vec::new(), None),
        Err(error) => {
            tracing::debug!(%error, "Parallel browser sessions are unavailable");
            (Vec::new(), Some(error))
        }
    }
}

fn missing_session() -> ProviderError {
    ProviderError::BrowserSignInRequired {
        message: MISSING_SESSION.into(),
        sign_in_url: SIGN_IN_URL.into(),
    }
}

/// Upstream's signed-out taxonomy: auth failures, redirects, and an HTML
/// sign-in page all mean the session was rejected.
fn is_signed_out(status: StatusCode, is_html: bool) -> bool {
    status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || status.is_redirection()
        || (status == StatusCode::OK && is_html)
}

async fn post_balance(
    client: &Client,
    url: &str,
    cookie: &str,
    timeout: Duration,
) -> Result<Reply, ProviderError> {
    let response = client
        .post(url)
        .header(ACCEPT, "application/json")
        .header(COOKIE, cookie)
        .header(ORIGIN, ORIGIN_URL)
        .json(&serde_json::json!({}))
        .timeout(timeout)
        .send()
        .await?;

    let status = response.status();
    let is_html = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"));
    if is_signed_out(status, is_html) {
        return Ok(Reply::SignedOut);
    }
    if status != StatusCode::OK {
        return Ok(Reply::Answer {
            status,
            body: Vec::new(),
        });
    }
    let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::Read(error) => ProviderError::Network(error),
            BoundedBodyError::TooLarge => ProviderError::Parse(FORMAT_CHANGED.into()),
        })?;
    Ok(Reply::Answer { status, body })
}

/// Maps a non-signed-out reply. Upstream's 429, 408/5xx, and other non-200
/// classes all surface the same HTTP message here, since the tray has no
/// typed rate-limit variant.
fn answer(status: StatusCode, body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    if status != StatusCode::OK {
        return Err(ProviderError::Other(format!(
            "Parallel returned HTTP {}.",
            status.as_u16()
        )));
    }
    parse_balance(body)
}

fn format_changed() -> ProviderError {
    ProviderError::Parse(FORMAT_CHANGED.into())
}

/// Upstream requires `kind: "ok"` and a finite `data.balance` in cents. A
/// non-zero finite `pendingDebitBalanceCents` adds a second row.
fn parse_balance(body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let payload: Value = serde_json::from_slice(body).map_err(|_| format_changed())?;
    let data = payload
        .as_object()
        .filter(|root| root.get("kind").and_then(Value::as_str) == Some("ok"))
        .and_then(|root| root.get("data"))
        .and_then(Value::as_object)
        .ok_or_else(format_changed)?;
    let balance = finite_cents(data.get("balance")).ok_or_else(format_changed)?;
    let pending = finite_cents(data.get("pendingDebitBalanceCents")).filter(|cents| *cents != 0.0);

    let balance_text = format::usd(balance / 100.0);
    let usage = UsageSnapshot::new(
        RateWindow::informational(format!("Balance: {balance_text}")).hidden_on_card(),
    )
    .with_login_method("Browser session");

    let mut rows = vec![
        ProviderDisplayDetail::new("balance", "Balance", balance_text)
            .and_then(|detail| detail.with_section_title(SECTION_TITLE)),
    ];
    if let Some(pending) = pending {
        rows.push(
            ProviderDisplayDetail::new(
                "pending-debit",
                "Pending debit",
                format::usd(pending / 100.0),
            )
            .and_then(|detail| detail.with_section_title(SECTION_TITLE)),
        );
    }
    Ok(ProviderFetchResult::new(usage, "web").with_display_details(rows.into_iter().flatten()))
}

fn finite_cents(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|cents| cents.is_finite())
}
