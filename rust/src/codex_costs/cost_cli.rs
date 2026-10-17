//! Codex's `codexbar cost` extension: project and conversation reports,
//! the partial-history notice, identified-request subtotals and host
//! summaries.

use std::path::PathBuf;

use futures::future::LocalBoxFuture;

use crate::codex_costs::{
    CodexHostCostsArgs, HostOutputFormat, IdentifiedUsageArgs, run_codex_host_costs,
    run_codex_identified_usage,
};
use crate::codex_workspaces::{
    CodexLocalProjectUsageSnapshot, CodexWorkspacesIndex, short_session_id,
};
use crate::core::{
    CostCliExtension, CostCliInvocation, CostCliMode, CostGroupedReportContext, CostGrouping,
    ProviderId, format_count,
};
use crate::cost_scanner::CostSummary;

/// Shown by the default and project reports when the local cost scan has not
/// fully covered the window.
pub(crate) const PARTIAL_HISTORY_NOTICE: &str =
    "  Coverage: partial (history catch-up in progress)";

pub(crate) static CODEX_COST_CLI: CodexCostCli = CodexCostCli {
    codex_home: None,
    sidecar_path: None,
};

/// `None` paths use the user's Codex home and workspace sidecar.
pub(crate) struct CodexCostCli {
    codex_home: Option<PathBuf>,
    sidecar_path: Option<PathBuf>,
}

impl CodexCostCli {
    #[cfg(test)]
    pub(crate) fn with_paths(codex_home: PathBuf, sidecar_path: PathBuf) -> Self {
        Self {
            codex_home: Some(codex_home),
            sidecar_path: Some(sidecar_path),
        }
    }

    fn workspaces_index(&self, history_days: u32) -> CodexWorkspacesIndex {
        let mut index = CodexWorkspacesIndex::new(history_days);
        if let Some(home) = &self.codex_home {
            index = index.with_codex_home(home);
        }
        if let Some(path) = &self.sidecar_path {
            index = index.with_sidecar_path(path);
        }
        index
    }
}

impl CostCliExtension for CodexCostCli {
    fn exclusive_mode(&self, invocation: &CostCliInvocation<'_>) -> Option<CostCliMode> {
        // The identified window is explicit UTC milliseconds; saved periods
        // and time zones never apply.
        if invocation.identified_only || invocation.identified_end.is_some() {
            Some(CostCliMode::BeforePeriod)
        } else if invocation.remote.is_some() || invocation.summary_only {
            Some(CostCliMode::BoundedWindow)
        } else {
            None
        }
    }

    fn run_exclusive_mode<'a>(
        &'a self,
        mode: CostCliMode,
        invocation: &'a CostCliInvocation<'a>,
        window_days: Option<u32>,
    ) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            match (mode, window_days) {
                (CostCliMode::BeforePeriod, _) => {
                    run_codex_identified_usage(&identified_usage_args(invocation)).await
                }
                (CostCliMode::BoundedWindow, Some(days)) => {
                    run_codex_host_costs(&host_costs_args(invocation, days)).await
                }
                (CostCliMode::BoundedWindow, None) => {
                    anyhow::bail!("host cost reports need a resolved day window")
                }
            }
        })
    }

    fn grouped_report(
        &self,
        grouping: CostGrouping,
        context: &CostGroupedReportContext<'_>,
    ) -> Option<Vec<String>> {
        let index = self.workspaces_index(context.history_days);
        let lines = match grouping {
            CostGrouping::Project => {
                let snapshot = index.load_snapshot(false, |_| {}).map(|mut snapshot| {
                    if context.hide_personal_info {
                        snapshot.redact_for_privacy();
                    }
                    snapshot
                });
                render_project_lines(context.summary.history_coverage_established, &snapshot)
            }
            CostGrouping::Session => render_session_lines(context.summary, &index),
        };
        Some(lines)
    }

    fn coverage_notice(&self, summary: &CostSummary) -> Option<&'static str> {
        // A16 (upstream 0.48.0): coverage status for Codex.
        (!summary.history_coverage_established).then_some(PARTIAL_HISTORY_NOTICE)
    }
}

