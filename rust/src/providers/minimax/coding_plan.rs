//! MiniMax coding-plan parser — upstream-parity port of the cookie/web fetch.
//!
//! Parses the coding-plan page JSON and remains API response into a snapshot
//! that maps onto the shared `UsageSnapshot`/`RateWindow` model. HTML scrape
//! lives in `coding_plan_html`. See issue #246; upstream reference is
//! `steipete/CodexBar`' `MiniMaxUsageFetcher.swift`.

use chrono::{DateTime, Duration, FixedOffset, TimeZone, Utc};
use regex_lite::Regex;
use serde_json::Value;

use crate::core::ProviderError;

use super::{scalar_string, value_i64};
use crate::providers::json;

/// Parsed result of a coding-plan fetch (page JSON, remains API, or HTML scrape).
#[derive(Debug, Clone)]
pub(super) enum MiniMaxCodingPlanSnapshot {
    /// Multi-service shape: `data.services[]`.
    Services(Vec<ServiceRow>),
    /// Single-service shape: `model_remains[]` entries.
    Remains {
        plan_name: Option<String>,
        rows: Vec<RemainsRow>,
    },
    /// Visible-text HTML scrape fallback.
    Html {
        plan_name: Option<String>,
        available_prompts: Option<i64>,
        window_minutes: Option<u32>,
        used_percent: Option<f64>,
        resets_at: Option<DateTime<Utc>>,
    },
}

/// A row in the multi-service `data.services[]` shape.
#[derive(Debug, Clone)]
pub(super) struct ServiceRow {
    pub service_type: String,
    pub window_type: String,
    pub time_range: String,
    pub percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub reset_description: Option<String>,
}

/// A row in the single-service `model_remains[]` shape.
#[derive(Debug, Clone)]
pub(super) struct RemainsRow {
    pub service_type: String,
    pub model_name: String,
    pub window_type: String,
    pub percent: f64,
    pub window_minutes: Option<u32>,
    pub resets_at: Option<DateTime<Utc>>,
    pub reset_description: Option<String>,
    pub is_unlimited: bool,
    pub is_weekly: bool,
}

// ponytail: skipped from upstream — optional `Authorization: Bearer` on cookie
// requests (our cookie import has no token concept), `GroupId` remains query
// param (no group id in cookie mode), `pointsBalance`, `formatMiniMaxDateTimeRange`
// for non-weekly rows, IANA timezone names, host/env URL overrides
// (`MiniMaxSettingsReader`).

/// MiniMax console times are China Standard Time.
const CN_TZ: FixedOffset = FixedOffset::east_opt(8 * 3600).unwrap();

/// Look up a value trying snake_case then camelCase (upstream normalizes the
/// camelCase aliases onto the snake_case keys).
fn get_field<'a>(obj: &'a Value, snake: &str, camel: &str) -> Option<&'a Value> {
    obj.get(snake).or_else(|| obj.get(camel))
}

