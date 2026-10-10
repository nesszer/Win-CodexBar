//! xKiro daily free-token provider (upstream 0.67.0, #3729).
//!
//! `GET https://api.xkiro.com/v1/usage` with `Authorization: Bearer <key>`.
//! The endpoint is documented as free to call and is account-wide. Only the
//! `free_tokens` counters feed the meter; paid plan windows and wallet
//! balances never count toward free-token headroom. The daily counter resets
//! at 00:00 UTC. Response bodies are never echoed into error messages.
//!
//! Live account behavior is unverified upstream: the fixtures are the
//! documented pay-as-you-go example with a synthetic identity.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use reqwest::header::HeaderMap;
use reqwest::{Client, StatusCode};
use serde_json::{Map, Value};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response};

const USAGE_URL: &str = "https://api.xkiro.com/v1/usage";
const CREDENTIAL_TARGET: &str = "codexbar-xkiro";
const ENV_KEYS: &[&str] = &["XKIRO_API_KEY"];
const REQUEST_TIMEOUT_SECS: u64 = 15;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const DAY_MINUTES: u32 = 24 * 60;
/// Largest integer a JSON number can carry without loss (`Number.MAX_SAFE_INTEGER`).
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
const PLAN_PAY_AS_YOU_GO: &str = "Pay as you go";

/// The parts of a usage response that feed the free-token meter.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FreeTokenUsage {
    used_today: Option<u64>,
    limit_per_day: Option<u64>,
    remaining: Option<u64>,
    /// `limit_per_day` was an explicit `null`: the account reports no daily cap.
    uncapped: bool,
    email: Option<String>,
    login_method: Option<String>,
}

pub struct XKiroProvider {
    client: Client,
    usage_url: String,
}

impl XKiroProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .build()
                .unwrap_or_else(|_| Client::new()),
            usage_url: USAGE_URL.to_string(),
        }
    }

    /// Point the provider at a local mock server; the production origin stays fixed.
    #[cfg(test)]
    fn with_usage_url(mut self, usage_url: String) -> Self {
        self.client = Client::builder()
            .no_proxy()
            .build()
            .expect("the test client should build");
        self.usage_url = usage_url;
        self
    }

    async fn fetch_api(
        &self,
        api_key: &str,
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let response = self
            .client
            .get(&self.usage_url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        validate_status(status, response.headers())?;
        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => unrecognized_response(),
                BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        let usage = parse_usage(&body)?;
        Ok(result_from_usage(&usage, now))
    }
}

impl Default for XKiroProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for XKiroProvider {
    fn id(&self) -> ProviderId {
        ProviderId::XKiro
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    ENV_KEYS,
                )?;
                self.fetch_api(&key, Utc::now()).await
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    /// Upstream source modes are `auto` + `api`; the API-key path uses the OAuth slot.
    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// Map an HTTP status to a friendly error. The response body is never read
/// for error statuses, so it cannot leak into a message.
fn validate_status(status: StatusCode, headers: &HeaderMap) -> Result<(), ProviderError> {
    match status {
        StatusCode::OK => Ok(()),
        StatusCode::UNAUTHORIZED => Err(ProviderError::AuthRequired),
        StatusCode::FORBIDDEN => Err(ProviderError::Other(
            "xKiro denied this API key (HTTP 403). Check the key's permissions.".to_string(),
        )),
        StatusCode::TOO_MANY_REQUESTS => {
            let retry_after = retry_after_seconds(
                headers
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok()),
            );
            Err(ProviderError::Other(format!(
                "xKiro rate limit reached; retry after {retry_after}s."
            )))
        }
        status if status.is_server_error() => Err(ProviderError::Other(format!(
            "xKiro is unavailable (HTTP {}).",
            status.as_u16()
        ))),
        status => Err(ProviderError::Other(format!(
            "xKiro returned HTTP {}.",
            status.as_u16()
        ))),
    }
}

/// `Retry-After` in seconds: a non-negative number capped at 10, default 1.
fn retry_after_seconds(value: Option<&str>) -> f64 {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map_or(1.0, |value| value.min(10.0))
}

fn unrecognized_response() -> ProviderError {
    ProviderError::Parse("xKiro returned an unrecognized free-token usage response.".to_string())
}

fn as_record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

/// A non-negative JSON integer within JavaScript's safe-integer range.
fn safe_counter(number: &serde_json::Number) -> Option<u64> {
    let value = number.as_u64()?;
    (value <= MAX_SAFE_INTEGER).then_some(value)
}

