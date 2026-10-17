//! X API prepaid credits from the signed-in console.
//!
//! Ported from upstream CodexBar v0.74.0 (`Resources/Plugins/xapi.js`,
//! `Providers/XAPI/XAPIProviderDescriptor.swift`). X API has no API key for
//! prepaid credits: the console session cookies `auth_token` and `ct0` read
//! `/api/me` and `/api/accounts/{account_id}/credits` endpoints. The CSRF
//! cookie value is echoed as `X-CSRF-Token`. A pasted or imported Cookie
//! header is the only session tried. Without one, Automatic tries every
//! detected browser holding both cookies for console.x.com, in order, and
//! moves to the next after a 401 (upstream `rejectCookie`). Response bodies
//! are never echoed in errors and the cookies are never logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode, header::ACCEPT};
use serde_json::{Map, Value};

use crate::core::RateWindow;
use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, ProviderStateKind, SourceMode,
    UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

/// Usage history cannot back a limit reset detector (upstream excludes this
/// provider from plan-utilization history).
pub(crate) const LIMIT_RESET_POLICY: crate::core::ProviderLimitResetPolicy =
    crate::core::ProviderLimitResetPolicy::UNSUPPORTED;

const BASE_URL: &str = "https://console.x.com";
const COOKIE_DOMAIN: &str = "x.com";
const AUTH_COOKIE: &str = "auth_token";
const CSRF_COOKIE: &str = "ct0";
const CSRF_HEADER: &str = "X-CSRF-Token";
const SIGN_IN_URL: &str = "https://console.x.com";
/// Upstream's fixed web timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

/// Upstream's message when every offered session answered 401.
const SESSION_EXPIRED: &str = "X API session expired. Sign in to console.x.com again.";
const MISSING_SESSION: &str = "Sign in to console.x.com in the selected browser, or paste its Cookie header with auth_token and ct0.";
const COOKIES_OFF: &str = "X API cookies are disabled.";

/// Every detected browser's Cookie header for the console, in browser order.
type BrowserCookieHeaders = dyn Fn() -> Result<Vec<(String, String)>, ProviderError> + Sync;

pub struct XapiProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
    base_url: String,
}

impl XapiProvider {
    pub fn new() -> Self {
        Self::with_base_url(BASE_URL)
    }

    fn with_base_url(base_url: &str) -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .cookie_store(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Xapi,
                display_name: "X API",
                session_label: "Balance",
                weekly_label: "Balance",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(SIGN_IN_URL),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client,
            base_url: base_url.to_owned(),
        }
    }

    /// Tries each candidate session in order. Only an expired session moves
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
            ProviderError::Other("Could not create a secure X API HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let mut last_error = None;
        for session in &sessions {
            match fetch_session(client, &self.base_url, session, timeout).await {
                Err(error) if is_session_expired(&error) => last_error = Some(error),
                result => return result,
            }
        }
        Err(last_error.unwrap_or_else(missing_session))
    }
}

impl Default for XapiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::core::Provider for XapiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Xapi
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
        if is_session_expired(error) {
            ProviderStateKind::ExpiredSession
        } else {
            error.state_kind()
        }
    }
}

/// One session's me → credits sequence.
async fn fetch_session(
    client: &Client,
    base_url: &str,
    session: &Session,
    timeout: Duration,
) -> Result<ProviderFetchResult, ProviderError> {
    let me = get_json(client, &format!("{base_url}/api/me"), session, timeout).await?;
    let account_id = parse_account_id(&me)?;
    let credits = get_json(
        client,
        &format!("{base_url}/api/accounts/{account_id}/credits"),
        session,
        timeout,
    )
    .await?;
    parse_credits(&account_id, &credits)
}

fn is_session_expired(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Other(message) if message == SESSION_EXPIRED)
}

/// A console session: the full Cookie header plus the CSRF value it echoes.
#[derive(Clone)]
struct Session {
    cookie_header: String,
    csrf: String,
}

fn missing_session() -> ProviderError {
    ProviderError::BrowserSignInRequired {
        message: MISSING_SESSION.into(),
        sign_in_url: SIGN_IN_URL.into(),
    }
}

/// Candidate sessions in upstream's order, plus a browser read failure worth
/// reporting when no candidate is usable. A pasted or shell-resolved Cookie
/// header is used alone, an empty Manual source offers nothing, and otherwise
/// every browser holding both console cookies is a candidate.
fn sessions(
    ctx: &FetchContext,
    browser: &BrowserCookieHeaders,
) -> (Vec<Session>, Option<ProviderError>) {
    if let Some(header) = ctx.manual_cookie_header.as_deref() {
        let session = crate::providers::normalize_cookie_header(header)
            .and_then(|header| session_from_header(&header));
        return (session.into_iter().collect(), None);
    }
    if ctx.manual_cookie_missing {
        return (Vec::new(), None);
    }
    match browser() {
        Ok(headers) => (
            headers
                .iter()
                .filter_map(|(_, header)| crate::providers::normalize_cookie_header(header))
                .filter_map(|header| session_from_header(&header))
                .collect(),
            None,
        ),
        // No browser holds console cookies: the sign-in hint is clearer.
        Err(ProviderError::NoCookies) => (Vec::new(), None),
        Err(error) => {
            tracing::debug!(%error, "X API browser sessions are unavailable");
            (Vec::new(), Some(error))
        }
    }
}

fn session_from_header(header: &str) -> Option<Session> {
    let first = |name: &str| {
        crate::providers::cookie_values(header, name)
            .into_iter()
            .find(|value| !value.is_empty())
            .map(str::to_owned)
    };
    first(AUTH_COOKIE)?;
    let csrf = first(CSRF_COOKIE)?;
    Some(Session {
        cookie_header: header.to_owned(),
        csrf,
    })
}

