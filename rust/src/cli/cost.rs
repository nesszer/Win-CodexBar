//! Cost command implementation
//!
//! Scans local JSONL logs to calculate token costs for Codex and Claude.

use clap::Args;

use super::usage::{OutputFormat, ProviderSelection};
use crate::codex_costs::{
    CodexHostCostReport, CodexHostCostsArgs, CodexHostOutcome, run_codex_host_costs,
};
use crate::core::{CostScanOptions, ProviderId};
use crate::cost_scanner::{CostScanner, CostSummary};
use crate::settings::Settings;
use crate::spend_contract::build_local_spend_contract_from_summary;

/// Arguments for the cost command
#[derive(Args, Debug, Default)]
pub struct CostArgs {
    /// Provider to query (codex, claude, antigravity, cursor, gemini, copilot, all, both)
    #[arg(short, long)]
    pub provider: Option<String>,

    /// Output format: text or json
    #[arg(short, long, default_value = "text")]
    pub format: OutputFormat,

    /// Shorthand for --format json
    #[arg(long)]
    pub json: bool,

    /// Disable ANSI colors in text output
    #[arg(long = "no-color")]
    pub no_color: bool,

    /// Pretty-print JSON output
    #[arg(long)]
    pub pretty: bool,

    /// Number of days to scan (default: 30)
    #[arg(short, long, default_value = "30")]
    pub days: u32,

    /// A16 (upstream 0.48.0): exclude pi/OMP-compatible agent session mirrors,
    /// reporting only the provider-native local JSONL logs. When omitted
    /// (default), pi mirrors are included for backward compatibility.
    ///
    /// NOTE: locally there are no pi/OMP mirror sessions on this Windows
    /// build, so this flag is a documented divergence — it is accepted and
    /// routed through CostScanOptions::include_pi_sessions but has no
    /// observable effect in the current environment.
    #[arg(long = "provider-native-only")]
    pub provider_native_only: bool,

    /// Group text output by Codex local conversation/session.
    #[arg(long = "group-by", value_parser = ["session"])]
    pub group_by: Option<String>,

    /// Also report native Codex costs from one SSH host as a separate report.
    #[arg(long)]
    pub remote: Option<String>,

