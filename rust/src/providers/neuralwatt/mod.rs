//! Neuralwatt API-key usage provider (upstream 0.44 #2220).
//!
//! `GET https://api.neuralwatt.com/v1/quota` — subscription kWh + prepaid credits.
//!
//! Behavior follows upstream v0.64.1 (`neuralwatt.js`, `ProviderHTTPRetryPolicy.transientIdempotent`,
//! `NeuralWattSettingsReader`): one retry of transient failures, strict quota validation, and an
//! HTTPS-only `NEURALWATT_API_URL` override.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, RETRY_AFTER};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;

use crate::core::{
    CostSnapshot, FetchContext, NamedRateWindow, Provider, ProviderError, ProviderFetchResult,
    ProviderId, RateWindow, SourceMode, SubscriptionMetadata, UsageSnapshot,
};
use crate::providers::format;

const DEFAULT_API_BASE: &str = "https://api.neuralwatt.com";
const CREDENTIAL_TARGET: &str = "codexbar-neuralwatt";
const ENV_KEYS: &[&str] = &["NEURALWATT_API_KEY"];
const API_URL_ENV: &str = "NEURALWATT_API_URL";
/// Delay before the single retry when the server sends no usable `Retry-After`.
const RETRY_DEFAULT_DELAY: Duration = Duration::from_secs(1);
/// Upper bound applied to a server-provided `Retry-After`.
const RETRY_MAX_DELAY: f64 = 10.0;

#[derive(Debug, Deserialize, Default)]
#[allow(
    dead_code,
    reason = "fields mirror the upstream schema checks; deserialized so malformed types fail the parse"
)]
struct QuotaResponse {
    snapshot_at: Option<String>,
    balance: Option<Balance>,
    usage: Option<Usage>,
    limits: Option<Limits>,
    subscription: Option<Subscription>,
    key: Option<KeyInfo>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(
    dead_code,
    reason = "fields mirror the upstream schema checks; deserialized so malformed types fail the parse"
)]
struct Limits {
    overage_limit_usd: Option<f64>,
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct Balance {
    credits_remaining_usd: Option<f64>,
    total_credits_usd: Option<f64>,
    credits_used_usd: Option<f64>,
    accounting_method: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(
    dead_code,
    reason = "fields mirror the upstream schema checks; deserialized so malformed types fail the parse"
)]
struct Usage {
    lifetime: Option<UsagePeriod>,
    current_month: Option<UsagePeriod>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(
    dead_code,
    reason = "fields mirror the upstream schema checks; deserialized so malformed types fail the parse"
)]
struct UsagePeriod {
    cost_usd: Option<f64>,
    energy_kwh: Option<f64>,
    requests: Option<i64>,
    tokens: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct Subscription {
    plan: Option<String>,
    status: Option<String>,
    #[allow(
        dead_code,
        reason = "field mirrors the upstream schema checks; deserialized so a malformed type fails the parse"
    )]
    billing_interval: Option<String>,
    auto_renew: Option<bool>,
    current_period_start: Option<String>,
    current_period_end: Option<String>,
    kwh_included: Option<f64>,
    kwh_used: Option<f64>,
    kwh_remaining: Option<f64>,
    #[allow(
        dead_code,
        reason = "field mirrors the NeuralWatt API payload; deserialized for round-trip fidelity but not read yet"
    )]
    in_overage: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(
    dead_code,
    reason = "fields mirror the upstream schema checks; deserialized so malformed types fail the parse"
)]
struct KeyInfo {
    name: Option<String>,
    allowance: Option<KeyAllowance>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct KeyAllowance {
    limit_usd: Option<f64>,
    period: Option<String>,
    spent_usd: Option<f64>,
    remaining_usd: Option<f64>,
    blocked: Option<bool>,
}

pub struct NeuralwattProvider {
    client: Client,
}

impl NeuralwattProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn quota_url() -> Result<Url, ProviderError> {
        quota_url_for(std::env::var(API_URL_ENV).ok().as_deref())
    }
}

/// Resolve the quota endpoint from an optional `NEURALWATT_API_URL` value.
///
/// A blank override uses the default host. Anything that is not HTTPS (or a
/// bare host, which is promoted to HTTPS) is rejected with the upstream text.
/// A configured `?query` is kept after `/v1/quota`.
fn quota_url_for(override_url: Option<&str>) -> Result<Url, ProviderError> {
    let raw = override_url
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .unwrap_or(DEFAULT_API_BASE);
    let mut url = crate::providers::validated_https_url(raw, "Neuralwatt").map_err(|_| {
        ProviderError::Other(format!(
            "Neuralwatt endpoint override {API_URL_ENV} must use HTTPS or a bare host."
        ))
    })?;
    let path = url.path().trim_end_matches('/');
    let quota_path = if path.ends_with("/v1") {
        format!("{path}/quota")
    } else {
        format!("{path}/v1/quota")
    };
    url.set_path(&quota_path);
    url.set_fragment(None);
    Ok(url)
}

