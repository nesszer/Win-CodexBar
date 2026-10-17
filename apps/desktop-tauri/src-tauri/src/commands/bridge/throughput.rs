//! Bridge DTOs for the last-hour Tokens/min grid (`throughput`, upstream 0.75.0
//! `ProviderThroughputSnapshot`, #4495).
//!
//! Mirrors `ProviderThroughputSnapshot` in `src/types/bridge.ts`. Times are epoch seconds.
//! Minutes after `frontier` are `isPending` and never count toward peaks or utilization.

use codexbar::core::{ProviderThroughput, ThroughputMinute, ThroughputModel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderThroughputSnapshot {
    #[serde(default)]
    pub minutes: Vec<ThroughputMinuteSnapshot>,
    #[serde(default)]
    pub frontier: Option<i64>,
    #[serde(default)]
    pub latest_tokens_per_minute: Option<u64>,
    pub peak_tokens_per_minute: u64,
    #[serde(default)]
    pub peak_at: Option<i64>,
    pub five_minute_average_tokens_per_minute: f64,
    #[serde(default)]
    pub models: Vec<ThroughputModelSnapshot>,
    pub window_minutes: u32,
    pub fetched_at: i64,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThroughputMinuteSnapshot {
    pub start: i64,
    pub total_tokens: u64,
    pub requests: u64,
    pub is_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThroughputModelSnapshot {
    pub name: String,
    pub latest_tpm: u64,
    pub peak_tpm: u64,
    pub total_tokens: u64,
    pub peak_limit_counted_tokens: u64,
    #[serde(default)]
    pub limit_counted_tokens_by_minute: Vec<u64>,
}

impl From<&ProviderThroughput> for ProviderThroughputSnapshot {
    fn from(throughput: &ProviderThroughput) -> Self {
        Self {
            minutes: throughput.minutes.iter().map(Into::into).collect(),
            frontier: throughput.frontier,
            latest_tokens_per_minute: throughput.latest_tokens_per_minute,
            peak_tokens_per_minute: throughput.peak_tokens_per_minute,
            peak_at: throughput.peak_at,
            five_minute_average_tokens_per_minute: throughput.five_minute_average_tokens_per_minute,
            models: throughput.models.iter().map(Into::into).collect(),
            window_minutes: throughput.window_minutes,
            fetched_at: throughput.fetched_at,
            project_id: throughput.project_id.clone(),
        }
    }
}

impl From<&ThroughputMinute> for ThroughputMinuteSnapshot {
    fn from(minute: &ThroughputMinute) -> Self {
        Self {
            start: minute.start,
            total_tokens: minute.total_tokens,
            requests: minute.requests,
            is_pending: minute.is_pending,
        }
    }
}

impl From<&ThroughputModel> for ThroughputModelSnapshot {
    fn from(model: &ThroughputModel) -> Self {
        Self {
            name: model.name.clone(),
            latest_tpm: model.latest_tpm,
            peak_tpm: model.peak_tpm,
            total_tokens: model.total_tokens,
            peak_limit_counted_tokens: model.peak_limit_counted_tokens,
            limit_counted_tokens_by_minute: model.limit_counted_tokens_by_minute.clone(),
        }
    }
}
