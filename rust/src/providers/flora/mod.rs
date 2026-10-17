//! FLORA workspace balance, member limit, and this month's spend.
//!
//! Ported from upstream CodexBar v0.75.0 (`Resources/Plugins/flora.js`,
//! `Providers/Flora/FloraProviderDescriptor.swift`). Reads
//! `GET https://app.flora.ai/api/v1/workspaces/{ws_id}/usage` for the current
//! UTC month with the API key as a Bearer token. API keys cannot list their
//! workspaces, so the `ws_…` workspace ID is required. Response bodies are
//! never echoed in errors and the key is never logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde_json::{Map, Value};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot, format_currency,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response, resolve_api_key};

const BASE_URL: &str = "https://app.flora.ai";
const CREDENTIAL_TARGET: &str = "codexbar-flora";
pub const API_KEY_ENV: &str = "FLORA_API_KEY";
pub const WORKSPACE_ENV: &str = "FLORA_WORKSPACE_ID";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const SECTION_TITLE: &str = "Workspace balance";
const LOGIN_METHOD: &str = "API key";
const WORKSPACE_HINT: &str = "Set the FLORA workspace ID (ws_…) in Settings or FLORA_WORKSPACE_ID.";
const WORKSPACE_FORMAT: &str = "FLORA workspace ID must look like ws_….";
const UNRECOGNIZED: &str = "FLORA returned an unrecognized workspace usage response.";

pub struct FloraProvider {
    client: Client,
    base_url: String,
}

impl FloraProvider {
    pub fn new() -> Self {
        Self::with_base_url(BASE_URL)
    }

    fn with_base_url(base_url: &str) -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url: base_url.trim_end_matches('/').to_owned(),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let workspace = workspace_id(ctx.workspace_id.as_deref())?;
        let key = resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, &[API_KEY_ENV])?;
        let (start_date, end_date) = period_bounds(now);

        let url = format!("{}/api/v1/workspaces/{workspace}/usage", self.base_url);
        let response = self
            .client
            .get(url)
            .bearer_auth(&key)
            .query(&[("start_date", start_date), ("end_date", end_date)])
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => {
                return Err(ProviderError::Other(
                    "FLORA rejected the API key. Check or replace it in Settings.".into(),
                ));
            }
            StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(
                    "FLORA rejected the key: it needs billing permission on this workspace.".into(),
                ));
            }
            StatusCode::NOT_FOUND => {
                return Err(ProviderError::Other(
                    "FLORA did not find this workspace ID.".into(),
                ));
            }
            status => {
                return Err(ProviderError::Other(format!(
                    "FLORA returned HTTP {}.",
                    status.as_u16()
                )));
            }
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::Read(error) => ProviderError::Network(error),
                BoundedBodyError::TooLarge => unrecognized(),
            })?;
        parse_usage(&body)
    }
}

impl Default for FloraProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for FloraProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Flora
    }

    fn metadata(&self) -> &ProviderMetadata {
        &FLORA_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_impl(ctx, Utc::now()).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// The workspace from settings, else `FLORA_WORKSPACE_ID`, validated against
/// `ws_` plus letters and digits so it is safe to place in the URL path.
fn workspace_id(configured: Option<&str>) -> Result<String, ProviderError> {
    let from_env;
    let raw = match configured.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value,
        None => {
            from_env = std::env::var(WORKSPACE_ENV).unwrap_or_default();
            from_env.trim()
        }
    };
    if raw.is_empty() {
        return Err(ProviderError::NotInstalled(WORKSPACE_HINT.into()));
    }
    if !is_workspace_id(raw) {
        return Err(ProviderError::NotInstalled(WORKSPACE_FORMAT.into()));
    }
    Ok(raw.to_string())
}

pub(crate) fn is_workspace_id(value: &str) -> bool {
    value
        .strip_prefix("ws_")
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric()))
}

/// The UTC month to date: `(first day of the month, today)`, both `YYYY-MM-DD`.
fn period_bounds(now: DateTime<Utc>) -> (String, String) {
    (
        format!("{}-01", now.format("%Y-%m")),
        now.format("%Y-%m-%d").to_string(),
    )
}

