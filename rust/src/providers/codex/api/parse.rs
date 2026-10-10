use super::CodexApi;
use crate::core::{
    CostSnapshot, NamedRateWindow, ProviderError, RateWindow, RateWindowCadence, UsageSnapshot,
};
use chrono::{DateTime, TimeZone, Utc};

impl CodexApi {
    pub(super) fn build_result_from_json(
        &self,
        json: &serde_json::Value,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
        // Extract plan type
        let plan_type = json
            .get("plan_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract rate limit info - handle multiple possible structures
        let (primary, secondary, monthly, code_review, code_review_verified) =
            self.extract_rate_limits(json);

        // Build login method string
        let login_method = plan_type.as_deref().map(format_plan_type);

        let mut usage = UsageSnapshot::new(primary);
        if let Some(sec) = secondary {
            usage = usage.with_secondary(sec);
        }
        // F5 (upstream 0.48.0): monthly (30-day) windows go to tertiary so the
        // bridge and frontend can show a monthly reset instead of swallowing it.
        if let Some(mo) = monthly {
            usage = usage.with_tertiary(mo);
        }
        if let Some(cr) = code_review {
            usage = if code_review_verified {
                usage.with_code_review(cr)
            } else {
                usage.with_model_specific(cr)
            };
        }
        for extra in self.extract_additional_rate_limits(json) {
            usage.extra_rate_windows.push(extra);
        }
        if let Some(method) = login_method {
            usage = usage.with_login_method(method);
        }

        // Extract credits if present
        let cost = self.extract_credits(json);

        Ok((usage, cost))
    }

    #[cfg(test)]
    pub(crate) fn build_result_from_json_for_test(
        &self,
        json: &serde_json::Value,
    ) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
        self.build_result_from_json(json)
    }

    fn extract_rate_limits(
        &self,
        json: &serde_json::Value,
    ) -> (
        RateWindow,
        Option<RateWindow>,
        Option<RateWindow>,
        Option<RateWindow>,
        bool,
    ) {
        // Try rate_limit object
        if let Some(rate_limit) = json.get("rate_limit") {
            let primary_opt = rate_limit
                .get("primary_window")
                .and_then(|w| self.parse_window_if_present(w));

            let secondary_opt = rate_limit
                .get("secondary_window")
                .and_then(|w| self.parse_window_if_present(w));

            let code_review = rate_limit
                .get("code_review_window")
                .and_then(|w| self.parse_window_if_present(w));

            let (primary, secondary) = normalize_named_windows(primary_opt, secondary_opt);

            // F5 (upstream 0.48.0): named windows carry only session/weekly/code_review.
            // Monthly is extracted separately (from array windows) — return None here.
            let code_review_verified = code_review.is_some();
            return (primary, secondary, None, code_review, code_review_verified);
        }

        // Try rate_limits array
        if let Some(rate_limits) = json.get("rate_limits").and_then(|v| v.as_array()) {
            let windows = rate_limits
                .iter()
                .filter_map(|window| self.parse_window_if_present(window))
                .collect::<Vec<_>>();
            let (primary, secondary, monthly, code_review) = normalize_array_windows(windows);
            // F5 (upstream 0.48.0): route monthly to its own tertiary lane.
            return (primary, secondary, monthly, code_review, false);
        }

        // Try direct fields
        let used_percent = json
            .get("used_percent")
            .or_else(|| json.get("usage_percent"))
            .and_then(json_f64);
        let primary = RateWindow::new(used_percent.unwrap_or(0.0))
            .with_usage_known(valid_used_percent(used_percent));

        (primary, None, None, None, false)
    }

    fn parse_window(&self, window: &serde_json::Value) -> RateWindow {
        let used_percent = window
            .get("used_percent")
            .or_else(|| window.get("usage_percent"))
            .and_then(json_f64);

        let window_minutes = window
            .get("limit_window_seconds")
            .and_then(json_i64)
            .and_then(|seconds| u32::try_from(seconds / 60).ok());

        let reset_at = window
            .get("reset_at")
            .and_then(json_i64)
            .and_then(|ts| Utc.timestamp_opt(ts, 0).single());

        RateWindow::with_details(
            used_percent.unwrap_or(0.0),
            window_minutes,
            reset_at,
            format_reset_countdown(reset_at),
        )
        .with_usage_known(valid_used_percent(used_percent))
    }

