//! Charm Hyper Hypercredit balance provider.
//!
//! Port of upstream `hyper.ts` (v0.75.0). Unless browser cookies are off, a
//! hyper.charm.land session reads the signed-in team dashboard with
//! `GET https://hyper.charm.land/` (same-origin redirects land on
//! `/teams/{team}/dashboard`) and shows the Hypercredit balance, plan, and
//! refresh note from that page. `/v1/credits` accepts bearer keys only, so
//! cookies are never sent there. Auto falls back to the API key when no
//! session cookie exists, the session request cannot connect, or the session
//! is rejected; Web fails closed instead. Every other response is final:
//! malformed balances, rate limits and server errors never fall back.

use async_trait::async_trait;
use regex_lite::Regex;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, COOKIE},
    redirect::Policy,
};
use std::sync::LazyLock;
use std::time::Duration;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response};

const CREDITS_URL: &str = "https://hyper.charm.land/v1/credits";
const DASHBOARD_URL: &str = "https://hyper.charm.land/";
const COOKIE_DOMAIN: &str = "hyper.charm.land";
const CREDENTIAL_TARGET: &str = "codexbar-hyper";
const API_KEY_ENV: &[&str] = &["HYPER_API_KEY"];
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const DASHBOARD_TIMEOUT: Duration = Duration::from_secs(8);
const API_TIMEOUT: Duration = Duration::from_secs(10);

const DASHBOARD_SECTION: &str = "Hypercredits";
const MISSING_CREDENTIAL: &str = "Sign in to hyper.charm.land or configure a Charm Hyper API key.";
const SESSION_REQUEST_FAILED: &str = "Charm Hyper session request failed.";
const SESSION_EXPIRED: &str =
    "Charm Hyper session expired. Sign in again or paste a fresh Cookie header.";
const DASHBOARD_RATE_LIMITED: &str = "Charm Hyper dashboard requests are rate limited.";
const DASHBOARD_BALANCE_INVALID: &str =
    "Charm Hyper dashboard balance is not a non-negative number.";
const API_KEY_REJECTED: &str = "Charm Hyper API key rejected (HTTP 401).";
const API_KEY_FORBIDDEN: &str = "Charm Hyper API key cannot access credits (HTTP 403).";
const RATE_LIMITED: &str = "Charm Hyper credits requests are rate limited.";
const INVALID_JSON: &str = "Charm Hyper credits response is not valid JSON.";
const INVALID_BALANCE: &str = "Charm Hyper balance must be a non-negative number.";

static TEAM_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/teams/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .expect("static team path regex")
});
static DASHBOARD_BALANCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"Hypercredits Available[\s\S]{0,400}?text-5xl font-bold">\s*([0-9][0-9,]*(?:\.[0-9]+)?)"#,
    )
    .expect("static balance regex")
});
static DASHBOARD_PLAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"Plan[\s\S]{0,200}?text-3xl font-bold">([^<]+)"#).expect("static plan regex")
});
static DASHBOARD_REFRESH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Next Hypercredit refresh in ([^<]+)").expect("static refresh regex")
});

pub struct HyperProvider {
    metadata: ProviderMetadata,
    /// Bearer-key requests to `/v1/credits`. Redirects are not followed.
    client: Client,
    /// Browser-session requests to the dashboard. Same-origin redirects are
    /// followed so a signed-in session reaches the team page.
    dashboard_client: Client,
    credits_url: String,
    dashboard_url: String,
}

/// A `/v1/credits` reply. Upstream reads the body as part of the request, so
/// an unreadable success body counts as a failed request; other bodies are
/// never shown and are not read.
struct CreditsReply {
    status: StatusCode,
    body: Vec<u8>,
}

/// A dashboard reply. The final URL is kept because a signed-in session is
/// recognised by the team path it redirects to.
struct DashboardReply {
    status: StatusCode,
    url: String,
    body: Vec<u8>,
}

