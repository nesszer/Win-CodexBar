//! Bridge DTOs for OpenAI Live Usage (upstream 0.75.0 #4495): the manual per-model
//! organization limits the Settings editor and the card read, and the project caps the
//! Live Usage window lists as context.
//!
//! Mirrors `ModelRateLimitSnapshot` and `OrgRateLimitsSnapshot` in `src/types/bridge.ts`.
//! The Tokens/min window itself reuses `ProviderThroughputSnapshot`.

use codexbar::providers::openaiapi::org_limits::OrgRateLimits;
use codexbar::settings::ModelRateLimit;
use serde::{Deserialize, Serialize};

/// One manual organization limit, as stored in settings and sent by the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRateLimitSnapshot {
    pub model: String,
    pub tokens_per_minute: u64,
    #[serde(default)]
    pub requests_per_minute: Option<u64>,
}

impl From<&ModelRateLimit> for ModelRateLimitSnapshot {
    fn from(limit: &ModelRateLimit) -> Self {
        Self {
            model: limit.model.clone(),
            tokens_per_minute: limit.tokens_per_minute,
            requests_per_minute: limit.requests_per_minute,
        }
    }
}

/// Project caps discovered across the organization, sorted by model ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgRateLimitsSnapshot {
    pub by_model: Vec<OrgModelRateLimitSnapshot>,
    pub projects_sampled: usize,
    pub truncated: bool,
    /// Epoch seconds.
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgModelRateLimitSnapshot {
    pub model: String,
    pub tokens_per_minute: u64,
    pub requests_per_minute: Option<u64>,
}

impl From<&OrgRateLimits> for OrgRateLimitsSnapshot {
    fn from(limits: &OrgRateLimits) -> Self {
        Self {
            by_model: limits
                .by_model
                .iter()
                .map(|(model, limit)| OrgModelRateLimitSnapshot {
                    model: model.clone(),
                    tokens_per_minute: limit.tokens_per_minute,
                    requests_per_minute: limit.requests_per_minute,
                })
                .collect(),
            projects_sampled: limits.projects_sampled,
            truncated: limits.truncated,
            fetched_at: limits.fetched_at,
        }
    }
}
