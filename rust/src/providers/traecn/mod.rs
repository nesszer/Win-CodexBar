//! Trae CN credits (trae.cn).
//!
//! Ported from upstream CodexBar (`Resources/Plugins/traecn.js`,
//! `Providers/TraeCN/TraeCNProviderDescriptor.swift`, #4682). The signed-in
//! browser session, which Trae keeps on the parent `trae.cn` domain (#4732),
//! exchanges its cookies for a token at `GetUserToken`, and that token reads
//! the account's current entitlement list. The token lives only for one
//! refresh and is sent only back to the
//! same API origin. Refresh makes no billable calls, and it infers no reset
//! or renewal date from pack timestamps. Response bodies are never echoed in
//! errors and cookies are never logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, AUTHORIZATION, COOKIE, ORIGIN, REFERER},
};
use serde_json::{Value, json};

use crate::browser::cookies::CookieRequestScope;
use crate::browser::session_scan::{BrowserCookieStore, SystemCookieStore};
use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, ProviderStateKind, RateWindow, SourceMode,
    UsageSnapshot,
};
use crate::providers::{
    BoundedBodyError, browser_cookie_request_sessions_from, format, read_bounded_response,
};

const BASE_URL: &str = "https://api.trae.cn";
/// Optional override for the API origin (proof runs and fake servers).
/// Accepts `https://` or loopback `http://` only; anything else is ignored.
const BASE_URL_ENV: &str = "TRAE_CN_BASE_URL";
/// Automatic browser sessions. Trae keeps its login cookies on the parent
/// `trae.cn` domain (upstream #4732), so the browser is read for `trae.cn`
/// and only the cookies a browser sends to the API origin are kept. Settings
/// "Import from browser" uses the same scope, and pasted or imported cookies
/// are sent only to `api.trae.cn` (`ProviderId::cookie_domain`).
const COOKIE_SCOPE: CookieRequestScope = CookieRequestScope {
    scan_domain: "trae.cn",
    host: "api.trae.cn",
};
const WEB_ORIGIN: &str = "https://www.trae.cn";
const WEB_REFERER: &str = "https://www.trae.cn/";
const SIGN_IN_URL: &str = "https://www.trae.cn/";
const DASHBOARD_URL: &str = "https://www.trae.cn/dashboard#usage";
const TOKEN_PATH: &str = "/cloudide/api/v3/common/GetUserToken";
const ENTITLEMENTS_PATH: &str = "/trae/api/v2/pay/user_current_entitlement_list";
/// Upstream's fixed request timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Upstream's application code for an expired or invalid login.
const SESSION_EXPIRED_CODE: i64 = 1001;
/// The entitlement group that carries the subscription plan name.
const PLAN_GROUP: i64 = 0;
const ACTIVE_PACK: i64 = 1;
const CREDITS_SECTION: &str = "Credits";

const SESSION_EXPIRED: &str =
    "Trae CN session expired. Sign in at trae.cn or paste a fresh Cookie header.";
const MISSING_SESSION: &str =
    "Sign in at trae.cn in a supported browser, or paste its Cookie header.";
const COOKIES_OFF: &str = "Trae CN cookies are disabled.";
const RATE_LIMITED: &str = "Trae CN is rate limited. Try refreshing later.";
const UNAVAILABLE: &str = "Trae CN is temporarily unavailable. Try refreshing later.";
const MALFORMED: &str = "Trae CN usage response format changed. Refresh to try again, or update CodexBar or report the issue.";
const NO_ALLOWANCE: &str = "No Trae CN credit allowance";

static TRAE_CN_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::TraeCn,
    display_name: "Trae CN",
    // Distinct from the "Credits" detail section so the gauge and the
    // breakdown get separate names in the usage-item pickers.
    session_label: "Usage",
    weekly_label: "Window",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some(DASHBOARD_URL),
    status_page_url: None,
    tertiary_label_key: None,
};

pub struct TraeCnProvider {
    client: Option<Client>,
    base_url: String,
}

/// Why one browser session did not produce usage.
enum SessionError {
    /// Trae rejected the session; the next browser session may still work.
    Rejected,
    Failed(ProviderError),
}

impl From<ProviderError> for SessionError {
    fn from(error: ProviderError) -> Self {
        SessionError::Failed(error)
    }
}

/// The subset of the entitlement response that the card shows.
#[derive(Debug, PartialEq)]
struct Entitlements {
    used: f64,
    total: f64,
    plan: Option<String>,
}