impl HyperProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .redirect(Policy::none())
            .timeout(API_TIMEOUT)
            .build()
            .expect("Charm Hyper HTTP client configuration is valid");
        let dashboard_client = crate::core::credentialed_http_client_builder()
            .timeout(API_TIMEOUT)
            .build()
            .expect("Charm Hyper dashboard client configuration is valid");
        Self::with_clients(CREDITS_URL, DASHBOARD_URL, client, dashboard_client)
    }

    fn with_clients(
        credits_url: impl Into<String>,
        dashboard_url: impl Into<String>,
        client: Client,
        dashboard_client: Client,
    ) -> Self {
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
            dashboard_client,
            credits_url: credits_url.into(),
            dashboard_url: dashboard_url.into(),
        }
    }

    /// `GET /v1/credits` with a bearer key. Never carries a cookie.
    async fn request_credits(&self, key: &str) -> Result<CreditsReply, ProviderError> {
        let response = self
            .client
            .get(&self.credits_url)
            .header(ACCEPT, "application/json")
            .bearer_auth(key)
            .timeout(API_TIMEOUT)
            .send()
            .await?;
        let status = response.status();
        let is_html = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"));
        let body = if status == StatusCode::OK && !is_html {
            read_body(response).await?
        } else {
            Vec::new()
        };
        Ok(CreditsReply { status, body })
    }

    /// `GET /` with only the browser session cookie.
    async fn request_dashboard(&self, cookie: &str) -> Result<DashboardReply, ProviderError> {
        let response = self
            .dashboard_client
            .get(&self.dashboard_url)
            .header(ACCEPT, "text/html")
            .header(COOKIE, cookie)
            .timeout(DASHBOARD_TIMEOUT)
            .send()
            .await?;
        let status = response.status();
        let url = response.url().to_string();
        let body = if status == StatusCode::OK {
            read_body(response).await?
        } else {
            Vec::new()
        };
        Ok(DashboardReply { status, url, body })
    }

    /// Runs upstream's session-then-key flow with an already resolved key.
    async fn fetch_with_key(
        &self,
        ctx: &FetchContext,
        key: Option<&str>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        self.fetch_with_sessions(ctx, key, || session_cookies(ctx))
            .await
    }

    /// `sessions` is called only on paths that may read a browser.
    async fn fetch_with_sessions<I>(
        &self,
        ctx: &FetchContext,
        key: Option<&str>,
        sessions: impl FnOnce() -> I + Send,
    ) -> Result<ProviderFetchResult, ProviderError>
    where
        I: Iterator<Item = String> + Send,
    {
        if ctx.source_mode == SourceMode::Cli {
            return Err(ProviderError::UnsupportedSource(SourceMode::Cli));
        }
        // Web never falls back, so a configured key is irrelevant there.
        let fallback_key = match ctx.source_mode {
            SourceMode::Web => None,
            _ => key.and_then(cleaned_key),
        };

        // Upstream turns browser cookies off for the API source.
        if ctx.source_mode != SourceMode::OAuth {
            let mut session_rejected = false;
            for cookie in sessions() {
                match self.request_dashboard(&cookie).await {
                    Ok(reply) => {
                        if let Some(result) = classify_dashboard(&reply)? {
                            return Ok(result);
                        }
                        // Upstream also drops its cached browser cookie here.
                        // This port keeps no cookie cache, so the next refresh
                        // reads the browser again. A rejected browser session
                        // moves on to the next browser's session first.
                        session_rejected = true;
                    }
                    Err(error) if fallback_key.is_none() => {
                        tracing::debug!(%error, "Charm Hyper session request failed");
                        return Err(ProviderError::Other(SESSION_REQUEST_FAILED.into()));
                    }
                    Err(error) => {
                        tracing::debug!(%error, "Charm Hyper session request failed; trying the API key");
                        session_rejected = false;
                        break;
                    }
                }
            }
            match (session_rejected, fallback_key.is_some()) {
                (true, false) => return Err(ProviderError::Other(SESSION_EXPIRED.into())),
                (true, true) => {
                    tracing::debug!("Charm Hyper session was rejected; trying the API key");
                }
                (false, false) => return Err(missing_credential()),
                (false, true) => {}
            }
        }

        let Some(key) = fallback_key else {
            return Err(missing_credential());
        };
        let reply = self.request_credits(key).await?;
        if reply.status != StatusCode::OK {
            return Err(status_error(reply.status));
        }
        let balance = parse_balance(&reply.body)?;
        Ok(api_balance_result(balance))
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
        // This repository's SourceMode::OAuth is the documented API-key lane
        // for non-OAuth providers; upstream calls this source "api". Web never
        // uses the key, so it is not read there.
        let key = match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => crate::providers::resolve_api_key(
                ctx.api_key.as_deref(),
                CREDENTIAL_TARGET,
                API_KEY_ENV,
            )
            .ok(),
            SourceMode::Web | SourceMode::Cli => None,
        };
        self.fetch_with_key(ctx, key.as_deref()).await
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::OAuth]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn cookie_source_scopes_session_only(&self) -> bool {
        true
    }

    /// The API source must not read browser cookies (upstream turns them off
    /// there), so the provider imports the session only when it will use it.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            // Upstream classifies both as `authenticationExpired`.
            ProviderError::Other(message)
                if message == SESSION_EXPIRED || message == API_KEY_REJECTED =>
            {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

/// Reads a success body within the shared size limit. Oversized bodies and
/// read failures are reported so the caller can treat them as a failed request.
async fn read_body(response: reqwest::Response) -> Result<Vec<u8>, ProviderError> {
    read_bounded_response(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::Read(error) => ProviderError::Network(error),
            BoundedBodyError::TooLarge => ProviderError::Parse(format!(
                "Charm Hyper response exceeded {MAX_RESPONSE_BYTES} bytes."
            )),
        })
}

