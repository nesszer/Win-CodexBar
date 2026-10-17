//! Provider-owned extensions to the `codexbar cost` command.
//!
//! The shared command renders the common per-provider report. A provider that
//! offers more (grouped reports, a coverage notice, or an exclusive mode that
//! replaces the report) implements [`CostCliExtension`] in its own module and
//! registers it in [`super::cost_cli_extension_for`]. Providers without an
//! extension get the shared "not available" output.

use futures::future::LocalBoxFuture;

use super::ProviderId;
use crate::cost_scanner::CostSummary;

/// A grouped text report requested with `--group-by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostGrouping {
    Project,
    Session,
}

/// When an exclusive mode runs relative to reporting-period resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CostCliMode {
    /// Runs before the reporting period is resolved; saved settings and the
    /// saved cost period never apply.
    BeforePeriod,
    /// Runs on the resolved period's day window. All needs an explicit
    /// `--days`, because the window must be 1...365 days.
    BoundedWindow,
}

/// Provider-neutral view of the `cost` arguments an exclusive mode inspects.
pub(crate) struct CostCliInvocation<'a> {
    /// The raw `--provider` value.
    pub(crate) provider_arg: Option<&'a str>,
    /// The resolved provider selection.
    pub(crate) providers: &'a [ProviderId],
    pub(crate) json: bool,
    pub(crate) pretty: bool,
    pub(crate) days: Option<u32>,
    pub(crate) period_set: bool,
    pub(crate) group_by_set: bool,
    pub(crate) remote: Option<&'a str>,
    pub(crate) summary_only: bool,
    pub(crate) identified_only: bool,
    pub(crate) identified_end: Option<i64>,
}

/// Inputs for one provider's grouped text report.
pub(crate) struct CostGroupedReportContext<'a> {
    pub(crate) summary: &'a CostSummary,
    /// Rolling history window in days (1...365).
    pub(crate) history_days: u32,
    pub(crate) hide_personal_info: bool,
}

pub(crate) trait CostCliExtension: Sync {
    /// The exclusive mode these arguments select, if this extension owns one.
    /// An exclusive mode replaces the whole report and validates the provider
    /// selection itself, so its errors stay owned by the provider.
    fn exclusive_mode(&self, _invocation: &CostCliInvocation<'_>) -> Option<CostCliMode> {
        None
    }

    /// Runs a mode returned by [`Self::exclusive_mode`]. `window_days` is the
    /// resolved day window for [`CostCliMode::BoundedWindow`], `None` otherwise.
    fn run_exclusive_mode<'a>(
        &'a self,
        _mode: CostCliMode,
        _invocation: &'a CostCliInvocation<'a>,
        _window_days: Option<u32>,
    ) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { anyhow::bail!("this cost mode is not supported") })
    }

    /// Text lines for a grouped report, or `None` when this provider does not
    /// support the grouping.
    fn grouped_report(
        &self,
        _grouping: CostGrouping,
        _context: &CostGroupedReportContext<'_>,
    ) -> Option<Vec<String>> {
        None
    }

    /// A line appended to the default text report, such as a notice that the
    /// local history scan has not yet covered the window.
    fn coverage_notice(&self, _summary: &CostSummary) -> Option<&'static str> {
        None
    }
}

/// Formats a count with comma thousands separators.
pub(crate) fn format_count(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }
    result
}
