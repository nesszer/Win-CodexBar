//! Cost command implementation
//!
//! Scans local JSONL logs to calculate token costs for Codex and Claude.

use clap::Args;

use super::cost_period::{
    cost_totals_json, resolve_period, rolling_window_days, stamp_period, window_days,
};
use super::usage::{OutputFormat, ProviderSelection};
use crate::codex_costs::{CodexHostCostsArgs, run_codex_host_costs};
use crate::codex_workspaces::short_session_id;
use crate::core::{CostScanOptions, ProviderId};
use crate::cost_reporting_period::CostReportingPeriod;
use crate::cost_scanner::{CostScanner, CostSummary};
use crate::settings::Settings;
use crate::spend_contract::build_local_spend_contract_from_summary;

/// Arguments for the cost command
#[derive(Args, Debug, Default)]
pub struct CostArgs {
    /// Provider to query (codex, claude, pi, muse, antigravity, cursor, gemini, copilot, all, both)
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

    /// Cost history window in days (1..=365); always rolling and overrides --period
    #[arg(short, long)]
    pub days: Option<u32>,

    /// Cost period: month-to-date or all. Without --days or --period the
    /// saved cost period applies (30 days by default)
    #[arg(long)]
    pub period: Option<String>,

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

    /// Antigravity only: when a recorded model has no known public price, wait
    /// for one bounded models.dev pricing refresh and rescan. Without it the
    /// CLI starts no pricing download.
    #[arg(long)]
    pub refresh: bool,
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

    let settings = Settings::load();
    let host_summary = args.remote.is_some() || args.summary_only;
    let period = resolve_period(
        args.days,
        args.period.as_deref(),
        settings.cost_reporting_period,
        host_summary,
    )?;
    let days = window_days(period);

