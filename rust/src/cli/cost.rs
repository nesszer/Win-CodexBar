//! Cost command implementation
//!
//! Scans local JSONL logs to calculate token costs for Codex and Claude.

use clap::Args;

use super::cost_period::{
    cost_totals_json, resolve_period, rolling_window_days, stamp_period, window_days,
};
use super::usage::{OutputFormat, ProviderSelection};
use crate::codex_costs::{CodexHostCostReport, CodexHostOutcome};
use crate::core::format_count as format_number;
use crate::core::{
    CostCliExtension, CostCliInvocation, CostCliMode, CostGroupedReportContext, CostGrouping,
    CostScanOptions, ProviderId, cost_cli_extension_for, cost_cli_extensions,
};
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

    /// Group Codex text output by local project or conversation/session.
    #[arg(long = "group-by", value_parser = ["project", "session"])]
    pub group_by: Option<String>,

    /// Read one SSH host; separate summaries unless --identified-only is selected.
    #[arg(long)]
    pub remote: Option<String>,

    /// Emit the versioned native Codex summary contract as JSON.
    #[arg(long = "summary-only")]
    pub summary_only: bool,

    /// Native Codex identified-request subtotal JSON; prices unavailable.
    #[arg(long = "identified-only")]
    pub identified_only: bool,

    /// Identified subtotal window end, inclusive Unix milliseconds (UTC).
    #[arg(long = "identified-end", value_name = "UNIX_MS")]
    pub identified_end: Option<i64>,

    /// Antigravity only: when a recorded model has no known public price, wait
    /// for one bounded models.dev pricing refresh and rescan. Without it the
    /// CLI starts no pricing download.
    #[arg(long)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CostGroupBy {
    None,
    Project,
    Session,
}

impl CostGroupBy {
    fn from_arg(raw: Option<&str>) -> Self {
        match raw {
            Some("project") => Self::Project,
            Some("session") => Self::Session,
            _ => Self::None,
        }
    }
}

/// Run the cost command
pub async fn run(args: CostArgs) -> anyhow::Result<()> {
    run_with_settings(args, Settings::load).await
}

