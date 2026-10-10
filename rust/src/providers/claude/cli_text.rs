use super::cli_reset::{label_section, normalized_for_label_search, parse_percent_line};
use regex_lite::Regex;

pub(super) fn is_non_interactive_slash_command_response(text: &str) -> bool {
    let mentions_usage_and_exit = text.contains("/usage") && text.contains("/exit");
    let says_entered_commands =
        text.contains("i see you've entered") || text.contains("you've entered two slash commands");
    let says_no_slash_command = text.contains("available custom slash commands")
        && text.contains("don't see these commands");
    let says_usage_is_cli_only = text
        .contains("token usage and statistics are typically displayed by the cli interface")
        || text.contains("i don't have direct access to those metrics");

    mentions_usage_and_exit
        && (says_entered_commands || says_no_slash_command || says_usage_is_cli_only)
}

pub(super) fn is_workspace_trust_prompt(text: &str) -> bool {
    text.contains("quick safety check")
        && text.contains("trust this folder")
        && text.contains("yes, i trust this folder")
}

/// Current `/usage` panels show a local session summary above the plan limits,
/// so the presence of a limit section outranks the activity-stats markers.
pub(super) fn has_plan_limit_section(text: &str) -> bool {
    text.contains("current session") || text.contains("current week")
}

pub(super) fn is_cli_activity_stats_response(text: &str) -> bool {
    let has_activity_overview = text.contains("favorite model:") || text.contains("total tokens:");
    let has_session_cost_summary =
        text.contains("total duration") && text.contains("usage:") && text.contains("cache read");

    has_activity_overview || has_session_cost_summary
}

/// The weekly heading Claude prints, newest wording first.
pub(super) const WEEKLY_LABELS: [&str; 2] = ["current week (all models)", "current week"];

/// First match of `find` in any section headed by `label`; each section is
/// scanned for at most `max_lines` lines, label line included.
fn find_near_label<T>(
    text: &str,
    label: &str,
    max_lines: usize,
    mut find: impl FnMut(&str) -> Option<T>,
) -> Option<T> {
    let label_normalized = normalized_for_label_search(label);
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| normalized_for_label_search(line).contains(&label_normalized))
        .find_map(|(idx, _)| {
            label_section(&lines, idx, &label_normalized, max_lines).find_map(&mut find)
        })
}

/// Percentage near a label (e.g. "Current session"), as "used".
pub(super) fn extract_percent_near_label(text: &str, label: &str) -> Option<f64> {
    find_near_label(text, label, 12, parse_percent_line)
}

pub(super) fn is_exhausted_short_form(clean_lower: &str) -> bool {
    clean_lower.contains("out of extra usage") || clean_lower.contains("hit your limit")
}

/// Extract email address from text
pub(super) fn extract_email(text: &str) -> Option<String> {
    // Try explicit patterns first
    let patterns = [
        r"Account:\s*([^\s@]+@[^\s@]+\.[^\s]+)",
        r"Email:\s*([^\s@]+@[^\s@]+\.[^\s]+)",
        r"([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})",
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern)
            && let Some(caps) = re.captures(text)
            && let Some(m) = caps.get(1)
        {
            return Some(m.as_str().trim().to_string());
        }
    }

    None
}

/// Extract login method / plan name from text
pub(super) fn extract_login_method(text: &str) -> Option<String> {
    // Look for explicit "Login method:" line
    if let Ok(re) = Regex::new(r"(?i)login\s+method:\s*(.+)")
        && let Some(caps) = re.captures(text)
        && let Some(m) = caps.get(1)
    {
        let method = m.as_str().trim();
        if !method.is_empty() {
            return Some(clean_plan_name(method));
        }
    }

    // Look for "Claude <plan>" patterns
    if let Ok(re) = Regex::new(r"(?i)(claude\s+(?:max|pro|ultra|team|free)[a-z0-9\s._-]*)")
        && let Some(caps) = re.captures(text)
        && let Some(m) = caps.get(1)
    {
        let plan = m.as_str().trim();
        if !plan.to_lowercase().contains("code") {
            return Some(clean_plan_name(plan));
        }
    }

    None
}

/// Reset text near a label, from "resets" to the end of its line.
pub(super) fn extract_reset_description(text: &str, label: &str) -> Option<String> {
    find_near_label(text, label, 14, extract_inline_reset_description)
}

/// Extract a "resets ..." suffix from a short single-line status.
pub(super) fn extract_inline_reset_description(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let pos = lower.find("resets")?;
    Some(text[pos..].trim().to_string())
}

/// Clean up a plan name from rendered (escape-free) text: drop bracketed
/// codes like `[22m` and trim.
fn clean_plan_name(text: &str) -> String {
    let re = Regex::new(r"\[\d+m").unwrap_or_else(|_| Regex::new(".^").unwrap());
    let result = re.replace_all(text, "");
    result.trim().to_string()
}