/// A counter that is absent or `null` reads as `None`; any other non-integer is a parse failure.
fn read_counter(
    free_tokens: &Map<String, Value>,
    name: &str,
) -> Result<Option<u64>, ProviderError> {
    match free_tokens.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => safe_counter(number)
            .map(Some)
            .ok_or_else(unrecognized_response),
        Some(_) => Err(unrecognized_response()),
    }
}

fn trimmed_text(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn parse_usage(body: &[u8]) -> Result<FreeTokenUsage, ProviderError> {
    let root: Value = serde_json::from_slice(body).map_err(|_| unrecognized_response())?;
    let root = as_record(Some(&root)).ok_or_else(unrecognized_response)?;
    let free_tokens = as_record(root.get("free_tokens")).ok_or_else(unrecognized_response)?;
    if root.get("object").and_then(Value::as_str) != Some("usage") {
        return Err(unrecognized_response());
    }

    let used_today = read_counter(free_tokens, "used_today")?;
    let limit_per_day = read_counter(free_tokens, "limit_per_day")?;
    let remaining = read_counter(free_tokens, "remaining")?;
    let uncapped = free_tokens.get("limit_per_day") == Some(&Value::Null);
    // A body with no usable counter is only meaningful for an explicitly
    // uncapped account.
    if used_today.is_none() && limit_per_day.is_none() && remaining.is_none() && !uncapped {
        return Err(unrecognized_response());
    }

    Ok(FreeTokenUsage {
        used_today,
        limit_per_day,
        remaining,
        uncapped,
        email: trimmed_text(as_record(root.get("user")).and_then(|user| user.get("email"))),
        // An explicit `"plan": null` is pay as you go; an absent plan stays unlabelled.
        login_method: match root.get("plan") {
            Some(Value::Null) => Some(PLAN_PAY_AS_YOU_GO.to_string()),
            plan => trimmed_text(plan),
        },
    })
}

/// The next 00:00 UTC strictly after `now`.
fn next_utc_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is a valid time")
        .and_utc()
        + Duration::days(1)
}

/// Percent of the daily allowance used. A zero allowance is exhausted.
fn used_percent(used: u64, limit: u64) -> f64 {
    if limit == 0 {
        100.0
    } else {
        (used as f64 / limit as f64 * 100.0).clamp(0.0, 100.0)
    }
}

fn primary_window(usage: &FreeTokenUsage, now: DateTime<Utc>) -> RateWindow {
    match (usage.used_today, usage.limit_per_day) {
        (Some(used), Some(limit)) => RateWindow::with_details(
            used_percent(used, limit),
            Some(DAY_MINUTES),
            Some(next_utc_midnight(now)),
            None,
        ),
        // Missing counters stay unknown: no percentage is invented.
        _ => RateWindow::informational(if let Some(remaining) = usage.remaining {
            format!("{} tokens remaining", format::count(remaining))
        } else if usage.uncapped {
            "No daily cap reported".to_string()
        } else {
            "Daily free-token usage unavailable".to_string()
        }),
    }
}

fn result_from_usage(usage: &FreeTokenUsage, now: DateTime<Utc>) -> ProviderFetchResult {
    let mut snapshot = UsageSnapshot::new(primary_window(usage, now));
    if let Some(email) = &usage.email {
        snapshot = snapshot.with_email(email.clone());
    }
    if let Some(login_method) = &usage.login_method {
        snapshot = snapshot.with_login_method(login_method.clone());
    }

    let allowance = match usage.limit_per_day {
        Some(limit) => Some(format::count(limit)),
        None if usage.uncapped => Some("No cap reported".to_string()),
        None => None,
    };
    let rows = [
        (
            "tokens-used-today",
            "Tokens used today",
            usage.used_today.map(format::count),
        ),
        ("daily-allowance", "Daily allowance", allowance),
        (
            "tokens-remaining",
            "Tokens remaining",
            usage.remaining.map(format::count),
        ),
        ("daily-reset", "Daily reset", Some("00:00 UTC".to_string())),
    ];
    rows.into_iter().fold(
        ProviderFetchResult::new(snapshot, "api"),
        |result, (id, label, value)| {
            result.with_display_detail(
                value.and_then(|value| ProviderDisplayDetail::new(id, label, value)),
            )
        },
    )
}

#[cfg(test)]
mod tests;
