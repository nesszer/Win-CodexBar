//! Perplexity provider implementation
//!
//! Fetches credit/usage data from Perplexity's REST billing endpoint with a
//! web session cookie. Behavior follows upstream v0.66.0 `perplexity.js`:
//! browser sessions are tried one by one (a 401/403 rejects only that
//! session), then the environment secret, unless the cookie is manual.

mod cookies;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fmt::Display;

use async_trait::async_trait;
use chrono::{DateTime, Local, TimeZone, Utc};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, RateWindow, SourceMode,
    UsageSnapshot,
};

use cookies::request_cookies;

const CREDITS_URL: &str =
    "https://www.perplexity.ai/rest/billing/credits?version=2.18&source=default";
const ORIGIN: &str = "https://www.perplexity.ai";
const REFERER: &str = "https://www.perplexity.ai/account/usage";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const ENV_SESSION_TOKEN: &str = "PERPLEXITY_SESSION_TOKEN";
const ENV_COOKIE: &str = "PERPLEXITY_COOKIE";
/// Recurring credits below this many cents are a Pro plan, above it Max.
const MAX_PLAN_MIN_RECURRING_CENTS: f64 = 5000.0;
/// Shown in place of the recurring lane when only bonus or purchased credits
/// exist. `UsageSnapshot` always carries a primary window, so this stands in
/// for upstream's absent lane and is skipped by Automatic metric selection.
const NO_RECURRING_CREDITS: &str = "No recurring credits";

/// Every numeric field is required and finite (JSON cannot carry NaN), as in
/// upstream's validation. The camelCase aliases are accepted there too.
#[derive(Debug, Deserialize)]
struct CreditsResponse {
    #[serde(rename = "balance_cents", alias = "balanceCents")]
    _balance_cents: f64,
    #[serde(alias = "renewalDateTs")]
    renewal_date_ts: f64,
    #[serde(alias = "currentPeriodPurchasedCents")]
    current_period_purchased_cents: f64,
    #[serde(alias = "creditGrants")]
    credit_grants: Vec<CreditGrant>,
    #[serde(alias = "totalUsageCents")]
    total_usage_cents: f64,
}

#[derive(Debug, Deserialize)]
struct CreditGrant {
    #[serde(rename = "type")]
    grant_type: String,
    #[serde(alias = "amountCents")]
    amount_cents: f64,
    #[serde(default, alias = "expiresAtTs")]
    expires_at_ts: Option<f64>,
}

pub struct PerplexityProvider {
    client: Client,
    credits_url: String,
}

