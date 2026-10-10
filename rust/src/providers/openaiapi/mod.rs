//! OpenAI API usage provider.
//!
//! Tracks organization usage from the Admin API, with the older platform credit
//! balance endpoint as a fallback for keys that are not project-scoped Admin keys.
//!
//! Upstream v0.66.0 `Resources/Plugins/openai.js` parity:
//! - `/v1/organization/costs` (`group_by=line_item`) and
//!   `/v1/organization/usage/completions` (`group_by=model`) use `bucket_width=1d`,
//!   `limit <= 31` and UTC-day-aligned 31-day ranges, following `has_more` / `next_page`
//!   (at most 100 pages per range; a missing or repeated cursor is an error).
//! - Token totals are `input + input_audio + output + output_audio`; cached input is a
//!   subset of input and is never added on top.
//! - Each Admin GET gets one transient retry (see [`RetryPolicy`]).
//!
//! The Admin path also returns a per-UTC-day [`crate::core::OpenAiApiUsageHistory`] (upstream's
//! `openAIAPIUsage` card, built in [`history`]) next to the spend summary.
//!
//! The history window is fixed at 30 days. Upstream's `OPENAI_HISTORY_DAYS` (1-365) is
//! deferred until a Windows setting exists for it; [`usage_ranges`] already takes the day
//! count, so honoring it later only needs to pass the setting through.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use reqwest::header::{HeaderValue, RETRY_AFTER};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

mod history;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::json;

const OPENAI_CREDIT_GRANTS_URL: &str = "https://api.openai.com/v1/dashboard/billing/credit_grants";
const OPENAI_ORG_COSTS_URL: &str = "https://api.openai.com/v1/organization/costs";
const OPENAI_ORG_COMPLETIONS_URL: &str = "https://api.openai.com/v1/organization/usage/completions";
const OPENAI_API_CREDENTIAL_TARGET: &str = "codexbar-openaiapi";

/// Days of history requested from the Admin API (period label `Last 30 days`).
const HISTORY_DAYS: u32 = 30;
/// Endpoint bucket limit: each request covers at most this many daily buckets.
const MAX_BUCKETS_PER_REQUEST: u32 = 31;
const SECONDS_PER_DAY: i64 = 86_400;
/// Upstream `pages()` stops with a parse failure after this many pages per range.
const MAX_PAGES_PER_RANGE: usize = 100;
/// Upstream fetches with `timeoutSeconds: 20`.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct CreditGrantsResponse {
    total_granted: f64,
    total_used: f64,
    total_available: f64,
    grants: Option<CreditGrantList>,
}

#[derive(Debug, Deserialize)]
struct CreditGrantList {
    data: Vec<CreditGrant>,
}

#[derive(Debug, Deserialize)]
struct CreditGrant {
    expires_at: Option<i64>,
}

/// One page of an Admin API listing. `has_more` is required; a page without it is malformed.
#[derive(Debug, Deserialize)]
struct Page<T> {
    data: Vec<T>,
    has_more: bool,
    next_page: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CostBucket {
    start_time: i64,
    end_time: i64,
    results: Vec<CostResult>,
}

#[derive(Debug, Deserialize)]
struct CostResult {
    amount: Option<CostAmount>,
    line_item: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CostAmount {
    value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct CompletionsUsageBucket {
    start_time: i64,
    end_time: i64,
    results: Vec<CompletionsUsageResult>,
}

#[derive(Debug, Deserialize)]
struct CompletionsUsageResult {
    model: Option<String>,
    input_tokens: Option<i64>,
    input_cached_tokens: Option<i64>,
    output_tokens: Option<i64>,
    input_audio_tokens: Option<i64>,
    output_audio_tokens: Option<i64>,
    num_model_requests: Option<i64>,
}

/// One `start_time..end_time` request window of at most [`MAX_BUCKETS_PER_REQUEST`] days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UsageRange {
    start: i64,
    end: i64,
    limit: u32,
}

/// Transient retry for the Admin GETs, mirroring upstream `transientIdempotent`
/// (one retry, base delay 1 s, `Retry-After` honored up to 10 s).
#[derive(Debug, Clone, Copy)]
struct RetryPolicy {
    base_delay: Duration,
    max_delay: Duration,
}

impl RetryPolicy {
    const DEFAULT: Self = Self {
        base_delay: Duration::from_secs(1),
        max_delay: Duration::from_secs(10),
    };

