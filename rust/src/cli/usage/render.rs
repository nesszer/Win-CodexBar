//! Usage output rendering: text lines, brief lines, JSON projection.
//!
//! Fetch plumbing lives in super::fetch_helpers; this module owns how a
//! fetched ProviderFetchResult becomes terminal text or JSON output.

use chrono::Utc;

use super::UsageOutput;
use crate::core::{
    CostSnapshot, ProviderFetchResult, ProviderId, ProviderInventoryItem, RateWindow, UsagePace,
    UsageSnapshot, instantiate_provider,
};
use crate::status::{ProviderStatus as StatusInfo, StatusLevel};

pub fn render_text_error(provider_id: ProviderId, error_msg: &str, use_color: bool) -> String {
    let header = if use_color {
        format!("\x1b[1m{}\x1b[0m", provider_id.display_name())
    } else {
        provider_id.display_name().to_string()
    };
    format!("{}  Error: {}", header, error_msg)
}

pub fn render_json_result(
    provider_id: ProviderId,
    result: ProviderFetchResult,
    status: Option<&StatusInfo>,
) -> serde_json::Value {
    let usage = &result.usage;
    let primary_pace = usage
        .primary
        .window_minutes
        .is_some_and(|m| m == crate::core::SESSION_WINDOW_MINUTES)
        .then(|| UsagePace::weekly(&usage.primary, None, crate::core::SESSION_WINDOW_MINUTES))
        .flatten()
        .map(pace_json);
    let secondary_pace = usage
        .secondary
        .as_ref()
        .and_then(|w| UsagePace::weekly(w, None, w.window_minutes.unwrap_or(10080)))
        .map(pace_json);

    let mut json_result = serde_json::json!({
        "provider": provider_id.cli_name(),
        "source": result.source_label,
        "usage": result.usage,
        "cost": result.cost,
    });
    if primary_pace.is_some() || secondary_pace.is_some() {
        json_result["pace"] = serde_json::json!({
            "primary": primary_pace,
            "secondary": secondary_pace,
        });
    }

    if !result.inventory.is_empty() {
        json_result["inventory"] = serde_json::Value::Array(
            result
                .inventory
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "id": &item.id,
                        "title": &item.title,
                        "availableCount": item.available_count,
                        "nextExpiresAt": item.next_expires_at.map(|date| date.to_rfc3339()),
                    })
                })
                .collect(),
        );
    }

    if let Some(s) = status {
        json_result["status"] = serde_json::json!({
            "level": format!("{:?}", s.level).to_lowercase(),
            "description": s.description,
        });
    }

    json_result
}

/// Serialize a [`UsagePace`] into a compact JSON object for the `--json` output.
fn pace_json(pace: UsagePace) -> serde_json::Value {
    serde_json::json!({
        "stage": format!("{:?}", pace.stage).to_lowercase(),
        "deltaPercent": pace.delta_percent,
        "expectedUsedPercent": pace.expected_used_percent,
        "willLastToReset": pace.will_last_to_reset,
    })
}

pub(super) fn print_usage_output(output: UsageOutput) -> anyhow::Result<()> {
    match output {
        UsageOutput::Text(sections) => {
            println!("{}", sections.join("\n\n"));
        }
        UsageOutput::Json { results, pretty } => {
            let output = if pretty {
                serde_json::to_string_pretty(&results)?
            } else {
                serde_json::to_string(&results)?
            };
            println!("{}", output);
        }
        UsageOutput::Toon(results) => {
            println!(
                "{}",
                crate::cli::toon::encode(&serde_json::Value::Array(results))
            );
        }
    }

    Ok(())
}

/// Check if stdout is a terminal
pub(super) fn is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Render usage as text with optional status
pub fn render_text_with_status(
    provider: ProviderId,
    result: &ProviderFetchResult,
    status: Option<&StatusInfo>,
    use_color: bool,
) -> String {
    let mut lines = Vec::new();
    let metadata = instantiate_provider(provider).metadata().clone();

    lines.push(render_usage_header(provider, result, status, use_color));
    append_status_line(&mut lines, status);
    append_account_lines(&mut lines, &result.usage);
    append_usage_window_lines(&mut lines, &result.usage, &metadata, use_color);
    append_inventory_lines(&mut lines, &result.inventory);
    append_cost_line(&mut lines, result.cost.as_ref());

    lines.join("\n")
}