/// The API origin: a valid `TRAE_CN_BASE_URL` override, otherwise production.
/// An invalid override is ignored with a warning.
fn base_url_from_override(raw: Option<&str>) -> String {
    let Some(raw) = raw.map(clean_override).filter(|raw| !raw.is_empty()) else {
        return BASE_URL.to_owned();
    };
    match validated_base_url(&raw) {
        Some(base) => base,
        None => {
            tracing::warn!(
                "Ignoring {BASE_URL_ENV}: use an https:// URL or a loopback http:// URL without credentials, query, or fragment"
            );
            BASE_URL.to_owned()
        }
    }
}

fn clean_override(raw: &str) -> String {
    let value = raw.trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|inner| inner.strip_suffix('\''))
        })
        .unwrap_or(value);
    unquoted.trim().to_owned()
}

/// Same policy as the Sub2API base URL: `https://`, or `http://` only for a
/// loopback host, with no userinfo, query, fragment, or encoded host delimiters.
/// Returns the URL without a trailing slash.
fn validated_base_url(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    if ["%2f", "%5c", "%3f", "%23", "%40", "%3a"]
        .iter()
        .any(|encoded| lower.contains(encoded))
    {
        return None;
    }
    let url = reqwest::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    let scheme_ok = match url.scheme() {
        "https" => true,
        "http" => is_loopback_host(host),
        _ => false,
    };
    if !scheme_ok
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || host.contains('%')
        || host.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    if normalized == "localhost" {
        return true;
    }
    let ip_candidate = normalized
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&normalized);
    match ip_candidate.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    }
}

impl TraeCnProvider {
    pub fn new() -> Self {
        let override_url = std::env::var(BASE_URL_ENV).ok();
        Self::with_base_url(&base_url_from_override(override_url.as_deref()))
    }