    /// Upstream `retryableStatusCodes`.
    fn retries_status(status: reqwest::StatusCode) -> bool {
        matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
    }

    /// Numeric `Retry-After` seconds capped at `max_delay`; anything else uses `base_delay`.
    fn delay(&self, retry_after: Option<&HeaderValue>) -> Duration {
        retry_after
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .map_or(self.base_delay, |seconds| {
                Duration::from_secs_f64(seconds.min(self.max_delay.as_secs_f64()))
            })
    }
}

#[derive(Debug, Clone)]
struct Endpoints {
    credit_grants: String,
    costs: String,
    completions: String,
}

impl Endpoints {
    fn production() -> Self {
        Self {
            credit_grants: OPENAI_CREDIT_GRANTS_URL.to_string(),
            costs: OPENAI_ORG_COSTS_URL.to_string(),
            completions: OPENAI_ORG_COMPLETIONS_URL.to_string(),
        }
    }
}

/// Resolved key plus whether it came from an Admin key source (Preferences, keychain,
/// `OPENAI_ADMIN_KEY`) rather than a plain `OPENAI_API_KEY`-style variable.
struct ApiCredential {
    key: String,
    is_admin: bool,
}

/// Upstream `OpenAIAPIUsageCredential.allowsLegacyBalanceFallback`: the credit-grants
/// endpoint is not project-filtered, so a project-scoped Admin key never falls back to it.
fn allows_legacy_balance_fallback(project_id: Option<&str>, is_admin: bool) -> bool {
    project_id.is_none() || !is_admin
}

pub struct OpenAIApiProvider {
    client: Client,
    endpoints: Endpoints,
    retry: RetryPolicy,
}

impl OpenAIApiProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
            endpoints: Endpoints::production(),
            retry: RetryPolicy::DEFAULT,
        }
    }

    fn credential(api_key: Option<&str>) -> Result<ApiCredential, ProviderError> {
        resolve_api_key(
            api_key,
            OPENAI_API_CREDENTIAL_TARGET,
            &[
                ("OPENAI_ADMIN_KEY", true),
                ("OPENAI_ADMIN_API_KEY", true),
                ("OPENAI_API_KEY", false),
                ("OPENAI_PLATFORM_API_KEY", false),
            ],
        )
    }

    async fn fetch_api(&self, api_key: &str) -> Result<ProviderFetchResult, ProviderError> {
        let response = self
            .client
            .get(&self.endpoints.credit_grants)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthRequired);
        }
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "OpenAI API credit balance returned status {}",
                response.status()
            )));
        }

        let decoded: CreditGrantsResponse = response.json().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse OpenAI API credit grants: {e}"))
        })?;
        Ok(result_from_grants(&decoded))
    }

    async fn fetch_admin_usage(
        &self,
        api_key: &str,
        project_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let project_id = clean_project_id(project_id);
        let ranges = usage_ranges(now, HISTORY_DAYS);

        let costs: Vec<CostBucket> = self
            .fetch_pages(
                &self.endpoints.costs,
                AdminQuery {
                    group_by: "line_item",
                    project_id: project_id.as_deref(),
                    label: "costs",
                },
                &ranges,
                api_key,
            )
            .await?;
        let completions: Vec<CompletionsUsageBucket> = self
            .fetch_pages(
                &self.endpoints.completions,
                AdminQuery {
                    group_by: "model",
                    project_id: project_id.as_deref(),
                    label: "completions",
                },
                &ranges,
                api_key,
            )
            .await?;

        result_from_admin_usage(&costs, &completions, now, project_id.as_deref())
    }

    /// Admin usage first; the legacy balance endpoint only when
    /// [`allows_legacy_balance_fallback`] permits it.
    async fn fetch_admin_or_balance(
        &self,
        credential: &ApiCredential,
        project_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let admin_error = match self
            .fetch_admin_usage(&credential.key, project_id, now)
            .await
        {
            Ok(result) => return Ok(result),
            Err(admin_error) => admin_error,
        };
        if !allows_legacy_balance_fallback(project_id, credential.is_admin) {
            return Err(admin_error);
        }
        match self.fetch_api(&credential.key).await {
            Ok(result) => Ok(ProviderFetchResult {
                source_label: "billing-api".to_string(),
                ..result
            }),
            Err(balance_error) => {
                if matches!(admin_error, ProviderError::AuthRequired) {
                    Err(balance_error)
                } else {
                    Err(admin_error)
                }
            }
        }
    }

    /// Fetch every page of every range, following `has_more` / `next_page`.
    async fn fetch_pages<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        query: AdminQuery<'_>,
        ranges: &[UsageRange],
        api_key: &str,
    ) -> Result<Vec<T>, ProviderError> {
        let label = query.label;
        let mut buckets = Vec::new();
        for range in ranges {
            let mut page: Option<String> = None;
            let mut seen = HashSet::new();
            for count in 0..MAX_PAGES_PER_RANGE {
                let params = admin_query(range, &query, page.as_deref());
                let decoded: Page<T> = self.fetch_admin_json(url, &params, api_key, label).await?;
                buckets.extend(decoded.data);
                if !decoded.has_more {
                    break;
                }
                let cursor = decoded
                    .next_page
                    .as_deref()
                    .map(str::trim)
                    .filter(|cursor| !cursor.is_empty())
                    .ok_or_else(|| {
                        ProviderError::Parse(format!(
                            "OpenAI API {label} pagination cursor missing"
                        ))
                    })?
                    .to_string();
                if !seen.insert(cursor.clone()) {
                    return Err(ProviderError::Parse(format!(
                        "OpenAI API {label} pagination cursor repeated"
                    )));
                }
                if count + 1 == MAX_PAGES_PER_RANGE {
                    return Err(ProviderError::Parse(format!(
                        "OpenAI API {label} pagination exceeded {MAX_PAGES_PER_RANGE} pages"
                    )));
                }
                page = Some(cursor);
            }
        }
        Ok(buckets)
    }

    async fn fetch_admin_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, String)],
        api_key: &str,
        label: &str,
    ) -> Result<T, ProviderError> {
        let response = self.get_with_retry(url, query, api_key).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let detail = response_error_detail(&body);
            return Err(ProviderError::Other(if detail.is_empty() {
                format!("OpenAI API {label} returned status {status}")
            } else {
                format!("OpenAI API {label} returned status {status}: {detail}")
            }));
        }
        response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse OpenAI API {label}: {e}")))
    }

    /// GET with at most one retry on a transient failure: 408/429/5xx, a timeout, or a
    /// refused connection. Auth failures, TLS errors and other transport errors are
    /// returned at once, and dropping the future (cancellation) never triggers a retry.
    async fn get_with_retry(
        &self,
        url: &str,
        query: &[(&str, String)],
        api_key: &str,
    ) -> Result<reqwest::Response, ProviderError> {
        let send = || {
            self.client
                .get(url)
                .query(query)
                .bearer_auth(api_key)
                .header("Accept", "application/json")
                .send()
        };
        match send().await {
            Ok(response) if RetryPolicy::retries_status(response.status()) => {
                let delay = self.retry.delay(response.headers().get(RETRY_AFTER));
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) => {
                let error = ProviderError::Network(error);
                if !error.is_transport_failure() {
                    return Err(error);
                }
                tokio::time::sleep(self.retry.delay(None)).await;
            }
        }
        Ok(send().await?)
    }
}