fn render_usage_header(
    provider: ProviderId,
    result: &ProviderFetchResult,
    status: Option<&StatusInfo>,
    use_color: bool,
) -> String {
    let status_indicator = render_status_indicator(status, use_color);
    if use_color {
        format!(
            "\x1b[1m{}\x1b[0m ({}){}",
            provider.display_name(),
            result.source_label,
            status_indicator
        )
    } else {
        format!(
            "{} ({}){}",
            provider.display_name(),
            result.source_label,
            status_indicator
        )
    }
}

pub fn render_status_indicator(status: Option<&StatusInfo>, use_color: bool) -> String {
    let Some(status) = status else {
        return String::new();
    };

    let (symbol, color) = match status.level {
        StatusLevel::Operational => ("●", "\x1b[32m"), // Green
        StatusLevel::Degraded => ("◐", "\x1b[33m"),    // Yellow
        StatusLevel::Partial => ("◑", "\x1b[33m"),     // Yellow
        StatusLevel::Major => ("○", "\x1b[31m"),       // Red
        StatusLevel::Unknown => ("?", "\x1b[90m"),     // Gray
    };

    if use_color {
        format!(" {}{}\x1b[0m", color, symbol)
    } else {
        format!(" {}", symbol)
    }
}

pub fn append_status_line(lines: &mut Vec<String>, status: Option<&StatusInfo>) {
    if let Some(s) = status
        && s.level != StatusLevel::Operational
        && s.level != StatusLevel::Unknown
    {
        lines.push(format!("  Status: {}", s.description));
    }
}

fn append_account_lines(lines: &mut Vec<String>, usage: &UsageSnapshot) {
    if let Some(ref email) = usage.account_email {
        lines.push(format!("  Account: {}", email));
    }
    if let Some(ref method) = usage.login_method {
        lines.push(format!("  Plan:    {}", method));
    }
}

fn append_usage_window_lines(
    lines: &mut Vec<String>,
    usage: &UsageSnapshot,
    metadata: &crate::core::ProviderMetadata,
    use_color: bool,
) {
    let primary_label = usage
        .primary_label
        .as_deref()
        .unwrap_or(metadata.session_label);
    append_window_line(lines, primary_label, &usage.primary, use_color);
    // Pace for primary windows whose provider published a common cadence.
    let pace_minutes = usage.primary.window_minutes.filter(|minutes| {
        *minutes == crate::core::SESSION_WINDOW_MINUTES
            || *minutes >= crate::core::WEEKLY_WINDOW_MINUTES
    });
    if let Some(minutes) = pace_minutes
        && let Some(pace) = UsagePace::weekly(&usage.primary, None, minutes)
    {
        lines.push(format!(
            "  Pace:    {} {}",
            pace.stage.emoji(),
            pace.format_status()
        ));
    }
    append_secondary_window_line(
        lines,
        usage.secondary.as_ref(),
        usage
            .secondary_label
            .as_deref()
            .unwrap_or(metadata.weekly_label),
        use_color,
    );
    append_model_specific_line(lines, usage.model_specific.as_ref(), use_color);
    // F5 (upstream 0.48.0): monthly (30-day) lane. Label by duration cadence.
    if let Some(tertiary) = usage.tertiary.as_ref() {
        let cadence =
            crate::core::RateWindowCadence::from_minutes(tertiary.window_minutes.unwrap_or(0));
        let label = match cadence {
            crate::core::RateWindowCadence::Monthly => "Monthly",
            _ => "Tertiary",
        };
        append_window_line(lines, label, tertiary, use_color);
    }
    for extra in &usage.extra_rate_windows {
        if extra.usage_known {
            append_window_line(lines, &extra.title, &extra.window, use_color);
        }
    }
}

fn append_inventory_lines(lines: &mut Vec<String>, inventory: &[ProviderInventoryItem]) {
    if inventory.is_empty() {
        return;
    }
    let now = Utc::now();
    for item in inventory {
        lines.push(format!(
            "  {}: {} available",
            item.title, item.available_count
        ));
        if let Some(expires_at) = item.next_expires_at {
            lines.push(format!(
                "    Next expires in {}",
                crate::core::format_countdown_until(expires_at, now)
            ));
        }
    }
}

fn append_window_line(lines: &mut Vec<String>, label: &str, window: &RateWindow, use_color: bool) {
    if window.is_informational {
        let description = window.reset_description.as_deref().unwrap_or("unavailable");
        lines.push(format!("  {:<8} {}", format!("{}:", label), description));
        return;
    }

    let bar = render_progress_bar(window.used_percent, 20, use_color);
    let reset = window
        .format_countdown()
        .map(|c| format!(" (resets in {})", c))
        .unwrap_or_default();
    lines.push(format!(
        "  {:<8} {} {} used{}",
        format!("{}:", label),
        bar,
        format_percent(window.used_percent),
        reset
    ));
}