/// Snake/camel key pairs for one quota lane of a `model_remains[]` entry.
struct LaneKeys {
    total: [&'static str; 2],
    usage: [&'static str; 2],
    remaining_percent: [&'static str; 2],
    status: [&'static str; 2],
    start: [&'static str; 2],
    end: [&'static str; 2],
    remains: [&'static str; 2],
}

const INTERVAL_LANE: LaneKeys = LaneKeys {
    total: ["current_interval_total_count", "currentIntervalTotalCount"],
    usage: ["current_interval_usage_count", "currentIntervalUsageCount"],
    remaining_percent: [
        "current_interval_remaining_percent",
        "currentIntervalRemainingPercent",
    ],
    status: ["current_interval_status", "currentIntervalStatus"],
    start: ["start_time", "startTime"],
    end: ["end_time", "endTime"],
    remains: ["remains_time", "remainsTime"],
};

const WEEKLY_LANE: LaneKeys = LaneKeys {
    total: ["current_weekly_total_count", "currentWeeklyTotalCount"],
    usage: ["current_weekly_usage_count", "currentWeeklyUsageCount"],
    remaining_percent: [
        "current_weekly_remaining_percent",
        "currentWeeklyRemainingPercent",
    ],
    status: ["current_weekly_status", "currentWeeklyStatus"],
    start: ["weekly_start_time", "weeklyStartTime"],
    end: ["weekly_end_time", "weeklyEndTime"],
    remains: ["weekly_remains_time", "weeklyRemainsTime"],
};

/// Raw lane fields. MiniMax's `usage_count` is the remaining count.
#[derive(Clone, Copy)]
struct LaneFields {
    total: Option<i64>,
    remaining: Option<i64>,
    remaining_percent: Option<f64>,
    status: Option<i64>,
    start: Option<i64>,
    end: Option<i64>,
    remains: Option<i64>,
}

impl LaneKeys {
    fn read(&self, entry: &Value) -> LaneFields {
        let field = |[snake, camel]: [&str; 2]| get_field(entry, snake, camel);
        LaneFields {
            total: value_i64(field(self.total)),
            remaining: value_i64(field(self.usage)),
            remaining_percent: json::lenient_f64(field(self.remaining_percent)),
            status: value_i64(field(self.status)),
            start: value_i64(field(self.start)),
            end: value_i64(field(self.end)),
            remains: value_i64(field(self.remains)),
        }
    }
}

/// The next `time` today in `tz`, or a day from now once it has passed.
pub(super) fn next_time_today(
    time: chrono::NaiveTime,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let dt = now
        .date_naive()
        .and_time(time)
        .and_local_timezone(tz)
        .single()?;
    let candidate = dt.with_timezone(&Utc);
    Some(if candidate < now {
        now + Duration::days(1)
    } else {
        candidate
    })
}

/// Decode an epoch integer that may be in milliseconds, seconds, or unparseable.
fn date_from_epoch(value: Option<i64>) -> Option<DateTime<Utc>> {
    let raw = value?;
    if raw > 1_000_000_000_000 {
        Utc.timestamp_opt(raw / 1000, 0).single()
    } else if raw > 1_000_000_000 {
        Utc.timestamp_opt(raw, 0).single()
    } else {
        None
    }
}

/// `usedPercent(remainingPercent:)` — `min(100, max(0, 100 - remainingPercent))`.
fn used_percent_from_remaining(remaining_percent: f64) -> f64 {
    100.0 - remaining_percent.clamp(0.0, 100.0)
}

/// `usedPercent(total:remaining:)` — `max(0, total-remaining)/total*100` clamped.
fn used_percent_from_counts(total: i64, remaining: i64) -> Option<f64> {
    if total <= 0 {
        return None;
    }
    let used = (total - remaining).max(0);
    Some((used as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
}

/// `windowMinutes(start:end:)`.
fn window_minutes(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> Option<u32> {
    let (start, end) = (start?, end?);
    let minutes = (end - start).num_minutes();
    if minutes > 0 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "guarded by `minutes > 0`; a window longer than ~8 million years is impossible"
        )]
        Some(minutes as u32)
    } else {
        None
    }
}

/// `resetsAt(end:remains:now:)`.
fn resets_at(
    end: Option<DateTime<Utc>>,
    remains: Option<i64>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if let Some(end) = end
        && end > now
    {
        return Some(end);
    }
    let remains = remains?;
    if remains <= 0 {
        return None;
    }
    let seconds = if remains > 1_000_000 {
        remains as f64 / 1000.0
    } else {
        remains as f64
    };
    // Display/rounding conversion of an epoch value; sub-second precision is
    // intentionally dropped. An offset that cannot be represented as a
    // timestamp (oversized `remains`) yields no reset line and keeps the usage
    // percentage (upstream #3758/#3764: safely handle unrepresentable
    // timestamps).
    #[allow(
        clippy::cast_possible_truncation,
        reason = "epoch seconds truncated to whole seconds by design"
    )]
    let total_seconds = Duration::try_seconds(seconds as i64)?;
    now.checked_add_signed(total_seconds)
        .filter(|reset| *reset > now)
}

/// Upstream `mapModelNameToServiceType`.
fn map_model_name_to_service_type(model_name: &str) -> String {
    let lower = model_name.trim().to_lowercase();
    if lower == "general" || lower == "video" {
        return lower;
    }
    if is_text_generation_model_name(model_name) {
        return "Text Generation".to_string();
    }
    if lower.contains("speech") {
        return "Text to Speech".to_string();
    }
    if lower.contains("hailuo") && lower.contains("fast") {
        return "Image to Video".to_string();
    }
    if lower.contains("hailuo") {
        return "Text to Video".to_string();
    }
    if lower.starts_with("image-") {
        return "Image Generation".to_string();
    }
    if lower.contains("music") {
        return "Music Generation".to_string();
    }
    model_name.to_string()
}

/// Upstream `isTextGenerationModelName`.
fn is_text_generation_model_name(model_name: &str) -> bool {
    let lower = model_name.to_lowercase();
    lower == "general" || lower.contains("minimax-m") || lower.starts_with("m2.")
}

/// Upstream `shouldRenderWeeklyWindow` — weekly rows only for text-generation models.
fn should_render_weekly_window(model_name: &str) -> bool {
    is_text_generation_model_name(model_name)
}

/// Upstream `isUnavailableQuotaPlaceholder` (with the unlimited-weekly exception).
fn is_unavailable_quota_placeholder(
    service_type: &str,
    window_type_override: Option<&str>,
    status: Option<i64>,
    total: Option<i64>,
    remaining: Option<i64>,
    remaining_percent: Option<f64>,
) -> bool {
    // The unlimited-weekly exception is NOT a placeholder.
    if let Some(window) = window_type_override
        && is_unlimited_quota_window(service_type, window, status, remaining_percent)
    {
        return false;
    }
    status == Some(3)
        && total.unwrap_or(0) == 0
        && remaining.unwrap_or(0) == 0
        && remaining_percent.map(|p| p >= 100.0).unwrap_or(false)
}

/// Upstream `isUnlimitedQuotaWindow`.
fn is_unlimited_quota_window(
    service_type: &str,
    window_type: &str,
    status: Option<i64>,
    remaining_percent: Option<f64>,
) -> bool {
    let normalized_service = service_type.trim().to_lowercase();
    let normalized_window = window_type.trim().to_lowercase();
    status == Some(3)
        && matches!(normalized_service.as_str(), "text generation" | "general")
        && normalized_window == "weekly"
        && remaining_percent.map(|p| p >= 100.0).unwrap_or(false)
}

/// Window type from duration start→end (upstream `parseWindowInfo`).
fn window_type_from_duration(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> String {
    let (Some(start), Some(end)) = (start, end) else {
        return "Unknown".to_string();
    };
    let duration_hours = (end - start).num_seconds() as f64 / 3600.0;
    if (23.0..=25.0).contains(&duration_hours) {
        "Today".to_string()
    } else if (4.0..=6.0).contains(&duration_hours) {
        "5 hours".to_string()
    } else if (1.0..23.0).contains(&duration_hours) {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "label only; duration already filtered into the 1..23 hours bucket"
        )]
        let label = format!("{} hours", duration_hours as i64);
        label
    } else {
        "Custom".to_string()
    }
}

/// `timeRange` formatted with UTC+8 offset.
fn time_range_string(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> String {
    let (Some(start), Some(end)) = (start, end) else {
        return "N/A".to_string();
    };
    let tz = CN_TZ;
    format!(
        "{}-{}(UTC+8)",
        start.with_timezone(&tz).format("%H:%M"),
        end.with_timezone(&tz).format("%H:%M")
    )
}

/// Weekly `timeRange` in `MM/dd HH:mm - MM/dd HH:mm(UTC+8)` format.
fn weekly_time_range_string(
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Option<String> {
    let start = start?;
    let end = end?;
    let tz = CN_TZ;
    Some(format!(
        "{} - {}(UTC+8)",
        start.with_timezone(&tz).format("%m/%d %H:%M"),
        end.with_timezone(&tz).format("%m/%d %H:%M")
    ))
}

/// Upstream `resetDescription(for:timeRange:now:resetsAt:)`.
fn reset_description(
    window_type: &str,
    time_range: &str,
    now: DateTime<Utc>,
    resets_at: Option<DateTime<Utc>>,
) -> String {
    if let Some(resets) = resets_at
        && resets > now
    {
        let interval = (resets - now).num_seconds();
        if interval < 60 {
            return format!("Resets in {interval} seconds");
        }
        let (unit_seconds, unit) = [(86_400, "day"), (3_600, "hour")]
            .into_iter()
            .find(|(unit_seconds, _)| interval >= *unit_seconds)
            .unwrap_or((60, "minute"));
        let count = interval / unit_seconds;
        let plural = if count == 1 { "" } else { "s" };
        return format!("Resets in {count} {unit}{plural}");
    }
    format!("{window_type}: {time_range}")
}

/// Build a `RemainsRow` from the raw interval/weekly fields (upstream
/// `makeServiceUsage`). Returns `None` when the row is a placeholder that
/// should be skipped.
fn make_remains_row(
    service_type: &str,
    window_type_override: Option<&str>,
    lane: &LaneFields,
    now: DateTime<Utc>,
    is_weekly: bool,
) -> Option<RemainsRow> {
    let LaneFields {
        total,
        remaining,
        remaining_percent,
        status,
        start,
        end,
        remains: remains_time,
    } = *lane;
    if is_unavailable_quota_placeholder(
        service_type,
        window_type_override,
        status,
        total,
        remaining,
        remaining_percent,
    ) {
        return None;
    }

    let start_dt = date_from_epoch(start);
    let end_dt = date_from_epoch(end);

    let mut window_type = window_type_from_duration(start_dt, end_dt);
    if let Some(override_wt) = window_type_override {
        window_type = override_wt.to_string();
    }
    let mut time_range = time_range_string(start_dt, end_dt);
    if window_type.eq_ignore_ascii_case("weekly")
        && let Some(weekly_range) = weekly_time_range_string(start_dt, end_dt)
    {
        time_range = weekly_range;
    }

    let is_unlimited =
        is_unlimited_quota_window(service_type, &window_type, status, remaining_percent);
    let resets = if is_unlimited {
        None
    } else {
        resets_at(end_dt, remains_time, now)
    };
    let desc = if is_unlimited {
        "Unlimited".to_string()
    } else {
        reset_description(&window_type, &time_range, now, resets)
    };

    let win_minutes = window_minutes(start_dt, end_dt);

    let percent = if is_unlimited {
        0.0
    } else if let Some(rp) = remaining_percent {
        used_percent_from_remaining(rp)
    } else {
        let total = total?;
        let remaining = remaining.unwrap_or(0);
        used_percent_from_counts(total, remaining)?
    };

    Some(RemainsRow {
        service_type: service_type.to_string(),
        model_name: String::new(),
        window_type,
        percent,
        window_minutes: win_minutes,
        resets_at: resets,
        reset_description: Some(desc),
        is_unlimited,
        is_weekly,
    })
}

/// Try the multi-service `data.services[]` shape.
fn parse_multi_service(json: &Value) -> Option<Vec<ServiceRow>> {
    let services = json
        .get("data")
        .and_then(|d| d.get("services"))
        .or_else(|| json.get("services"))
        .and_then(|s| s.as_array())?;
    if services.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    for item in services {
        let service_type = scalar_string(get_field(item, "service_type", "serviceType"))?;
        let window_type = scalar_string(get_field(item, "window_type", "windowType"))?;
        let time_range = scalar_string(get_field(item, "time_range", "timeRange"))?;
        let usage = value_i64(item.get("usage"))?;
        let limit = value_i64(item.get("limit"))?;
        if limit <= 0 {
            continue;
        }
        let percent = match json::lenient_f64(item.get("percent")) {
            Some(p) => p,
            None => (usage as f64 / limit as f64) * 100.0,
        };
        let percent = percent.clamp(0.0, 100.0);
        let resets_at = parse_resets_at_from_time_range(&time_range, &window_type, Utc::now());
        let desc = reset_description(&window_type, &time_range, Utc::now(), resets_at);
        rows.push(ServiceRow {
            service_type,
            window_type,
            time_range,
            percent,
            resets_at,
            reset_description: Some(desc),
        });
    }
    if rows.is_empty() { None } else { Some(rows) }
}

/// Parse a reset timestamp from a multi-service `time_range`.
fn parse_resets_at_from_time_range(
    time_range: &str,
    window_type: &str,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let lower = window_type.trim().to_lowercase();
    let tz = CN_TZ;

    if lower == "today" {
        let parts: Vec<&str> = time_range.splitn(2, '-').collect();
        if parts.len() != 2 {
            return None;
        }
        let end_str = parts[1].trim();
        // "yyyy/MM/dd HH:mm" in UTC+8
        let dt = chrono::NaiveDateTime::parse_from_str(end_str, "%Y/%m/%d %H:%M").ok()?;
        return Some(dt.and_local_timezone(tz).single()?.with_timezone(&Utc));
    }

    if lower.contains("hour") || lower.contains('h') {
        let parts: Vec<&str> = time_range.split('-').collect();
        if parts.len() < 2 {
            return None;
        }
        let end_part = parts[1].trim();
        // strip "(...)" suffix
        let end_clean = {
            let re = Regex::new(r#"\(.*\)"#).ok()?;
            re.replace_all(end_part, "").trim().to_string()
        };
        let time = chrono::NaiveTime::parse_from_str(&end_clean, "%H:%M").ok()?;
        return next_time_today(time, tz, now);
    }

    None
}

/// Extract the plan name from coding-plan data fields (upstream `parsePlanName(data:)`).
fn parse_plan_name_from_data(data: &Value) -> Option<String> {
    for key in [
        ("current_subscribe_title", "currentSubscribeTitle"),
        ("plan_name", "planName"),
        ("combo_title", "comboTitle"),
        ("current_plan_title", "currentPlanTitle"),
    ] {
        if let Some(val) = get_field(data, key.0, key.1)
            && let Some(s) = scalar_string(Some(val))
            && !s.trim().is_empty()
        {
            return Some(s.trim().to_string());
        }
    }
    // current_combo_card.title
    if let Some(card) = get_field(data, "current_combo_card", "currentComboCard")
        && let Some(title) = card.get("title")
        && let Some(s) = scalar_string(Some(title))
        && !s.trim().is_empty()
    {
        return Some(s.trim().to_string());
    }
    // inferred token plan name
    let model_remains = get_field(data, "model_remains", "modelRemains").and_then(|v| v.as_array());
    if let Some(entries) = model_remains {
        let has_text_generation = entries.iter().any(|e| {
            scalar_string(get_field(e, "model_name", "modelName"))
                .map(|n| is_text_generation_model_name(&n))
                .unwrap_or(false)
        });
        let has_unavailable_video = entries.iter().any(|e| {
            let name = scalar_string(get_field(e, "model_name", "modelName"))
                .map(|n| n.trim().to_lowercase())
                .unwrap_or_default();
            let lane = INTERVAL_LANE.read(e);
            name == "video"
                && is_unavailable_quota_placeholder(
                    "Text to Video",
                    None,
                    lane.status,
                    lane.total,
                    lane.remaining,
                    lane.remaining_percent,
                )
        });
        if has_text_generation && has_unavailable_video {
            return Some("Plus".to_string());
        }
    }
    None
}

/// The single-service `model_remains[]` parser (upstream
/// `parseCodingPlanRemains(payload:now:)`).
fn parse_remains(
    json: &Value,
    now: DateTime<Utc>,
) -> Result<MiniMaxCodingPlanSnapshot, ProviderError> {
    // base_resp from data first, then root
    let base_resp = json
        .get("data")
        .and_then(|d| d.get("base_resp"))
        .or_else(|| json.get("base_resp"))
        .or_else(|| json.get("baseResp"))
        .or_else(|| json.get("data").and_then(|d| d.get("baseResp")));

    if let Some(base) = base_resp
        && let Some(status_code) = value_i64(base.get("status_code"))
        && status_code != 0
    {
        let status_msg = scalar_string(base.get("status_msg"))
            .or_else(|| scalar_string(base.get("statusMessage")))
            .unwrap_or_else(|| format!("MiniMax coding plan status {status_code}"));
        let lower = status_msg.to_lowercase();
        if status_code == 1004
            || lower.contains("cookie")
            || lower.contains("log in")
            || lower.contains("login")
        {
            return Err(ProviderError::AuthRequired);
        }
        return Err(ProviderError::Other(status_msg));
    }

    // model_remains from data first, then root
    let model_remains = get_field(json, "model_remains", "modelRemains")
        .or_else(|| {
            json.get("data")
                .and_then(|d| get_field(d, "model_remains", "modelRemains"))
        })
        .and_then(|v| v.as_array());

    let entries = match model_remains {
        Some(arr) if !arr.is_empty() => arr,
        _ => {
            return Err(ProviderError::Parse("Missing coding plan data.".into()));
        }
    };

    // Locate data for plan-name extraction
    let data_obj = json.get("data").unwrap_or(json);
    let plan_name = parse_plan_name_from_data(data_obj);

    let mut rows: Vec<RemainsRow> = Vec::new();
    for entry in entries {
        let model_name = match scalar_string(get_field(entry, "model_name", "modelName")) {
            Some(n) => n,
            None => continue,
        };
        let service_type = map_model_name_to_service_type(&model_name);

        // Interval row
        if let Some(mut row) =
            make_remains_row(&service_type, None, &INTERVAL_LANE.read(entry), now, false)
        {
            row.model_name = model_name.clone();
            rows.push(row);
        }

        // Weekly row (only for text-generation models)
        if should_render_weekly_window(&model_name)
            && let Some(mut row) = make_remains_row(
                &service_type,
                Some("Weekly"),
                &WEEKLY_LANE.read(entry),
                now,
                true,
            )
        {
            row.model_name = model_name;
            rows.push(row);
        }
    }

    Ok(MiniMaxCodingPlanSnapshot::Remains { plan_name, rows })
}

/// The one parser used for page JSON, `__NEXT_DATA__` payloads, and remains API.
pub(super) fn parse_coding_plan_value(
    json: &Value,
    now: DateTime<Utc>,
) -> Result<MiniMaxCodingPlanSnapshot, ProviderError> {
    // Try multi-service shape first
    if let Some(rows) = parse_multi_service(json) {
        return Ok(MiniMaxCodingPlanSnapshot::Services(rows));
    }
    // Fall through to single-service (model_remains)
    parse_remains(json, now)
}

/// True when the coding-plan endpoint reports that this account has no coding
/// plan because it runs on a Token Plan subscription instead — live evidence:
/// base_resp 2062 "no active token plan subscription" (issue #254). Such
/// accounts are served by the console token-plan endpoints, so `mod.rs` uses
/// this to switch fetch paths.
pub(crate) fn is_token_plan_without_coding_plan(err: &ProviderError) -> bool {
    matches!(err, ProviderError::Other(msg) if msg.to_ascii_lowercase().contains("token plan"))
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_remains_yields_no_reset_line_and_keeps_usage() {
        // `remains` above ~8.3e15 ms cannot be represented as a timestamp:
        // `Duration::try_seconds` must return None, so `resets_at` is None and
        // the usage percentage survives (upstream #3758/#3764).
        assert_eq!(resets_at(None, Some(i64::MAX), now()), None);
    }

    #[test]
    fn negative_remains_yields_no_reset_line() {
        assert_eq!(resets_at(None, Some(-5), now()), None);
        assert_eq!(resets_at(None, Some(0), now()), None);
    }

    #[test]
    fn positive_remains_keeps_the_reset_line() {
        // 90_000 > 1_000_000? No: below the heuristic, treated as seconds.
        let reset = resets_at(None, Some(60), now()).expect("small remains");
        assert_eq!(reset, now() + Duration::seconds(60));
        // Above the heuristic: treated as milliseconds.
        let reset_ms = resets_at(None, Some(2_000_000), now()).expect("ms remains");
        assert_eq!(reset_ms, now() + Duration::seconds(2_000));
    }

    #[test]
    fn end_time_in_the_future_wins_over_remains() {
        let end = now() + Duration::seconds(120);
        assert_eq!(resets_at(Some(end), Some(60), now()), Some(end));
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap()
    }

    #[test]
    fn reset_description_buckets_by_largest_whole_unit() {
        let cases: [(Option<i64>, &str); 14] = [
            (None, "Today: 00:00-24:00"),
            (Some(-10), "Today: 00:00-24:00"),
            (Some(0), "Today: 00:00-24:00"),
            (Some(1), "Resets in 1 seconds"),
            (Some(59), "Resets in 59 seconds"),
            (Some(60), "Resets in 1 minute"),
            (Some(119), "Resets in 1 minute"),
            (Some(120), "Resets in 2 minutes"),
            (Some(3_599), "Resets in 59 minutes"),
            (Some(3_600), "Resets in 1 hour"),
            (Some(7_200), "Resets in 2 hours"),
            (Some(86_399), "Resets in 23 hours"),
            (Some(86_400), "Resets in 1 day"),
            (Some(172_800), "Resets in 2 days"),
        ];
        for (offset, expected) in cases {
            let resets = offset.map(|seconds| now() + Duration::seconds(seconds));
            assert_eq!(
                reset_description("Today", "00:00-24:00", now(), resets),
                expected,
                "offset {offset:?}"
            );
        }
    }

    #[test]
    fn time_range_resets_read_utc_plus_8_end_times() {
        let at = |y, mo, d, h, mi| Some(Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap());
        let tomorrow = Some(now() + Duration::days(1));
        let cases = [
            (
                "2026/08/03 00:00 - 2026/08/04 00:00",
                "Today",
                at(2026, 8, 3, 16, 0),
            ),
            ("2026/08/03 00:00", "today", None),
            ("garbage - later", "Today", None),
            ("10:00-23:00(UTC+8)", "5 hours", at(2026, 8, 3, 15, 0)),
            ("10:00 - 23:30", "2h", at(2026, 8, 3, 15, 30)),
            ("08:00-19:00(UTC+8)", "5 hours", tomorrow),
            ("10:00-23:00", "Month", at(2026, 8, 3, 15, 0)),
            ("10:00", "5 hours", None),
            ("10:00-25:99", "5 hours", None),
            ("10:00-23:00", "Weekly", None),
        ];
        for (range, window, expected) in cases {
            assert_eq!(
                parse_resets_at_from_time_range(range, window, now()),
                expected,
                "{range} / {window}"
            );
        }
    }

    #[test]
    fn time_range_labels_render_in_utc_plus_8() {
        let start = Some(Utc.with_ymd_and_hms(2026, 8, 3, 0, 0, 0).unwrap());
        let end = Some(Utc.with_ymd_and_hms(2026, 8, 10, 5, 30, 0).unwrap());
        assert_eq!(time_range_string(start, end), "08:00-13:30(UTC+8)");
        assert_eq!(time_range_string(start, None), "N/A");
        assert_eq!(
            weekly_time_range_string(start, end).as_deref(),
            Some("08/03 08:00 - 08/10 13:30(UTC+8)")
        );
        assert_eq!(weekly_time_range_string(None, end), None);
    }

    fn camel_keys(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let mut camel = String::new();
                        let mut upper = false;
                        for ch in key.chars() {
                            if ch == '_' {
                                upper = true;
                            } else if upper {
                                camel.extend(ch.to_uppercase());
                                upper = false;
                            } else {
                                camel.push(ch);
                            }
                        }
                        (camel, camel_keys(value))
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(camel_keys).collect()),
            other => other.clone(),
        }
    }

    #[test]
    fn remains_lanes_read_every_snake_and_camel_key() {
        let snake = serde_json::json!({
            "model_remains": [{
                "model_name": "General",
                "current_interval_total_count": 200,
                "current_interval_usage_count": 50,
                "current_interval_status": 0,
                "start_time": 1785754800,
                "end_time": 1785758340,
                "remains_time": 600,
                "current_weekly_total_count": 1000,
                "current_weekly_usage_count": 100,
                "current_weekly_remaining_percent": 40,
                "current_weekly_status": 0,
                "weekly_start_time": 1785585600,
                "weekly_end_time": 1786190400,
                "weekly_remains_time": 30
            }]
        });
        let camel = camel_keys(&snake);
        assert!(camel["modelRemains"][0].get("weeklyRemainsTime").is_some());

        let rows_for = |json: &Value| {
            let MiniMaxCodingPlanSnapshot::Remains { plan_name, rows } =
                parse_coding_plan_value(json, now()).unwrap()
            else {
                panic!("expected Remains");
            };
            assert_eq!(plan_name, None);
            rows
        };
        let rows = rows_for(&snake);
        assert_eq!(format!("{rows:?}"), format!("{:?}", rows_for(&camel)));
        assert_eq!(rows.len(), 2);

        let interval = &rows[0];
        assert_eq!(interval.model_name, "General");
        assert_eq!(interval.service_type, "general");
        assert_eq!(interval.window_type, "Custom");
        // usage_count is the remaining count: (200 - 50) / 200.
        assert_eq!(interval.percent, 75.0);
        assert_eq!(interval.window_minutes, Some(59));
        assert_eq!(interval.resets_at, Some(now() + Duration::seconds(600)));
        assert_eq!(
            interval.reset_description.as_deref(),
            Some("Resets in 10 minutes")
        );
        assert!(!interval.is_weekly);

        let weekly = &rows[1];
        assert_eq!(weekly.window_type, "Weekly");
        assert_eq!(weekly.percent, 60.0);
        assert_eq!(weekly.window_minutes, Some(10080));
        assert_eq!(weekly.resets_at, Some(now() + Duration::days(5)));
        assert_eq!(
            weekly.reset_description.as_deref(),
            Some("Resets in 5 days")
        );
        assert!(weekly.is_weekly);
    }

    #[test]
    fn plan_name_infers_plus_from_an_unavailable_video_lane() {
        let entries = |video_remaining_percent: i64| {
            serde_json::json!({
                "model_remains": [
                    {"model_name": "MiniMax-M2", "current_interval_total_count": 10,
                     "current_interval_usage_count": 5},
                    {"model_name": " Video ", "current_interval_status": 3,
                     "current_interval_total_count": 0, "current_interval_usage_count": 0,
                     "current_interval_remaining_percent": video_remaining_percent}
                ]
            })
        };
        for (remaining, expected) in [(100, Some("Plus")), (50, None)] {
            let snake = entries(remaining);
            assert_eq!(
                parse_plan_name_from_data(&snake).as_deref(),
                expected,
                "snake {remaining}"
            );
            assert_eq!(
                parse_plan_name_from_data(&camel_keys(&snake)).as_deref(),
                expected,
                "camel {remaining}"
            );
        }
    }

    #[test]
    fn parses_remains_json_snake_case() {
        let json = serde_json::json!({
            "data": {
                "current_subscribe_title": "MiniMax Pro",
                "model_remains": [{
                    "model_name": "General",
                    "current_interval_total_count": 100,
                    "current_interval_usage_count": 73,
                    "start_time": 1785792000,
                    "end_time": 1785878400,
                    "current_interval_status": 0,
                    "current_weekly_total_count": 1000,
                    "current_weekly_usage_count": 850,
                    "current_weekly_remaining_percent": 85,
                    "weekly_start_time": 1785792000,
                    "weekly_end_time": 1786396800,
                    "current_weekly_status": 0
                }]
            }
        });
        let snapshot = parse_coding_plan_value(&json, now()).unwrap();
        let MiniMaxCodingPlanSnapshot::Remains { plan_name, rows } = snapshot else {
            panic!("expected Remains");
        };
        assert_eq!(plan_name.as_deref(), Some("MiniMax Pro"));
        // remaining percent 73 → used 27
        assert!((rows[0].percent - 27.0).abs() < 0.01);
        // window_minutes = (end-start)/60 = 86400/60 = 1440
        assert_eq!(rows[0].window_minutes, Some(1440));
        assert!(rows[0].resets_at.is_some());
        // General is text-gen → weekly row generated
        assert_eq!(rows.len(), 2);
        assert!(rows[1].is_weekly);
    }

    #[test]
    fn parses_counts_only_entry() {
        let json = serde_json::json!({
            "data": {
                "model_remains": [{
                    "model_name": "General",
                    "current_interval_total_count": 100,
                    "current_interval_usage_count": 40,
                    "start_time": 1722691200,
                    "end_time": 1722777600,
                    "current_interval_status": 0
                }]
            }
        });
        let snapshot = parse_coding_plan_value(&json, now()).unwrap();
        let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
            panic!("expected Remains");
        };
        // total=100, remaining=40 → used 60/100 = 60%
        assert!((rows[0].percent - 60.0).abs() < 0.01);
    }

    #[test]
    fn parses_remaining_percent_with_boost_field_ignored() {
        let json = serde_json::json!({
            "data": {
                "model_remains": [{
                    "model_name": "General",
                    "current_interval_total_count": 0,
                    "current_interval_usage_count": 0,
                    "current_interval_remaining_percent": 50,
                    "interval_boost_permill": 1500,
                    "start_time": 1722691200,
                    "end_time": 1722777600,
                    "current_interval_status": 0
                }]
            }
        });
        let snapshot = parse_coding_plan_value(&json, now()).unwrap();
        let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
            panic!("expected Remains");
        };
        // remainingPercent 50 → used 50%; boost field is ignored dead plumbing
        assert!((rows[0].percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn placeholder_lane_skipped_for_video() {
        let json = serde_json::json!({
            "data": {
                "model_remains": [{
                    "model_name": "video",
                    "current_interval_total_count": 0,
                    "current_interval_usage_count": 0,
                    "current_interval_remaining_percent": 100,
                    "current_interval_status": 3
                }]
            }
        });
        let snapshot = parse_coding_plan_value(&json, now()).unwrap();
        let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
            panic!("expected Remains");
        };
        // video is not text-gen → no weekly; interval is placeholder → skipped
        assert!(rows.is_empty());
    }

    #[test]
    fn placeholder_general_weekly_kept_as_unlimited() {
        let json = serde_json::json!({
            "data": {
                "model_remains": [{
                    "model_name": "General",
                    "current_interval_total_count": 0,
                    "current_interval_usage_count": 0,
                    "current_interval_remaining_percent": 100,
                    "current_interval_status": 3,
                    "current_weekly_total_count": 0,
                    "current_weekly_usage_count": 0,
                    "current_weekly_remaining_percent": 100,
                    "current_weekly_status": 3
                }]
            }
        });
        let snapshot = parse_coding_plan_value(&json, now()).unwrap();
        let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
            panic!("expected Remains");
        };
        // interval is placeholder → skipped, but weekly is unlimited → kept
        let weekly = rows.iter().find(|r| r.is_weekly).unwrap();
        assert!(weekly.is_unlimited);
        assert_eq!(weekly.reset_description.as_deref(), Some("Unlimited"));
        assert!((weekly.percent - 0.0).abs() < 0.01);
    }

    #[test]
    fn base_resp_status_maps_to_errors() {
        // `None` expects AuthRequired; `Some` expects Other with that message.
        let cases = [
            (serde_json::json!({ "status_code": 1004 }), None),
            (
                serde_json::json!({ "status_code": 2000, "status_msg": "please log in" }),
                None,
            ),
            (
                serde_json::json!({ "status_code": 2000, "status_msg": "quota sync failed" }),
                Some("quota sync failed"),
            ),
        ];
        for (base_resp, expected) in cases {
            let json = serde_json::json!({
                "data": { "base_resp": base_resp, "model_remains": [] }
            });
            let err = parse_coding_plan_value(&json, now()).unwrap_err();
            match (expected, err) {
                (None, ProviderError::AuthRequired) => {}
                (Some(message), ProviderError::Other(actual)) => assert_eq!(actual, message),
                (expected, err) => panic!("{base_resp}: expected {expected:?}, got {err:?}"),
            }
        }
    }

    #[test]
    fn base_resp_without_status_code_parses_fine() {
        let json = serde_json::json!({
            "data": {
                "base_resp": {},
                "model_remains": [{
                    "model_name": "General",
                    "current_interval_total_count": 100,
                    "current_interval_usage_count": 30,
                    "current_interval_remaining_percent": 70,
                    "start_time": 1785792000,
                    "end_time": 1785878400,
                    "current_interval_status": 0
                }]
            }
        });
        let snapshot = parse_coding_plan_value(&json, now()).unwrap();
        let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
            panic!("expected Remains");
        };
        // remainingPercent 70 → used 30%; no error from absent status_code
        assert!((rows[0].percent - 30.0).abs() < 0.01);
    }

    #[test]
    fn empty_model_remains_is_parse_error() {
        let json = serde_json::json!({
            "data": {
                "model_remains": []
            }
        });
        let err = parse_coding_plan_value(&json, now()).unwrap_err();
        assert!(matches!(err, ProviderError::Parse(_)));
    }

    #[test]
    fn parses_multi_service_json() {
        let json = serde_json::json!({
            "data": {
                "services": [{
                    "service_type": "Text Generation Pro",
                    "window_type": "Today",
                    "time_range": "2026/08/03 00:00 - 2026/08/04 00:00",
                    "usage": 250,
                    "limit": 1000
                }]
            }
        });
        let snapshot = parse_coding_plan_value(&json, now()).unwrap();
        match snapshot {
            MiniMaxCodingPlanSnapshot::Services(rows) => {
                assert_eq!(rows.len(), 1);
                assert!((rows[0].percent - 25.0).abs() < 0.01);
                assert!(rows[0].service_type.contains("Pro"));
            }
            _ => panic!("expected Services"),
        }
    }

    #[test]
    fn token_plan_predicate_matches_reporter_2062_message() {
        // Live reporter evidence (issue #254): the legacy remains endpoint 200s
        // with base_resp 2062 for Token Plan accounts.
        let json = serde_json::json!({
            "base_resp": {
                "status_code": 2062,
                "status_msg": "no active token plan subscription"
            }
        });
        let err = parse_coding_plan_value(&json, now()).unwrap_err();
        assert!(matches!(err, ProviderError::Other(_)));
        assert!(is_token_plan_without_coding_plan(&err));
    }

    #[test]
    fn token_plan_predicate_rejects_unrelated_errors() {
        assert!(!is_token_plan_without_coding_plan(
            &ProviderError::AuthRequired
        ));
        assert!(!is_token_plan_without_coding_plan(&ProviderError::Other(
            "MiniMax coding plan status 2000".to_string()
        )));
        assert!(!is_token_plan_without_coding_plan(&ProviderError::Parse(
            "Missing coding plan data.".to_string()
        )));
        // Case-insensitive match on the message itself.
        assert!(is_token_plan_without_coding_plan(&ProviderError::Other(
            "No Active Token Plan Subscription".to_string()
        )));
    }
}
