//! OpenAI Live Usage: tokens per minute from the Admin API's 1-minute completion buckets
//! (upstream 0.75.0 #4495: `OpenAILiveUsage`, `OpenAILiveUsageBuilder`,
//! `OpenAILiveUsageFetcher`, and the `liveUsage()` step of `Resources/Plugins/openai.js`).
//!
//! Two fetches read `GET /v1/organization/usage/completions` with `bucket_width=1m`,
//! `group_by=model` and `group_by=batch`, in the same project scope as the daily view:
//! - Each refresh makes one unpaginated `limit=60` request for the last hour
//!   ([`OpenAIApiProvider::fetch_refresh_throughput`]). Any failure drops only the
//!   Tokens/min data; the daily usage stays usable.
//! - The on-demand Live Usage window ([`OpenAIApiProvider::fetch_live_usage`]) pages through
//!   a 15-minute, 1-hour or 6-hour window and reports typed errors.
//!
//! Token totals are `input + output`; cached input is a subset of input. Batch API traffic
//! is included in totals but excluded from limit-counted tokens because it has its own
//! rate-limit pool. Buckets commonly arrive 2-3 minutes late, so every view stops at a
//! shared reporting frontier and treats later minutes as pending instead of zero.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

use super::{OpenAIApiProvider, clean_project_id};
use crate::core::{ProviderThroughput, ThroughputMinute, ThroughputModel};

/// Windows the Live Usage window offers, in minutes.
pub const LIVE_WINDOW_MINUTES: [u32; 3] = [15, 60, 360];
const REFRESH_WINDOW_MINUTES: u32 = 60;
/// Largest integer a JavaScript number holds exactly; upstream rejects anything above it.
const MAX_SAFE_COUNT: u64 = 9_007_199_254_740_991;
/// Upstream card bound for each per-model value of the refresh payload.
const MAX_CARD_COUNT: u64 = 900_719_925_474;
/// 9999-12-31T23:59:59Z.
const MAX_EPOCH_SECONDS: i64 = 253_402_300_799;
const MAX_WINDOW_PAGES: usize = 20;
const MAX_RESULTS_PER_BUCKET: usize = 400;
const MAX_MODELS_PER_BUCKET: usize = 200;
const MAX_REFRESH_ENTRIES: usize = 10_000;
/// The frontier is searched within the last 10 complete minutes.
const FRONTIER_LOOKBACK_SECONDS: i64 = 9 * 60;
const FIVE_MINUTES_SECONDS: i64 = 5 * 60;
const OTHER_MODELS: &str = "Other";
const UNKNOWN_MODEL: &str = "unknown";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiveUsageError {
    #[error("Configure an OpenAI organization Admin API key in Settings → Providers → OpenAI.")]
    MissingCredential,
    #[error(
        "Live usage needs an OpenAI organization Admin API key (sk-admin-…). Project and service-account keys cannot read organization usage."
    )]
    AdminKeyRequired,
    #[error("OpenAI live usage is rate limited.")]
    RateLimited,
    #[error("OpenAI live usage API failed (HTTP {0}).")]
    ApiFailure(u16),
    #[error("Could not parse the OpenAI live usage response.")]
    ParseFailure,
    #[error("OpenAI live usage supports 15-minute, 1-hour, and 6-hour windows.")]
    InvalidWindow,
    #[error("Could not reach the OpenAI live usage API.")]
    Network,
}

/// One model's usage in one minute. `batch_*` is the Batch API share of the totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveModelUsage {
    pub name: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub requests: u64,
    pub batch_tokens: u64,
    pub batch_requests: u64,
}

impl LiveModelUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveMinute {
    /// Epoch seconds, minute-aligned.
    pub start: i64,
    /// Ascending by name.
    pub models: Vec<LiveModelUsage>,
}

/// A validated, regular minute grid ending at the minute containing `fetched_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiLiveUsage {
    pub minutes: Vec<LiveMinute>,
    pub window_minutes: u32,
    /// Epoch seconds.
    pub fetched_at: i64,
    /// Project the usage is scoped to; `None` covers the whole organization.
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveModelFilter<'a> {
    AllModels,
    Model(&'a str),
}