impl Default for NeuralwattProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for NeuralwattProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Neuralwatt
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    ENV_KEYS,
                )?;
                let url = Self::quota_url()?;
                let body = fetch_quota(&self.client, &url, &key, RETRY_DEFAULT_DELAY).await?;
                let (snap, cost) = snapshot_from_quota(&body)?;
                let mut result = ProviderFetchResult::new(snap, "api");
                if let Some(cost) = cost {
                    result = result.with_cost(cost);
                }
                Ok(result)
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// `GET` the quota endpoint, retrying one transient failure.
///
/// Retryable: HTTP 408/429/500/502/503/504 and timeout, connection-lost,
/// connect-refused and DNS failures. The retry waits `Retry-After` (capped at
/// 10 s) or `default_delay`. Authentication, TLS and other failures are
/// final. Cancellation drops this future, including the retry sleep.
async fn fetch_quota(
    client: &Client,
    url: &Url,
    key: &str,
    default_delay: Duration,
) -> Result<QuotaResponse, ProviderError> {
    let mut retried = false;
    let resp = loop {
        let outcome = client
            .get(url.clone())
            .bearer_auth(key)
            .header(ACCEPT, "application/json")
            .send()
            .await;
        let delay = match &outcome {
            Ok(resp) if is_retryable_status(resp.status()) => Some(retry_delay(
                resp.headers()
                    .get(RETRY_AFTER)
                    .and_then(|value| value.to_str().ok()),
                default_delay,
            )),
            Err(error) if is_transient_transport_error(error) => Some(default_delay),
            _ => None,
        };
        match delay {
            Some(delay) if !retried => {
                retried = true;
                tokio::time::sleep(delay).await;
            }
            _ => break outcome?,
        }
    };
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::AuthRequired);
    }
    if status != StatusCode::OK {
        return Err(ProviderError::Other(format!(
            "Neuralwatt API error: HTTP {}",
            status.as_u16()
        )));
    }
    resp.json()
        .await
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Neuralwatt quota: {e}")))
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

fn retry_delay(retry_after: Option<&str>, default_delay: Duration) -> Duration {
    retry_after
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map_or(default_delay, |seconds| {
            Duration::from_secs_f64(seconds.min(RETRY_MAX_DELAY))
        })
}

/// Timeout, refused/reset/closed connection and DNS failures are transient.
/// A `connect` error alone is too broad because it also covers TLS handshake
/// failures, so connect errors need a typed socket kind or a DNS marker.
fn is_transient_transport_error(error: &reqwest::Error) -> bool {
    if error.is_body() || error.is_decode() {
        return false;
    }
    if error.is_timeout() {
        return true;
    }
    let mut source = std::error::Error::source(error);
    let mut transient = false;
    while let Some(current) = source {
        if let Some(io_error) = current.downcast_ref::<std::io::Error>() {
            use std::io::ErrorKind::{
                BrokenPipe, ConnectionAborted, ConnectionRefused, ConnectionReset, TimedOut,
                UnexpectedEof,
            };
            transient |= matches!(
                io_error.kind(),
                BrokenPipe
                    | ConnectionAborted
                    | ConnectionRefused
                    | ConnectionReset
                    | TimedOut
                    | UnexpectedEof
            );
        }
        let message = current.to_string();
        transient |= message.starts_with("dns error")
            || message.contains("connection closed before message completed");
        source = current.source();
    }
    transient
}

fn valid_nn(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x >= 0.0)
}