fn unrecognized() -> ProviderError {
    ProviderError::Parse(UNRECOGNIZED.into())
}

/// Upstream: a JSON object with a finite `available_cost` is required. The
/// member limit and spend sections are optional and never invent values.
fn parse_usage(body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let root: Value = serde_json::from_slice(body).map_err(|_| unrecognized())?;
    let root = root.as_object().ok_or_else(unrecognized)?;
    let available = finite(root.get("available_cost")).ok_or_else(unrecognized)?;
    let currency = currency(root);
    let money = |value: f64| format_currency(value, &currency);
    let scope = root.get("balance_scope").and_then(Value::as_str);

    let mut rows = vec![row(
        "available",
        "Available",
        money(available),
        match scope {
            Some("workspace") => Some("Workspace pool"),
            Some("caller") => Some("Your allowance"),
            _ => None,
        },
    )];

    if let Some(limit) = root.get("member_limit").and_then(Value::as_object)
        && scope == Some("caller")
        && let Some(limit_cost) = finite(limit.get("limit_cost"))
    {
        let period = match limit.get("period").and_then(Value::as_str) {
            Some("month") => Some("Monthly"),
            Some("all_time") => Some("All time"),
            _ => None,
        };
        rows.push(row(
            "member-limit",
            "Member limit",
            money(limit_cost),
            period,
        ));
    }

    if let Some(spend) = root.get("spend").and_then(Value::as_object) {
        match spend.get("status").and_then(Value::as_str) {
            Some("available") => {
                let total = finite(spend.get("total_cost")).ok_or_else(unrecognized)?;
                rows.push(row("spent", "Spent this month", money(total), None));
                if let Some(runs) = whole_count(spend.get("classified_runs")) {
                    rows.push(row(
                        "runs",
                        "Runs this month",
                        format::number(runs, 0),
                        None,
                    ));
                }
            }
            Some("unavailable") => {
                let reason = match spend.get("reason").and_then(Value::as_str) {
                    Some("entitlement_missing") => "Plan has no usage reporting",
                    Some("permission_missing") => "Needs billing permission",
                    Some("summary_unavailable") => "Totals not computed; try again",
                    _ => "Not reported",
                };
                rows.push(row(
                    "spent",
                    "Spent this month",
                    "Unavailable".into(),
                    Some(reason),
                ));
            }
            _ => {}
        }
    }

    let usage = UsageSnapshot::new(RateWindow::informational(SECTION_TITLE).hidden_on_card())
        .with_login_method(LOGIN_METHOD);
    let result = ProviderFetchResult::new(usage, "api");
    Ok(result.with_display_details(rows.into_iter().flatten()))
}

/// One detail row in the workspace section; the secondary text is optional.
fn row(
    id: &str,
    label: &str,
    value: String,
    secondary: Option<&str>,
) -> Option<ProviderDisplayDetail> {
    let detail = ProviderDisplayDetail::new(format!("flora-{id}"), label, value)?;
    let detail = match secondary {
        Some(secondary) => detail.with_secondary_value(secondary)?,
        None => detail,
    };
    detail.with_section_title(SECTION_TITLE)
}

fn finite(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

/// JavaScript `Number.isInteger` on a non-negative value, so `3` and `3.0` both
/// count while `3.5` and negatives are dropped.
fn whole_count(value: Option<&Value>) -> Option<f64> {
    finite(value).filter(|value| value.fract() == 0.0 && *value >= 0.0)
}

/// Upstream uses the reported ISO code only when it is three uppercase letters.
fn currency(root: &Map<String, Value>) -> String {
    root.get("currency")
        .and_then(Value::as_str)
        .filter(|code| code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()))
        .unwrap_or("USD")
        .to_string()
}

const FLORA_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Flora,
    display_name: "FLORA",
    session_label: "Balance",
    weekly_label: "Balance",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://app.flora.ai"),
    status_page_url: None,
    tertiary_label_key: None,
};
