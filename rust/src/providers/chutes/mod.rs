use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const CREDENTIAL_TARGET: &str = "codexbar-chutes";

pub struct ChutesProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ChutesProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Chutes,
                display_name: "Chutes",
                session_label: "4-hour quota",
                weekly_label: "Monthly quota",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://chutes.ai"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for ChutesProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ChutesProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Chutes
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    &["CHUTES_API_KEY"],
                )?;
                let base = std::env::var("CHUTES_API_URL")
                    .unwrap_or_else(|_| "https://api.chutes.ai".into());
                let url = crate::providers::validated_https_url(&base, "Chutes API")?
                    .join("users/me/subscription_usage")
                    .map_err(|e| ProviderError::Other(format!("Invalid Chutes API URL: {e}")))?;
                let response = self.client.get(url).bearer_auth(key).send().await?;
                if response.status() == reqwest::StatusCode::UNAUTHORIZED
                    || response.status() == reqwest::StatusCode::FORBIDDEN
                {
                    return Err(ProviderError::AuthRequired);
                }
                if !response.status().is_success() {
                    return Err(ProviderError::Other(format!(
                        "Chutes usage returned status {}",
                        response.status()
                    )));
                }
                let value: Value = response.json().await.map_err(|e| {
                    ProviderError::Parse(format!("Failed to parse Chutes usage: {e}"))
                })?;
                Ok(ProviderFetchResult::new(snapshot_from_usage(&value), "api"))
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn snapshot_from_usage(value: &Value) -> UsageSnapshot {
    let windows = quota_windows(value);
    let primary = windows
        .first()
        .cloned()
        .unwrap_or_else(|| RateWindow::new(0.0));
    let mut snapshot = UsageSnapshot::new(primary);
    if let Some(second) = windows.get(1).cloned() {
        snapshot = snapshot.with_secondary(second);
    }
    snapshot.with_login_method("Chutes API")
}

fn quota_windows(value: &Value) -> Vec<RateWindow> {
    let mut out = Vec::new();
    collect_windows(value, &mut out);
    out
}

fn collect_windows(value: &Value, out: &mut Vec<RateWindow>) {
    match value {
        Value::Object(map) => {
            if let Some(window) = window_from_object(map) {
                out.push(window);
            }
            for value in map.values() {
                collect_windows(value, out);
            }
        }
        Value::Array(items) => {
            for value in items {
                collect_windows(value, out);
            }
        }
        _ => {}
    }
}

/// Build a rate window from a quota object.
///
/// Quota counts (`used`/`limit`) go into `reset_description` as detail text
/// (e.g. `"25/100 credits"`). They are never treated as reset schedules.
fn window_from_object(map: &serde_json::Map<String, Value>) -> Option<RateWindow> {
    let percent = percent_from_object(map)?;
    let detail = quota_count_description(map);
    let window_minutes = window_minutes_from_object(map);
    // Only parse dedicated reset timestamp keys — never raw quota counts.
    let resets_at = first_reset_timestamp(map);
    Some(RateWindow::with_details(
        percent,
        window_minutes,
        resets_at,
        detail,
    ))
}

fn percent_from_object(map: &serde_json::Map<String, Value>) -> Option<f64> {
    for key in [
        "usage_percent",
        "usagePercent",
        "percent_used",
        "percentUsed",
    ] {
        let Some(v) = map.get(key).and_then(Value::as_f64) else {
            continue;
        };
        // Chutes usage/quota payloads (GET /users/me/subscription_usage, and
        // whatever `collect_windows` recurses into) carry whole percent values
        // in 0..=100 — no documented field is a 0..=1 fraction. Rescaling
        // 0..=1 as fractions turned a real 1% into a false 100% exhausted
        // state (#408; same class as #247 / upstream #3216, fixed for
        // opencodego in #407).
        if v.is_finite() {
            return Some(v.clamp(0.0, 100.0));
        }
    }
    let used = first_f64(map, &["used", "usage", "current_usage", "currentUsage"]);
    let limit = first_f64(
        map,
        &["limit", "quota", "quota_limit", "quotaLimit", "total"],
    );
    let remaining = first_f64(map, &["remaining", "remaining_quota", "remainingQuota"]);
    match (used, limit, remaining) {
        (Some(used), Some(limit), _) if limit > 0.0 => {
            let percent = (used / limit) * 100.0;
            percent.is_finite().then_some(percent.clamp(0.0, 100.0))
        }
        (None, Some(limit), Some(remaining)) if limit > 0.0 => {
            let percent = ((limit - remaining).max(0.0) / limit) * 100.0;
            percent.is_finite().then_some(percent.clamp(0.0, 100.0))
        }
        (Some(used), None, Some(remaining)) => {
            let limit = used + remaining;
            if limit <= 0.0 {
                return None;
            }
            let percent = (used / limit) * 100.0;
            percent.is_finite().then_some(percent.clamp(0.0, 100.0))
        }
        _ => None,
    }
}