    fn with_base_url(base_url: &str) -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .cookie_store(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    /// A pasted header is the only session tried. Otherwise each browser
    /// holding a `trae.cn` session that the API origin receives is tried in
    /// browser order, moving on only after Trae rejects a session (upstream
    /// `rejectCookie`).
    async fn fetch_web<S: BrowserCookieStore>(
        &self,
        ctx: &FetchContext,
        store: S,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure Trae CN HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        if let Some(raw) = ctx.manual_cookie_header.as_deref() {
            let cookie =
                crate::providers::normalize_cookie_header(raw).ok_or_else(missing_session)?;
            return match self.fetch_session(client, &cookie, timeout).await {
                Ok(result) => Ok(result),
                Err(SessionError::Rejected) => Err(session_expired()),
                Err(SessionError::Failed(error)) => Err(error),
            };
        }
        if ctx.manual_cookie_missing {
            return Err(missing_session());
        }

        let mut sessions = browser_cookie_request_sessions_from(store, ctx, COOKIE_SCOPE);
        let mut rejected = false;
        for header in sessions.by_ref() {
            let Some(cookie) = crate::providers::normalize_cookie_header(&header) else {
                continue;
            };
            match self.fetch_session(client, &cookie, timeout).await {
                Ok(result) => return Ok(result),
                Err(SessionError::Rejected) => rejected = true,
                Err(SessionError::Failed(error)) => return Err(error),
            }
        }
        if rejected {
            return Err(session_expired());
        }
        match sessions.exhaustion_error() {
            ProviderError::NoCookies => Err(missing_session()),
            error => {
                tracing::debug!(%error, "Trae CN browser sessions are unavailable");
                Err(error)
            }
        }
    }

    async fn fetch_session(
        &self,
        client: &Client,
        cookie: &str,
        timeout: Duration,
    ) -> Result<ProviderFetchResult, SessionError> {
        let token_payload = self
            .post_json(client, TOKEN_PATH, cookie, &json!({}), None, timeout)
            .await?;
        let token = token_payload
            .get("Result")
            .and_then(|result| result.get("Token"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(malformed)?;

        let payload = self
            .post_json(
                client,
                ENTITLEMENTS_PATH,
                cookie,
                &json!({ "require_usage": true, "full_data": true }),
                Some(token),
                timeout,
            )
            .await?;
        if payload.get("code").and_then(Value::as_i64) == Some(SESSION_EXPIRED_CODE) {
            return Err(SessionError::Rejected);
        }
        let entitlements = parse_entitlements(&payload)?;
        Ok(build_result(&entitlements))
    }

    /// One JSON POST. A 401, 403 or redirect means Trae rejected the session.
    async fn post_json(
        &self,
        client: &Client,
        path: &str,
        cookie: &str,
        body: &Value,
        token: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, SessionError> {
        let mut request = client
            .post(format!("{}{path}", self.base_url))
            .header(ACCEPT, "application/json")
            .header(COOKIE, cookie)
            .header(ORIGIN, WEB_ORIGIN)
            .header(REFERER, WEB_REFERER)
            .json(body)
            .timeout(timeout);
        if let Some(token) = token {
            request = request.header(AUTHORIZATION, format!("Cloud-IDE-JWT {token}"));
        }
        let response = request
            .send()
            .await
            .map_err(|error| SessionError::Failed(ProviderError::Network(error)))?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
            || status.is_redirection()
        {
            return Err(SessionError::Rejected);
        }
        match status {
            StatusCode::OK => {}
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(ProviderError::Other(RATE_LIMITED.into()).into());
            }
            StatusCode::REQUEST_TIMEOUT => {
                return Err(ProviderError::Other(UNAVAILABLE.into()).into());
            }
            status if status.is_server_error() => {
                return Err(ProviderError::Other(UNAVAILABLE.into()).into());
            }
            status => {
                return Err(ProviderError::Other(format!(
                    "Trae CN request failed (HTTP {}). Try refreshing later.",
                    status.as_u16()
                ))
                .into());
            }
        }
        let bytes = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| {
                SessionError::Failed(match error {
                    BoundedBodyError::Read(error) => ProviderError::Network(error),
                    BoundedBodyError::TooLarge => malformed(),
                })
            })?;
        serde_json::from_slice(&bytes).map_err(|_| malformed().into())
    }
}

impl Default for TraeCnProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for TraeCnProvider {
    fn id(&self) -> ProviderId {
        ProviderId::TraeCn
    }

    fn metadata(&self) -> &ProviderMetadata {
        &TRAE_CN_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => self.fetch_web(ctx, SystemCookieStore).await,
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

    /// Manual mode sends only the pasted header, so an empty manual source
    /// must not fall back to a browser session.
    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }

    /// The provider walks browsers itself so a rejected session can move on
    /// to the next browser.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn cookie_request_scope(&self) -> Option<CookieRequestScope> {
        Some(COOKIE_SCOPE)
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

fn session_expired() -> ProviderError {
    ProviderError::Other(SESSION_EXPIRED.into())
}

fn missing_session() -> ProviderError {
    ProviderError::BrowserSignInRequired {
        message: MISSING_SESSION.into(),
        sign_in_url: SIGN_IN_URL.into(),
    }
}

fn malformed() -> ProviderError {
    ProviderError::Parse(MALFORMED.into())
}

/// A non-negative JSON number. Numeric strings are rejected, as upstream does.
fn amount(value: Option<&Value>) -> Result<f64, ProviderError> {
    value
        .and_then(Value::as_f64)
        .filter(|amount| amount.is_finite() && *amount >= 0.0)
        .ok_or_else(malformed)
}

/// Upstream `group_type === 0` with `status` absent or `1`.
fn is_plan_pack(pack: &Value) -> bool {
    pack.get("group_type").and_then(Value::as_i64) == Some(PLAN_GROUP)
        && pack
            .get("status")
            .is_none_or(|status| status.as_i64() == Some(ACTIVE_PACK))
}

fn parse_entitlements(payload: &Value) -> Result<Entitlements, ProviderError> {
    if payload.get("is_credits_billing").and_then(Value::as_bool) != Some(true) {
        return Err(malformed());
    }
    let summary = payload
        .get("usage_summary")
        .filter(|summary| summary.is_object())
        .ok_or_else(malformed)?;
    let total = amount(summary.get("total_amount"))?;
    let used = amount(summary.get("consumed_amount"))?;
    let plan = payload
        .get("user_entitlement_pack_list")
        .and_then(Value::as_array)
        .and_then(|packs| packs.iter().find(|pack| is_plan_pack(pack)))
        .and_then(|pack| pack.get("display_desc"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
        .map(str::to_owned);
    Ok(Entitlements { used, total, plan })
}

/// Upstream returns no reset and no window. A zero allowance keeps its
/// details but gets no gauge.
fn build_result(entitlements: &Entitlements) -> ProviderFetchResult {
    let Entitlements { used, total, plan } = entitlements;
    let primary = if *total > 0.0 {
        RateWindow::new((used / total * 100.0).min(100.0))
    } else {
        RateWindow::informational(NO_ALLOWANCE).hidden_on_card()
    };
    let mut usage = UsageSnapshot::new(primary);
    if let Some(plan) = plan {
        usage = usage.with_login_method(plan.clone());
    }

    let remaining = (total - used).max(0.0);
    let rows = [
        ("credits-used", "Used", *used),
        ("credits-total", "Total", *total),
        ("credits-remaining", "Remaining", remaining),
    ]
    .into_iter()
    .filter_map(|(id, title, value)| {
        ProviderDisplayDetail::new(id, title, format::number(value, 2))
            .and_then(|row| row.with_section_title(CREDITS_SECTION))
    });
    ProviderFetchResult::new(usage, "web").with_display_details(rows)
}