fn identified_usage_args(invocation: &CostCliInvocation<'_>) -> IdentifiedUsageArgs {
    IdentifiedUsageArgs {
        identified_only: invocation.identified_only,
        // The literal name, not an alias such as `openai`.
        provider_is_codex_only: invocation
            .provider_arg
            .is_some_and(|provider| provider.trim().eq_ignore_ascii_case("codex")),
        json: invocation.json,
        days: invocation.days,
        end_unix_ms: invocation.identified_end,
        remote: invocation.remote.map(str::to_string),
        period_set: invocation.period_set,
        group_by_set: invocation.group_by_set,
        summary_only: invocation.summary_only,
        pretty: invocation.pretty,
    }
}

fn host_costs_args(invocation: &CostCliInvocation<'_>, days: u32) -> CodexHostCostsArgs {
    CodexHostCostsArgs {
        days,
        remote: invocation.remote.map(str::to_string),
        summary_only: invocation.summary_only,
        pretty: invocation.pretty,
        format: if invocation.json {
            HostOutputFormat::Json
        } else {
            HostOutputFormat::Text
        },
        provider_is_codex_only: invocation.providers == [ProviderId::Codex],
        group_by_rejected: invocation.group_by_set,
    }
}

fn render_session_lines(summary: &CostSummary, index: &CodexWorkspacesIndex) -> Vec<String> {
    let mut lines = Vec::new();
    let snapshot = match index.load_snapshot(false, |_| {}) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            lines.push(format!("  Conversation history unavailable: {err}"));
            return lines;
        }
    };

    // The conversation index keeps rolling 1..=365 day windows, so All lists
    // the most recent year; the heading reports the window it used.
    lines.push(format!(
        "  Conversations (last {} days):",
        snapshot.history_days
    ));
    if snapshot.source_status.is_partial() {
        lines.push(
            "  Conversation history is incomplete while local indexing catches up.".to_string(),
        );
    }

    if snapshot.sessions.is_empty() {
        lines.push("  —".to_string());
    } else {
        for session in &snapshot.sessions {
            let id = short_session_id(&session.id);
            let cost = if session.cost_estimate.unknown_tokens > 0 {
                format!("~${:.2} partial", session.cost_estimate.known_usd)
            } else {
                format!("${:.2}", session.cost_estimate.known_usd)
            };
            let model = session.top_model.as_deref().unwrap_or("unknown model");
            lines.push(format!(
                "  Session {id}: {cost} · {} tokens · {model}",
                format_count(session.totals.total_tokens)
            ));
            if let Some(activity) = session.latest_activity {
                lines.push(format!(
                    "    {}",
                    activity
                        .with_timezone(&chrono::Local)
                        .format("%b %d, %H:%M")
                ));
            }
        }
    }

    if !summary.history_coverage_established {
        lines.push("  Coverage: partial (cost history catch-up in progress)".to_string());
    }
    lines.push(
        "  Not a subscription bill or plan value · local usage × public API prices".to_string(),
    );
    lines
}

