//! Grok CLI credits-proxy source (`cli-chat-proxy.grok.com/v1/billing?format=credits`).
//!
//! The grok.com gRPC-web billing endpoint requires a browser-held Web Key
//! Exchange keypair, so bearer logins read credits from the CLI proxy first
//! and only fall back to gRPC-web when the proxy is unavailable. Wire shape
//! and precedence follow upstream `GrokCreditsProxyFetcher` (v0.67.0).

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::core::ProviderError;
use crate::providers::{BoundedBodyError, read_bounded_response};

use super::billing::GrokBillingSnapshot;
use super::product_usage::LossyProductUsage;
use super::{GrokCredentials, GrokProvider, grok_plan_display_name};

pub(super) const CREDITS_PROXY_ENDPOINT: &str =
    "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Budget for the grok.com retry that supplies a percent a period-only
/// credits answer lacks. Period-only payloads recur on every refresh, so a
/// grok.com outage must not delay the credits answer already in hand.
const PERCENT_RETRY_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// Bearer billing result plus the plan name the credits payload carried.
#[derive(Debug, Clone)]
pub(super) struct BearerBilling {
    pub(super) billing: GrokBillingSnapshot,
    pub(super) subscription_tier: Option<String>,
    /// Purchased credits in USD, from the proxy wallet only (grok.com gRPC-web
    /// does not publish one). `None` when absent, invalid, or unknown.
    pub(super) prepaid_balance_usd: Option<f64>,
}

#[derive(Deserialize)]
struct CreditsResponse {
    config: Option<CreditsConfig>,
    #[serde(rename = "subscriptionTier")]
    subscription_tier: Option<String>,
}

#[derive(Deserialize)]
struct CreditsConfig {
    #[serde(rename = "creditUsagePercent")]
    credit_usage_percent: Option<f64>,
    #[serde(rename = "currentPeriod")]
    current_period: Option<CurrentPeriod>,
    #[serde(rename = "billingPeriodStart")]
    billing_period_start: Option<String>,
    #[serde(rename = "billingPeriodEnd")]
    billing_period_end: Option<String>,
    #[serde(rename = "onDemandCap")]
    on_demand_cap: Option<CreditsAmount>,
    #[serde(rename = "onDemandUsed")]
    on_demand_used: Option<CreditsAmount>,
    #[serde(rename = "subscriptionTier")]
    subscription_tier: Option<String>,
    #[serde(default, rename = "productUsage")]
    product_usage: LossyProductUsage,
    /// Purchased credits, read leniently by [`prepaid_balance_usd`] so a
    /// malformed wallet can never fail an otherwise valid quota answer.
    #[serde(default, rename = "prepaidBalance")]
    prepaid_balance: Option<Value>,
}

#[derive(Deserialize)]
struct CurrentPeriod {
    start: Option<String>,
    end: Option<String>,
}

/// The proxy reports amounts as `{ "val": <number> }`; fractional values are
/// accepted so an unusual cap/used shape cannot fail an otherwise valid answer.
#[derive(Deserialize)]
struct CreditsAmount {
    val: Option<f64>,
}

/// Purchased credits in USD from `config.prepaidBalance`. The contract is
/// `{ "val": <USD cents> }`: an integer or decimal-integer string converts to
/// dollars, and an empty object is a known zero (proto3 omits the zero scalar).
/// Negative, inexact, out-of-range, or unrecognized values are unknown.
/// Upstream `GrokCreditsProxyFetcher.PrepaidBalance` (v0.72.0).
fn prepaid_balance_usd(wallet: Option<&Value>) -> Option<f64> {
    let object = wallet?.as_object()?;
    let Some(val) = object.get("val") else {
        return object.is_empty().then_some(0.0);
    };
    let cents = match val {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }?;
    usd_from_exact_cents(cents)
}

/// Dollars from cents, accepting only values an `f64` holds exactly (the
/// Swift `Double(exactly:)` check). `2^63` is excluded so `i64::MAX` cannot
/// round-trip through a saturating cast.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the integer round trip below is the exactness check; out-of-range values fail it"
)]
fn usd_from_exact_cents(cents: i64) -> Option<f64> {
    let value = cents as f64;
    (cents >= 0 && value < 9_223_372_036_854_775_808.0 && value as i64 == cents)
        .then(|| value / 100.0)
}

fn parse_timestamp(raw: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw?)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

