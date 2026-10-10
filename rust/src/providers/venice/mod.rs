//! Venice provider implementation.
//!
//! Fetches API balance data from Venice's billing endpoint.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};

const VENICE_BALANCE_URL: &str = "https://api.venice.ai/api/v1/billing/balance";
const VENICE_SESSION_URL: &str = "https://outerface.venice.ai/api/user/session";
const VENICE_CREDENTIAL_TARGET: &str = "codexbar-venice";
const VENICE_SESSION_COOKIE: &str = "__venice-auth.session-token";
const VENICE_COOKIE_DOMAIN: &str = "venice.ai";
const VENICE_CLERK_SESSION_COOKIE: &str = "__session";
const VENICE_MISSING_CREDENTIALS_MESSAGE: &str = "Venice session cookie not found (__session, __session_<suffix>, or __venice-auth.session-token). Open a signed-in venice.ai tab and retry, or paste a fresh Cookie header.";
const VENICE_INVALID_SESSION_MESSAGE: &str = "Venice browser session is invalid or expired. Keep a signed-in venice.ai tab active and retry; Clerk sessions last about 60 seconds. In Manual mode, paste a fresh Cookie header.";
const VENICE_EXPIRATION_SKEW_SECS: i64 = 60;
const MAX_VENICE_COOKIE_HEADER_LEN: usize = 1_048_576;
const MAX_VENICE_COOKIE_VALUE_LEN: usize = 16_384;
const MAX_VENICE_COOKIE_CHUNKS: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VeniceBalanceResponse {
    can_consume: bool,
    consumption_currency: Option<String>,
    balances: VeniceBalances,
    diem_epoch_allocation: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct VeniceBalances {
    diem: Option<f64>,
    usd: Option<f64>,
}

/// Why a `/api/auth/session` reply carried no usable session token.
#[derive(Debug, PartialEq, Eq)]
enum SessionTokenError {
    /// The reply parsed but has a missing, null, non-string or blank `token`.
    /// Upstream treats this as invalid credentials (an expired Clerk session).
    Invalid,
    /// The body is not a JSON object.
    Malformed(String),
}

/// Extracts the session token from a `/api/auth/session` body the way upstream
/// `VeniceWebUsageFetcher.snapshot(fromSessionData:)` does: a JSON object whose
/// `token` is a non-blank string.
fn session_token_from_body(body: &[u8]) -> Result<String, SessionTokenError> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| SessionTokenError::Malformed(error.to_string()))?;
    let Some(object) = value.as_object() else {
        return Err(SessionTokenError::Malformed(
            "expected a JSON object".to_string(),
        ));
    };
    match object.get("token").and_then(Value::as_str) {
        Some(token) if !token.trim().is_empty() => Ok(token.to_string()),
        _ => Err(SessionTokenError::Invalid),
    }
}

pub struct VeniceProvider {
    client: Client,
}

#[derive(Debug)]
enum VeniceWebFailure {
    InvalidSession,
    Anonymous,
    MissingQuota(ProviderError),
    Other(ProviderError),
}

impl VeniceWebFailure {
    fn into_provider_error(self) -> ProviderError {
        match self {
            Self::InvalidSession => invalid_session_error(),
            Self::Anonymous => ProviderError::AuthRequired,
            Self::MissingQuota(error) | Self::Other(error) => error,
        }
    }

    fn is_unusable_session(&self) -> bool {
        matches!(
            self,
            Self::InvalidSession | Self::Anonymous | Self::MissingQuota(_)
        )
    }
}