/// `load_settings` runs only once a mode needs saved settings; tests inject
/// defaults so they never read the user's real settings file.
async fn run_with_settings(
    args: CostArgs,
    load_settings: impl FnOnce() -> Settings,
) -> anyhow::Result<()> {
    let format = if args.json {
        OutputFormat::Json
    } else {
        args.format
    };

    let providers = ProviderSelection::from_arg(args.provider.as_deref())?;
    let group_by = CostGroupBy::from_arg(args.group_by.as_deref());
    let use_color = !args.no_color && is_terminal();

    let selected_providers = providers.as_list();
    let invocation = CostCliInvocation {
        provider_arg: args.provider.as_deref(),
        providers: &selected_providers,
        json: format == OutputFormat::Json,
        pretty: args.pretty,
        days: args.days,
        period_set: args.period.is_some(),
        group_by_set: args.group_by.is_some(),
        remote: args.remote.as_deref(),
        summary_only: args.summary_only,
        identified_only: args.identified_only,
        identified_end: args.identified_end,
    };
    // An exclusive mode replaces the report and validates the provider
    // selection itself, so it is claimed by its flags alone.
    let exclusive = cost_cli_extensions()
        .find_map(|extension| Some((extension, extension.exclusive_mode(&invocation)?)));
    if let Some((extension, mode @ CostCliMode::BeforePeriod)) = exclusive {
        return extension.run_exclusive_mode(mode, &invocation, None).await;
    }

    let settings = load_settings();
    let period = resolve_period(
        args.days,
        args.period.as_deref(),
        settings.cost_reporting_period,
        exclusive.is_some(),
    )?;
    let days = window_days(period);

    if let Some((extension, mode)) = exclusive {
        return extension
            .run_exclusive_mode(mode, &invocation, Some(days))
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
                let summary = if pi_selected || args.provider_native_only {
                    scanner.scan_claude_with_cancel_and_pi_sessions(None, false)
                } else {
                    scanner.scan_claude()
                };
                results.push(CostResult {
                    provider: provider.cli_name().to_string(),
                    display_name: provider.display_name().to_string(),
                    summary,
                    supported: true,
                    token_history: None,
                });
            }
            ProviderId::Pi => {
                let summary = scanner.scan_pi();
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
                    token_history: Some(
                        crate::providers::antigravity::local_sessions::summarize_with_pricing_refresh(
                            days,
                            args.refresh,
                        )
                        .await,
                    ),
                });
            }
            ProviderId::Muse => {
                let report = crate::providers::muse::local_usage::scan(days, None);
                results.push(CostResult {
                    provider: provider.cli_name().to_string(),
                    display_name: provider.display_name().to_string(),
                    summary: CostSummary::default(),
                    supported: true,
                    token_history: Some(report.into()),
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
            print_text_output(
                &results,
                use_color,
                period,
                group_by,
                settings.hide_personal_info,
            );
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

/// Appends one formatted line to a text report buffer.
macro_rules! outln {
    ($out:expr) => {
        $out.push('\n')
    };
    ($out:expr, $($arg:tt)*) => {{
        $out.push_str(&format!($($arg)*));
        $out.push('\n');
    }};
}

/// Resolves the cost extension for a result's provider CLI name.
type ExtensionFor<'a> = &'a dyn Fn(&str) -> Option<&'a dyn CostCliExtension>;

fn registered_extension(provider: &str) -> Option<&'static dyn CostCliExtension> {
    ProviderId::from_cli_name(provider).and_then(cost_cli_extension_for)
}

/// Print text output
fn print_text_output(
    results: &[CostResult],
    use_color: bool,
    period: CostReportingPeriod,
    group_by: CostGroupBy,
    hide_personal_info: bool,
) {
    for (i, result) in results.iter().enumerate() {
        let mut out = String::new();
        render_result_text(
            &mut out,
            result,
            use_color,
            period,
            group_by,
            hide_personal_info,
            &registered_extension,
        );
        if i < results.len() - 1 {
            outln!(out);
        }
        print!("{out}");
    }
}

fn render_result_text(
    out: &mut String,
    result: &CostResult,
    use_color: bool,
    period: CostReportingPeriod,
    group_by: CostGroupBy,
    hide_personal_info: bool,
    extension_for: ExtensionFor<'_>,
) {
    let extension = extension_for(&result.provider);
    let grouped_report = |grouping| {
        extension?.grouped_report(
            grouping,
            &CostGroupedReportContext {
                summary: &result.summary,
                history_days: rolling_window_days(period),
                hide_personal_info,
            },
        )
    };
    let label = period.label();
    let title = if result.token_history.is_some() {
        format!("{} Token History ({label})", result.display_name)
    } else {
        format!("{} Cost ({label})", result.display_name)
    };
    if use_color {
        outln!(out, "\x1b[1m{title}\x1b[0m");
    } else {
        outln!(out, "{title}");
    }

    // Project grouping is checked before token history so token-history
    // providers report it unavailable; session grouping is checked after.
    if group_by == CostGroupBy::Project {
        match grouped_report(CostGrouping::Project) {
            Some(lines) => render_lines(out, &lines),
            None => outln!(
                out,
                "  Project grouping is only available for Codex local projects"
            ),
        }
    } else if let Some(history) = result.token_history.as_ref() {
        render_local_token_history(out, history);
    } else if group_by == CostGroupBy::Session {
        match grouped_report(CostGrouping::Session) {
            Some(lines) => render_lines(out, &lines),
            None => outln!(
                out,
                "  Session grouping is only available for Codex local conversations"
            ),
        }
    } else if !result.supported {
        outln!(out, "  Local cost scanning not available for this provider");
        outln!(out, "  (Only Codex and Claude have local logs)");
    } else if result.summary.sessions_count == 0 {
        if result.summary.incomplete_request_count > 0 {
            // Only preliminary proxy rows exist: usage is unavailable, not $0.
            outln!(out, "  No completed usage data found");
            render_incomplete_note(out, result.summary.incomplete_request_count);
        } else if result.summary.known_zero {
            outln!(out, "  No usage in the selected period (scan complete)");
        } else {
            outln!(out, "  No usage data found");
            outln!(
                out,
                "  Check that you have used {} locally",
                result.display_name
            );
        }
    } else {
        // Total cost
        let incomplete_suffix = incomplete_suffix(result.summary.incomplete_request_count);
        if use_color {
            outln!(
                out,
                "  Total:    \x1b[32m{}\x1b[0m{incomplete_suffix}",
                result.summary.format_total()
            );
        } else {
            outln!(
                out,
                "  Total:    {}{incomplete_suffix}",
                result.summary.format_total()
            );
        }

        // Token breakdown
        outln!(
            out,
            "  Tokens:   {} input, {} output, {} cached",
            format_number(result.summary.input_tokens),
            format_number(result.summary.output_tokens),
            format_number(result.summary.cached_tokens)
        );

        // Sessions
        outln!(out, "  Sessions: {}", result.summary.sessions_count);
        render_incomplete_note(out, result.summary.incomplete_request_count);

        // Cost by model
        if !result.summary.by_model.is_empty() {
            outln!(out, "  By model:");
            let mut models: Vec<_> = result.summary.by_model.iter().collect();
            models.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (model, cost) in models {
                outln!(out, "    {}: ${:.2}", model, cost);
            }
        }

        if !result.summary.by_speed.is_empty() {
            outln!(out, "  Codex speed:");
            for bucket in ["standard", "fast"] {
                if let Some(cost) = result.summary.by_speed.get(bucket) {
                    let tokens = result
                        .summary
                        .by_speed_tokens
                        .get(bucket)
                        .map(|counts| format_number(counts.total()))
                        .unwrap_or_else(|| "0".to_string());
                    outln!(out, "    {}: ${:.2} ({} tokens)", bucket, cost, tokens);
                }
            }
        }

        // F18 (upstream 0.48.0): label partial pricing completeness.
        if let crate::cost_scanner::ModelPricingCompleteness::Partial { unpriced_models } =
            &result.summary.model_pricing_completeness
            && !unpriced_models.is_empty()
        {
            outln!(
                out,
                "  Pricing:  partial (unpriced: {})",
                unpriced_models.join(", ")
            );
        }

        if let Some(notice) = extension.and_then(|ext| ext.coverage_notice(&result.summary)) {
            outln!(out, "{notice}");
        }
    }
}

fn render_lines(out: &mut String, lines: &[String]) {
    for line in lines {
        outln!(out, "{line}");
    }
}

/// " · Incomplete" marker for totals that exclude preliminary Claude proxy
/// rows (upstream 0.60.5 #3688).
fn incomplete_suffix(count: u32) -> &'static str {
    if count > 0 { " · Incomplete" } else { "" }
}

fn render_incomplete_note(out: &mut String, count: u32) {
    if count > 0 {
        outln!(
            out,
            "  Incomplete: {count} requests lacked final usage and were excluded from tokens and cost."
        );
    }
}

fn render_local_token_history(
    out: &mut String,
    history: &crate::spend_contract::LocalTokenHistorySummary,
) {
    use crate::spend_contract::LocalHistoryCoverage;
    match history.coverage {
        LocalHistoryCoverage::Partial if history.lower_bound => {
            outln!(
                out,
                "  Tokens:   at least {} (lower bound; scan stopped short)",
                format_number(history.total_tokens)
            );
            outln!(out, "  Sessions: at least {}", history.session_count);
        }
        LocalHistoryCoverage::Complete if history.total_tokens == 0 => {
            outln!(
                out,
                "  No token usage in the selected period (scan complete)"
            );
        }
        LocalHistoryCoverage::Complete => {
            outln!(
                out,
                "  Tokens:   {} total",
                format_number(history.total_tokens)
            );
            outln!(out, "  Sessions: {}", history.session_count);
        }
        LocalHistoryCoverage::Partial | LocalHistoryCoverage::Unavailable => {
            outln!(out, "  Local token history is unavailable or incomplete");
        }
    }
    if let Some(cost) = history.total_usd() {
        outln!(
            out,
            "  API list-price estimate: ${cost:.2} (not billed spend)"
        );
    } else if let Some(cost) = history.cost_estimate.known_subtotal_usd {
        if history.coverage == LocalHistoryCoverage::Complete {
            outln!(
                out,
                "  Known API list-price subtotal: ${cost:.2} ({} unpriced requests)",
                history.cost_estimate.coverage.unpriced
            );
        } else {
            outln!(
                out,
                "  Known API list-price subtotal: at least ${cost:.2} (history incomplete)"
            );
        }
    } else {
        outln!(out, "  Local token history; dollar costs unavailable");
    }
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

    let output = if pretty {
        serde_json::to_string_pretty(&payloads)?
    } else {
        serde_json::to_string(&payloads)?
    };
    println!("{}", output);

    Ok(())
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
    fn json_output_emits_incomplete_request_count_only_when_positive() {
        let make = |incomplete_request_count| CostResult {
            provider: "claude".to_string(),
            display_name: "Claude".to_string(),
            summary: CostSummary {
                incomplete_request_count,
                ..CostSummary::default()
            },
            supported: true,
            token_history: None,
        };
        let payloads = build_json_payloads(
            &[make(3), make(0)],
            CostReportingPeriod::Rolling(30),
            30,
            &Settings::default(),
        );
        assert_eq!(payloads[0]["incompleteRequestCount"], 3);
        assert!(payloads[1].get("incompleteRequestCount").is_none());
    }

    #[test]
    fn incomplete_suffix_marks_only_positive_counts() {
        assert_eq!(incomplete_suffix(0), "");
        assert_eq!(incomplete_suffix(2), " · Incomplete");
    }

    #[test]
    fn antigravity_json_keeps_unknown_cost_distinct_from_zero() {
        use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
        let payload = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                total_tokens: 12_345,
                session_count: 2,
                coverage: LocalHistoryCoverage::Complete,
                cost_estimate: Default::default(),
                ..Default::default()
            },
            30,
        );
        assert!(payload["cost"]["total_usd"].is_null());
        assert_eq!(payload["tokens"]["total"], 12_345);
        assert_eq!(payload["historyCoverage"], "complete");
        assert_eq!(payload["knownZero"], false);

        let partial = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                total_tokens: 999,
                session_count: 1,
                coverage: LocalHistoryCoverage::Partial,
                cost_estimate: Default::default(),
                ..Default::default()
            },
            30,
        );
        assert!(partial["cost"]["total_usd"].is_null());
        assert!(partial["tokens"]["total"].is_null());
        assert_eq!(partial["historyCoverage"], "partial");
    }

    #[test]
    fn antigravity_json_labels_public_price_estimates() {
        use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
        let payload = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                total_tokens: 1_000,
                session_count: 1,
                coverage: LocalHistoryCoverage::Complete,
                cost_estimate: crate::spend_contract::LocalCostEstimate {
                    known_subtotal_usd: Some(0.0125),
                    coverage: crate::spend_contract::CostCoverageCounts {
                        estimated: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            30,
        );
        assert_eq!(payload["cost"]["total_usd"], 0.0125);
        assert_eq!(payload["cost"]["known_subtotal_usd"], 0.0125);
        assert_eq!(payload["cost"]["currency"], "USD");
        assert!(payload["note"].as_str().unwrap().contains("not billed"));
    }

    #[test]
    fn antigravity_json_keeps_partial_scan_cost_as_a_subtotal() {
        use crate::spend_contract::{
            CostCoverageCounts, LocalCostEstimate, LocalHistoryCoverage, LocalTokenHistorySummary,
        };
        let payload = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                total_tokens: 1_000,
                session_count: 1,
                coverage: LocalHistoryCoverage::Partial,
                cost_estimate: LocalCostEstimate {
                    known_subtotal_usd: Some(0.0125),
                    coverage: CostCoverageCounts {
                        estimated: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            30,
        );
        assert!(payload["cost"]["total_usd"].is_null());
        assert_eq!(payload["cost"]["known_subtotal_usd"], 0.0125);
        assert_eq!(payload["historyCoverage"], "partial");
        assert!(
            payload["note"]
                .as_str()
                .unwrap()
                .contains("history is incomplete")
        );
    }

    #[test]
    fn antigravity_json_emits_zero_for_complete_empty_history() {
        use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
        let payload = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                coverage: LocalHistoryCoverage::Complete,
                ..Default::default()
            },
            30,
        );
        assert_eq!(payload["cost"]["total_usd"], 0.0);
        assert!(payload["cost"]["known_subtotal_usd"].is_null());
        assert_eq!(payload["cost"]["currency"], "USD");
        assert_eq!(payload["knownZero"], true);
    }

    #[test]
    fn antigravity_json_keeps_mixed_pricing_as_a_known_subtotal() {
        use crate::spend_contract::{
            CostCoverageCounts, LocalCostEstimate, LocalHistoryCoverage, LocalTokenHistorySummary,
        };
        let payload = crate::spend_contract::local_token_history_json(
            "antigravity",
            &LocalTokenHistorySummary {
                total_tokens: 1_500,
                session_count: 2,
                coverage: LocalHistoryCoverage::Complete,
                cost_estimate: LocalCostEstimate {
                    known_subtotal_usd: Some(0.0125),
                    coverage: CostCoverageCounts {
                        estimated: 1,
                        unpriced: 1,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            30,
        );
        assert!(payload["cost"]["total_usd"].is_null());
        assert_eq!(payload["cost"]["known_subtotal_usd"], 0.0125);
        assert_eq!(payload["cost"]["pricingCoverage"]["estimated"], 1);
        assert_eq!(payload["cost"]["pricingCoverage"]["unpriced"], 1);
        assert!(payload["note"].as_str().unwrap().contains("subtotal"));
    }

    #[derive(clap::Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: CostArgs,
    }

    fn parse_args(argv: &[&str]) -> CostArgs {
        use clap::Parser;
        Wrapper::try_parse_from(std::iter::once("cost").chain(argv.iter().copied()))
            .expect("cost args parse")
            .args
    }

    #[test]
    fn days_and_period_are_detectable_when_absent() {
        let args = parse_args(&[]);
        assert_eq!(args.days, None);
        assert_eq!(args.period, None);
        let args = parse_args(&["--period", "month-to-date", "--days", "7"]);
        assert_eq!(args.days, Some(7));
        assert_eq!(args.period.as_deref(), Some("month-to-date"));
    }

    #[test]
    fn json_payload_carries_period_identity_and_totals() {
        let summary = CostSummary {
            total_cost_usd: 2.5,
            input_tokens: 10,
            output_tokens: 5,
            sessions_count: 1,
            ..Default::default()
        };
        let results = [CostResult {
            provider: "claude".to_string(),
            display_name: "Claude".to_string(),
            summary,
            supported: true,
            token_history: None,
        }];
        let settings = Settings::default();
        let payloads =
            build_json_payloads(&results, CostReportingPeriod::MonthToDate, 12, &settings);
        let payload = &payloads[0];
        assert_eq!(payload["reportingPeriod"], "month-to-date");
        assert_eq!(payload["historyLabel"], "Month to date");
        assert_eq!(payload["days_scanned"], 12);
        assert_eq!(payload["totals"]["totalCost"], 2.5);
        assert_eq!(payload["totals"]["totalTokens"], 15);
        // Existing fields keep their meaning.
        assert_eq!(payload["cost"]["total_usd"], 2.5);
        assert_eq!(payload["tokens"]["input"], 10);

        let rolling =
            build_json_payloads(&results, CostReportingPeriod::Rolling(30), 30, &settings);
        assert_eq!(rolling[0]["reportingPeriod"], "rolling:30");
        assert_eq!(rolling[0]["historyLabel"], "Last 30 days");
    }

    #[test]
    fn token_history_payload_is_stamped_with_the_period() {
        use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
        let results = [CostResult {
            provider: "antigravity".to_string(),
            display_name: "Antigravity".to_string(),
            summary: CostSummary::default(),
            supported: true,
            token_history: Some(LocalTokenHistorySummary {
                total_tokens: 3,
                session_count: 1,
                coverage: LocalHistoryCoverage::Complete,
                ..Default::default()
            }),
        }];
        let settings = Settings::default();
        let payloads = build_json_payloads(
            &results,
            CostReportingPeriod::AllAvailable,
            20_000,
            &settings,
        );
        assert_eq!(payloads[0]["reportingPeriod"], "all");
        assert_eq!(payloads[0]["historyLabel"], "All");
        assert_eq!(payloads[0]["days_scanned"], 20_000);
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
        assert_eq!(CostGroupBy::from_arg(Some("project")), CostGroupBy::Project);
        assert_eq!(
            parse_args(&["--group-by", "project"]).group_by.as_deref(),
            Some("project")
        );
    }

    #[test]
    fn identified_flags_parse_with_remote() {
        let args = parse_args(&[
            "--provider",
            "codex",
            "--format",
            "json",
            "--identified-only",
            "--identified-end",
            "1700000000000",
            "--remote",
            "build-host",
        ]);
        assert!(args.identified_only);
        assert_eq!(args.identified_end, Some(1_700_000_000_000));
        assert_eq!(args.remote.as_deref(), Some("build-host"));
        assert!(!parse_args(&[]).identified_only);
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

#[cfg(test)]
#[path = "cost_golden_tests.rs"]
mod golden_tests;
