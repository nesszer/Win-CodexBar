//! DevPass provider: plan credits and premium weekly usage through the LLM
//! Gateway API.
//!
//! Ported from upstream CodexBar 0.66.0 (`devpass` plugin).
//! `GET https://api.llmgateway.io/v1/key` with a regular gateway API key
//! returns the key's all-time spend plus the organization's DevPass plan
//! state. Only that fixed HTTPS origin receives the key, redirects are not
//! followed, and response bodies are never echoed into errors.
//!
//! Remaining plan credits are an allowance, not a wallet balance, so no typed
//! `CostSnapshot` balance is emitted. The premium window starts with the
//! first premium request; an inactive window has zero usage and no reset, and
//! no monthly reset is ever inferred.

#[cfg(test)]
mod tests;
mod wire;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};
use wire::{DevPlan, KeyData, Money, PlanFields};

const API_URL: &str = "https://api.llmgateway.io/v1/key";
const CREDENTIAL_TARGET: &str = "codexbar-devpass";
const ENV_KEYS: &[&str] = &["DEVPASS_API_KEY"];
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BODY_BYTES: usize = 1024 * 1024;
/// The premium allowance window is seven days.
const PREMIUM_WINDOW_MINUTES: u32 = 7 * 24 * 60;

pub struct DevPassProvider {
    client: Client,
}

impl DevPassProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for DevPassProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for DevPassProvider {
    fn id(&self) -> ProviderId {
        ProviderId::DevPass
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    ENV_KEYS,
                )?;
                fetch_key(&self.client, API_URL, &key).await
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

async fn fetch_key(
    client: &Client,
    url: &str,
    key: &str,
) -> Result<ProviderFetchResult, ProviderError> {
    let response = client
        .get(url)
        .bearer_auth(key)
        .header("Accept", "application/json")
        .send()
        .await?;
    check_status(response.status())?;
    let body = read_bounded_response(response, MAX_BODY_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::TooLarge => unrecognized(),
            BoundedBodyError::Read(error) => ProviderError::Network(error),
        })?;
    parse_result(&body)
}

fn parse_result(body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let data = wire::parse_key(body).ok_or_else(unrecognized)?;
    let plan = match data.dev_plan {
        DevPlan::None => None,
        _ => Some(wire::parse_plan(body).ok_or_else(unrecognized)?),
    };
    Ok(build_result(&data, plan.as_ref()))
}

fn unrecognized() -> ProviderError {
    ProviderError::Parse("DevPass returned an unrecognized usage response.".into())
}

fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    match status.as_u16() {
        200 => Ok(()),
        401 => Err(ProviderError::OAuthExpired(
            "DevPass API key was rejected or is inactive.".into(),
        )),
        403 => Err(ProviderError::Other(
            "DevPass requires a regular gateway API key.".into(),
        )),
        429 => Err(ProviderError::Other(
            "DevPass usage requests are rate limited.".into(),
        )),
        500.. => Err(ProviderError::Other(
            "DevPass usage is temporarily unavailable.".into(),
        )),
        code => Err(ProviderError::Other(format!(
            "DevPass returned HTTP {code}."
        ))),
    }
}

fn usd(money: Money) -> String {
    format!("${:.2}", money.value())
}

/// Percentage used, clamped to 0-100 so an over-limit amount fills the bar
/// while the row text keeps the actual figures.
fn percent(used: Money, limit: Money) -> f64 {
    (used.value() / limit.value() * 100.0).clamp(0.0, 100.0)
}

/// A used-of-limit row; the bar appears only for a positive limit.
fn allowance_row(
    id: &str,
    title: &str,
    used: Money,
    limit: Money,
) -> Option<ProviderDisplayDetail> {
    let row = ProviderDisplayDetail::new(id, title, format!("{} / {}", usd(used), usd(limit)))?;
    if limit.value() > 0.0 {
        row.with_progress(used.value().min(limit.value()), limit.value())
    } else {
        Some(row)
    }
}

fn build_result(data: &KeyData, plan: Option<&PlanFields>) -> ProviderFetchResult {
    let usage = match plan {
        Some(plan) => plan_usage(plan),
        None => UsageSnapshot::new(RateWindow::informational("Pay as you go, no DevPass plan")),
    }
    .with_login_method(data.dev_plan.login_method());

    let mut result = ProviderFetchResult::new(usage, "api");
    if let Some(plan) = plan {
        result = result
            .with_display_detail(allowance_row(
                "cycle-used",
                "Cycle used",
                plan.credits_used,
                plan.credits_limit,
            ))
            .with_display_detail(ProviderDisplayDetail::new(
                "cycle-remaining",
                "Cycle remaining",
                usd(plan.credits_remaining),
            ))
            .with_display_detail(allowance_row(
                "premium-weekly",
                "Premium weekly",
                plan.premium_used,
                plan.premium_limit,
            ));
    }
    result = result.with_display_detail(ProviderDisplayDetail::new(
        "key-usage",
        "All-time key usage",
        usd(data.usage),
    ));
    if let Some(limit) = data.limit {
        result = result.with_display_detail(ProviderDisplayDetail::new(
            "key-limit",
            "Key spending limit",
            usd(limit),
        ));
    }
    result
}

/// Plan credits are the primary lane and carry no reset or window length:
/// the endpoint exposes no monthly reset, so none is inferred. A zero
/// allowance has nothing to measure against and stays informational.
fn plan_usage(plan: &PlanFields) -> UsageSnapshot {
    let primary = if plan.credits_limit.value() > 0.0 {
        RateWindow::new(percent(plan.credits_used, plan.credits_limit))
    } else {
        RateWindow::informational("No plan credit allowance reported")
    };
    let mut usage = UsageSnapshot::new(primary);
    if plan.premium_limit.value() > 0.0 {
        usage = usage.with_secondary(RateWindow::with_details(
            percent(plan.premium_used, plan.premium_limit),
            Some(PREMIUM_WINDOW_MINUTES),
            plan.premium_resets_at,
            None,
        ));
    }
    usage
}