fn result_from_grants(grants: &CreditGrantsResponse) -> ProviderFetchResult {
    let used_percent = if grants.total_granted > 0.0 {
        grants.total_used / grants.total_granted * 100.0
    } else if grants.total_available > 0.0 {
        0.0
    } else {
        100.0
    };
    let next_expiry = grants.grants.as_ref().and_then(|list| {
        list.data
            .iter()
            .filter_map(|grant| grant.expires_at)
            .filter_map(|ts| Utc.timestamp_opt(ts, 0).single())
            .filter(|date| *date > Utc::now())
            .min()
    });

    let mut primary = RateWindow::with_details(
        used_percent,
        None,
        next_expiry,
        Some(format!("${:.2} available", grants.total_available.max(0.0))),
    );
    if grants.total_granted <= 0.0 && grants.total_available > 0.0 {
        primary.used_percent = 0.0;
    }

    let usage = UsageSnapshot::new(primary).with_login_method(format!(
        "API balance: ${:.2}",
        grants.total_available.max(0.0)
    ));
    let cost = CostSnapshot::new(grants.total_used.max(0.0), "USD", "API credits")
        .with_limit(grants.total_granted.max(0.0));
    let cost = if let Some(expiry) = next_expiry {
        cost.with_resets_at(expiry)
    } else {
        cost
    };
    ProviderFetchResult::new(usage, "api").with_cost(cost)
}

