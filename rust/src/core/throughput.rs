//! Compact per-minute token throughput behind a provider's Tokens/min chart and
//! menu-bar token (upstream 0.75.0 `ProviderThroughputSnapshot`, #4495).
//!
//! Times are epoch seconds. Minutes form a regular ascending grid; minutes after the
//! reporting `frontier` are `is_pending` (the provider has not reported them yet) and
//! never count toward peaks or limit utilization.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderThroughput {
    pub minutes: Vec<ThroughputMinute>,
    /// Newest complete minute with reported traffic; `None` when no minute is complete yet.
    pub frontier: Option<i64>,
    /// Tokens in the frontier minute; `None` without a frontier.
    pub latest_tokens_per_minute: Option<u64>,
    pub peak_tokens_per_minute: u64,
    pub peak_at: Option<i64>,
    pub five_minute_average_tokens_per_minute: f64,
    /// Ranked by reported volume, then pending volume, then name.
    pub models: Vec<ThroughputModel>,
    pub window_minutes: u32,
    pub fetched_at: i64,
    /// Project the traffic is filtered to; `None` covers the whole organization.
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThroughputMinute {
    pub start: i64,
    pub total_tokens: u64,
    pub requests: u64,
    pub is_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThroughputModel {
    pub name: String,
    pub latest_tpm: u64,
    pub peak_tpm: u64,
    pub total_tokens: u64,
    /// Peak non-pending tokens that count toward a rate limit (Batch API excluded).
    pub peak_limit_counted_tokens: u64,
    /// Limit-counted tokens aligned with [`ProviderThroughput::minutes`]; pending
    /// values are retained but excluded from utilization.
    pub limit_counted_tokens_by_minute: Vec<u64>,
}