async fn get_json(
    client: &Client,
    url: &str,
    session: &Session,
    timeout: Duration,
) -> Result<Map<String, Value>, ProviderError> {
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .header("Cookie", &session.cookie_header)
        .header(CSRF_HEADER, &session.csrf)
        .timeout(timeout)
        .send()
        .await?;

    let status = response.status();
    let body_bytes = read_bounded_response(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::TooLarge => invalid("JSON response"),
            BoundedBodyError::Read(error) => ProviderError::Network(error),
        })?;

    let body = std::str::from_utf8(&body_bytes).map_err(|_| invalid("JSON response"))?;

    // For 400, check if it's code 215 (legacy auth error) before parsing the body
    if status == StatusCode::BAD_REQUEST
        && let Ok(json) = serde_json::from_str::<Value>(body)
        && let Some(errors) = json.get("errors").and_then(|v| v.as_array())
        && errors
            .iter()
            .any(|e| e.get("code").and_then(|c| c.as_i64()) == Some(215))
    {
        return Err(ProviderError::Other(SESSION_EXPIRED.to_owned()));
    }

    check_status(status)?;
    parse_object(body)
}

fn parse_object(text: &str) -> Result<Map<String, Value>, ProviderError> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .ok_or_else(|| invalid("JSON object"))
}

fn parse_account_id(me: &Map<String, Value>) -> Result<String, ProviderError> {
    let id = me
        .get("account")
        .and_then(|v| v.get("id"))
        .ok_or_else(|| invalid("account or dollar balance"))?;

    // Upstream: `Number.isSafeInteger(id) && id >= 0 ? String(id) : id`.
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    let account_id = match id {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && (0.0..=MAX_SAFE_INTEGER).contains(&f) => {
                // `+ 0.0` turns -0 into 0, as JavaScript's String(-0) does.
                format!("{:.0}", f + 0.0)
            }
            _ => return Err(invalid("account or dollar balance")),
        },
        Value::String(s) => s.clone(),
        _ => return Err(invalid("account or dollar balance")),
    };

    // Validate account ID format: ASCII alphanumeric, underscore, hyphen; 1-128 chars
    if account_id.is_empty()
        || account_id.len() > 128
        || !account_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(invalid("account or dollar balance"));
    }

    Ok(account_id)
}

fn parse_credits(
    _account_id: &str,
    credits: &Map<String, Value>,
) -> Result<ProviderFetchResult, ProviderError> {
    // Paid credits: required
    let paid = credits
        .get("credits")
        .and_then(|v| v.get("balance"))
        .ok_or_else(|| invalid("account or dollar balance"))
        .and_then(amount)?;

    // Free credits: absent means 0; present (even null) needs a valid balance.
    let free = match credits.get("freeCredits") {
        None => 0.0,
        Some(v) => amount(v.get("balance").unwrap_or(&Value::Null))?,
    };

    // Upstream `amount(paid + free)`: two finite balances can overflow to infinity.
    let balance = paid + free;
    if !balance.is_finite() {
        return Err(invalid("account or dollar balance"));
    }

    // Upstream has no window: the balance reaches the tray and float bar only.
    let usage = UsageSnapshot::new(
        RateWindow::informational(format!("Balance: {}", super::format::usd(balance)))
            .hidden_on_card(),
    )
    .with_login_method("Browser session");

    // Upstream's cost presenter uses `menuCardStyle: .hidden`, so no cost
    // block: the Credits detail rows are the only card surface.
    let mut result = ProviderFetchResult::new(usage, "web");

    // Add display details as three rows, all with "Credits" section title
    let balance_text = super::format::usd(balance);
    let paid_text = super::format::usd(paid);
    let free_text = super::format::usd(free);

    let detail_rows = vec![
        ProviderDisplayDetail::new("balance", "Balance", balance_text)
            .and_then(|d| d.with_section_title("Credits")),
        ProviderDisplayDetail::new("purchased-credits", "Purchased credits", paid_text)
            .and_then(|d| d.with_section_title("Credits")),
        ProviderDisplayDetail::new("free-credits", "Free credits", free_text)
            .and_then(|d| d.with_section_title("Credits")),
    ];

    let detail_rows: Vec<_> = detail_rows.into_iter().flatten().collect();

    if detail_rows.len() != 3 {
        return Err(invalid("account or dollar balance"));
    }

    result = result.with_display_details(detail_rows);
    Ok(result)
}

/// Any finite JSON number, integers included (upstream `amount`).
fn amount(value: &Value) -> Result<f64, ProviderError> {
    value
        .as_f64()
        .filter(|f| f.is_finite())
        .ok_or_else(|| invalid("account or dollar balance"))
}

fn invalid(field: &str) -> ProviderError {
    ProviderError::Other(format!("X API returned an unrecognized {}.", field))
}

/// Upstream's HTTP status taxonomy; only 200 continues to the body.
/// Note: 400 with code 215 is handled specially in get_json; this is for generic 400s.
pub(crate) fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    let message = match status.as_u16() {
        200 => return Ok(()),
        401 => SESSION_EXPIRED.to_owned(),
        403 => "X developer console access was denied.".to_owned(),
        429 => "X API console is rate limited. Wait before refreshing.".to_owned(),
        500.. => "X developer console is unavailable.".to_owned(),
        code => format!("X developer console returned HTTP {}.", code),
    };
    Err(ProviderError::Other(message))
}