fn result_from_admin_usage(
    costs: &[CostBucket],
    completions: &[CompletionsUsageBucket],
    now: DateTime<Utc>,
    project_id: Option<&str>,
) -> Result<ProviderFetchResult, ProviderError> {
    let daily = history::daily_usage(costs, completions, now, HISTORY_DAYS)?;

    let cost_total: f64 = daily.iter().map(|day| day.cost_usd).sum();
    let request_total: u128 = daily.iter().map(|day| u128::from(day.requests)).sum();
    let token_total: u128 = daily.iter().map(|day| u128::from(day.total_tokens)).sum();
    let mut model_tokens: HashMap<&str, u128> = HashMap::new();
    let mut line_item_costs: HashMap<&str, f64> = HashMap::new();
    for day in &daily {
        for model in &day.models {
            *model_tokens.entry(&model.name).or_default() += u128::from(model.total_tokens);
        }
        for item in &day.line_items {
            *line_item_costs.entry(&item.name).or_default() += item.cost_usd;
        }
    }
    let start = daily
        .first()
        .and_then(|day| Utc.timestamp_opt(day.start_time, 0).single());
    let project_id = project_id.filter(|id| !id.is_empty());

    let mut usage = UsageSnapshot::new(RateWindow::with_details(
        0.0,
        None,
        start,
        Some(format!("${cost_total:.2} over last {HISTORY_DAYS} days")),
    ))
    .with_extra_rate_window(
        "requests",
        "Requests",
        RateWindow::informational(format!("{request_total} requests")),
    )
    .with_extra_rate_window(
        "tokens",
        "Tokens",
        RateWindow::informational(format!("{token_total} tokens")),
    )
    .with_login_method(
        project_id
            .map(|id| format!("Admin API: {id}"))
            .unwrap_or_else(|| "Admin API".to_string()),
    );
    if let Some(project_id) = project_id {
        usage = usage.with_organization(format!("Project: {project_id}"));
    }
    usage.updated_at = now;

    let mut top_models: Vec<_> = model_tokens.into_iter().collect();
    top_models.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (idx, (model, tokens)) in top_models.into_iter().take(3).enumerate() {
        usage = usage.with_extra_rate_window(
            format!("model-{idx}"),
            format!("Model: {model}"),
            RateWindow::informational(format!("{tokens} tokens")),
        );
    }

    let mut top_items: Vec<_> = line_item_costs.into_iter().collect();
    top_items.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (idx, (item, amount)) in top_items.into_iter().take(3).enumerate() {
        usage = usage.with_extra_rate_window(
            format!("line-item-{idx}"),
            format!("Cost: {item}"),
            RateWindow::informational(format!("${amount:.2}")),
        );
    }

    let mut result = ProviderFetchResult::new(usage, "admin-api").with_cost(CostSnapshot::new(
        cost_total,
        "USD",
        format!("Last {HISTORY_DAYS} days"),
    ));
    if let Some(history) = history::usage_history(daily, HISTORY_DAYS, project_id) {
        result = result.with_open_ai_api_usage(history);
    }
    Ok(result)
}

/// A missing, null or blank amount counts as zero; a present but non-numeric or
/// non-finite one (`NaN`, `Infinity`, `1e309`) is a parse failure, as upstream.
fn cost_amount(result: &CostResult) -> Result<f64, ProviderError> {
    let Some(amount) = &result.amount else {
        return Ok(0.0);
    };
    match &amount.value {
        serde_json::Value::Null => Ok(0.0),
        serde_json::Value::String(text) if text.trim().is_empty() => Ok(0.0),
        value => json::lenient_finite_f64(value).ok_or_else(|| {
            ProviderError::Parse("OpenAI API costs amount must be numeric".to_string())
        }),
    }
}