fn valid_pos(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

/// Upstream accepts only `YYYY-MM-DDThh:mm:ss[.fff](Z|±hh:mm)` timestamps.
fn parse_iso(raw: &str) -> Option<DateTime<Utc>> {
    if raw.as_bytes().get(10) != Some(&b'T') {
        return None;
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// A present timestamp must parse; an absent one is `None`.
fn optional_iso(raw: Option<&str>, field: &str) -> Result<Option<DateTime<Utc>>, ProviderError> {
    raw.map(|raw| parse_iso(raw).ok_or_else(|| parse_failure(&format!("invalid {field}"))))
        .transpose()
}

fn parse_failure(message: &str) -> ProviderError {
    ProviderError::Parse(format!("Failed to parse Neuralwatt response: {message}"))
}

fn subscription_window(sub: &Subscription) -> Result<Option<RateWindow>, ProviderError> {
    let start = optional_iso(sub.current_period_start.as_deref(), "current_period_start")?;
    let end = optional_iso(sub.current_period_end.as_deref(), "current_period_end")?;
    let Some(total) = valid_pos(sub.kwh_included).or_else(|| {
        let used = valid_nn(sub.kwh_used)?;
        let remaining = valid_nn(sub.kwh_remaining)?;
        let t = used + remaining;
        (t > 0.0).then_some(t)
    }) else {
        return Ok(None);
    };
    let Some(used) = valid_nn(sub.kwh_used).or_else(|| {
        let remaining = valid_nn(sub.kwh_remaining)?;
        Some((total - remaining).max(0.0))
    }) else {
        return Ok(None);
    };
    let mut w = RateWindow::new(((used / total) * 100.0).clamp(0.0, 100.0));
    if let Some(end) = end {
        if let Some(start) = start
            && end > start
        {
            // Billing-window minutes are clamped to u32 range, so the cast cannot truncate.
            #[expect(clippy::cast_possible_truncation, reason = "clamped to u32 range")]
            let mins = ((end - start).num_minutes()).clamp(1, u32::MAX as i64) as u32;
            w.window_minutes = Some(mins);
        }
        w.resets_at = Some(end);
    }
    w.reset_description = Some(format!(
        "{} / {} kWh",
        format::whole_or_two_decimals(used),
        format::whole_or_two_decimals(total)
    ));
    Ok(Some(w))
}

fn prepaid_remaining(bal: &Balance) -> Option<f64> {
    if let Some(r) = valid_nn(bal.credits_remaining_usd) {
        return Some(r);
    }
    let total = valid_pos(bal.total_credits_usd)?;
    let used = valid_nn(bal.credits_used_usd)?;
    Some((total - used).max(0.0))
}

fn prepaid_cost(bal: &Balance) -> Option<CostSnapshot> {
    let remaining = prepaid_remaining(bal)?;
    let used = valid_nn(bal.credits_used_usd)
        .or_else(|| {
            let total = valid_pos(bal.total_credits_usd)?;
            Some((total - remaining).max(0.0))
        })
        .unwrap_or(0.0);
    let mut cost =
        CostSnapshot::new(used, "USD", "Neuralwatt prepaid balance").with_balance(remaining);
    if let Some(total) = valid_pos(bal.total_credits_usd) {
        cost = cost.with_limit(total);
    }
    Some(cost)
}

/// Upstream `title`: lowercase, then uppercase the first letter of each word.
fn title_case(value: &str) -> String {
    let mut previous_is_word = false;
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            let is_word = c.is_ascii_alphanumeric() || c == '_';
            let mapped = if is_word && !previous_is_word {
                c.to_ascii_uppercase()
            } else {
                c
            };
            previous_is_word = is_word;
            mapped
        })
        .collect()
}

fn login_method(body: &QuotaResponse, balance: &Balance) -> Option<String> {
    let plan = body
        .subscription
        .as_ref()
        .and_then(|sub| sub.plan.as_deref())
        .map(str::trim)
        .filter(|plan| !plan.is_empty());
    match plan {
        Some(plan) => Some(format!("{} plan", title_case(&plan.replace('_', " ")))),
        None => balance
            .accounting_method
            .as_deref()
            .filter(|method| !method.is_empty())
            .map(title_case),
    }
}

fn snapshot_from_quota(
    body: &QuotaResponse,
) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
    let balance = body
        .balance
        .as_ref()
        .ok_or_else(|| parse_failure("Missing Neuralwatt balance object"))?;
    if valid_nn(balance.credits_remaining_usd).is_none()
        && valid_nn(balance.credits_used_usd).is_none()
        && valid_pos(balance.total_credits_usd).is_none()
    {
        return Err(parse_failure("Missing Neuralwatt credit balance fields"));
    }

    let sub_window = body
        .subscription
        .as_ref()
        .map(subscription_window)
        .transpose()?
        .flatten();
    let renews_at = body
        .subscription
        .as_ref()
        .filter(|sub| sub.auto_renew != Some(false))
        .and(sub_window.as_ref())
        .and_then(|window| window.resets_at);

    let mut snap = UsageSnapshot::new(
        sub_window.unwrap_or_else(|| RateWindow::informational("No active subscription kWh")),
    );
    if let Some(method) = login_method(body, balance) {
        snap = snap.with_login_method(method);
    }
    if renews_at.is_some() {
        snap = snap.with_subscription(Some(SubscriptionMetadata::new(None, None, renews_at)));
    }

    if let Some(allowance) = body.key.as_ref().and_then(|k| k.allowance.as_ref()) {
        let percent = if allowance.blocked == Some(true) {
            Some(100.0)
        } else if let (Some(spent), Some(limit)) = (
            valid_nn(allowance.spent_usd),
            valid_pos(allowance.limit_usd),
        ) {
            Some(((spent / limit) * 100.0).clamp(0.0, 100.0))
        } else {
            None
        };
        if let Some(percent) = percent {
            let title = format!(
                "Key {}",
                title_case(allowance.period.as_deref().unwrap_or("allowance"))
            );
            snap.extra_rate_windows.push(NamedRateWindow::new(
                "key-allowance",
                title,
                RateWindow::new(percent),
            ));
        }
    }

    Ok((snap, prepaid_cost(balance)))
}

#[cfg(test)]
mod tests;
