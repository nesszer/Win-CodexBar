//! OpenAI organization rate-limit headroom for Live Usage (upstream 0.75.0 #4495:
//! `OpenAIOrgRateLimits`, `OpenAIOrgLimitResolver`, `OpenAIOrgRateLimitsFetcher`, and the
//! menu-card `throughputPresentation`).
//!
//! OpenAI has no organization-level limits endpoint. Only the user's manual limits
//! ([`ModelRateLimit`]) drive utilization. Per-project caps are context: they are read from
//! up to 25 active projects, take the maximum TPM and RPM independently per model, and
//! never drive a percentage. Project-scoped usage is only one slice of the organization
//! traffic the limits apply to, so it never reports utilization.
//!
//! Utilization counts all input (cached included) plus output and excludes Batch API
//! traffic. Models that resolve to the same configured limit (an alias and its dated
//! snapshots) share it, and their traffic is summed per minute.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, HashSet};

use super::OpenAIApiProvider;
use super::live::{LiveModelFilter, LiveModelRow, OpenAiLiveUsage};
use crate::core::ProviderThroughput;
use crate::settings::ModelRateLimit;

const MAX_PROJECTS: usize = 25;
const MAX_LIST_PAGES: usize = 20;
const LIST_PAGE_LIMIT: &str = "100";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrgRateLimit {
    pub tokens_per_minute: u64,
    pub requests_per_minute: Option<u64>,
}

/// Project caps discovered across the organization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgRateLimits {
    pub by_model: BTreeMap<String, OrgRateLimit>,
    pub projects_sampled: usize,
    /// More than [`MAX_PROJECTS`] active projects exist; the sample is labeled truncated.
    pub truncated: bool,
    /// Epoch seconds.
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OrgRateLimitsError {
    #[error("Configure an OpenAI organization Admin API key.")]
    MissingCredential,
    #[error("Organization rate limits require an Admin API key with project read permissions.")]
    AdminKeyRequired,
    #[error("OpenAI organization rate limits are temporarily rate limited.")]
    RateLimited,
    #[error("OpenAI organization rate limits API failed (HTTP {0}).")]
    ApiFailure(u16),
    #[error("Could not parse the OpenAI organization rate limits response.")]
    ParseFailure,
    #[error("Could not reach the OpenAI organization rate limits API.")]
    Network,
}

/// Resolves a usage model ID to its manual limit and project cap.
#[derive(Debug, Clone, Default)]
pub struct OrgLimitResolver {
    manual: Vec<ModelRateLimit>,
    project_caps: Option<OrgRateLimits>,
}

impl OrgLimitResolver {
    pub fn new(manual: &[ModelRateLimit], project_caps: Option<OrgRateLimits>) -> Self {
        Self {
            manual: ModelRateLimit::validated(manual.to_vec()),
            project_caps,
        }
    }

    pub fn resolve(&self, model: &str) -> Option<OrgRateLimit> {
        let key = self.manual_limit_key(model)?;
        self.manual
            .iter()
            .find(|row| row.model == key)
            .map(|row| OrgRateLimit {
                tokens_per_minute: row.tokens_per_minute,
                requests_per_minute: row.requests_per_minute,
            })
    }

    /// The configured model ID whose limit applies to `model`.
    pub fn manual_limit_key(&self, model: &str) -> Option<&str> {
        match_key(model, self.manual.iter().map(|row| row.model.as_str()))
    }

    pub fn project_cap(&self, model: &str) -> Option<OrgRateLimit> {
        let caps = &self.project_caps.as_ref()?.by_model;
        match_key(model, caps.keys().map(String::as_str)).and_then(|key| caps.get(key).copied())
    }
}

/// Exact IDs first; otherwise the longest key followed by `-` and a dated snapshot suffix,
/// so `gpt-5` covers `gpt-5-2025-08-07` and `gpt-4` covers `gpt-4-0613`, while distinct
/// models such as `gpt-5-mini` or `gpt-4o` keep their own limits.
fn match_key<'a>(model: &str, keys: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    if let Some(exact) = keys.clone().find(|key| *key == model) {
        return Some(exact);
    }
    let mut best: Option<&'a str> = None;
    for key in keys {
        let matches = model
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(is_snapshot_suffix);
        if matches && best.is_none_or(|best| key.len() > best.len()) {
            best = Some(key);
        }
    }
    best
}