impl VeniceProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn api_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        crate::providers::resolve_api_key(api_key, VENICE_CREDENTIAL_TARGET, &["VENICE_API_KEY"])
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let response = self
            .client
            .get(VENICE_BALANCE_URL)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Venice API returned status {}",
                response.status()
            )));
        }

        let balance: VeniceBalanceResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Venice balance: {e}")))?;
        Ok(snapshot_from_balance(&balance))
    }

    async fn fetch_web(
        &self,
        manual_cookie_header: Option<&str>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        if let Some(header) = manual_cookie_header {
            let credential = session_credential_from_header(header)
                .ok_or_else(|| ProviderError::Other(VENICE_MISSING_CREDENTIALS_MESSAGE.into()))?;
            return self
                .fetch_web_session(&credential)
                .await
                .map_err(VeniceWebFailure::into_provider_error);
        }

        let candidates =
            match crate::providers::browser_cookie_candidates_for_domain(VENICE_COOKIE_DOMAIN) {
                Ok(candidates) => candidates,
                Err(ProviderError::NoCookies) => {
                    return Err(ProviderError::Other(
                        VENICE_MISSING_CREDENTIALS_MESSAGE.into(),
                    ));
                }
                Err(error) => return Err(error),
            };
        let candidates = browser_session_candidates(candidates);
        fetch_web_sessions(candidates, |credential| async move {
            self.fetch_web_session(&credential).await
        })
        .await
    }

    async fn fetch_web_session(
        &self,
        credential: &VeniceSessionCredential,
    ) -> Result<ProviderFetchResult, VeniceWebFailure> {
        let response = credential
            .apply(self.client.get(VENICE_SESSION_URL))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| VeniceWebFailure::Other(error.into()))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(VeniceWebFailure::InvalidSession);
        }
        if !response.status().is_success() {
            return Err(VeniceWebFailure::Other(ProviderError::Other(format!(
                "Venice web session returned status {}",
                response.status()
            ))));
        }

        let body = response
            .bytes()
            .await
            .map_err(|error| VeniceWebFailure::Other(error.into()))?;
        snapshot_from_session_body(&body, Utc::now())
    }
}