fn quota_count_description(map: &serde_json::Map<String, Value>) -> Option<String> {
    let used = first_f64(map, &["used", "usage", "current_usage", "currentUsage"]);
    let mut limit = first_f64(
        map,
        &["limit", "quota", "quota_limit", "quotaLimit", "total"],
    );
    let remaining = first_f64(map, &["remaining", "remaining_quota", "remainingQuota"]);

    if limit.is_none()
        && let (Some(used), Some(remaining)) = (used, remaining)
    {
        limit = Some(used + remaining);
    }
    let limit = limit.filter(|l| l.is_finite() && *l > 0.0)?;
    let used = match used {
        Some(u) => u,
        None => remaining.map(|r| (limit - r).max(0.0))?,
    };
    let unit = first_str(map, &["unit", "units", "quota_unit", "quotaUnit"]).unwrap_or("credits");
    Some(format!(
        "{}/{} {}",
        format_quota_amount(used),
        format_quota_amount(limit),
        unit
    ))
}

fn window_minutes_from_object(map: &serde_json::Map<String, Value>) -> Option<u32> {
    for (keys, multiplier) in [
        (
            [
                "window_minutes",
                "windowMinutes",
                "period_minutes",
                "periodMinutes",
                "duration_minutes",
                "durationMinutes",
            ]
            .as_slice(),
            1.0,
        ),
        (
            [
                "window_hours",
                "windowHours",
                "period_hours",
                "periodHours",
                "duration_hours",
                "durationHours",
            ]
            .as_slice(),
            60.0,
        ),
        (
            [
                "window_days",
                "windowDays",
                "period_days",
                "periodDays",
                "duration_days",
                "durationDays",
            ]
            .as_slice(),
            24.0 * 60.0,
        ),
        (
            [
                "window_seconds",
                "windowSeconds",
                "period_seconds",
                "periodSeconds",
                "duration_seconds",
                "durationSeconds",
            ]
            .as_slice(),
            1.0 / 60.0,
        ),
    ] {
        if let Some(minutes) = keys.iter().find_map(|key| {
            map.get(*key)
                .and_then(numeric_value)
                .and_then(|value| rounded_window_minutes(value * multiplier))
        }) {
            return Some(minutes);
        }
    }

    ["window", "period", "interval", "duration"]
        .iter()
        .find_map(|key| map.get(*key).and_then(Value::as_str))
        .and_then(parse_window_duration_text)
}

fn numeric_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite()),
        _ => None,
    }
}

fn rounded_window_minutes(value: f64) -> Option<u32> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let rounded = value.round();
    if rounded <= 0.0 || rounded > u32::MAX as f64 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "rounded value is bounded by u32::MAX"
    )]
    Some(rounded as u32)
}

fn parse_window_duration_text(raw: &str) -> Option<u32> {
    let compact: String = raw
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let split_at = compact.find(|character: char| {
        !character.is_ascii_digit() && !matches!(character, '.' | '+' | '-' | 'e' | 'E')
    })?;
    let (number, suffix) = compact.split_at(split_at);
    let value = number.parse::<f64>().ok()?;
    let multiplier = if suffix.starts_with("min") || suffix == "m" {
        1.0
    } else if suffix.starts_with("hour") || suffix.starts_with("hr") || suffix == "h" {
        60.0
    } else if suffix.starts_with("day") || suffix == "d" {
        24.0 * 60.0
    } else if suffix.starts_with("month") || suffix == "mo" {
        30.0 * 24.0 * 60.0
    } else {
        return None;
    };
    rounded_window_minutes(value * multiplier)
}

fn first_reset_timestamp(map: &serde_json::Map<String, Value>) -> Option<DateTime<Utc>> {
    for key in [
        "resets_at",
        "resetsAt",
        "reset_at",
        "resetAt",
        "reset_time",
        "resetTime",
        "renews_at",
        "renewsAt",
    ] {
        if let Some(dt) = map.get(key).and_then(parse_timestamp_value) {
            return Some(dt);
        }
    }
    None
}

fn parse_timestamp_value(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(raw) => {
            let text = raw.trim();
            if text.is_empty() {
                return None;
            }
            // ISO-8601 / RFC3339 only for strings that look like dates.
            // Bare small numbers as strings are quota counts, not schedules.
            if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
                return Some(dt.with_timezone(&Utc));
            }
            if let Ok(number) = text.parse::<f64>() {
                return epoch_to_datetime(number);
            }
            None
        }
        Value::Number(n) => n.as_f64().and_then(epoch_to_datetime),
        _ => None,
    }
}