/// Project report lines (upstream #4472): incomplete history carries the same
/// partial-history notice as the default report, even with no project rows.
fn render_project_lines<E: std::fmt::Display>(
    cost_history_established: bool,
    snapshot: &Result<CodexLocalProjectUsageSnapshot, E>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let catalog_partial = snapshot
        .as_ref()
        .is_ok_and(|snapshot| snapshot.source_status.is_partial());
    if !cost_history_established || catalog_partial {
        lines.push(PARTIAL_HISTORY_NOTICE.to_string());
    }
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(err) => {
            lines.push(format!("  Project history unavailable: {err}"));
            return lines;
        }
    };
    lines.push(format!("  Projects (last {} days):", snapshot.history_days));
    if snapshot.projects.is_empty() {
        lines.push("  —".to_string());
    }
    for project in &snapshot.projects {
        let cost = if project.cost_estimate.unknown_tokens > 0 {
            format!("~${:.2} partial", project.cost_estimate.known_usd)
        } else {
            format!("${:.2}", project.cost_estimate.known_usd)
        };
        lines.push(format!(
            "  {}: {cost} · {} tokens",
            project.display_name,
            format_count(project.totals.total_tokens)
        ));
        if let Some(path) = project.path.as_deref() {
            lines.push(format!("    {path}"));
        }
    }
    lines.push(
        "  Not a subscription bill or plan value · local usage × public API prices".to_string(),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_snapshot(
        projects: Vec<crate::codex_workspaces::ProjectUsage>,
    ) -> CodexLocalProjectUsageSnapshot {
        CodexLocalProjectUsageSnapshot {
            updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            history_days: 30,
            scope_signature: String::new(),
            indexed_file_count: 0,
            skipped_file_count: 0,
            total: Default::default(),
            sessions: Vec::new(),
            projects,
            daily: Vec::new(),
            source_status: crate::codex_workspaces::SourceStatus::Complete,
        }
    }

    #[test]
    fn project_report_marks_partial_history_even_when_empty() {
        let empty: Result<_, String> = Ok(project_snapshot(Vec::new()));
        let partial = render_project_lines(false, &empty);
        assert_eq!(partial[0], PARTIAL_HISTORY_NOTICE);
        assert!(partial.contains(&"  Projects (last 30 days):".to_string()));
        assert!(partial.contains(&"  —".to_string()));

        let complete = render_project_lines(true, &empty);
        assert!(!complete.iter().any(|line| line == PARTIAL_HISTORY_NOTICE));
        assert!(complete.contains(&"  —".to_string()));

        let unavailable: Result<CodexLocalProjectUsageSnapshot, _> = Err("locked".to_string());
        let lines = render_project_lines(false, &unavailable);
        assert_eq!(lines[0], PARTIAL_HISTORY_NOTICE);
        assert_eq!(lines[1], "  Project history unavailable: locked");

        let mut locked_catalog = project_snapshot(Vec::new());
        locked_catalog.source_status = crate::codex_workspaces::SourceStatus::CatalogLocked;
        let lines = render_project_lines(true, &Ok::<_, String>(locked_catalog));
        assert_eq!(lines[0], PARTIAL_HISTORY_NOTICE);
        assert_eq!(
            lines
                .iter()
                .filter(|line| *line == PARTIAL_HISTORY_NOTICE)
                .count(),
            1
        );
    }

    #[test]
    fn project_report_lists_projects_with_partial_costs() {
        use crate::codex_workspaces::{CostEstimate, ProjectUsage, UsageTotals};
        let project = |name: &str, path: Option<&str>, unknown_tokens: u64| ProjectUsage {
            id: name.to_string(),
            display_name: name.to_string(),
            path: path.map(str::to_string),
            totals: UsageTotals::from_parts(1_000, 0, 234),
            cost_estimate: CostEstimate {
                known_usd: 1.5,
                unknown_tokens,
            },
            session_count: 1,
            latest_activity: None,
            top_model: None,
            top_sessions: Vec::new(),
            is_projectless: false,
        };
        let snapshot: Result<_, String> = Ok(project_snapshot(vec![
            project("alpha", Some("C:\\src\\alpha"), 0),
            project("beta", None, 10),
        ]));
        let lines = render_project_lines(true, &snapshot);
        assert!(lines.contains(&"  alpha: $1.50 · 1,234 tokens".to_string()));
        assert!(lines.contains(&"    C:\\src\\alpha".to_string()));
        assert!(lines.contains(&"  beta: ~$1.50 partial · 1,234 tokens".to_string()));
        assert!(!lines.iter().any(|line| line == "  —"));
    }

    fn invocation<'a>(providers: &'a [ProviderId]) -> CostCliInvocation<'a> {
        CostCliInvocation {
            provider_arg: Some("codex"),
            providers,
            json: true,
            pretty: false,
            days: None,
            period_set: false,
            group_by_set: false,
            remote: None,
            summary_only: false,
            identified_only: false,
            identified_end: None,
        }
    }

    #[test]
    fn exclusive_modes_follow_the_mode_flags() {
        let codex = [ProviderId::Codex];
        let plain = invocation(&codex);
        assert_eq!(CODEX_COST_CLI.exclusive_mode(&plain), None);
        let identified = CostCliInvocation {
            identified_end: Some(1),
            remote: Some("build-host"),
            ..invocation(&codex)
        };
        assert_eq!(
            CODEX_COST_CLI.exclusive_mode(&identified),
            Some(CostCliMode::BeforePeriod)
        );
        let summary = CostCliInvocation {
            summary_only: true,
            ..invocation(&codex)
        };
        assert_eq!(
            CODEX_COST_CLI.exclusive_mode(&summary),
            Some(CostCliMode::BoundedWindow)
        );
    }

    #[test]
    fn coverage_notice_marks_only_uncovered_history() {
        let covered = CostSummary {
            history_coverage_established: true,
            ..CostSummary::default()
        };
        assert_eq!(CODEX_COST_CLI.coverage_notice(&covered), None);
        assert_eq!(
            CODEX_COST_CLI.coverage_notice(&CostSummary::default()),
            Some(PARTIAL_HISTORY_NOTICE)
        );
    }
}