/// Turns a `/api/auth/session` body into a snapshot. A missing or blank token
/// is an invalid session, so the caller can try the next browser session.
fn snapshot_from_session_body(
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<ProviderFetchResult, VeniceWebFailure> {
    let token = match session_token_from_body(body) {
        Ok(token) => token,
        Err(SessionTokenError::Invalid) => return Err(VeniceWebFailure::InvalidSession),
        Err(SessionTokenError::Malformed(detail)) => {
            return Err(VeniceWebFailure::Other(ProviderError::Parse(format!(
                "Failed to parse Venice web session: {detail}"
            ))));
        }
    };
    let claims = crate::codex_accounts::api::jwt_payload(&token).ok_or_else(|| {
        VeniceWebFailure::Other(ProviderError::Parse(
            "Venice session token is not a JWT".into(),
        ))
    })?;
    snapshot_from_web_claims(&claims, now)
}

fn snapshot_from_balance(balance: &VeniceBalanceResponse) -> UsageSnapshot {
    let active_currency = balance
        .consumption_currency
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase();

    let (used_percent, detail) = if !balance.can_consume {
        (100.0, "Balance unavailable for API calls".to_string())
    } else if active_currency == "USD" && balance.balances.usd.unwrap_or(0.0) > 0.0 {
        (
            0.0,
            format!("${:.2} USD remaining", balance.balances.usd.unwrap_or(0.0)),
        )
    } else if active_currency != "USD" {
        if let (Some(diem), Some(allocation)) =
            (balance.balances.diem, balance.diem_epoch_allocation)
            && allocation > 0.0
        {
            let used = ((allocation - diem) / allocation * 100.0).clamp(0.0, 100.0);
            (
                used,
                format!("DIEM {:.2} / {:.2} epoch allocation", diem, allocation),
            )
        } else if let Some(diem) = balance.balances.diem
            && diem > 0.0
        {
            (0.0, format!("DIEM {diem:.2} remaining"))
        } else if let Some(usd) = balance.balances.usd
            && usd > 0.0
        {
            (0.0, format!("${usd:.2} USD remaining"))
        } else {
            (100.0, "No Venice API balance available".to_string())
        }
    } else {
        (100.0, "No Venice API balance available".to_string())
    };

    let mut window = RateWindow::new(used_percent);
    window.reset_description = Some(detail.clone());
    UsageSnapshot::new(window).with_login_method(detail)
}

impl Default for VeniceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for VeniceProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Venice
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = Self::api_key(ctx.api_key.as_deref())?;
                Ok(ProviderFetchResult::new(
                    self.fetch_api(&api_key).await?,
                    "api",
                ))
            }
            SourceMode::Web => self.fetch_web(ctx.manual_cookie_header.as_deref()).await,
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(ctx.source_mode)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth, SourceMode::Web]
    }

    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    /// Venice web credential failures carry upstream's recovery text in
    /// `ProviderError::Other`; keep them classified as sign-in gates.
    fn error_state_kind(&self, error: &ProviderError) -> crate::core::ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == VENICE_MISSING_CREDENTIALS_MESSAGE => {
                crate::core::ProviderStateKind::NeedsAuthentication
            }
            ProviderError::Other(message) if message == VENICE_INVALID_SESSION_MESSAGE => {
                crate::core::ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

fn invalid_session_error() -> ProviderError {
    ProviderError::Other(VENICE_INVALID_SESSION_MESSAGE.into())
}

fn browser_session_candidates(
    candidates: Vec<(
        crate::browser::detection::BrowserType,
        Vec<crate::browser::cookies::Cookie>,
    )>,
) -> Vec<(
    crate::browser::detection::BrowserType,
    VeniceSessionCredential,
)> {
    candidates
        .into_iter()
        .filter_map(|(browser, cookies)| {
            session_credential_from_browser_cookies(&cookies)
                .map(|credential| (browser, credential))
        })
        .collect()
}

async fn fetch_web_sessions<F, Fut>(
    candidates: Vec<(
        crate::browser::detection::BrowserType,
        VeniceSessionCredential,
    )>,
    mut loader: F,
) -> Result<ProviderFetchResult, ProviderError>
where
    F: FnMut(VeniceSessionCredential) -> Fut,
    Fut: Future<Output = Result<ProviderFetchResult, VeniceWebFailure>>,
{
    let mut candidates = candidates.into_iter().peekable();
    let mut last_failure = None;
    while let Some((browser, credential)) = candidates.next() {
        match loader(credential).await {
            Ok(result) => return Ok(result),
            Err(failure) if failure.is_unusable_session() => {
                if candidates.peek().is_some() {
                    tracing::debug!(
                        browser = %browser.display_name(),
                        "Venice session unusable; trying the next browser"
                    );
                }
                last_failure = Some(failure.into_provider_error());
            }
            Err(failure) => return Err(failure.into_provider_error()),
        }
    }

    Err(last_failure
        .unwrap_or_else(|| ProviderError::Other(VENICE_MISSING_CREDENTIALS_MESSAGE.into())))
}

/// Credential accepted by the Venice session endpoint.
#[derive(Debug, PartialEq, Eq)]
enum VeniceSessionCredential {
    /// Legacy `__venice-auth.session-token` value, sent as a Cookie header.
    Legacy(String),
    /// Clerk `__session` / `__session_<suffix>` value, sent as a Bearer token
    /// with no Cookie header.
    Clerk(String),
}

impl VeniceSessionCredential {
    fn apply(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            Self::Legacy(value) => {
                request.header("Cookie", format!("{VENICE_SESSION_COOKIE}={value}"))
            }
            Self::Clerk(value) => request.bearer_auth(value),
        }
    }
}

fn is_clerk_session_cookie_name(name: &str) -> bool {
    name == VENICE_CLERK_SESSION_COOKIE
        || name
            .strip_prefix("__session_")
            .is_some_and(|suffix| !suffix.is_empty())
}

/// Browser cookies are trusted only from the exact `venice.ai` host, so
/// `clerk.venice.ai` cookies such as `__client` are never used. The shared
/// extractor also returns subdomain cookies, hence the filter here.
fn session_credential_from_browser_cookies(
    cookies: &[crate::browser::cookies::Cookie],
) -> Option<VeniceSessionCredential> {
    session_credential(
        cookies
            .iter()
            .filter(|cookie| {
                cookie
                    .domain
                    .trim()
                    .trim_matches('.')
                    .eq_ignore_ascii_case(VENICE_COOKIE_DOMAIN)
            })
            .map(|cookie| (cookie.name.as_str(), cookie.value.as_str())),
    )
}