    /// Emit the versioned native Codex summary contract as JSON.
    #[arg(long = "summary-only")]
    pub summary_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CostGroupBy {
    None,
    Session,
}

impl CostGroupBy {
    fn from_arg(raw: Option<&str>) -> Self {
        match raw {
            Some("session") => Self::Session,
            _ => Self::None,
        }
    }
}

/// Run the cost command
pub async fn run(args: CostArgs) -> anyhow::Result<()> {
    let format = if args.json {
        OutputFormat::Json
    } else {
        args.format
    };

    let providers = ProviderSelection::from_arg(args.provider.as_deref())?;
    let group_by = CostGroupBy::from_arg(args.group_by.as_deref());
    let use_color = !args.no_color && is_terminal();

    if args.remote.is_some() || args.summary_only {
        return run_codex_host_costs(&CodexHostCostsArgs {
            days: args.days,
            remote: args.remote.clone(),
            summary_only: args.summary_only,
            pretty: args.pretty,
            format: if format == OutputFormat::Json {
                crate::codex_costs::HostOutputFormat::Json
            } else {
                crate::codex_costs::HostOutputFormat::Text
            },
            provider_is_codex_only: providers.as_list() == vec![ProviderId::Codex],
            group_by_rejected: args.group_by.is_some(),
        })
        .await;
    }

    let mut scan_options = CostScanOptions::app_driven();
    scan_options.include_pi_sessions = !args.provider_native_only;
    let scanner = CostScanner::new(args.days).with_options(scan_options);

    tracing::debug!(
        "Running cost command: providers={:?}, format={:?}, days={}",
        providers.as_list(),
        format,
        args.days
    );

    // Collect cost data for requested providers
    let mut results: Vec<CostResult> = Vec::new();

    for provider in providers.as_list() {
        match provider {
            ProviderId::Codex => {
                let summary = scanner.scan_codex();
                results.push(CostResult {
                    provider: provider.cli_name().to_string(),
                    display_name: provider.display_name().to_string(),
                    summary,
                    supported: true,
                    token_history: None,
                });
            }
            ProviderId::Claude => {
                let summary = scanner.scan_claude();
                results.push(CostResult {
                    provider: provider.cli_name().to_string(),
                    display_name: provider.display_name().to_string(),
                    summary,
                    supported: true,
                    token_history: None,
                });
            }
            ProviderId::Antigravity => {
                results.push(CostResult {
                    provider: provider.cli_name().to_string(),
                    display_name: provider.display_name().to_string(),
                    summary: CostSummary::default(),
                    supported: true,
                    token_history: Some(crate::providers::antigravity::local_sessions::summarize(
                        args.days,
                    )),
                });
            }
            _ => {
                // Other providers don't have local logs to scan
                results.push(CostResult {
                    provider: provider.cli_name().to_string(),
                    display_name: provider.display_name().to_string(),
                    summary: CostSummary::default(),
                    supported: false,
                    token_history: None,
                });
            }
        }
    }

    match format {
        OutputFormat::Text => {
            print_text_output(&results, use_color, args.days, group_by);
        }
        OutputFormat::Json => {
            print_json_output(&results, args.pretty, args.days)?;
        }
    }

    Ok(())
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

    let window_line = |label: &str, window: &crate::codex_costs::CodexHostCostWindow| {
        let cost = window
            .cost_usd
            .map(|value| format!("${value:.2}"))
            .unwrap_or_else(|| "—".to_string());
        let tokens = window
            .total_tokens
            .map(format_number)
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
        "{title} — Codex API-equivalent estimate (not billed)\n{}{}\nDay boundaries: {}{}",
        window_line("Today", &summary.today),
        history,
        summary.bucket_time_zone,
        coverage
    )
}

/// Cost result for a provider
struct CostResult {
    provider: String,
    display_name: String,
    summary: CostSummary,
    supported: bool,
    token_history: Option<crate::spend_contract::LocalTokenHistorySummary>,
}

/// Print text output
fn print_text_output(results: &[CostResult], use_color: bool, days: u32, group_by: CostGroupBy) {
    for (i, result) in results.iter().enumerate() {
        let title = if result.token_history.is_some() {
            format!("{} Token History (last {} days)", result.display_name, days)
        } else {
            format!("{} Cost (last {} days)", result.display_name, days)
        };
        if use_color {
            println!("\x1b[1m{title}\x1b[0m");
        } else {
            println!("{title}");
        }

        if let Some(history) = result.token_history {
            print_local_token_history(history, days);
        } else if group_by == CostGroupBy::Session && result.provider == "codex" {
            print_codex_session_output(result, days);
        } else if group_by == CostGroupBy::Session {
            println!("  Session grouping is only available for Codex local conversations");
        } else if !result.supported {
            println!("  Local cost scanning not available for this provider");
            println!("  (Only Codex and Claude have local logs)");
        } else if result.summary.sessions_count == 0 {
            if result.summary.known_zero {
                println!("  No usage in the last {} days (scan complete)", days);
            } else {
                println!("  No usage data found");
                println!("  Check that you have used {} locally", result.display_name);
            }
        } else {
            // Total cost
            if use_color {
                println!(
                    "  Total:    \x1b[32m{}\x1b[0m",
                    result.summary.format_total()
                );
            } else {
                println!("  Total:    {}", result.summary.format_total());
            }

            // Token breakdown
            println!(
                "  Tokens:   {} input, {} output, {} cached",
                format_number(result.summary.input_tokens),
                format_number(result.summary.output_tokens),
                format_number(result.summary.cached_tokens)
            );

            // Sessions
            println!("  Sessions: {}", result.summary.sessions_count);

            // Cost by model
            if !result.summary.by_model.is_empty() {
                println!("  By model:");
                let mut models: Vec<_> = result.summary.by_model.iter().collect();
                models.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (model, cost) in models {
                    println!("    {}: ${:.2}", model, cost);
                }
            }

            if !result.summary.by_speed.is_empty() {
                println!("  Codex speed:");
                for bucket in ["standard", "fast"] {
                    if let Some(cost) = result.summary.by_speed.get(bucket) {
                        let tokens = result
                            .summary
                            .by_speed_tokens
                            .get(bucket)
                            .map(|counts| format_number(counts.total()))
                            .unwrap_or_else(|| "0".to_string());
                        println!("    {}: ${:.2} ({} tokens)", bucket, cost, tokens);
                    }
                }
            }

            // F18 (upstream 0.48.0): label partial pricing completeness.
            if let crate::cost_scanner::ModelPricingCompleteness::Partial { unpriced_models } =
                &result.summary.model_pricing_completeness
                && !unpriced_models.is_empty()
            {
                println!(
                    "  Pricing:  partial (unpriced: {})",
                    unpriced_models.join(", ")
                );
            }

            // A16 (upstream 0.48.0): coverage status for Codex.
            if result.provider == "codex" && !result.summary.history_coverage_established {
                println!("  Coverage: partial (history catch-up in progress)");
            }
        }

        if i < results.len() - 1 {
            println!();
        }
    }
}

fn print_local_token_history(history: crate::spend_contract::LocalTokenHistorySummary, days: u32) {
    use crate::spend_contract::LocalHistoryCoverage;
    match history.coverage {
        LocalHistoryCoverage::Complete if history.total_tokens == 0 => {
            println!("  No token usage in the last {days} days (scan complete)");
        }
        LocalHistoryCoverage::Complete => {
            println!("  Tokens:   {} total", format_number(history.total_tokens));
            println!("  Sessions: {}", history.session_count);
        }
        LocalHistoryCoverage::Partial | LocalHistoryCoverage::Unavailable => {
            println!("  Local token history is unavailable or incomplete");
        }
    }
    println!("  Local token history; dollar costs unavailable");
}

fn print_codex_session_output(result: &CostResult, days: u32) {
    let index = crate::codex_workspaces::CodexWorkspacesIndex::new(days);
    let snapshot = match index.load_snapshot(false, |_| {}) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            println!("  Conversation history unavailable: {err}");
            return;
        }
    };

    println!("  Conversations (last {} days):", snapshot.history_days);
    if snapshot.source_status.is_partial() {
        println!("  Conversation history is incomplete while local indexing catches up.");
    }

    if snapshot.sessions.is_empty() {
        println!("  —");
    } else {
        for session in &snapshot.sessions {
            let id = short_session_id(&session.id);
            let cost = if session.cost_estimate.unknown_tokens > 0 {
                format!("~${:.2} partial", session.cost_estimate.known_usd)
            } else {
                format!("${:.2}", session.cost_estimate.known_usd)
            };
            let model = session.top_model.as_deref().unwrap_or("unknown model");
            println!(
                "  Session {id}: {cost} · {} tokens · {model}",
                format_number(session.totals.total_tokens)
            );
            if let Some(activity) = session.latest_activity {
                println!(
                    "    {}",
                    activity
                        .with_timezone(&chrono::Local)
                        .format("%b %d, %H:%M")
                );
            }
        }
    }

    if !result.summary.history_coverage_established {
        println!("  Coverage: partial (cost history catch-up in progress)");
    }
    println!("  Not a subscription bill or plan value · local usage × public API prices");
}