/// Upstream's dashboard classification. Rate limits and server errors are
/// final. A 200 that reaches the team dashboard (by final URL or body) is a
/// session and yields its balance. Any other 200, a redirect, 401 or 403 is a
/// rejected session (`None`). Remaining statuses are final.
fn classify_dashboard(
    reply: &DashboardReply,
) -> Result<Option<ProviderFetchResult>, ProviderError> {
    let status = reply.status;
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::Other(DASHBOARD_RATE_LIMITED.into()));
    }
    if status.is_server_error() {
        return Err(ProviderError::Other(format!(
            "Charm Hyper dashboard error: HTTP {}",
            status.as_u16()
        )));
    }
    let page = String::from_utf8_lossy(&reply.body);
    if status == StatusCode::OK && (TEAM_PATH.is_match(&reply.url) || TEAM_PATH.is_match(&page)) {
        return dashboard_balance(&page).map(Some);
    }
    if status != StatusCode::OK
        && status != StatusCode::UNAUTHORIZED
        && status != StatusCode::FORBIDDEN
        && !status.is_redirection()
    {
        return Err(ProviderError::Other(format!(
            "Charm Hyper dashboard returned HTTP {}",
            status.as_u16()
        )));
    }
    Ok(None)
}

/// Reads the balance card and the optional plan and refresh rows from the
/// team dashboard. A missing or non-numeric balance fails closed.
fn dashboard_balance(page: &str) -> Result<ProviderFetchResult, ProviderError> {
    let balance = capture(&DASHBOARD_BALANCE, page)
        .and_then(|raw| raw.replace(',', "").parse::<f64>().ok())
        .filter(|balance| balance.is_finite() && *balance >= 0.0)
        .ok_or_else(|| ProviderError::Parse(DASHBOARD_BALANCE_INVALID.into()))?;
    let plan = capture(&DASHBOARD_PLAN, page)
        .map(collapse_whitespace)
        .filter(|plan| !plan.is_empty());
    let refresh = capture(&DASHBOARD_REFRESH, page)
        .map(collapse_whitespace)
        .filter(|refresh| !refresh.is_empty());
    let rows = [
        ProviderDisplayDetail::new(
            "hypercredits-balance",
            "Balance",
            format!("{} HC", format::number(balance, 2)),
        ),
        plan.and_then(|plan| ProviderDisplayDetail::new("hypercredits-plan", "Plan", plan)),
        refresh.and_then(|refresh| {
            ProviderDisplayDetail::new("hypercredits-refresh", "Refresh", refresh)
        }),
    ];
    let rows = rows
        .into_iter()
        .flatten()
        .filter_map(|row| row.with_section_title(DASHBOARD_SECTION));
    let usage = UsageSnapshot::new(RateWindow::informational("Hypercredit balance"))
        .with_login_method("Browser session");
    Ok(ProviderFetchResult::new(usage, "web").with_display_details(rows))
}