fn session_credential_from_header(raw: &str) -> Option<VeniceSessionCredential> {
    if raw.len() > MAX_VENICE_COOKIE_HEADER_LEN {
        return None;
    }
    // A header pasted from DevTools often keeps its "Cookie:" prefix.
    let raw = raw.trim();
    let raw = match raw.get(.."cookie:".len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case("cookie:") => raw["cookie:".len()..].trim(),
        _ => raw,
    };
    session_credential(raw.split(';').filter_map(|part| part.split_once('=')))
}

/// The legacy cookie (exact, then contiguous chunks) wins over Clerk; among
/// Clerk cookies the unsuffixed `__session` wins over the first suffixed one.
fn session_credential<'a>(
    pairs: impl Iterator<Item = (&'a str, &'a str)>,
) -> Option<VeniceSessionCredential> {
    let mut exact = None;
    let mut clerk: Option<(&str, &str)> = None;
    let mut chunks = BTreeMap::new();
    let chunk_prefix = format!("{VENICE_SESSION_COOKIE}.");

    for (raw_name, raw_value) in pairs {
        let name = raw_name.trim();
        let value = raw_value.trim();
        if value.is_empty()
            || value.len() > MAX_VENICE_COOKIE_VALUE_LEN
            || value.chars().any(char::is_control)
        {
            continue;
        }
        if is_clerk_session_cookie_name(name) {
            if clerk.is_none() || name == VENICE_CLERK_SESSION_COOKIE {
                clerk = Some((name, value));
            }
            continue;
        }
        if name == VENICE_SESSION_COOKIE {
            if exact.is_some() {
                return None;
            }
            exact = Some(value.to_string());
            continue;
        }
        let Some(index) = name
            .strip_prefix(&chunk_prefix)
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };
        if index >= MAX_VENICE_COOKIE_CHUNKS || chunks.contains_key(&index) {
            return None;
        }
        chunks.insert(index, value.to_string());
    }

    if let Some(value) = exact {
        return Some(VeniceSessionCredential::Legacy(value));
    }
    // Chunked cookies are contiguous 0..len-1 by construction; a gap or a
    // tail that starts above 0 means a partial or forged set, so the session
    // token cannot be reassembled safely and a Clerk cookie is used instead.
    if !chunks.is_empty() && chunks.keys().max() == Some(&(chunks.len() - 1)) {
        let values: Vec<String> = chunks.into_values().collect();
        return Some(VeniceSessionCredential::Legacy(values.concat()));
    }
    clerk.map(|(_, value)| VeniceSessionCredential::Clerk(value.to_string()))
}