/// Query parameters shared by every page of one Admin API listing.
struct AdminQuery<'a> {
    group_by: &'static str,
    project_id: Option<&'a str>,
    label: &'static str,
}

fn admin_query(
    range: &UsageRange,
    query: &AdminQuery<'_>,
    page: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("start_time", range.start.to_string()),
        ("end_time", range.end.to_string()),
        ("bucket_width", "1d".to_string()),
        ("limit", range.limit.to_string()),
        ("group_by", query.group_by.to_string()),
    ];
    if let Some(project_id) = clean_project_id(query.project_id) {
        params.push(("project_ids", project_id));
    }
    if let Some(page) = page {
        params.push(("page", page.to_string()));
    }
    params
}

/// Upstream `ranges()`: UTC-day-aligned windows of at most [`MAX_BUCKETS_PER_REQUEST`] days
/// covering the last `history_days` days including today. Upstream ends the final window
/// at tomorrow's midnight; the API rejects a future `end_time`
/// (`end_time must not be in the future`), so the end is clamped to `now`.
fn usage_ranges(now: DateTime<Utc>, history_days: u32) -> Vec<UsageRange> {
    let now = now.timestamp();
    let today = now - now.rem_euclid(SECONDS_PER_DAY);
    let mut start = today - (i64::from(history_days) - 1) * SECONDS_PER_DAY;
    let mut remaining = history_days;
    let mut ranges = Vec::new();
    while remaining > 0 {
        let limit = remaining.min(MAX_BUCKETS_PER_REQUEST);
        let end = start + i64::from(limit) * SECONDS_PER_DAY;
        ranges.push(UsageRange {
            start,
            end: end.min(now),
            limit,
        });
        start = end;
        remaining -= limit;
    }
    ranges
}

fn clean_project_id(project_id: Option<&str>) -> Option<String> {
    project_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
}
fn response_error_detail(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(error) = value.get("error")
    {
        if let Some(message) = error.get("message").and_then(serde_json::Value::as_str) {
            return message.to_string();
        }
        if let Some(message) = error.as_str() {
            return message.to_string();
        }
    }

    trimmed.chars().take(500).collect()
}

impl Default for OpenAIApiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OpenAIApiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAIApi
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let credential = Self::credential(ctx.api_key.as_deref())?;
                let project_id = clean_project_id(ctx.workspace_id.as_deref());
                self.fetch_admin_or_balance(&credential, project_id.as_deref(), Utc::now())
                    .await
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

/// Resolve the key from Preferences, the credential store, then the environment.
/// Each `env_names` entry carries whether that variable holds an Admin key.
fn resolve_api_key(
    explicit: Option<&str>,
    credential_target: &str,
    env_names: &[(&str, bool)],
) -> Result<ApiCredential, ProviderError> {
    if let Some(key) = explicit
        && !key.trim().is_empty()
    {
        return Ok(ApiCredential {
            key: key.trim().to_string(),
            is_admin: true,
        });
    }
    if let Ok(entry) = keyring::Entry::new(credential_target, "api_key")
        && let Ok(key) = entry.get_password()
        && !key.trim().is_empty()
    {
        return Ok(ApiCredential {
            key,
            is_admin: true,
        });
    }
    for (env, is_admin) in env_names {
        if let Ok(key) = std::env::var(env)
            && !key.trim().is_empty()
        {
            return Ok(ApiCredential {
                key,
                is_admin: *is_admin,
            });
        }
    }
    let names: Vec<&str> = env_names.iter().map(|(name, _)| *name).collect();
    Err(ProviderError::NotInstalled(format!(
        "API key not found. Set {} in Preferences or environment.",
        names.join(" / ")
    )))
}

#[allow(
    dead_code,
    reason = "OpenAI API helper reserved for future dashboard integration"
)]
fn _assert_datetime_send(_: DateTime<Utc>) {}

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod tests;
