//! Versioned SSH wire contract for the Codex cost comparison.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cost_scanner::CostSummary;
use crate::spend_contract::{CostCoverageCounts, CostProvenance};

pub(super) use super::coverage_from_summary;

pub(crate) const CODEX_COST_SUMMARY_SCHEMA_VERSION: u32 = 1;
pub(crate) const MAX_REMOTE_CODEX_COST_BYTES: usize = 16 * 1024;
pub(crate) const REMOTE_CODEX_COST_UNAVAILABLE: &str = "Could not read remote Codex costs. Check SSH and that the remote CodexBar CLI supports --summary-only.";
pub(crate) const REMOTE_CODEX_COST_INVALID: &str = "The remote CLI returned an unsupported or invalid cost summary. Update CodexBar on the remote host.";

/// A path-free, host-local cost window used by the SSH comparison transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexHostCostWindow {
    pub total_tokens: Option<u64>,
    #[serde(rename = "costUSD")]
    pub cost_usd: Option<f64>,
    pub coverage: CostCoverageCounts,
    pub provenance: CostProvenance,
}

impl CodexHostCostWindow {
    fn from_summary(summary: &CostSummary) -> Self {
        let coverage = coverage_from_summary(summary);
        let provenance = if coverage.estimated > 0 {
            CostProvenance::ListPriceEstimate
        } else {
            CostProvenance::Unknown
        };
        let complete = summary.history_coverage_established;
        let total_tokens =
            complete.then_some(summary.input_tokens.saturating_add(summary.output_tokens));
        let cost_usd = complete
            .then_some(summary.total_cost_usd)
            .filter(|cost| cost.is_finite() && *cost >= 0.0);

        Self {
            total_tokens,
            cost_usd,
            coverage,
            provenance,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self
            .cost_usd
            .is_some_and(|cost| !cost.is_finite() || cost < 0.0)
        {
            return Err(REMOTE_CODEX_COST_INVALID.to_string());
        }
        Ok(())
    }
}

/// Versioned, path-free Codex cost summary exchanged over SSH.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexCostSummary {
    pub schema_version: u32,
    pub provider: String,
    pub updated_at: DateTime<Utc>,
    pub bucket_time_zone: String,
    pub currency_code: String,
    pub history_days: u32,
    pub history_coverage_is_established: bool,
    pub today: CodexHostCostWindow,
    pub history: CodexHostCostWindow,
}

impl CodexCostSummary {
    pub(crate) fn from_summaries_at(
        history: &CostSummary,
        today: &CostSummary,
        history_days: u32,
        updated_at: DateTime<Utc>,
        bucket_time_zone: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: CODEX_COST_SUMMARY_SCHEMA_VERSION,
            provider: "codex".to_string(),
            updated_at,
            bucket_time_zone: bucket_time_zone.into(),
            currency_code: "USD".to_string(),
            history_days,
            history_coverage_is_established: history.history_coverage_established,
            today: CodexHostCostWindow::from_summary(today),
            history: CodexHostCostWindow::from_summary(history),
        }
    }

    pub(crate) fn validate(&self, expected_history_days: u32) -> Result<(), String> {
        if self.schema_version != CODEX_COST_SUMMARY_SCHEMA_VERSION
            || self.provider != "codex"
            || !(1..=365).contains(&expected_history_days)
            || self.history_days != expected_history_days
            || self.currency_code != "USD"
            || self.bucket_time_zone.parse::<chrono_tz::Tz>().is_err()
            || !(0..=253_402_300_799).contains(&self.updated_at.timestamp())
        {
            return Err(REMOTE_CODEX_COST_INVALID.to_string());
        }
        if !self.history_coverage_is_established
            && ([self.today.total_tokens, self.history.total_tokens]
                .into_iter()
                .any(|value| value.is_some())
                || [self.today.cost_usd, self.history.cost_usd]
                    .into_iter()
                    .any(|value| value.is_some()))
        {
            return Err(REMOTE_CODEX_COST_INVALID.to_string());
        }
        self.today.validate()?;
        self.history.validate()?;
        Ok(())
    }
}

/// One host's comparison outcome. A failed row preserves the other host's
/// successful report: overlapping histories are never combined.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodexHostOutcome {
    Success(CodexCostSummary),
    Failed(String),
}

/// One host's report in the comparison output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexHostCostReport {
    pub host: String,
    pub source: String,
    pub outcome: CodexHostOutcome,
}

impl CodexHostCostReport {
    pub(crate) fn success(
        host: impl Into<String>,
        source: impl Into<String>,
        summary: CodexCostSummary,
    ) -> Self {
        Self {
            host: host.into(),
            source: source.into(),
            outcome: CodexHostOutcome::Success(summary),
        }
    }

    pub(crate) fn failure(
        host: impl Into<String>,
        source: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            source: source.into(),
            outcome: CodexHostOutcome::Failed(error.into()),
        }
    }
}
impl CodexHostCostReport {
    /// The retained summary, when this host produced one.
    pub(crate) fn summary(&self) -> Option<&CodexCostSummary> {
        match &self.outcome {
            CodexHostOutcome::Success(summary) => Some(summary),
            CodexHostOutcome::Failed(_) => None,
        }
    }
}

pub(crate) fn decode_remote_codex_summary(
    output: &str,
    history_days: u32,
) -> Result<CodexCostSummary, String> {
    if output.len() > MAX_REMOTE_CODEX_COST_BYTES {
        return Err(REMOTE_CODEX_COST_INVALID.to_string());
    }

    let reports = serde_json::from_str::<Vec<CodexCostSummary>>(output)
        .map_err(|_| REMOTE_CODEX_COST_INVALID.to_string())?;
    if reports.len() != 1 {
        return Err(REMOTE_CODEX_COST_INVALID.to_string());
    }

    let report = reports
        .into_iter()
        .next()
        .ok_or_else(|| REMOTE_CODEX_COST_INVALID.to_string())?;
    report.validate(history_days)?;
    Ok(report)
}
