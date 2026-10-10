//! Abacus AI provider implementation
//!
//! Fetches compute-point usage and billing info via apps.abacus.ai web APIs.
//! Uses browser cookies for authentication.

use std::future::Future;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderStateKind,
    RateWindow, SourceMode, UsageSnapshot,
};

#[cfg(test)]
mod tests;

const ORIGIN: &str = "https://apps.abacus.ai";
const COMPUTE_PATH: &str = "/api/_getOrganizationComputePoints";
const BILLING_PATH: &str = "/api/_getBillingInfo";
/// Parent domain of `apps.abacus.ai`; one browser query covers both hosts.
const COOKIE_DOMAIN: &str = "abacus.ai";
pub(crate) const CREDITS_LABEL: &str = "Credits";
const FALLBACK_MONTHLY_WINDOW_MINUTES: u32 = 30 * 24 * 60;
const MAX_COOKIE_CANDIDATES: u32 = 5;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const BILLING_BUDGET_CAP: Duration = Duration::from_secs(5);
const REFRESH_DEADLINE_CAP: Duration = Duration::from_secs(90);
const MISSING_SESSION_MESSAGE: &str = "No Abacus AI session found. Please log in to apps.abacus.ai in your browser or paste a Cookie header in manual mode.";

/// Exact cookie names that carry Abacus session state. CSRF tokens are
/// excluded on purpose: anonymous jars contain them.
const KNOWN_SESSION_COOKIE_NAMES: [&str; 5] = [
    "sessionid",
    "session_id",
    "session_token",
    "auth_token",
    "access_token",
];
/// Substrings that mark a session cookie when no exact name matches.
const SESSION_COOKIE_SUBSTRINGS: [&str; 4] = ["session", "auth", "sid", "jwt"];
/// Prefixes that mark a non-session cookie even if a substring matches.
const EXCLUDED_COOKIE_PREFIXES: [&str; 5] = ["csrf", "_ga", "_gid", "tracking", "analytics"];
/// `success:false` messages containing one of these mean the session is bad.
const AUTH_ERROR_KEYWORDS: [&str; 7] = [
    "expired",
    "session",
    "login",
    "authenticate",
    "unauthorized",
    "unauthenticated",
    "forbidden",
];

/// `{"success": true, "result": {...}}`. Every field tolerates a wrong type so
/// a malformed envelope becomes a classified error rather than a serde error.
#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    #[serde(default, deserialize_with = "lenient_true")]
    success: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default, deserialize_with = "lenient_string")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComputePoints {
    #[serde(default, deserialize_with = "lenient_finite_number")]
    total_compute_points: Option<f64>,
    #[serde(default, deserialize_with = "lenient_finite_number")]
    compute_points_left: Option<f64>,
}

#[derive(Debug, PartialEq)]
struct ComputePoints {
    total: f64,
    left: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingInfo {
    #[serde(default, deserialize_with = "lenient_string")]
    next_billing_date: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    current_tier: Option<String>,
}

fn lenient_true<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(Value::deserialize(deserializer)? == Value::Bool(true))
}

fn lenient_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) => Some(text),
        _ => None,
    })
}

fn lenient_finite_number<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f64>, D::Error> {
    Ok(Value::deserialize(deserializer)?
        .as_f64()
        .filter(|number| number.is_finite()))
}

pub struct AbacusProvider {
    client: Client,
    origin: String,
}

impl AbacusProvider {
    pub fn new() -> Self {
        Self::with_origin(ORIGIN)
    }

    fn with_origin(origin: &str) -> Self {
        Self {
            // Every request sets its own timeout; the client has none so a
            // configured web timeout above 30 s is honored.
            client: crate::core::credentialed_http_client_builder()
                .build()
                .unwrap_or_else(|_| Client::new()),
            origin: origin.to_string(),
        }
    }