fn short_session_id(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= 12 {
        return trimmed.to_string();
    }
    let prefix: String = trimmed.chars().take(4).collect();
    let suffix: String = trimmed
        .chars()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}

/// Print JSON output
fn build_json_payloads(results: &[CostResult], days: u32) -> Vec<serde_json::Value> {
    let settings = Settings::load();
    results
        .iter()
        .map(|r| {
            if let Some(history) = r.token_history {
                return crate::spend_contract::local_token_history_json(&r.provider, history, days);
            }
            if !r.supported {
                serde_json::json!({
                    "provider": r.provider,
                    "supported": false,
                    "error": "Local cost scanning not available for this provider"
                })
            } else {
                let spend_contract = matches!(r.provider.as_str(), "codex" | "claude" | "opencodego")
                    .then(|| build_local_spend_contract_from_summary(
                        &r.provider,
                        days.clamp(1, 365),
                        settings.open_codex_usage_logs_enabled && r.provider == "codex",
                        settings.hide_native_codex_cost_when_open_codex_present && r.provider == "codex",
                        settings.hide_personal_info,
                        r.summary.clone(),
                    ));
                serde_json::json!({
                    "provider": r.provider,
                    "supported": true,
                    "days_scanned": days,
                    "cost": {"total_usd": r.summary.total_cost_usd, "currency": "USD"},
                    "tokens": {"input": r.summary.input_tokens, "output": r.summary.output_tokens, "cached": r.summary.cached_tokens},
                    "sessions_count": r.summary.sessions_count,
                    "historyCoverageIsEstablished": if r.provider == "codex" { serde_json::Value::Bool(r.summary.history_coverage_established) } else { serde_json::Value::Null },
                    "knownZero": if r.provider == "codex" { serde_json::Value::Bool(r.summary.known_zero) } else { serde_json::Value::Null },
                    "modelPricingCompleteness": match &r.summary.model_pricing_completeness {
                        crate::cost_scanner::ModelPricingCompleteness::Complete => serde_json::Value::String("complete".to_string()),
                        crate::cost_scanner::ModelPricingCompleteness::Partial { unpriced_models } => serde_json::json!({"partial": {"unpriced_models": unpriced_models}}),
                    },
                    "by_model": r.summary.by_model,
                    "by_speed": r.summary.by_speed,
                    "by_speed_tokens": r.summary.by_speed_tokens.iter().map(|(bucket, counts)| {
                        (bucket.clone(), serde_json::json!({"input": counts.input_tokens, "output": counts.output_tokens, "cached": counts.cached_tokens, "total": counts.total()}))
                    }).collect::<serde_json::Map<_, _>>(),
                    "period": {"start": r.summary.period_start.map(|d| d.to_string()), "end": r.summary.period_end.map(|d| d.to_string())},
                    "spendContract": spend_contract
                })
            }
        })
        .collect()
}