fn capture<'a>(pattern: &Regex, page: &'a str) -> Option<&'a str> {
    pattern
        .captures(page)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str())
}

/// Collapses runs of whitespace (including line breaks in the markup) to one space.
fn collapse_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The session cookies upstream's browser broker would hand out: a manual or
/// shell-resolved header alone, nothing when the cookie source is off or an
/// empty manual source, otherwise each browser's hyper.charm.land session in
/// browser order, read only as far as needed.
fn session_cookies(ctx: &FetchContext) -> Box<dyn Iterator<Item = String> + Send> {
    if let Some(header) = ctx.manual_cookie_header.as_deref() {
        return Box::new(crate::providers::normalize_cookie_header(header).into_iter());
    }
    Box::new(
        crate::providers::browser_cookie_sessions(ctx, &[COOKIE_DOMAIN])
            .filter_map(|header| crate::providers::normalize_cookie_header(&header)),
    )
}

/// Upstream's `SettingsValue.cleaned`: trimmed, one pair of wrapping quotes
/// removed, trimmed again, and blank keys treated as absent.
fn cleaned_key(raw: &str) -> Option<&str> {
    let value = raw.trim();
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    let value = if quoted {
        value.get(1..value.len() - 1).unwrap_or_default()
    } else {
        value
    };
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn status_error(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::Other(API_KEY_REJECTED.into()),
        StatusCode::FORBIDDEN => ProviderError::Other(API_KEY_FORBIDDEN.into()),
        StatusCode::TOO_MANY_REQUESTS => ProviderError::Other(RATE_LIMITED.into()),
        status => ProviderError::Other(format!("Charm Hyper API error: HTTP {}", status.as_u16())),
    }
}

/// Upstream parses any JSON document and requires a finite, non-negative
/// numeric `balance`. serde_json rejects out-of-range numbers such as
/// `1e400` while parsing, so those report invalid JSON; upstream reaches the
/// balance check with `Infinity`. Both are parse failures without fallback.
fn parse_balance(body: &[u8]) -> Result<f64, ProviderError> {
    let payload: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| ProviderError::Parse(INVALID_JSON.into()))?;
    payload
        .get("balance")
        .and_then(serde_json::Value::as_f64)
        .filter(|balance| balance.is_finite() && *balance >= 0.0)
        .ok_or_else(|| ProviderError::Parse(INVALID_BALANCE.into()))
}

fn api_balance_result(balance: f64) -> ProviderFetchResult {
    let detail = ProviderDisplayDetail::new(
        "hypercredits",
        "Hypercredits",
        format!("{} HC", format::number(balance, 2)),
    );
    let usage = UsageSnapshot::new(RateWindow::informational("Hypercredit balance"))
        .with_login_method("API key");
    ProviderFetchResult::new(usage, "api").with_display_detail(detail)
}

fn missing_credential() -> ProviderError {
    ProviderError::NotInstalled(MISSING_CREDENTIAL.into())
}

#[cfg(test)]
mod tests;
