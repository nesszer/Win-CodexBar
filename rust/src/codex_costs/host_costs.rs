//! Host-level Codex cost comparison orchestration.
//!
//! CLI-facing policy (argument validation, exit codes) lives here with the
//! report assembly so `cli/cost.rs` stays arg parsing + rendering.

use crate::agent_sessions::RemoteSessionFetcher;
use crate::codex_costs::{
    CodexCostSummary, CodexHostCostReport, CodexHostOutcome, REMOTE_CODEX_COST_INVALID,
    REMOTE_CODEX_COST_UNAVAILABLE, build_codex_cost_summary, decode_remote_codex_summary,
};
use crate::core::CostScanOptions;
use crate::cost_scanner::CostScanner;

/// Validated inputs extracted from the CLI arguments.
pub(crate) struct CodexHostCostsArgs {
    pub(crate) days: u32,
    pub(crate) remote: Option<String>,
    pub(crate) summary_only: bool,
    pub(crate) pretty: bool,
    pub(crate) format: HostOutputFormat,
    /// The cost command selected exactly `--provider codex`.
    pub(crate) provider_is_codex_only: bool,
    /// `--group-by` was supplied (incompatible with host cost modes).
    pub(crate) group_by_rejected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostOutputFormat {
    Text,
    Json,
}

pub(crate) async fn run_codex_host_costs(args: &CodexHostCostsArgs) -> anyhow::Result<()> {
    if !args.provider_is_codex_only {
        anyhow::bail!(
            "--remote and --summary-only require exactly --provider codex; they do not support all or both"
        );
    }
    if args.group_by_rejected {
        anyhow::bail!("--remote and --summary-only cannot be combined with --group-by");
    }
    if args.remote.is_some() && args.summary_only {
        anyhow::bail!("--remote and --summary-only cannot be combined");
    }
    if args.summary_only && args.format != HostOutputFormat::Json {
        anyhow::bail!("--summary-only requires --format json or --json");
    }
    if !(1..=365).contains(&args.days) {
        anyhow::bail!("--days must be between 1 and 365 for host cost reports");
    }

    let remote = args
        .remote
        .as_deref()
        .map(RemoteSessionFetcher::validate_codex_cost_host)
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let has_remote = remote.is_some();

    // Host comparison is deliberately native-only on both sides. This keeps
    // provider-owned Codex totals comparable and avoids sending or combining
    // any pi/OMP session mirror details.
    let mut reports = vec![CodexHostCostReport::success(
        "local",
        "local",
        scan_local_codex_summary(args.days),
    )];
    if let Some(host) = remote {
        reports.push(ssh_codex_host_report(&host, args.days).await);
    }

    match args.summary_only {
        true => {
            let summaries: Vec<_> = reports
                .iter()
                .filter_map(|report| report.summary())
                .cloned()
                .collect();
            let output = if args.pretty {
                serde_json::to_string_pretty(&summaries)?
            } else {
                serde_json::to_string(&summaries)?
            };
            println!("{output}");
        }
        false => {
            println!(
                "{}",
                reports
                    .iter()
                    .map(render_codex_host_report)
                    .collect::<Vec<_>>()
                    .join("\n\n")
            );
            if has_remote {
                println!(
                    "\nHost reports are separate; overlapping histories are not added together."
                );
            }
        }
    }

    if reports
        .iter()
        .skip(1)
        .any(|report| matches!(report.outcome, CodexHostOutcome::Failed(_)))
    {
        anyhow::bail!("Remote Codex cost report failed; local results were retained.");
    }
    Ok(())
}

async fn ssh_codex_host_report(host: &str, days: u32) -> CodexHostCostReport {
    let fetcher = RemoteSessionFetcher::default();
    match fetcher.fetch_codex_cost_summary(host, days, false).await {
        Ok(output) => {
            let result =
                decode_remote_codex_summary(&output, days).map_err(|_| REMOTE_CODEX_COST_INVALID);
            match result {
                Ok(summary) => CodexHostCostReport::success(host, "ssh", summary),
                Err(error) => CodexHostCostReport::failure(host, "ssh", error),
            }
        }
        Err(error) => {
            let message = if error == REMOTE_CODEX_COST_INVALID {
                REMOTE_CODEX_COST_INVALID
            } else {
                REMOTE_CODEX_COST_UNAVAILABLE
            };
            CodexHostCostReport::failure(host, "ssh", message)
        }
    }
}

fn scan_local_codex_summary(days: u32) -> CodexCostSummary {
    let mut scan_options = CostScanOptions::app_driven();
    scan_options.include_pi_sessions = false;
    let (history, _, cache) = CostScanner::new(days)
        .with_options(scan_options)
        .scan_codex_detailed_with_cache(None);
    build_codex_cost_summary(history, &cache, days)
}

fn render_codex_host_report(report: &CodexHostCostReport) -> String {
    let title = if report.source == "local" {
        "This machine".to_string()
    } else {
        report.host.clone()
    };
    let summary = match &report.outcome {
        CodexHostOutcome::Success(summary) => summary,
        CodexHostOutcome::Failed(error) => {
            return format!("{title}: {error}");
        }
    };

    let window_line =
        |label: &str, window: &crate::codex_costs::summary_contract::CodexHostCostWindow| {
            let cost = window
                .cost_usd
                .map(|value| format!("${value:.2}"))
                .unwrap_or_else(|| "—".to_string());
            let tokens = window
                .total_tokens
                .map(format_tokens)
                .unwrap_or_else(|| "—".to_string());
            let mut line = format!("{label}: {cost} · {tokens} tokens");
            if window.coverage.unpriced > 0 || window.coverage.unmetered > 0 {
                line.push_str(" (some usage has no known price)");
            }
            line
        };

    let history = if summary.history_days == 1 {
        String::new()
    } else {
        format!(
            "\n{}",
            window_line(
                &format!("Last {} days", summary.history_days),
                &summary.history
            )
        )
    };
    let coverage = if summary.history_coverage_is_established {
        String::new()
    } else {
        "\nPartial history; scan is incomplete.".to_string()
    };
    format!(
        "{title} — Codex API-equivalent estimate (not billed)\n{}{}\nSnapshot updated: {}\nDay boundaries: {}{}",
        window_line("Today", &summary.today),
        history,
        summary
            .updated_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        summary.bucket_time_zone,
        coverage
    )
}

fn format_tokens(value: u64) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost_scanner::CostSummary;

    #[test]
    fn ssh_cost_text_shows_source_snapshot_timestamp_in_utc() {
        let complete = CostSummary {
            history_coverage_established: true,
            ..CostSummary::default()
        };
        let summary = CodexCostSummary::from_summaries_at(
            &complete,
            &complete,
            30,
            chrono::DateTime::from_timestamp(946_684_800, 0).unwrap(),
            "Asia/Tokyo",
        );

        let text =
            render_codex_host_report(&CodexHostCostReport::success("qa-windows", "ssh", summary));

        assert!(text.contains("Snapshot updated: 2000-01-01T00:00:00Z"));
        assert!(text.contains("Day boundaries: Asia/Tokyo"));
    }
}