    if host_summary {
        return run_codex_host_costs(&CodexHostCostsArgs {
            days,
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
    let requested_providers = providers.as_list();
    let pi_selected = requested_providers.contains(&ProviderId::Pi);
    // When Pi is selected alongside native providers, the standalone Pi row
    // owns its mirrored Codex/Claude events. A single native-provider request
    // keeps the historical inclusive behavior unless explicitly narrowed.
    scan_options.include_pi_sessions = !args.provider_native_only && !pi_selected;
    let scanner = CostScanner::for_period(period).with_options(scan_options);

    tracing::debug!(
        "Running cost command: providers={:?}, format={:?}, period={}, days={}",
        providers.as_list(),
        format,
        period,
        days
    );

    // Collect cost data for requested providers
    let mut results: Vec<CostResult> = Vec::new();

    for provider in providers.as_list() {
        let (summary, supported, token_history) = match provider {
            ProviderId::Codex => (scanner.scan_codex(), true, None),
            ProviderId::Claude => {
                let summary = if pi_selected || args.provider_native_only {
                    scanner.scan_claude_with_cancel_and_pi_sessions(None, false)
                } else {
                    scanner.scan_claude()
                };
                (summary, true, None)
            }
            ProviderId::Pi => (scanner.scan_pi(), true, None),
            ProviderId::Antigravity => (
                CostSummary::default(),
                true,
                Some(
                    crate::providers::antigravity::local_sessions::summarize_with_pricing_refresh(
                        days,
                        args.refresh,
                    )
                    .await,
                ),
            ),
            ProviderId::Muse => {
                let report = crate::providers::muse::local_usage::scan(days, None);
                (CostSummary::default(), true, Some(report.into()))
            }
            // Other providers don't have local logs to scan
            _ => (CostSummary::default(), false, None),
        };
        results.push(CostResult {
            provider: provider.cli_name().to_string(),
            display_name: provider.display_name().to_string(),
            summary,
            supported,
            token_history,
        });
    }

    match format {
        OutputFormat::Text => {
            print_text_output(&results, use_color, period, group_by);
        }
        OutputFormat::Json => {
            // Upstream 0.60.4 refreshes OpenCodex prices before the JSON
            // payload; on Windows the import lives in the Codex contract.
            if results.iter().any(|result| result.provider == "codex")
                && settings.open_codex_usage_logs_enabled
            {
                crate::spend_contract::refresh_opencodex_pricing_if_needed().await;
            }
            print_json_output(&results, args.pretty, period, days, &settings)?;
        }
    }

    Ok(())
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
fn print_text_output(
    results: &[CostResult],
    use_color: bool,
    period: CostReportingPeriod,
    group_by: CostGroupBy,
) {
    let label = period.label();
    for (i, result) in results.iter().enumerate() {
        let title = if result.token_history.is_some() {
            format!("{} Token History ({label})", result.display_name)
        } else {
            format!("{} Cost ({label})", result.display_name)
        };
        if use_color {
            println!("\x1b[1m{title}\x1b[0m");
        } else {
            println!("{title}");
        }

        if let Some(history) = result.token_history.as_ref() {
            print_local_token_history(history);
        } else if group_by == CostGroupBy::Session && result.provider == "codex" {
            print_codex_session_output(result, period);
        } else if group_by == CostGroupBy::Session {
            println!("  Session grouping is only available for Codex local conversations");
        } else if !result.supported {
            println!("  Local cost scanning not available for this provider");
            println!("  (Only Codex and Claude have local logs)");
        } else if result.summary.sessions_count == 0 {
            if result.summary.incomplete_request_count > 0 {
                // Only preliminary proxy rows exist: usage is unavailable, not $0.
                println!("  No completed usage data found");
                print_incomplete_note(result.summary.incomplete_request_count);
            } else if result.summary.known_zero {
                println!("  No usage in the selected period (scan complete)");
            } else {
                println!("  No usage data found");
                println!("  Check that you have used {} locally", result.display_name);
            }
        } else {
            // Total cost
            let incomplete_suffix = incomplete_suffix(result.summary.incomplete_request_count);
            if use_color {
                println!(
                    "  Total:    \x1b[32m{}\x1b[0m{incomplete_suffix}",
                    result.summary.format_total()
                );
            } else {
                println!(
                    "  Total:    {}{incomplete_suffix}",
                    result.summary.format_total()
                );
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
            print_incomplete_note(result.summary.incomplete_request_count);

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

/// " · Incomplete" marker for totals that exclude preliminary Claude proxy
/// rows (upstream 0.60.5 #3688).
fn incomplete_suffix(count: u32) -> &'static str {
    if count > 0 { " · Incomplete" } else { "" }
}

fn print_incomplete_note(count: u32) {
    if count > 0 {
        println!(
            "  Incomplete: {count} requests lacked final usage and were excluded from tokens and cost."
        );
    }
}

fn print_local_token_history(history: &crate::spend_contract::LocalTokenHistorySummary) {
    use crate::spend_contract::LocalHistoryCoverage;
    match history.coverage {
        LocalHistoryCoverage::Partial if history.lower_bound => {
            println!(
                "  Tokens:   at least {} (lower bound; scan stopped short)",
                format_number(history.total_tokens)
            );
            println!("  Sessions: at least {}", history.session_count);
        }
        LocalHistoryCoverage::Complete if history.total_tokens == 0 => {
            println!("  No token usage in the selected period (scan complete)");
        }
        LocalHistoryCoverage::Complete => {
            println!("  Tokens:   {} total", format_number(history.total_tokens));
            println!("  Sessions: {}", history.session_count);
        }
        LocalHistoryCoverage::Partial | LocalHistoryCoverage::Unavailable => {
            println!("  Local token history is unavailable or incomplete");
        }
    }
    if let Some(cost) = history.total_usd() {
        println!("  API list-price estimate: ${cost:.2} (not billed spend)");
    } else if let Some(cost) = history.cost_estimate.known_subtotal_usd {
        if history.coverage == LocalHistoryCoverage::Complete {
            println!(
                "  Known API list-price subtotal: ${cost:.2} ({} unpriced requests)",
                history.cost_estimate.coverage.unpriced
            );
        } else {
            println!("  Known API list-price subtotal: at least ${cost:.2} (history incomplete)");
        }
    } else {
        println!("  Local token history; dollar costs unavailable");
    }
}

fn print_codex_session_output(result: &CostResult, period: CostReportingPeriod) {
    // The conversation index keeps rolling 1..=365 day windows, so All lists
    // the most recent year; the heading below reports the window it used.
    let index = crate::codex_workspaces::CodexWorkspacesIndex::new(rolling_window_days(period));
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

/// Print JSON output
fn build_json_payloads(
    results: &[CostResult],
    period: CostReportingPeriod,
    days: u32,
    settings: &Settings,
) -> Vec<serde_json::Value> {
    results
        .iter()
        .map(|r| {
            if let Some(history) = r.token_history.as_ref() {
                let mut payload =
                    crate::spend_contract::local_token_history_json(&r.provider, history, days);
                stamp_period(&mut payload, period);
                return payload;
            }
            if !r.supported {
                serde_json::json!({
                    "provider": r.provider,
                    "supported": false,
                    "error": "Local cost scanning not available for this provider"
                })
            } else {
                let spend_contract = matches!(r.provider.as_str(), "codex" | "claude" | "pi" | "opencodego")
                    .then(|| build_local_spend_contract_from_summary(
                        &r.provider,
                        days.clamp(1, 365),
                        settings.open_codex_usage_logs_enabled && r.provider == "codex",
                        settings.hide_native_codex_cost_when_open_codex_present && r.provider == "codex",
                        settings.hide_personal_info,
                        r.summary.clone(),
                    ));
                let mut payload = serde_json::json!({
                    "provider": r.provider,
                    "supported": true,
                    "days_scanned": days,
                    "totals": cost_totals_json(&r.provider, &r.summary),
                    "cost": {"total_usd": r.summary.total_cost_usd, "currency": "USD"},
                    "tokens": {"input": r.summary.input_tokens, "output": r.summary.output_tokens, "cached": r.summary.cached_tokens},
                    "sessions_count": r.summary.sessions_count,
                    "historyCoverageIsEstablished": if matches!(r.provider.as_str(), "codex" | "pi") { serde_json::Value::Bool(r.summary.history_coverage_established) } else { serde_json::Value::Null },
                    "knownZero": if matches!(r.provider.as_str(), "codex" | "pi") { serde_json::Value::Bool(r.summary.known_zero) } else { serde_json::Value::Null },
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
                });
                // Only emitted when a scan excluded preliminary Claude proxy rows.
                if r.summary.incomplete_request_count > 0
                    && let Some(object) = payload.as_object_mut()
                {
                    object.insert(
                        "incompleteRequestCount".to_string(),
                        serde_json::json!(r.summary.incomplete_request_count),
                    );
                }
                stamp_period(&mut payload, period);
                payload
            }
        })
        .collect()
}

fn print_json_output(
    results: &[CostResult],
    pretty: bool,
    period: CostReportingPeriod,
    days: u32,
    settings: &Settings,
) -> anyhow::Result<()> {
    let payloads = build_json_payloads(results, period, days, settings);

    super::print_json(&payloads, pretty)
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
mod tests;