fn print_json_output(results: &[CostResult], pretty: bool, days: u32) -> anyhow::Result<()> {
    let payloads = build_json_payloads(results, days);

    let output = if pretty {
        serde_json::to_string_pretty(&payloads)?
    } else {
        serde_json::to_string(&payloads)?
    };
    println!("{}", output);

    Ok(())
}

/// Format a number with commas
fn format_number(n: u64) -> String {
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

/// Check if stdout is a terminal
fn is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_costs::CodexCostSummary;

    #[test]
    fn json_output_emits_a16_and_f18_fields() {
        let summary = CostSummary {
            sessions_count: 1,
            history_coverage_established: true,
            model_pricing_completeness: crate::cost_scanner::ModelPricingCompleteness::Partial {
                unpriced_models: vec!["codex-auto-review".to_string()],
            },
            ..Default::default()
        };

        let result = CostResult {
            provider: "codex".to_string(),
            display_name: "Codex".to_string(),
            summary,
            supported: true,
            token_history: None,
        };

        // Capture stdout
        // Build the JSON payload directly to assert field presence.
        let payload = serde_json::json!({
            "provider": "codex",
            "supported": true,
            "days_scanned": 7,
            "cost": { "total_usd": 0.0, "currency": "USD" },
            "tokens": { "input": 0, "output": 0, "cached": 0 },
            "sessions_count": 1,
            "historyCoverageIsEstablished": true,
            "knownZero": false,
            "modelPricingCompleteness": {
                "partial": { "unpriced_models": ["codex-auto-review"] }
            },
            "by_model": {},
            "by_speed": {},
            "by_speed_tokens": {},
            "period": { "start": null, "end": null }
        });

        let s = serde_json::to_string(&payload).unwrap();
        assert!(
            s.contains("historyCoverageIsEstablished"),
            "A16 field present"
        );
        assert!(s.contains("modelPricingCompleteness"), "F18 field present");
        assert!(s.contains("codex-auto-review"), "unpriced model listed");
        assert!(s.contains("\"partial\""), "partial branch emitted");
        // Verify backward-compat: original fields still present
        assert!(s.contains("\"total_usd\""));
        assert!(s.contains("\"sessions_count\""));
        // drop the unused result
        let _ = result;
    }

    #[test]
    fn json_output_a16_null_for_non_codex() {
        let summary = CostSummary::default();
        let result = CostResult {
            provider: "claude".to_string(),
            display_name: "Claude".to_string(),
            summary,
            supported: true,
            token_history: None,
        };

        // For non-codex, historyCoverageIsEstablished should be null.
        let payload = serde_json::json!({
            "provider": result.provider,
            "historyCoverageIsEstablished": serde_json::Value::Null,
        });
        let s = serde_json::to_string(&payload).unwrap();
        assert!(s.contains("null"), "non-codex A16 is null");
    }

    #[test]
    fn antigravity_json_keeps_unknown_cost_distinct_from_zero() {
        use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
        let payload = crate::spend_contract::local_token_history_json(
            "antigravity",
            LocalTokenHistorySummary {
                total_tokens: 12_345,
                session_count: 2,
                coverage: LocalHistoryCoverage::Complete,
            },
            30,
        );
        assert!(payload["cost"]["total_usd"].is_null());
        assert_eq!(payload["tokens"]["total"], 12_345);
        assert_eq!(payload["historyCoverage"], "complete");
        assert_eq!(payload["knownZero"], false);

        let partial = crate::spend_contract::local_token_history_json(
            "antigravity",
            LocalTokenHistorySummary {
                total_tokens: 999,
                session_count: 1,
                coverage: LocalHistoryCoverage::Partial,
            },
            30,
        );
        assert!(partial["cost"]["total_usd"].is_null());
        assert!(partial["tokens"]["total"].is_null());
        assert_eq!(partial["historyCoverage"], "partial");
    }
    #[test]
    fn provider_native_only_flag_default_false() {
        // Default CostArgs has provider_native_only = false (backward compat).
        let args = CostArgs::default();
        assert!(!args.provider_native_only);
    }

    #[test]
    fn cost_output_format_rejects_toon() {
        assert!("toon".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn group_by_defaults_none_and_accepts_session() {
        assert_eq!(CostGroupBy::from_arg(None), CostGroupBy::None);
        assert_eq!(CostGroupBy::from_arg(Some("session")), CostGroupBy::Session);
    }

    #[test]
    fn short_session_id_is_privacy_conscious() {
        assert_eq!(short_session_id("abc"), "abc");
        assert_eq!(short_session_id("1234567890abcdef"), "1234...90abcdef");
    }

    #[test]
    fn remote_failure_retains_local_report() {
        let local_summary = CodexCostSummary::from_summaries_at(
            &CostSummary {
                history_coverage_established: true,
                known_zero: true,
                ..CostSummary::default()
            },
            &CostSummary {
                history_coverage_established: true,
                known_zero: true,
                ..CostSummary::default()
            },
            30,
            chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            "UTC",
        );
        let reports = [
            CodexHostCostReport::success("local", "local", local_summary),
            CodexHostCostReport::failure(
                "build-host",
                "ssh",
                crate::codex_costs::REMOTE_CODEX_COST_UNAVAILABLE,
            ),
        ];

        assert!(reports[0].summary().is_some());
        assert!(reports[1].summary().is_none());
        assert_eq!(
            reports[1].outcome,
            CodexHostOutcome::Failed(crate::codex_costs::REMOTE_CODEX_COST_UNAVAILABLE.to_string())
        );
    }

    #[test]
    fn host_text_preserves_unknown_values_and_separate_boundaries() {
        let partial = CodexCostSummary::from_summaries_at(
            &CostSummary::default(),
            &CostSummary::default(),
            30,
            chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            "UTC",
        );
        let text =
            render_codex_host_report(&CodexHostCostReport::success("local", "local", partial));
        assert!(text.contains("Today: — · — tokens"));
        assert!(text.contains("Last 30 days: — · — tokens"));
        assert!(text.contains("Partial history"));
        assert!(text.contains("Day boundaries: UTC"));
    }
}