/// `YYYY-MM-DD` or the older `MMDD`.
fn is_snapshot_suffix(suffix: &str) -> bool {
    let bytes = suffix.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    match bytes.len() {
        4 => digits(0..4),
        10 => digits(0..4) && bytes[4] == b'-' && digits(5..7) && bytes[7] == b'-' && digits(8..10),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrgLimitUtilization {
    pub limit: OrgRateLimit,
    /// Every model on the snapshot that shares the configured limit.
    pub pooled_models: Vec<String>,
    pub latest_tpm_percent: f64,
    pub peak_tpm_percent: f64,
    pub peak_rpm_percent: Option<f64>,
}

impl OpenAiLiveUsage {
    /// Manual-limit utilization for one model's pool; `None` for all models, project-scoped
    /// usage, or a model without a manual limit.
    pub fn utilization(
        &self,
        filter: LiveModelFilter<'_>,
        limits: &OrgLimitResolver,
    ) -> Option<OrgLimitUtilization> {
        if self.project_id.is_some() {
            return None;
        }
        let LiveModelFilter::Model(name) = filter else {
            return None;
        };
        let key = limits.manual_limit_key(name)?;
        let limit = limits.resolve(name)?;
        let pooled_models: Vec<String> = self
            .model_names()
            .into_iter()
            .filter(|model| limits.manual_limit_key(model) == Some(key))
            .collect();
        let members: HashSet<&str> = pooled_models.iter().map(String::as_str).collect();
        let frontier = self.reporting_frontier();
        let points: Vec<(i64, u64, u64, bool)> = self
            .minutes
            .iter()
            .map(|minute| {
                let pooled = minute
                    .models
                    .iter()
                    .filter(|model| members.contains(model.name.as_str()));
                let (tokens, requests) = pooled.fold((0u64, 0u64), |(tokens, requests), model| {
                    (
                        tokens.saturating_add(
                            model.total_tokens().saturating_sub(model.batch_tokens),
                        ),
                        requests
                            .saturating_add(model.requests.saturating_sub(model.batch_requests)),
                    )
                });
                (
                    minute.start,
                    tokens,
                    requests,
                    Self::is_pending(frontier, minute.start),
                )
            })
            .collect();
        let latest_tokens = points
            .iter()
            .find(|point| Some(point.0) == frontier)
            .map_or(0, |point| point.1);
        let reported = points.iter().filter(|point| !point.3);
        let peak_tokens = reported.clone().map(|point| point.1).max().unwrap_or(0);
        let peak_requests = reported.map(|point| point.2).max().unwrap_or(0);
        let tpm = limit.tokens_per_minute as f64;
        Some(OrgLimitUtilization {
            limit,
            pooled_models,
            latest_tpm_percent: latest_tokens as f64 / tpm * 100.0,
            peak_tpm_percent: peak_tokens as f64 / tpm * 100.0,
            peak_rpm_percent: limit
                .requests_per_minute
                .filter(|rpm| *rpm > 0)
                .map(|rpm| peak_requests as f64 / rpm as f64 * 100.0),
        })
    }

    /// [`Self::model_rows`] with manual limits, utilization and project caps.
    pub fn model_rows_with_limits(&self, limits: &OrgLimitResolver) -> Vec<LiveModelRow> {
        self.model_rows()
            .into_iter()
            .map(|mut row| {
                let utilization = self.utilization(LiveModelFilter::Model(&row.name), limits);
                row.tpm_limit = utilization.as_ref().map(|u| u.limit.tokens_per_minute);
                row.rpm_limit = utilization
                    .as_ref()
                    .and_then(|u| u.limit.requests_per_minute);
                row.project_cap = limits
                    .project_cap(&row.name)
                    .map(|cap| cap.tokens_per_minute);
                row.peak_tpm_percent = utilization.as_ref().map(|u| u.peak_tpm_percent);
                row.latest_tpm_percent = utilization.as_ref().map(|u| u.latest_tpm_percent);
                row
            })
            .collect()
    }

    /// The row with the highest peak utilization; ties pick the alphabetically first name.
    pub fn highest_utilization(&self, limits: &OrgLimitResolver) -> Option<LiveModelRow> {
        let mut best: Option<(f64, LiveModelRow)> = None;
        for row in self.model_rows_with_limits(limits) {
            let Some(percent) = row.peak_tpm_percent else {
                continue;
            };
            let better = best.as_ref().is_none_or(|(best_percent, best_row)| {
                percent > *best_percent || (percent == *best_percent && row.name < best_row.name)
            });
            if better {
                best = Some((percent, row));
            }
        }
        best.map(|(_, row)| row)
    }
}

/// The menu card's limit tile: the highest manual-limit utilization across every model.
#[derive(Debug, Clone, PartialEq)]
pub struct ThroughputLimitUtilization {
    /// The model, or the configured limit ID when several models share it.
    pub label: String,
    /// Peak non-pending limit-counted tokens of the pool in one minute.
    pub peak_tokens: u64,
    pub tokens_per_minute_limit: u64,
    pub percent: f64,
}

/// Upstream `OpenAIAPIProviderImplementation.throughputPresentation` limit line: `None` for
/// project-scoped snapshots (the snapshot's own scope, which can outlive a settings change)
/// or when no model has a manual limit. A tie keeps the earlier pool in snapshot order.
pub fn throughput_limit_utilization(
    snapshot: &ProviderThroughput,
    manual: &[ModelRateLimit],
) -> Option<ThroughputLimitUtilization> {
    if snapshot.project_id.is_some() {
        return None;
    }
    let resolver = OrgLimitResolver::new(manual, None);
    let mut groups: Vec<(&str, Vec<&crate::core::ThroughputModel>)> = Vec::new();
    for model in &snapshot.models {
        let Some(key) = resolver.manual_limit_key(&model.name) else {
            continue;
        };
        match groups.iter_mut().find(|(group, _)| *group == key) {
            Some((_, models)) => models.push(model),
            None => groups.push((key, vec![model])),
        }
    }
    let mut best: Option<ThroughputLimitUtilization> = None;
    for (key, models) in groups {
        let Some(limit) = resolver.resolve(&models[0].name) else {
            continue;
        };
        let aligned = models
            .iter()
            .all(|model| model.limit_counted_tokens_by_minute.len() == snapshot.minutes.len());
        let peak = if aligned {
            snapshot
                .minutes
                .iter()
                .enumerate()
                .filter(|(_, minute)| !minute.is_pending)
                .map(|(index, _)| {
                    models
                        .iter()
                        .map(|model| model.limit_counted_tokens_by_minute[index])
                        .sum::<u64>()
                })
                .max()
                .unwrap_or(0)
        } else {
            models
                .iter()
                .map(|model| model.peak_limit_counted_tokens)
                .max()
                .unwrap_or(0)
        };
        let percent = peak as f64 / limit.tokens_per_minute as f64 * 100.0;
        if best.as_ref().is_none_or(|best| percent > best.percent) {
            best = Some(ThroughputLimitUtilization {
                label: if models.len() > 1 {
                    key.to_string()
                } else {
                    models[0].name.clone()
                },
                peak_tokens: peak,
                tokens_per_minute_limit: limit.tokens_per_minute,
                percent,
            });
        }
    }
    best
}

#[derive(Debug, Deserialize)]
struct ListPage<T> {
    data: Vec<T>,
    has_more: bool,
    last_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Project {
    id: String,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectRateLimit {
    model: String,
    max_tokens_per_1_minute: Option<i64>,
    max_requests_per_1_minute: Option<i64>,
}

impl OpenAIApiProvider {
    /// Project caps across the organization for the Live Usage window. Discovery is always
    /// organization-wide, even when usage is project-scoped.
    pub async fn fetch_org_rate_limits(
        &self,
        api_key: Option<&str>,
    ) -> Result<OrgRateLimits, OrgRateLimitsError> {
        let credential =
            Self::credential(api_key).map_err(|_| OrgRateLimitsError::MissingCredential)?;
        self.fetch_org_rate_limits_with(&credential.key, Utc::now())
            .await
    }

    pub(super) async fn fetch_org_rate_limits_with(
        &self,
        api_key: &str,
        now: DateTime<Utc>,
    ) -> Result<OrgRateLimits, OrgRateLimitsError> {
        let projects_url = reqwest::Url::parse(&self.endpoints.projects)
            .map_err(|_| OrgRateLimitsError::ParseFailure)?;
        // A missing or null status counts as active; archived projects are excluded by default.
        let (projects, truncated) = self
            .list_pages::<Project>(
                projects_url.clone(),
                api_key,
                Some(MAX_PROJECTS),
                |project| project.status.as_deref().unwrap_or("active") == "active",
            )
            .await?;
        let mut by_model: BTreeMap<String, OrgRateLimit> = BTreeMap::new();
        for project in &projects {
            let mut url = projects_url.clone();
            url.path_segments_mut()
                .map_err(|_| OrgRateLimitsError::ParseFailure)?
                .push(&project.id)
                .push("rate_limits");
            let (rows, _) = self
                .list_pages::<ProjectRateLimit>(url, api_key, None, |_| true)
                .await?;
            for row in rows {
                let model = row.model.trim();
                let tpm = row.max_tokens_per_1_minute;
                let rpm = row.max_requests_per_1_minute;
                if model.is_empty() || tpm.is_some_and(|v| v < 0) || rpm.is_some_and(|v| v < 0) {
                    return Err(OrgRateLimitsError::ParseFailure);
                }
                // Some models omit a token limit (video models, for example).
                let Some(tpm) = tpm.and_then(|v| u64::try_from(v).ok()).filter(|v| *v > 0) else {
                    continue;
                };
                let rpm = rpm.and_then(|v| u64::try_from(v).ok()).filter(|v| *v > 0);
                let entry = by_model.entry(model.to_string()).or_insert(OrgRateLimit {
                    tokens_per_minute: 0,
                    requests_per_minute: None,
                });
                entry.tokens_per_minute = entry.tokens_per_minute.max(tpm);
                entry.requests_per_minute = entry.requests_per_minute.max(rpm);
            }
        }
        Ok(OrgRateLimits {
            by_model,
            projects_sampled: projects.len(),
            truncated,
            fetched_at: now.timestamp(),
        })
    }

    /// Follows `has_more` / `last_id` (`after=`) for at most [`MAX_LIST_PAGES`] pages.
    /// With `max_items`, stops once that many included rows exist and reports truncation.
    async fn list_pages<T: DeserializeOwned>(
        &self,
        url: reqwest::Url,
        api_key: &str,
        max_items: Option<usize>,
        include: impl Fn(&T) -> bool,
    ) -> Result<(Vec<T>, bool), OrgRateLimitsError> {
        let mut rows = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..MAX_LIST_PAGES {
            let mut query = vec![("limit", LIST_PAGE_LIMIT.to_string())];
            if let Some(cursor) = &cursor {
                query.push(("after", cursor.clone()));
            }
            let response = self
                .client
                .get(url.clone())
                .query(&query)
                .bearer_auth(api_key)
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|_| OrgRateLimitsError::Network)?;
            let status = response.status();
            if !status.is_success() {
                return Err(match status.as_u16() {
                    401 | 403 => OrgRateLimitsError::AdminKeyRequired,
                    429 => OrgRateLimitsError::RateLimited,
                    code => OrgRateLimitsError::ApiFailure(code),
                });
            }
            let body = response
                .text()
                .await
                .map_err(|_| OrgRateLimitsError::Network)?;
            let page: ListPage<T> =
                serde_json::from_str(&body).map_err(|_| OrgRateLimitsError::ParseFailure)?;
            for row in page.data {
                if !include(&row) {
                    continue;
                }
                if max_items.is_some_and(|max| rows.len() == max) {
                    return Ok((rows, true));
                }
                rows.push(row);
            }
            if !page.has_more {
                return Ok((rows, false));
            }
            if max_items.is_some_and(|max| rows.len() == max) {
                return Ok((rows, true));
            }
            let next = page
                .last_id
                .filter(|next| !next.is_empty())
                .ok_or(OrgRateLimitsError::ParseFailure)?;
            if !seen.insert(next.clone()) {
                return Err(OrgRateLimitsError::ParseFailure);
            }
            cursor = Some(next);
        }
        Err(OrgRateLimitsError::ParseFailure)
    }
}