impl LiveModelFilter<'_> {
    fn includes(self, name: &str) -> bool {
        match self {
            Self::AllModels => true,
            Self::Model(model) => model == name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePoint {
    pub start: i64,
    pub total_tokens: u64,
    pub uncached_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub requests: u64,
    pub is_in_progress: bool,
    pub is_pending: bool,
    pub batch_tokens: u64,
    pub batch_requests: u64,
}

impl LivePoint {
    /// Cached input counts toward TPM; Batch API traffic has its own pool and is excluded.
    pub fn limit_counted_tokens(&self) -> u64 {
        self.total_tokens - self.batch_tokens
    }

    pub fn limit_counted_requests(&self) -> u64 {
        self.requests - self.batch_requests
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveModelStack {
    pub minute: i64,
    /// A top model's name, or `Other` for the rest.
    pub model_or_other: String,
    pub tokens: u64,
    pub is_in_progress: bool,
    pub is_pending: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveSummary {
    pub latest_tpm: u64,
    pub latest_minute_start: Option<i64>,
    pub five_minute_average_tpm: f64,
    pub peak_tpm: u64,
    pub peak_minute_start: Option<i64>,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub requests: u64,
    /// Batch API share of `total_tokens`.
    pub batch_tokens: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveModelRow {
    pub name: String,
    pub latest_tpm: u64,
    pub peak_tpm: u64,
    pub total_tokens: u64,
    pub requests: u64,
    /// Manual organization limit, when one resolves for this model.
    pub tpm_limit: Option<u64>,
    pub rpm_limit: Option<u64>,
    /// Highest per-project cap for context only; never drives utilization.
    pub project_cap: Option<u64>,
    pub peak_tpm_percent: Option<f64>,
    pub latest_tpm_percent: Option<f64>,
}

impl OpenAiLiveUsage {
    fn is_in_progress(&self, start: i64) -> bool {
        start + 60 > self.fetched_at
    }

    pub(super) fn is_pending(frontier: Option<i64>, start: i64) -> bool {
        frontier.is_none_or(|frontier| start > frontier)
    }

    /// Newest complete minute with any reported tokens within the last 10 complete
    /// minutes, else the last complete minute, so lagging buckets never read as a drop to
    /// zero while genuine idle periods still do.
    pub fn reporting_frontier(&self) -> Option<i64> {
        let complete: Vec<&LiveMinute> = self
            .minutes
            .iter()
            .filter(|minute| minute.start + 60 <= self.fetched_at)
            .collect();
        let last = complete.last()?;
        let cutoff = last.start - FRONTIER_LOOKBACK_SECONDS;
        Some(
            complete
                .iter()
                .rev()
                .find(|minute| {
                    minute.start >= cutoff
                        && minute.models.iter().any(|model| model.total_tokens() > 0)
                })
                .map_or(last.start, |minute| minute.start),
        )
    }

    /// Ranked by reported (non-pending) volume so partial buckets cannot evict visible
    /// models from the top series; pending-only volume, then the name, break ties.
    pub fn model_names(&self) -> Vec<String> {
        let frontier = self.reporting_frontier();
        let mut reported: HashMap<&str, u64> = HashMap::new();
        let mut totals: HashMap<&str, u64> = HashMap::new();
        for minute in &self.minutes {
            let is_reported = frontier.is_some_and(|frontier| minute.start <= frontier);
            for model in &minute.models {
                *totals.entry(&model.name).or_default() += model.total_tokens();
                if is_reported {
                    *reported.entry(&model.name).or_default() += model.total_tokens();
                }
            }
        }
        let mut names: Vec<&str> = totals.keys().copied().collect();
        names.sort_by(|left, right| {
            let reported_of = |name: &str| reported.get(name).copied().unwrap_or(0);
            reported_of(right)
                .cmp(&reported_of(left))
                .then_with(|| totals[right].cmp(&totals[left]))
                .then_with(|| left.cmp(right))
        });
        names.into_iter().map(ToOwned::to_owned).collect()
    }

    pub fn series(&self, filter: LiveModelFilter<'_>) -> Vec<LivePoint> {
        let frontier = self.reporting_frontier();
        self.minutes
            .iter()
            .map(|minute| {
                let mut point = LivePoint {
                    start: minute.start,
                    total_tokens: 0,
                    uncached_input_tokens: 0,
                    cached_input_tokens: 0,
                    output_tokens: 0,
                    requests: 0,
                    is_in_progress: self.is_in_progress(minute.start),
                    is_pending: Self::is_pending(frontier, minute.start),
                    batch_tokens: 0,
                    batch_requests: 0,
                };
                for model in minute.models.iter().filter(|m| filter.includes(&m.name)) {
                    point.total_tokens += model.total_tokens();
                    point.uncached_input_tokens += model.input_tokens - model.cached_input_tokens;
                    point.cached_input_tokens += model.cached_input_tokens;
                    point.output_tokens += model.output_tokens;
                    point.requests += model.requests;
                    point.batch_tokens += model.batch_tokens;
                    point.batch_requests += model.batch_requests;
                }
                point
            })
            .collect()
    }

    /// Per-minute stacks for the top `top_n` models plus `Other`.
    pub fn stacked_by_model(&self, top_n: usize) -> Vec<LiveModelStack> {
        let frontier = self.reporting_frontier();
        let names = self.model_names();
        let top: Vec<&str> = names.iter().take(top_n).map(String::as_str).collect();
        let has_other = names.len() > top.len();
        let mut labels = top.clone();
        if has_other {
            labels.push(OTHER_MODELS);
        }
        self.minutes
            .iter()
            .flat_map(|minute| {
                let mut totals: HashMap<&str, u64> = HashMap::new();
                for model in &minute.models {
                    let label = if top.contains(&model.name.as_str()) {
                        model.name.as_str()
                    } else {
                        OTHER_MODELS
                    };
                    *totals.entry(label).or_default() += model.total_tokens();
                }
                labels
                    .iter()
                    .map(|label| LiveModelStack {
                        minute: minute.start,
                        model_or_other: (*label).to_string(),
                        tokens: totals.get(label).copied().unwrap_or(0),
                        is_in_progress: self.is_in_progress(minute.start),
                        is_pending: Self::is_pending(frontier, minute.start),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn summary(&self, filter: LiveModelFilter<'_>) -> LiveSummary {
        let points = self.series(filter);
        let frontier = self.reporting_frontier();
        let latest = points.iter().find(|point| Some(point.start) == frontier);
        let recent: Vec<&LivePoint> = frontier.map_or_else(Vec::new, |frontier| {
            points
                .iter()
                .filter(|point| {
                    point.start <= frontier && point.start > frontier - FIVE_MINUTES_SECONDS
                })
                .collect()
        });
        // Pending minutes can hold partial data, so peaks stop at the frontier; a tie keeps
        // the earliest minute.
        let mut peak: Option<&LivePoint> = None;
        for point in points.iter().filter(|point| !point.is_pending) {
            if peak.is_none_or(|best| point.total_tokens > best.total_tokens) {
                peak = Some(point);
            }
        }
        let sum = |field: fn(&LivePoint) -> u64| points.iter().map(field).sum::<u64>();
        LiveSummary {
            latest_tpm: latest.map_or(0, |point| point.total_tokens),
            latest_minute_start: frontier,
            five_minute_average_tpm: if recent.is_empty() {
                0.0
            } else {
                recent
                    .iter()
                    .map(|point| point.total_tokens as f64)
                    .sum::<f64>()
                    / recent.len() as f64
            },
            peak_tpm: peak.map_or(0, |point| point.total_tokens),
            peak_minute_start: peak.map(|point| point.start),
            total_tokens: sum(|point| point.total_tokens),
            input_tokens: sum(|point| point.uncached_input_tokens + point.cached_input_tokens),
            cached_input_tokens: sum(|point| point.cached_input_tokens),
            output_tokens: sum(|point| point.output_tokens),
            requests: sum(|point| point.requests),
            batch_tokens: sum(|point| point.batch_tokens),
        }
    }

    /// One row per model in [`Self::model_names`] order, without limits.
    pub fn model_rows(&self) -> Vec<LiveModelRow> {
        self.model_names()
            .into_iter()
            .map(|name| {
                let summary = self.summary(LiveModelFilter::Model(&name));
                LiveModelRow {
                    name,
                    latest_tpm: summary.latest_tpm,
                    peak_tpm: summary.peak_tpm,
                    total_tokens: summary.total_tokens,
                    requests: summary.requests,
                    tpm_limit: None,
                    rpm_limit: None,
                    project_cap: None,
                    peak_tpm_percent: None,
                    latest_tpm_percent: None,
                }
            })
            .collect()
    }

    /// The compact snapshot the menu card, tray token and bridge carry.
    pub fn throughput(&self) -> ProviderThroughput {
        let frontier = self.reporting_frontier();
        let summary = self.summary(LiveModelFilter::AllModels);
        ProviderThroughput {
            minutes: self
                .series(LiveModelFilter::AllModels)
                .into_iter()
                .map(|point| ThroughputMinute {
                    start: point.start,
                    total_tokens: point.total_tokens,
                    requests: point.requests,
                    is_pending: point.is_pending,
                })
                .collect(),
            frontier,
            latest_tokens_per_minute: frontier.map(|_| summary.latest_tpm),
            peak_tokens_per_minute: summary.peak_tpm,
            peak_at: summary.peak_minute_start,
            five_minute_average_tokens_per_minute: summary.five_minute_average_tpm,
            models: self
                .model_names()
                .into_iter()
                .map(|name| {
                    let model_summary = self.summary(LiveModelFilter::Model(&name));
                    let points = self.series(LiveModelFilter::Model(&name));
                    ThroughputModel {
                        latest_tpm: model_summary.latest_tpm,
                        peak_tpm: model_summary.peak_tpm,
                        total_tokens: model_summary.total_tokens,
                        peak_limit_counted_tokens: points
                            .iter()
                            .filter(|point| !point.is_pending)
                            .map(LivePoint::limit_counted_tokens)
                            .max()
                            .unwrap_or(0),
                        limit_counted_tokens_by_minute: points
                            .iter()
                            .map(LivePoint::limit_counted_tokens)
                            .collect(),
                        name,
                    }
                })
                .collect(),
            window_minutes: self.window_minutes,
            fetched_at: self.fetched_at,
            project_id: self.project_id.clone(),
        }
    }
}

fn checked_add(left: u64, right: u64) -> Result<u64, LiveUsageError> {
    left.checked_add(right)
        .filter(|sum| *sum <= MAX_SAFE_COUNT)
        .ok_or(LiveUsageError::ParseFailure)
}

/// Upstream `OpenAILiveUsageBuilder.build`: validates every bucket, drops minutes outside
/// the window, merges duplicate model rows, and fills missing minutes with empty ones.
pub(super) fn build(
    buckets: Vec<LiveMinute>,
    window_minutes: u32,
    fetched_at: i64,
    project_id: Option<String>,
) -> Result<OpenAiLiveUsage, LiveUsageError> {
    if !LIVE_WINDOW_MINUTES.contains(&window_minutes)
        || !(0..=MAX_EPOCH_SECONDS).contains(&fetched_at)
    {
        return Err(LiveUsageError::ParseFailure);
    }
    let current = fetched_at / 60 * 60;
    let first = current - (i64::from(window_minutes) - 1) * 60;
    let mut grid: HashMap<i64, BTreeMap<String, LiveModelUsage>> = HashMap::new();
    let mut tokens = 0_u64;
    let mut requests = 0_u64;
    for bucket in buckets {
        if !(0..=MAX_EPOCH_SECONDS).contains(&bucket.start) {
            return Err(LiveUsageError::ParseFailure);
        }
        let minute = bucket.start / 60 * 60;
        for model in bucket.models {
            if model.cached_input_tokens > model.input_tokens
                || model.batch_requests > model.requests
            {
                return Err(LiveUsageError::ParseFailure);
            }
            let total = checked_add(model.input_tokens, model.output_tokens)?;
            if model.batch_tokens > total {
                return Err(LiveUsageError::ParseFailure);
            }
            if minute < first || minute > current {
                continue;
            }
            tokens = checked_add(tokens, total)?;
            requests = checked_add(requests, model.requests)?;
            let models = grid.entry(minute).or_default();
            match models.get_mut(&model.name) {
                Some(prior) => {
                    prior.input_tokens = checked_add(prior.input_tokens, model.input_tokens)?;
                    prior.cached_input_tokens =
                        checked_add(prior.cached_input_tokens, model.cached_input_tokens)?;
                    prior.output_tokens = checked_add(prior.output_tokens, model.output_tokens)?;
                    prior.requests = checked_add(prior.requests, model.requests)?;
                    prior.batch_tokens = checked_add(prior.batch_tokens, model.batch_tokens)?;
                    prior.batch_requests = checked_add(prior.batch_requests, model.batch_requests)?;
                }
                None => {
                    models.insert(model.name.clone(), model);
                }
            }
        }
    }
    Ok(OpenAiLiveUsage {
        minutes: (0..i64::from(window_minutes))
            .map(|index| {
                let start = first + index * 60;
                LiveMinute {
                    start,
                    models: grid
                        .remove(&start)
                        .map(|models| models.into_values().collect())
                        .unwrap_or_default(),
                }
            })
            .collect(),
        window_minutes,
        fetched_at,
        project_id,
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct LivePage {
    data: Vec<LiveBucket>,
    #[serde(default)]
    has_more: Option<bool>,
    #[serde(default)]
    next_page: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LiveBucket {
    start_time: i64,
    #[serde(default)]
    end_time: Option<i64>,
    results: Vec<LiveResult>,
}

#[derive(Debug, Deserialize)]
struct LiveResult {
    model: Option<String>,
    batch: Option<bool>,
    input_tokens: Option<i64>,
    input_cached_tokens: Option<i64>,
    output_tokens: Option<i64>,
    num_model_requests: Option<i64>,
    input_audio_tokens: Option<i64>,
    output_audio_tokens: Option<i64>,
}

impl LiveResult {
    /// Absent counts are zero; negative counts, cached above input, or an input+output
    /// total beyond the safe-integer range are parse failures.
    fn usage(&self) -> Result<LiveModelUsage, LiveUsageError> {
        let count = |value: Option<i64>| {
            u64::try_from(value.unwrap_or(0)).map_err(|_| LiveUsageError::ParseFailure)
        };
        let input = count(self.input_tokens)?;
        let cached = count(self.input_cached_tokens)?;
        let output = count(self.output_tokens)?;
        let requests = count(self.num_model_requests)?;
        // input/output already aggregate text, audio and image; audio is only a breakdown.
        count(self.input_audio_tokens)?;
        count(self.output_audio_tokens)?;
        let total = checked_add(input, output)?;
        if cached > input {
            return Err(LiveUsageError::ParseFailure);
        }
        let name = self
            .model
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(UNKNOWN_MODEL)
            .to_string();
        let batch = self.batch == Some(true);
        Ok(LiveModelUsage {
            name,
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
            requests,
            batch_tokens: if batch { total } else { 0 },
            batch_requests: if batch { requests } else { 0 },
        })
    }
}

fn live_query(
    start: i64,
    end: i64,
    limit: u32,
    project_id: Option<&str>,
    page: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("start_time", start.to_string()),
        ("end_time", end.to_string()),
        ("bucket_width", "1m".to_string()),
        ("group_by", "model".to_string()),
        ("group_by", "batch".to_string()),
        ("limit", limit.to_string()),
    ];
    if let Some(project_id) = clean_project_id(project_id) {
        params.push(("project_ids", project_id));
    }
    if let Some(page) = page {
        params.push(("page", page.to_string()));
    }
    params
}

fn status_error(status: reqwest::StatusCode) -> LiveUsageError {
    match status.as_u16() {
        401 | 403 => LiveUsageError::AdminKeyRequired,
        429 => LiveUsageError::RateLimited,
        code => LiveUsageError::ApiFailure(code),
    }
}

/// Upstream plugin `liveUsage()` bounds for the one-page refresh response.
fn refresh_buckets(
    page: LivePage,
    start: i64,
    fetched_at: i64,
) -> Result<Vec<LiveMinute>, LiveUsageError> {
    if page.data.len() > REFRESH_WINDOW_MINUTES as usize || page.has_more.is_none() {
        return Err(LiveUsageError::ParseFailure);
    }
    let mut entries = 0_usize;
    let mut total_tokens = 0_u64;
    let mut total_requests = 0_u64;
    page.data
        .into_iter()
        .map(|bucket| {
            if bucket.start_time < start
                || bucket.start_time > fetched_at
                || bucket.start_time % 60 != 0
                || bucket.end_time.is_none_or(|end| end <= bucket.start_time)
                || bucket.results.len() > MAX_RESULTS_PER_BUCKET
            {
                return Err(LiveUsageError::ParseFailure);
            }
            let mut merged: BTreeMap<String, LiveModelUsage> = BTreeMap::new();
            for result in &bucket.results {
                let row = result.usage()?;
                total_tokens = checked_add(total_tokens, row.total_tokens())?;
                total_requests = checked_add(total_requests, row.requests)?;
                match merged.get_mut(&row.name) {
                    Some(prior) => {
                        prior.input_tokens = checked_add(prior.input_tokens, row.input_tokens)?;
                        prior.cached_input_tokens =
                            checked_add(prior.cached_input_tokens, row.cached_input_tokens)?;
                        prior.output_tokens = checked_add(prior.output_tokens, row.output_tokens)?;
                        prior.requests = checked_add(prior.requests, row.requests)?;
                        prior.batch_tokens = checked_add(prior.batch_tokens, row.batch_tokens)?;
                        prior.batch_requests =
                            checked_add(prior.batch_requests, row.batch_requests)?;
                    }
                    None => {
                        merged.insert(row.name.clone(), row);
                    }
                }
            }
            if merged.len() > MAX_MODELS_PER_BUCKET {
                return Err(LiveUsageError::ParseFailure);
            }
            entries += merged.len();
            if entries > MAX_REFRESH_ENTRIES {
                return Err(LiveUsageError::ParseFailure);
            }
            let models: Vec<LiveModelUsage> = merged.into_values().collect();
            let within_card_bounds = models.iter().all(|model| {
                [
                    model.input_tokens,
                    model.cached_input_tokens,
                    model.output_tokens,
                    model.requests,
                    model.batch_tokens,
                    model.batch_requests,
                ]
                .into_iter()
                .all(|value| value <= MAX_CARD_COUNT)
            });
            if !within_card_bounds {
                return Err(LiveUsageError::ParseFailure);
            }
            Ok(LiveMinute {
                start: bucket.start_time,
                models,
            })
        })
        .collect()
}

impl OpenAIApiProvider {
    async fn get_live_page(
        &self,
        query: &[(&'static str, String)],
        api_key: &str,
    ) -> Result<(reqwest::StatusCode, String), LiveUsageError> {
        let response = self
            .client
            .get(&self.endpoints.completions)
            .query(query)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| LiveUsageError::Network)?;
        let status = response.status();
        let body = response.text().await.map_err(|_| LiveUsageError::Network)?;
        Ok((status, body))
    }

    /// The per-refresh last-hour Tokens/min grid, or `None` when it is unavailable. One
    /// request without retries, as upstream; the daily data never depends on it.
    pub(super) async fn fetch_refresh_throughput(
        &self,
        api_key: &str,
        project_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<ProviderThroughput> {
        match self.fetch_refresh_live(api_key, project_id, now).await {
            Ok(live) => Some(live.throughput()),
            Err(error) => {
                tracing::debug!(%error, "OpenAI live usage unavailable; daily usage is unaffected");
                None
            }
        }
    }

    pub(super) async fn fetch_refresh_live(
        &self,
        api_key: &str,
        project_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<OpenAiLiveUsage, LiveUsageError> {
        let fetched_at = now.timestamp();
        let start = fetched_at / 60 * 60 - (i64::from(REFRESH_WINDOW_MINUTES) - 1) * 60;
        let query = live_query(start, fetched_at, REFRESH_WINDOW_MINUTES, project_id, None);
        let (status, body) = self.get_live_page(&query, api_key).await?;
        if status != reqwest::StatusCode::OK {
            return Err(status_error(status));
        }
        let page: LivePage =
            serde_json::from_str(&body).map_err(|_| LiveUsageError::ParseFailure)?;
        let buckets = refresh_buckets(page, start, fetched_at)?;
        build(
            buckets,
            REFRESH_WINDOW_MINUTES,
            fetched_at,
            clean_project_id(project_id),
        )
    }

    /// The on-demand Live Usage window: every page of a 15-, 60- or 360-minute window.
    /// The key resolves like refreshes do (Preferences, credential store, environment);
    /// `project_id` uses the provider's configured project scope.
    pub async fn fetch_live_usage(
        &self,
        api_key: Option<&str>,
        project_id: Option<&str>,
        window_minutes: u32,
    ) -> Result<OpenAiLiveUsage, LiveUsageError> {
        if !LIVE_WINDOW_MINUTES.contains(&window_minutes) {
            return Err(LiveUsageError::InvalidWindow);
        }
        let credential =
            Self::credential(api_key).map_err(|_| LiveUsageError::MissingCredential)?;
        self.fetch_live_window(&credential.key, project_id, window_minutes, Utc::now())
            .await
    }

    pub(super) async fn fetch_live_window(
        &self,
        api_key: &str,
        project_id: Option<&str>,
        window_minutes: u32,
        now: DateTime<Utc>,
    ) -> Result<OpenAiLiveUsage, LiveUsageError> {
        if !LIVE_WINDOW_MINUTES.contains(&window_minutes) {
            return Err(LiveUsageError::InvalidWindow);
        }
        let fetched_at = now.timestamp();
        let first = fetched_at / 60 * 60 - (i64::from(window_minutes) - 1) * 60;
        let mut buckets = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..MAX_WINDOW_PAGES {
            let query = live_query(
                first,
                fetched_at,
                window_minutes,
                project_id,
                cursor.as_deref(),
            );
            let (status, body) = self.get_live_page(&query, api_key).await?;
            if !status.is_success() {
                return Err(status_error(status));
            }
            let page: LivePage =
                serde_json::from_str(&body).map_err(|_| LiveUsageError::ParseFailure)?;
            for bucket in page.data {
                if bucket.start_time < 0 {
                    return Err(LiveUsageError::ParseFailure);
                }
                buckets.push(LiveMinute {
                    start: bucket.start_time,
                    models: bucket
                        .results
                        .iter()
                        .map(LiveResult::usage)
                        .collect::<Result<_, _>>()?,
                });
            }
            if !page.has_more.unwrap_or(false) {
                return build(
                    buckets,
                    window_minutes,
                    fetched_at,
                    clean_project_id(project_id),
                );
            }
            let next = page
                .next_page
                .filter(|next| !next.is_empty())
                .ok_or(LiveUsageError::ParseFailure)?;
            if !seen.insert(next.clone()) {
                return Err(LiveUsageError::ParseFailure);
            }
            cursor = Some(next);
        }
        Err(LiveUsageError::ParseFailure)
    }
}