fn append_secondary_window_line(
    lines: &mut Vec<String>,
    secondary: Option<&RateWindow>,
    label: &str,
    use_color: bool,
) {
    if let Some(secondary) = secondary {
        append_window_line(lines, label, secondary, use_color);
        let window_minutes = secondary.window_minutes.unwrap_or(10080);
        if let Some(pace) = UsagePace::weekly(secondary, None, window_minutes) {
            lines.push(format!(
                "  Pace:    {} {}",
                pace.stage.emoji(),
                pace.format_status()
            ));
        }
    }
}

fn append_model_specific_line(
    lines: &mut Vec<String>,
    model_specific: Option<&RateWindow>,
    use_color: bool,
) {
    if let Some(opus) = model_specific {
        let opus_bar = render_progress_bar(opus.used_percent, 20, use_color);
        lines.push(format!(
            "  Opus:    {} {} used",
            opus_bar,
            format_percent(opus.used_percent)
        ));
    }
}

pub fn render_brief_text(provider: ProviderId, result: &ProviderFetchResult) -> String {
    let metadata = instantiate_provider(provider).metadata().clone();
    let usage = &result.usage;
    let primary_label = usage
        .primary_label
        .as_deref()
        .unwrap_or(metadata.session_label);
    let mut parts = Vec::new();
    let reset = if usage.primary.is_informational {
        parts.push(format!("{primary_label} unavailable"));
        usage.secondary.as_ref().unwrap_or(&usage.primary)
    } else {
        parts.push(format!(
            "{} {}",
            primary_label,
            format_percent(usage.primary.used_percent)
        ));
        &usage.primary
    }
    .format_countdown()
    .unwrap_or_else(|| "n/a".to_string());
    if let Some(secondary) = &usage.secondary {
        parts.push(format!(
            "{} {}",
            usage
                .secondary_label
                .as_deref()
                .unwrap_or(metadata.weekly_label),
            format_percent(secondary.used_percent)
        ));
    }
    parts.push(format!("resets {reset}"));
    if let Some(plan) = &usage.login_method {
        parts.push(plan.clone());
    }
    format!("{}: {}", provider.display_name(), parts.join(", "))
}

pub fn format_percent(percent: f64) -> String {
    if !percent.is_finite() {
        "0%".to_string()
    } else if percent > 0.0 && percent < 1.0 {
        "<1%".to_string()
    } else {
        format!("{:.0}%", percent.clamp(0.0, 100.0))
    }
}

fn append_cost_line(lines: &mut Vec<String>, cost: Option<&CostSnapshot>) {
    let Some(cost) = cost else {
        return;
    };

    // Provider-supplied Activity history is a completed reporting window,
    // rather than the ordinary current-cost meter. Providers mark such
    // snapshots `always_visible`; keep their source period and known zero
    // visible in text output without adding a second generic cost line. The
    // daily points remain available in the JSON cost payload.
    if cost.limit.is_none() && cost.always_visible {
        lines.push(format!("  {}: {}", cost.period, cost.format_used()));
        return;
    }

    if let Some(limit) = cost.format_limit() {
        lines.push(format!(
            "  Cost:    {} / {} ({})",
            cost.format_used(),
            limit,
            cost.period
        ));
    } else {
        lines.push(format!(
            "  Cost:    {} ({})",
            cost.format_used(),
            cost.period
        ));
    }
}

/// Render usage as text (backwards compatible version)
pub fn render_text(provider: ProviderId, result: &ProviderFetchResult, use_color: bool) -> String {
    render_text_with_status(provider, result, None, use_color)
}

/// Render a text-based progress bar
fn render_progress_bar(percent: f64, width: usize, use_color: bool) -> String {
    let percent = if percent.is_finite() {
        percent.clamp(0.0, 100.0)
    } else {
        0.0
    };
    // percent is clamped to 0..=100, so the rounded product cannot exceed width.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "percent clamped to 0..=100, so the product cannot exceed width"
    )]
    let filled = ((percent / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

    if use_color {
        let color = if percent >= 90.0 {
            "\x1b[31m" // Red
        } else if percent >= 70.0 {
            "\x1b[33m" // Yellow
        } else {
            "\x1b[32m" // Green
        };
        format!("{}{}\x1b[0m", color, bar)
    } else {
        bar
    }
}