/// Full billing-cycle length; `None` unless the start precedes the end and
/// is not in the future, so a reset timestamp alone never invents a cadence.
/// Upstream `GrokCreditsProxyFetcher.windowMinutes` keeps the length of a
/// period that has already ended.
fn window_minutes(
    start: Option<&str>,
    end: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<u32> {
    let start = parse_timestamp(start)?;
    let end = end?;
    if start > now || end <= start {
        return None;
    }
    u32::try_from((end - start).num_minutes())
        .ok()
        .filter(|minutes| *minutes > 0)
}

pub(super) fn parse_credits_response(
    data: &[u8],
    now: DateTime<Utc>,
) -> Result<BearerBilling, ProviderError> {
    let malformed = || ProviderError::Parse("Grok credits response is malformed".to_string());
    let response: CreditsResponse = serde_json::from_slice(data).map_err(|_| malformed())?;
    let config = response.config.ok_or_else(malformed)?;

    let subscription_tier = grok_plan_display_name(
        config
            .subscription_tier
            .or(response.subscription_tier)
            .as_deref(),
    );
    let current_end = parse_timestamp(
        config
            .current_period
            .as_ref()
            .and_then(|period| period.end.as_deref()),
    );
    let resets_at = current_end.or_else(|| parse_timestamp(config.billing_period_end.as_deref()));
    // Match the start to the selected end; never combine different billing periods.
    let period_start = if current_end.is_some() {
        config
            .current_period
            .as_ref()
            .and_then(|period| period.start.as_deref())
    } else {
        config.billing_period_start.as_deref()
    };
    let window_minutes = window_minutes(period_start, resets_at, now);
    let prepaid_balance_usd = prepaid_balance_usd(config.prepaid_balance.as_ref());

    let wire_percent = config
        .credit_usage_percent
        .filter(|percent| percent.is_finite());
    // Composition is checked against the raw percent, before display clamping.
    let product_usage = wire_percent
        .map(|percent| config.product_usage.composing(percent))
        .unwrap_or_default();
    let used_percent = wire_percent
        .or_else(|| {
            let cap = config.on_demand_cap?.val.filter(|cap| *cap > 0.0)?;
            Some(config.on_demand_used?.val? / cap * 100.0)
        })
        .filter(|percent| percent.is_finite())
        .map(|percent| percent.clamp(0.0, 100.0));
    if used_percent.is_none() && resets_at.is_none() && prepaid_balance_usd.is_none() {
        return Err(malformed());
    }
    Ok(BearerBilling {
        billing: GrokBillingSnapshot {
            used_percent,
            // A percent taken from this payload is published on the wire; a
            // period-only answer leaves usage unknown rather than zero.
            used_percent_is_wire_published: used_percent.is_some(),
            used_percent_is_implicit_zero: false,
            resets_at,
            window_minutes,
            // Shares of this payload's wire percent; empty for every other
            // percent source (on-demand ratio, period-only).
            product_usage,
        },
        subscription_tier,
        prepaid_balance_usd,
    })
}

/// Keep the proxy's authoritative period and plan, adopting the grok.com
/// reading only when it actually published a percent (or an implicit zero).
fn adopt_grpc_percent(proxy: BearerBilling, grpc: GrokBillingSnapshot) -> BearerBilling {
    let publishes_percent = grpc.used_percent.is_some()
        && (grpc.used_percent_is_wire_published || grpc.used_percent_is_implicit_zero);
    if !publishes_percent {
        return proxy;
    }
    BearerBilling {
        billing: GrokBillingSnapshot {
            used_percent: grpc.used_percent,
            used_percent_is_wire_published: grpc.used_percent_is_wire_published,
            used_percent_is_implicit_zero: grpc.used_percent_is_implicit_zero,
            // The grok.com percent is a different total than any proxy share
            // list, so only the grok.com breakdown may accompany it.
            product_usage: grpc.product_usage,
            ..proxy.billing
        },
        subscription_tier: proxy.subscription_tier,
        prepaid_balance_usd: proxy.prepaid_balance_usd,
    }
}

impl GrokProvider {
    /// Bearer billing: credits proxy first, then the grok.com gRPC-web path.
    pub(super) async fn fetch_bearer_billing(
        &self,
        credentials: &GrokCredentials,
    ) -> Result<BearerBilling, ProviderError> {
        // Recheck after optional work was scheduled; the token may have expired meanwhile.
        if credentials.is_expired(Utc::now()) {
            return Err(ProviderError::AuthRequired);
        }
        let authorization = format!("Bearer {}", credentials.access_token);
        let proxy = match self.fetch_credits_proxy(&authorization).await {
            Ok(proxy) => proxy,
            Err(error) => {
                tracing::debug!("Grok credits proxy failed, trying grok.com billing: {error}");
                let billing = self.fetch_billing(Some(authorization), None).await?;
                return Ok(BearerBilling {
                    billing,
                    subscription_tier: None,
                    prepaid_balance_usd: None,
                });
            }
        };
        if proxy.billing.used_percent.is_some() {
            return Ok(proxy);
        }
        match tokio::time::timeout(
            PERCENT_RETRY_TIMEOUT,
            self.fetch_billing(Some(authorization), None),
        )
        .await
        {
            Ok(Ok(grpc)) => Ok(adopt_grpc_percent(proxy, grpc)),
            Ok(Err(error)) => {
                tracing::debug!("Grok grok.com percent retry failed: {error}");
                Ok(proxy)
            }
            Err(_) => {
                tracing::debug!("Grok grok.com percent retry timed out");
                Ok(proxy)
            }
        }
    }

    async fn fetch_credits_proxy(
        &self,
        authorization: &str,
    ) -> Result<BearerBilling, ProviderError> {
        let response = self
            .client
            .get(&self.credits_proxy_endpoint)
            .timeout(REQUEST_TIMEOUT)
            .header("Authorization", authorization)
            .header("x-xai-token-auth", "xai-grok-cli")
            .header("Accept", "application/json")
            .header("User-Agent", "CodexBar")
            .send()
            .await?;
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthRequired);
        }
        if status != StatusCode::OK {
            return Err(ProviderError::Other(format!(
                "Grok credits proxy returned status {status}"
            )));
        }
        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => {
                    ProviderError::Parse("Grok credits response is too large".to_string())
                }
                BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        parse_credits_response(&body, Utc::now())
    }
}