impl PerplexityProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            credits_url: CREDITS_URL.to_string(),
        }
    }

    fn ts_to_datetime(ts: f64) -> Option<DateTime<Utc>> {
        if !ts.is_finite() {
            return None;
        }
        // Grant expiry epochs are whole-second unix timestamps, far below i64::MAX.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "whole-second epoch fits i64"
        )]
        let secs = ts as i64;
        Utc.timestamp_opt(secs, 0).single()
    }

    /// Map the credit payload to usage lanes.
    ///
    /// Usage burns recurring credits first, then purchased, then promotional.
    /// Only unexpired promotional grants count. Secondary (bonus) and tertiary
    /// (purchased) lanes are always present and read 100% used when their pool
    /// is empty; the primary lane is dropped when recurring credits are absent
    /// but another pool exists, and reads `0/0 credits` at 100% when none does.
    /// `expiry_tz` renders the promotional expiry date.
    fn parse_response<Tz>(
        resp: &CreditsResponse,
        now: DateTime<Utc>,
        expiry_tz: &Tz,
    ) -> UsageSnapshot
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        #[expect(
            clippy::cast_precision_loss,
            reason = "epoch milliseconds are far below 2^53"
        )]
        let now_seconds = now.timestamp_millis() as f64 / 1000.0;
        let sum = |grants: &[&CreditGrant]| {
            grants
                .iter()
                .fold(0.0, |total, grant| total + grant.amount_cents)
                .max(0.0)
        };
        let of_type = |kind: &str| -> Vec<&CreditGrant> {
            resp.credit_grants
                .iter()
                .filter(|grant| grant.grant_type == kind)
                .collect()
        };

        let recurring = sum(&of_type("recurring"));
        let promo_grants: Vec<&CreditGrant> = of_type("promotional")
            .into_iter()
            .filter(|grant| grant.expires_at_ts.unwrap_or(f64::INFINITY) > now_seconds)
            .collect();
        let promo = sum(&promo_grants);
        // Purchased credits may sit in the top-level field, in `purchased`
        // grants, or both; the larger figure avoids double counting.
        let purchased = sum(&of_type("purchased"))
            .max(resp.current_period_purchased_cents)
            .max(0.0);

        let mut remaining = resp.total_usage_cents;
        let recurring_used = remaining.min(recurring);
        remaining -= recurring_used;
        let purchased_used = remaining.min(purchased);
        remaining -= purchased_used;
        let promo_used = remaining.min(promo);

        let promo_expiry = promo_grants
            .iter()
            .filter_map(|grant| grant.expires_at_ts)
            .min_by(f64::total_cmp)
            .and_then(Self::ts_to_datetime);
        let renewal = Self::ts_to_datetime(resp.renewal_date_ts);

        let primary = if recurring > 0.0 {
            RateWindow::with_details(
                (recurring_used / recurring * 100.0).clamp(0.0, 100.0),
                None,
                renewal,
                Self::credit_description(recurring_used, recurring, "credits"),
            )
        } else if promo > 0.0 || purchased > 0.0 {
            RateWindow::informational(NO_RECURRING_CREDITS)
        } else {
            RateWindow::with_details(100.0, None, renewal, Some("0/0 credits".to_string()))
        };

        let mut promo_description = Self::credit_description(promo_used, promo, "bonus");
        if let (Some(expiry), Some(description)) = (promo_expiry, promo_description.as_mut()) {
            description.push_str(&format!(
                " · exp. {}",
                expiry.with_timezone(expiry_tz).format("%b %-d")
            ));
        }
        let secondary = Self::pool_window(promo_used, promo, promo_description);
        let tertiary = Self::pool_window(
            purchased_used,
            purchased,
            Self::credit_description(purchased_used, purchased, "credits"),
        );

        let mut snapshot = UsageSnapshot::new(primary)
            .with_secondary(secondary)
            .with_tertiary(tertiary);
        if recurring > 0.0 {
            let plan = if recurring < MAX_PLAN_MIN_RECURRING_CENTS {
                "Pro"
            } else {
                "Max"
            };
            snapshot = snapshot.with_login_method(plan);
        }
        snapshot
    }

    /// Lane for a bonus or purchased pool. An empty pool is informational
    /// (`0/0 credits` text, no bar): a depleted-looking 100% lane would fire
    /// exhausted notifications and quota hooks for every account that simply
    /// has no promotional or purchased credits.
    fn pool_window(used: f64, total: f64, description: Option<String>) -> RateWindow {
        if total > 0.0 {
            let percent = (used / total * 100.0).clamp(0.0, 100.0);
            RateWindow::with_details(percent, None, None, description)
        } else {
            RateWindow::informational(description.unwrap_or_default())
        }
    }

    /// `used/total unit` as whole numbers: used rounds half away from zero,
    /// total truncates, and a negative zero prints as `0`. Non-finite counts
    /// have no description.
    fn credit_description(used: f64, total: f64, unit: &str) -> Option<String> {
        if !used.is_finite() || !total.is_finite() {
            return None;
        }
        let whole = |value: f64| if value == 0.0 { 0.0 } else { value };
        let used = whole(used.round());
        let total = whole(total.trunc());
        Some(format!("{used:.0}/{total:.0} {unit}"))
    }

    /// Request the credits payload with one session cookie. `Ok(None)` means
    /// Perplexity rejected this cookie (401/403), so the next candidate may
    /// still work.
    async fn fetch_credits(&self, cookie: &str) -> Result<Option<UsageSnapshot>, ProviderError> {
        let response = self
            .client
            .get(&self.credits_url)
            .header("Cookie", cookie)
            .header("Origin", ORIGIN)
            .header("Referer", REFERER)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .send()
            .await?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Ok(None);
        }
        if status != StatusCode::OK {
            return Err(ProviderError::Other(format!(
                "Perplexity API error: HTTP {}",
                status.as_u16()
            )));
        }

        let body = response.text().await?;
        let parsed: CreditsResponse = serde_json::from_str(&body).map_err(|error| {
            let reason = if error.is_data() {
                "invalid credit fields"
            } else {
                "invalid JSON"
            };
            ProviderError::Parse(format!("Failed to parse Perplexity usage data: {reason}"))
        })?;

        Ok(Some(Self::parse_response(&parsed, Utc::now(), &Local)))
    }

    /// Try each header's session cookies in order and return the first
    /// accepted response. A rejected cookie moves on to the next one; any
    /// other failure ends the attempt.
    async fn fetch_first_accepted(
        &self,
        headers: &[String],
    ) -> Result<UsageSnapshot, ProviderError> {
        let mut attempted = HashSet::new();
        let mut rejected = false;
        for header in headers {
            for cookie in request_cookies(header) {
                if !attempted.insert(cookie.clone()) {
                    continue;
                }
                match self.fetch_credits(&cookie).await? {
                    Some(usage) => return Ok(usage),
                    None => rejected = true,
                }
            }
        }
        Err(if rejected {
            ProviderError::AuthRequired
        } else {
            ProviderError::NoCookies
        })
    }

    async fn fetch_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        // A manual cookie is exclusive: no browser sessions, no environment.
        if let Some(manual) = ctx.manual_cookie_header.as_deref() {
            return self.fetch_first_accepted(&[manual.to_string()]).await;
        }

        // An unreadable browser store is only reported when no session at all
        // (browser or environment) was found.
        let (mut headers, browser_error) =
            match crate::providers::browser_cookie_headers_for_domain("perplexity.ai") {
                Ok(candidates) => (
                    candidates.into_iter().map(|(_, header)| header).collect(),
                    None,
                ),
                Err(ProviderError::NoCookies) => (Vec::new(), None),
                Err(error) => (Vec::new(), Some(error)),
            };
        headers.extend(environment_cookie(|name| std::env::var(name).ok()));

        let result = self.fetch_first_accepted(&headers).await;
        match (result, browser_error) {
            (Err(ProviderError::NoCookies), Some(error)) => Err(error),
            (result, _) => result,
        }
    }
}

/// Environment session, `PERPLEXITY_SESSION_TOKEN` before `PERPLEXITY_COOKIE`.
/// Blank values are ignored and one pair of surrounding quotes is removed.
fn environment_cookie(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    let cleaned = |name: &str| {
        let value = get(name)?;
        let value = value.trim();
        let unquoted = ['"', '\'']
            .iter()
            .find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote))
            .unwrap_or(value)
            .trim();
        (!unquoted.is_empty()).then(|| unquoted.to_string())
    };
    cleaned(ENV_SESSION_TOKEN).or_else(|| cleaned(ENV_COOKIE))
}

impl Default for PerplexityProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for PerplexityProvider {
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    fn id(&self) -> ProviderId {
        ProviderId::Perplexity
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Perplexity usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let usage = self.fetch_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
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

    /// Browser sessions are tried one by one inside the provider, so the shell
    /// must not pre-merge a single browser cookie header for Auto.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }
}