fn snapshot_from_web_claims(
    claims: &serde_json::Map<String, Value>,
    now: DateTime<Utc>,
) -> Result<ProviderFetchResult, VeniceWebFailure> {
    let expiration =
        epoch_value_to_datetime(claims.get("exp")).ok_or(VeniceWebFailure::InvalidSession)?;
    if expiration < now - chrono::Duration::seconds(VENICE_EXPIRATION_SKEW_SECS) {
        return Err(VeniceWebFailure::InvalidSession);
    }

    if claims
        .get("userType")
        .and_then(Value::as_str)
        .is_some_and(is_anonymous_user_type)
    {
        return Err(VeniceWebFailure::Anonymous);
    }

    let usage = claims
        .get("bundledCreditsUsage")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            VeniceWebFailure::MissingQuota(ProviderError::Parse(
                "Venice web session has no credits usage".into(),
            ))
        })?;
    let used_this_cycle = finite_non_negative(usage.get("usedThisCycle")).ok_or_else(|| {
        VeniceWebFailure::MissingQuota(ProviderError::Parse(
            "Venice web session has invalid usage".into(),
        ))
    })?;
    let monthly_refill_credits = finite_non_negative(usage.get("monthlyRefillCredits"))
        .filter(|value| *value > 0.0)
        .ok_or_else(|| {
            VeniceWebFailure::MissingQuota(ProviderError::Parse(
                "Venice web session has invalid refill credits".into(),
            ))
        })?;

    let available_credits = finite_non_negative(usage.get("availableCredits"))
        .or_else(|| finite_non_negative(claims.get("bundledCredits")));
    let venice_credits = finite_non_negative(claims.get("veniceCredits"));
    let tier_cap = finite_non_negative(usage.get("tierCap"));
    let next_refill_at = epoch_value_to_datetime(usage.get("nextRefillAt"));

    let mut details: Vec<Option<ProviderDisplayDetail>> = Vec::new();
    if let Some(available) = available_credits {
        details.push(ProviderDisplayDetail::new(
            "subscription-credits",
            "Subscription credits available",
            format_credits(available),
        ));
    }
    if let Some(total) = venice_credits {
        details.push(ProviderDisplayDetail::new(
            "total-credits",
            "Total credits available",
            format_credits(total),
        ));
    }
    details.push(
        ProviderDisplayDetail::new(
            "used-this-cycle",
            "Used this cycle",
            format_credits(used_this_cycle),
        )
        .and_then(|row| {
            row.with_secondary_value(format!(
                "Monthly refill: {}",
                format_credits(monthly_refill_credits)
            ))
        })
        .and_then(|row| row.with_progress(used_this_cycle, monthly_refill_credits)),
    );
    if let Some(cap) = tier_cap {
        details.push(ProviderDisplayDetail::new(
            "bank-cap",
            "Bank cap",
            format_credits(cap),
        ));
    }
    if let Some(next_refill) = next_refill_at {
        details.push(ProviderDisplayDetail::new(
            "next-refill",
            "Next refill",
            next_refill.to_rfc3339(),
        ));
    }
    if let Some(user_type) = claims.get("userType").and_then(Value::as_str) {
        let user_type = user_type.trim();
        if !user_type.is_empty() {
            details.push(ProviderDisplayDetail::new("plan", "Plan", user_type));
        }
    }

    let mut result = ProviderFetchResult::new(
        UsageSnapshot::new(RateWindow::informational("Venice web credits")),
        "web",
    )
    .with_non_authoritative_pace();
    for detail in details {
        result = result.with_display_detail(detail);
    }
    Ok(result)
}

fn finite_non_negative(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|value| value.is_finite() && *value >= 0.0),
        Some(Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0),
        _ => None,
    }
}

/// Convert one epoch-valued JSON field (seconds or milliseconds) into a UTC
/// timestamp. Accepts only values in the sane 2001-2096 second range, which
/// also bounds the i64 cast below.
fn epoch_value_to_datetime(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = finite_non_negative(value)?;
    let seconds = if value > 4_000_000_000.0 {
        value / 1000.0
    } else {
        value
    };
    if !(1_000_000_000.0..=4_000_000_000.0).contains(&seconds) {
        return None;
    }
    // The range check above proves this conversion is within i64 bounds; the
    // fractional part is intentionally discarded because JWT/epoch values are
    // rendered at second precision.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the preceding range check bounds this conversion to valid Unix seconds"
    )]
    let seconds = seconds as i64;
    DateTime::<Utc>::from_timestamp(seconds, 0)
}

/// User types that mean a logged-out web session. Upstream
/// `VeniceWebUsageFetcher.anonymousUserTypes` lists these spellings and
/// matches them case-insensitively; any other value is an authenticated user.
const ANONYMOUS_USER_TYPES: [&str; 5] = [
    "anonymous",
    "anon",
    "guest",
    "unauthenticated",
    "logged_out",
];

fn is_anonymous_user_type(value: &str) -> bool {
    ANONYMOUS_USER_TYPES
        .iter()
        .any(|candidate| value.eq_ignore_ascii_case(candidate))
}

fn format_credits(value: f64) -> String {
    if (value - value.round()).abs() < 0.005 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

#[cfg(test)]
mod tests;