    fn build_snapshot(
        compute: ComputePoints,
        billing: Option<BillingInfo>,
    ) -> Result<UsageSnapshot, ProviderError> {
        let ComputePoints { total, left } = compute;
        let used = total - left;
        let percent = if total > 0.0 {
            ((used / total) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let resets_at = billing
            .as_ref()
            .and_then(|b| b.next_billing_date.as_deref())
            .and_then(parse_billing_date);
        let primary = RateWindow::with_details(
            percent,
            RateWindow::monthly_window_minutes(resets_at).or(Some(FALLBACK_MONTHLY_WINDOW_MINUTES)),
            resets_at,
            Some(format_credit_detail(used, total)),
        );

        let mut snapshot = UsageSnapshot::new(primary).with_primary_label(CREDITS_LABEL);
        if let Some(b) = billing
            && let Some(tier) = b.current_tier
            && !tier.is_empty()
        {
            snapshot = snapshot.with_login_method(tier);
        }
        Ok(snapshot)
    }

    async fn fetch_compute(
        &self,
        cookie_header: &str,
        timeout: Duration,
    ) -> Result<ComputePoints, ProviderError> {
        let request = self
            .client
            .get(format!("{}{COMPUTE_PATH}", self.origin))
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .timeout(timeout);
        let raw: RawComputePoints = send_envelope(request).await?;
        match (raw.total_compute_points, raw.compute_points_left) {
            (Some(total), Some(left)) => Ok(ComputePoints { total, left }),
            _ => Err(parse_failure(
                "Missing credit fields in compute points response",
            )),
        }
    }

    /// Billing only enriches the credits result, so every failure is `None`.
    async fn fetch_billing(&self, cookie_header: &str, budget: Duration) -> Option<BillingInfo> {
        let request = self
            .client
            .post(format!("{}{BILLING_PATH}", self.origin))
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body("{}")
            .timeout(budget);
        match send_envelope(request).await {
            Ok(billing) => Some(billing),
            Err(error) => {
                tracing::debug!(%error, "Abacus billing info unavailable; using fallback window");
                None
            }
        }
    }

    async fn fetch_with_cookies(
        &self,
        cookie_header: &str,
        request_timeout: Duration,
    ) -> Result<UsageSnapshot, ProviderError> {
        let budget = request_timeout.min(BILLING_BUDGET_CAP);
        let (compute, billing) = credits_with_billing(
            self.fetch_compute(cookie_header, request_timeout),
            self.fetch_billing(cookie_header, budget),
            budget,
        )
        .await?;
        Self::build_snapshot(compute, billing)
    }

    /// Try each imported session in order. Any failure moves on to the next
    /// one; the last failure is reported when none succeeds.
    async fn fetch_with_candidates(
        &self,
        candidates: &[(String, String)],
        request_timeout: Duration,
    ) -> Result<UsageSnapshot, ProviderError> {
        let mut last_error = None;
        for (label, cookie_header) in candidates {
            match self
                .fetch_with_cookies(cookie_header, request_timeout)
                .await
            {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => {
                    tracing::debug!(browser = %label, %error, "Abacus session candidate failed");
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| ProviderError::Other(MISSING_SESSION_MESSAGE.to_string())))
    }

    async fn fetch_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let request_timeout = request_timeout(ctx.web_timeout);
        let refresh = async {
            // A pasted header is exclusive: never fall back to browser cookies.
            if let Some(cookie_header) = ctx.manual_cookie_header.as_deref() {
                return self
                    .fetch_with_cookies(cookie_header, request_timeout)
                    .await;
            }
            let candidates = session_candidates(
                crate::providers::browser_cookie_headers_for_domain(COOKIE_DOMAIN),
            )?;
            self.fetch_with_candidates(&candidates, request_timeout)
                .await
        };
        tokio::time::timeout(refresh_timeout(request_timeout), refresh)
            .await
            .map_err(|_| ProviderError::Timeout)?
    }
}

/// Web timeout clamped to the 1..=90 s request range.
fn request_timeout(web_timeout: u64) -> Duration {
    Duration::from_secs(web_timeout.clamp(1, 90))
}

/// Total refresh deadline: room for every candidate plus one billing budget,
/// never more than 90 s.
fn refresh_timeout(request_timeout: Duration) -> Duration {
    (request_timeout * MAX_COOKIE_CANDIDATES + request_timeout.min(BILLING_BUDGET_CAP))
        .min(REFRESH_DEADLINE_CAP)
}

/// Run the required credits request and the optional billing request
/// concurrently. Billing gets `budget` from the start; when it errors or runs
/// out it is dropped, and a credits failure cancels it immediately.
async fn credits_with_billing<T, U>(
    credits: impl Future<Output = Result<T, ProviderError>>,
    billing: impl Future<Output = Option<U>>,
    budget: Duration,
) -> Result<(T, Option<U>), ProviderError> {
    let billing = async { tokio::time::timeout(budget, billing).await.ok().flatten() };
    tokio::pin!(credits, billing);
    let mut billing_result = None;
    let credits = loop {
        tokio::select! {
            result = &mut credits => break result?,
            result = &mut billing, if billing_result.is_none() => billing_result = Some(result),
        }
    };
    let billing = match billing_result {
        Some(result) => result,
        None => billing.await,
    };
    Ok((credits, billing))
}

/// Send `request` and decode the `{success, result}` envelope into `T`.
async fn send_envelope<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, ProviderError> {
    let response = request.send().await?;
    let status = response.status().as_u16();
    if status == 401 || status == 403 {
        return Err(ProviderError::AuthRequired);
    }
    if status != 200 {
        return Err(ProviderError::Other(format!(
            "Abacus AI API error: HTTP {status}"
        )));
    }
    let body = crate::providers::read_bounded_response(response, MAX_BODY_BYTES)
        .await
        .map_err(|error| match error {
            crate::providers::BoundedBodyError::TooLarge => parse_failure("response is too large"),
            crate::providers::BoundedBodyError::Read(error) => ProviderError::Network(error),
        })?;
    decode_envelope(&body)
}

fn decode_envelope<T: DeserializeOwned>(body: &[u8]) -> Result<T, ProviderError> {
    let root: Value = serde_json::from_slice(body).map_err(|_| parse_failure("invalid JSON"))?;
    if !root.is_object() {
        return Err(parse_failure("invalid response"));
    }
    let envelope: ApiEnvelope =
        serde_json::from_value(root).map_err(|_| parse_failure("invalid response"))?;
    match envelope.result {
        Some(result) if envelope.success && result.is_object() => {
            serde_json::from_value(result).map_err(|_| parse_failure("invalid result"))
        }
        _ => {
            let message = envelope
                .error
                .map_or_else(|| "unknown error".to_string(), |text| text.to_lowercase());
            if AUTH_ERROR_KEYWORDS
                .iter()
                .any(|keyword| message.contains(keyword))
            {
                Err(ProviderError::AuthRequired)
            } else {
                Err(parse_failure(&message))
            }
        }
    }
}

fn parse_failure(message: &str) -> ProviderError {
    ProviderError::Parse(format!("Could not parse Abacus AI usage: {message}"))
}

/// Billing dates must look like ISO 8601 date-times before they are trusted.
fn parse_billing_date(value: &str) -> Option<DateTime<Utc>> {
    let bytes = value.as_bytes();
    let looks_iso = bytes.len() > 10
        && bytes[..10]
            .iter()
            .enumerate()
            .all(|(index, byte)| match index {
                4 | 7 => *byte == b'-',
                _ => byte.is_ascii_digit(),
            })
        && bytes[10] == b'T';
    if !looks_iso {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

/// Browser cookie sets worth trying, Chrome first, at most five. Sets without
/// a session cookie (anonymous or marketing-only jars) are skipped.
fn session_candidates(
    headers: Result<Vec<(String, String)>, ProviderError>,
) -> Result<Vec<(String, String)>, ProviderError> {
    let mut candidates = match headers {
        Ok(headers) => headers,
        Err(ProviderError::NoCookies) => Vec::new(),
        Err(error) => return Err(error),
    };
    candidates.retain(|(_, header)| has_session_cookie(header));
    candidates.sort_by_key(|(label, _)| label != "Google Chrome");
    candidates.truncate(MAX_COOKIE_CANDIDATES as usize);
    Ok(candidates)
}

fn has_session_cookie(cookie_header: &str) -> bool {
    cookie_header.split(';').any(|pair| {
        let name = pair
            .split_once('=')
            .map_or(pair, |(name, _)| name)
            .trim()
            .to_ascii_lowercase();
        if KNOWN_SESSION_COOKIE_NAMES.contains(&name.as_str()) {
            return true;
        }
        if EXCLUDED_COOKIE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            return false;
        }
        SESSION_COOKIE_SUBSTRINGS
            .iter()
            .any(|needle| name.contains(needle))
    })
}

fn format_credit_detail(used: f64, total: f64) -> String {
    format!(
        "{} / {} credits",
        format_credit_value(used),
        format_credit_value(total)
    )
}

fn format_credit_value(value: f64) -> String {
    let value = if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    };
    let formatted = if value >= 1000.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };

    let (integer, fraction) = match formatted.split_once('.') {
        Some(parts) => parts,
        None => (formatted.as_str(), ""),
    };
    let grouped = group_credit_digits(integer);
    if fraction.is_empty() {
        grouped
    } else {
        format!("{grouped}.{fraction}")
    }
}

fn group_credit_digits(value: &str) -> String {
    let mut reversed = String::with_capacity(value.len() + value.len() / 3);
    for (index, digit) in value.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            reversed.push(',');
        }
        reversed.push(digit);
    }
    reversed.chars().rev().collect()
}

impl Default for AbacusProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AbacusProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Abacus
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Abacus AI usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let usage = self.fetch_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == MISSING_SESSION_MESSAGE => {
                ProviderStateKind::NeedsAuthentication
            }
            _ => error.state_kind(),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        false
    }
}