    fn parse_window_if_present(&self, window: &serde_json::Value) -> Option<RateWindow> {
        (!window.is_null() && !is_placeholder_window(window)).then(|| self.parse_window(window))
    }

    fn extract_additional_rate_limits(&self, json: &serde_json::Value) -> Vec<NamedRateWindow> {
        json.get("additional_rate_limits")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|entry| self.parse_additional_rate_limit(entry))
            .collect()
    }

    fn parse_additional_rate_limit(&self, entry: &serde_json::Value) -> Option<NamedRateWindow> {
        let metered_feature = entry
            .get("metered_feature")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let limit_name = entry
            .get("limit_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty());

        let rate_limit = entry.get("rate_limit").unwrap_or(entry);
        let primary = rate_limit.get("primary_window");
        let secondary = rate_limit.get("secondary_window");
        let window = primary.or(secondary)?;
        if is_placeholder_window(window) {
            return None;
        }

        let parsed = self.parse_window(window);
        let feature = metered_feature.unwrap_or_default();
        let limit = limit_name.unwrap_or_default();
        let is_spark = feature.eq_ignore_ascii_case("codex_spark")
            || feature.eq_ignore_ascii_case("spark")
            || limit.to_ascii_lowercase().contains("spark");

        if is_spark {
            let is_weekly = secondary.is_some() && primary.is_none()
                || parsed
                    .window_minutes
                    .is_some_and(|mins| mins >= 7 * 24 * 60);
            let (id, title) = if is_weekly {
                ("codex-spark-weekly", "Codex Spark Weekly")
            } else {
                ("codex-spark", "Codex Spark 5-hour")
            };
            return Some(NamedRateWindow::new(id, title, parsed));
        }

        let label = limit_name.or(metered_feature)?;
        let slug = slugify(label);
        if slug.is_empty() {
            return None;
        }

        Some(NamedRateWindow::new(
            format!("codex-{slug}"),
            titleize_limit_label(label),
            parsed,
        ))
    }

    fn extract_credits(&self, json: &serde_json::Value) -> Option<CostSnapshot> {
        let credits = json.get("credits")?;

        let has_credits = credits
            .get("has_credits")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !has_credits {
            return None;
        }

        let unlimited = credits
            .get("unlimited")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if unlimited {
            return None;
        }

        let balance = credits
            .get("balance")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        Some(CostSnapshot::new(balance, "USD", "Credits"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexWindowRole {
    Session,
    Weekly,
    Monthly,
    Unknown,
}

fn codex_window_role(window: &RateWindow) -> CodexWindowRole {
    match window
        .window_minutes
        .map(RateWindowCadence::from_minutes)
        .unwrap_or(RateWindowCadence::Unknown)
    {
        RateWindowCadence::Session => CodexWindowRole::Session,
        RateWindowCadence::Monthly => CodexWindowRole::Monthly,
        RateWindowCadence::Weekly => CodexWindowRole::Weekly,
        RateWindowCadence::Unknown => CodexWindowRole::Unknown,
    }
}

/// Normalize the named `primary_window`/`secondary_window` fields by duration.
pub(super) fn normalize_named_windows(
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
) -> (RateWindow, Option<RateWindow>) {
    match (primary, secondary) {
        (None, None) => (RateWindow::no_active_session(), None),
        (Some(window), None) | (None, Some(window)) => {
            if codex_window_role(&window) == CodexWindowRole::Weekly {
                (RateWindow::no_active_session(), Some(window))
            } else {
                (window, None)
            }
        }
        (Some(primary), Some(secondary)) => {
            match (codex_window_role(&primary), codex_window_role(&secondary)) {
                (CodexWindowRole::Weekly, CodexWindowRole::Session) => (secondary, Some(primary)),
                (CodexWindowRole::Weekly, CodexWindowRole::Unknown) => {
                    (RateWindow::no_active_session(), Some(primary))
                }
                (CodexWindowRole::Unknown, CodexWindowRole::Session) => (secondary, Some(primary)),
                _ => (primary, Some(secondary)),
            }
        }
    }
}

/// Normalize an array of Codex windows without relying on the API's ordering.
///
/// Returns (session, weekly, monthly, code_review). F5 (upstream 0.48.0):
/// monthly (30-day) windows are routed to their own lane so surfaces can
/// display a monthly reset instead of swallowing it into the weekly label.
pub(super) fn normalize_array_windows(
    windows: Vec<RateWindow>,
) -> (
    RateWindow,
    Option<RateWindow>,
    Option<RateWindow>,
    Option<RateWindow>,
) {
    if windows.is_empty() {
        return (RateWindow::no_active_session(), None, None, None);
    }

    // Preserve the old positional fallback when the API provides no role
    // metadata at all. There is no safe way to infer session vs weekly then.
    if !windows
        .iter()
        .any(|window| codex_window_role(window) != CodexWindowRole::Unknown)
    {
        let mut windows = windows.into_iter();
        return (
            windows.next().unwrap_or_else(RateWindow::no_active_session),
            windows.next(),
            windows.next(),
            windows.next(),
        );
    }

    let mut session = None;
    let mut weekly = None;
    let mut monthly = None;
    let mut remaining = Vec::new();

    for window in windows {
        match codex_window_role(&window) {
            CodexWindowRole::Session if session.is_none() => session = Some(window),
            CodexWindowRole::Weekly if weekly.is_none() => weekly = Some(window),
            CodexWindowRole::Monthly if monthly.is_none() => monthly = Some(window),
            _ => remaining.push(window),
        }
    }

    (
        session.unwrap_or_else(RateWindow::no_active_session),
        weekly,
        monthly,
        remaining.into_iter().next(),
    )
}

pub(super) fn format_plan_type(plan_type: &str) -> String {
    match plan_type {
        "guest" => "Guest".to_string(),
        "free" => "ChatGPT Free".to_string(),
        "go" => "Codex Go".to_string(),
        "plus" => "ChatGPT Plus".to_string(),
        "pro" => "ChatGPT Pro".to_string(),
        "pro_lite" | "prolite" | "pro-lite" => "Pro Lite".to_string(),
        "team" => "ChatGPT Team".to_string(),
        "business" => "ChatGPT Business".to_string(),
        "enterprise" => "ChatGPT Enterprise".to_string(),
        "education" | "edu" => "ChatGPT Education".to_string(),
        "free_workspace" | "freeWorkspace" => "Free Workspace".to_string(),
        "quorum" => "Codex Quorum".to_string(),
        "k12" => "Codex K12".to_string(),
        other => format!("ChatGPT {}", capitalize(other)),
    }
}

fn json_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn json_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

fn valid_used_percent(value: Option<f64>) -> bool {
    value.is_some_and(|value| value.is_finite() && (0.0..=100.0).contains(&value))
}

fn is_placeholder_window(window: &serde_json::Value) -> bool {
    let has_usage = window
        .get("used_percent")
        .or_else(|| window.get("usage_percent"))
        .and_then(json_f64)
        .is_some();
    let has_duration = window
        .get("limit_window_seconds")
        .and_then(json_i64)
        .is_some();
    let has_reset = window.get("reset_at").and_then(json_i64).is_some();

    !has_usage && !has_duration && !has_reset
}

fn slugify(label: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;

    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn titleize_limit_label(label: &str) -> String {
    label
        .split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn format_reset_countdown(reset_at: Option<DateTime<Utc>>) -> Option<String> {
    let dt = reset_at?;
    let now = Utc::now();
    if dt <= now {
        return Some("now".to_string());
    }
    let diff = dt - now;
    let total_mins = diff.num_minutes();
    let hours = diff.num_hours();
    let mins = total_mins % 60;
    if hours >= 24 {
        let days = hours / 24;
        let rem_h = hours % 24;
        if rem_h == 0 {
            Some(format!("{}d", days))
        } else {
            Some(format!("{}d {}h", days, rem_h))
        }
    } else if hours > 0 {
        if mins == 0 {
            Some(format!("{}h", hours))
        } else {
            Some(format!("{}h {}m", hours, mins))
        }
    } else {
        Some(format!("{}m", mins))
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}