fn epoch_to_datetime(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    // Reject small integers that are clearly quota counts, not unix epochs.
    // Unix seconds ~1e9; ms ~1e12. Quota counts are typically << 1e8.
    if value < 1_000_000_000.0 {
        return None;
    }
    let seconds = if value > 10_000_000_000.0 {
        value / 1000.0
    } else {
        value
    };
    // Epoch seconds; the sub-second fraction is below timestamp resolution.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "epoch seconds; sub-second fraction below timestamp resolution"
    )]
    let whole_seconds = seconds as i64;
    Utc.timestamp_opt(whole_seconds, 0).single()
}

fn first_f64(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| {
        map.get(*k)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
    })
}

fn first_str<'a>(map: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| map.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn format_quota_amount(value: f64) -> String {
    if !value.is_finite() {
        return "unknown".to_string();
    }
    let rounded = value.round();
    if (value - rounded).abs() < 0.0001 && rounded >= i64::MIN as f64 && rounded < i64::MAX as f64 {
        // Guarded above: value is within 0.0001 of a whole number, so the
        // fractional part is zero and the rounded value fits in i64.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "finite rounded value is bounded to the i64 range above"
        )]
        let whole = rounded as i64;
        format!("{}", whole)
    } else {
        let mut text = format!("{value:.2}");
        while text.contains('.') && text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ratio_window() {
        let snapshot =
            snapshot_from_usage(&serde_json::json!({"quotas":[{"used":25,"limit":100}]}));
        assert_eq!(snapshot.primary.used_percent, 25.0);
    }

    #[test]
    fn quota_counts_go_to_reset_description_not_schedule() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "quotas": [{
                "used": 25,
                "limit": 100,
                "unit": "credits",
                "remaining": 75
            }]
        }));
        assert_eq!(snapshot.primary.used_percent, 25.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("25/100 credits")
        );
        assert!(snapshot.primary.resets_at.is_none());
    }

    #[test]
    fn small_numeric_fields_are_not_reset_schedules() {
        // A mis-keyed payload where "reset_time" is accidentally a remaining count.
        // Values below unix-epoch range must not become resets_at.
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "quotas": [{
                "used": 10,
                "limit": 50,
                "reset_time": 40
            }]
        }));
        assert_eq!(snapshot.primary.used_percent, 20.0);
        assert!(snapshot.primary.resets_at.is_none());
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("10/50 credits")
        );
    }

    #[test]
    fn iso_reset_timestamps_still_parse() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "quotas": [{
                "used": 1,
                "limit": 2,
                "resets_at": "2026-08-01T00:00:00Z"
            }]
        }));
        assert_eq!(
            snapshot.primary.resets_at,
            Some(Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn usage_percent_one_stays_one() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "quotas": [{"usage_percent": 1, "limit": 100}]
        }));
        assert_eq!(snapshot.primary.used_percent, 1.0);
    }

    #[test]
    fn usage_percent_half_stays_half() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "quotas": [{"usage_percent": 0.5, "limit": 100}]
        }));
        assert_eq!(snapshot.primary.used_percent, 0.5);
    }

    #[test]
    fn usage_percent_hundred_stays_hundred() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "quotas": [{"usage_percent": 100, "limit": 100}]
        }));
        assert_eq!(snapshot.primary.used_percent, 100.0);
    }

    #[test]
    fn large_quota_amounts_keep_their_description() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "rolling_window": {"used": 1e20, "limit": 2e20, "unit": "credits"}
        }));
        assert_eq!(snapshot.primary.used_percent, 50.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("100000000000000000000/200000000000000000000 credits")
        );
    }

    #[test]
    fn duration_fields_populate_window_minutes() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "quotas": [
                {"used": 25, "limit": 100, "duration": "4 hours"},
                {"used": 1, "limit": 2, "window_seconds": 1800}
            ]
        }));
        assert_eq!(snapshot.primary.window_minutes, Some(240));
        assert_eq!(
            snapshot.secondary.as_ref().unwrap().window_minutes,
            Some(30)
        );
    }

    #[test]
    fn unrepresentable_duration_keeps_usage_with_unknown_window() {
        let snapshot = snapshot_from_usage(&serde_json::json!({
            "rolling_window": {
                "used": 25,
                "limit": 100,
                "window_hours": "1e308"
            }
        }));
        assert_eq!(snapshot.primary.used_percent, 25.0);
        assert_eq!(snapshot.primary.window_minutes, None);
    }

    #[test]
    fn non_finite_amount_formatting_is_safe() {
        assert_eq!(format_quota_amount(f64::INFINITY), "unknown");
        assert_eq!(format_quota_amount(f64::NAN), "unknown");
    }

    #[test]
    fn oversized_integral_amount_is_not_saturated_to_i64_max() {
        let value = 2_f64.powi(63);

        assert_eq!(format_quota_amount(value), "9223372036854775808");
    }
}
