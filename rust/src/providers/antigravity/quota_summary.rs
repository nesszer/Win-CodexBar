use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use super::legacy_status::slug;
use crate::core::{NamedRateWindow, ProviderError, RateWindow, UsageSnapshot};

const WINDOW_ID_PREFIX: &str = "antigravity-quota-summary-";
const SESSION_MINUTES: u32 = 300;
const WEEKLY_MINUTES: u32 = 7 * 24 * 60;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryEnvelope {
    response: Option<QuotaSummaryPayload>,
    summary: Option<QuotaSummaryPayload>,
    description: Option<String>,
    groups: Option<Vec<QuotaSummaryGroup>>,
}

#[derive(Debug, Deserialize)]
struct QuotaSummaryCliReport {
    status: String,
    command: QuotaSummaryCliCommand,
}

#[derive(Debug, Deserialize)]
struct QuotaSummaryCliCommand {
    name: String,
    data: QuotaSummaryPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryPayload {
    #[allow(dead_code, reason = "mirrors the local quota-summary response")]
    description: Option<String>,
    #[serde(default)]
    groups: Vec<QuotaSummaryGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryGroup {
    display_name: Option<String>,
    name: Option<String>,
    #[allow(dead_code, reason = "mirrors the local quota-summary response")]
    description: Option<String>,
    #[serde(default)]
    buckets: Vec<QuotaSummaryBucket>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryBucket {
    bucket_id: Option<String>,
    id: Option<String>,
    display_name: Option<String>,
    name: Option<String>,
    description: Option<String>,
    disabled: Option<bool>,
    #[serde(alias = "remaining_fraction")]
    remaining_fraction: Option<f64>,
    remaining: Option<QuotaSummaryRemaining>,
    #[serde(alias = "reset_time")]
    reset_time: Option<String>,
    /// Explicit cadence (`weekly`, `5h`, ...). When non-empty it replaces the id/name candidates.
    window: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryRemaining {
    #[serde(alias = "remaining_fraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "case")]
    oneof_case: Option<String>,
    value: Option<f64>,
}

impl QuotaSummaryBucket {
    fn resolved_remaining_fraction(&self) -> Option<f64> {
        self.remaining_fraction.or_else(|| {
            let remaining = self.remaining.as_ref()?;
            remaining.remaining_fraction.or_else(|| {
                (remaining.oneof_case.as_deref() == Some("remainingFraction"))
                    .then_some(remaining.value)
                    .flatten()
            })
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum BucketKind {
    Session,
    Weekly,
    Other,
}

pub(super) fn parse_usage_snapshot(data: &[u8]) -> Result<UsageSnapshot, ProviderError> {
    let envelope: QuotaSummaryEnvelope = serde_json::from_slice(data)
        .map_err(|error| ProviderError::Parse(format!("Antigravity quota summary: {error}")))?;
    let payload = envelope
        .response
        .or(envelope.summary)
        .or_else(|| {
            envelope.groups.map(|groups| QuotaSummaryPayload {
                description: envelope.description,
                groups,
            })
        })
        .ok_or_else(|| ProviderError::Parse("Antigravity quota summary missing payload".into()))?;

    build_usage_snapshot(payload.groups)
}

pub(super) fn parse_cli_usage_report(data: &[u8]) -> Result<UsageSnapshot, ProviderError> {
    let report: QuotaSummaryCliReport = serde_json::from_slice(data)
        .map_err(|error| ProviderError::Parse(format!("Antigravity CLI usage report: {error}")))?;
    if report.status != "SUCCESS" || report.command.name != "usage" {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage report was not successful".into(),
        ));
    }
    build_usage_snapshot(report.command.data.groups)
}

fn build_usage_snapshot(groups: Vec<QuotaSummaryGroup>) -> Result<UsageSnapshot, ProviderError> {
    let (primary_windows, extra_windows, has_gemini_group) = split_quota_windows(groups);
    let all_windows: Vec<NamedRateWindow> = primary_windows
        .iter()
        .chain(extra_windows.iter())
        .cloned()
        .collect();

    if !all_windows.iter().any(|window| window.usage_known) {
        return Err(ProviderError::Parse(
            "Antigravity quota summary has no usable quota buckets".into(),
        ));
    }

    let cadence_windows = if has_gemini_group {
        &primary_windows
    } else {
        &all_windows
    };
    let primary_selected = most_constrained_named(cadence_windows, SESSION_MINUTES);
    let secondary_selected = most_constrained_named(cadence_windows, WEEKLY_MINUTES);

    let mut selected_ids = Vec::new();
    let mut snapshot = if let Some(primary) = primary_selected {
        selected_ids.push(primary.id.clone());
        let label = if primary.title.to_ascii_lowercase().contains("gemini") {
            "Gemini 5h".to_string()
        } else {
            primary.title.clone()
        };
        UsageSnapshot::new(primary.window.clone()).with_primary_label(label)
    } else {
        UsageSnapshot::new(RateWindow::no_active_session()).with_primary_label("Session")
    };

    if let Some(secondary) = secondary_selected {
        selected_ids.push(secondary.id.clone());
        let label = if secondary.title.to_ascii_lowercase().contains("gemini") {
            "Gemini Weekly".to_string()
        } else {
            secondary.title.clone()
        };
        snapshot = snapshot
            .with_secondary(secondary.window.clone())
            .with_secondary_label(label);
    }

    snapshot.extra_rate_windows = all_windows
        .into_iter()
        .filter(|window| !selected_ids.iter().any(|id| id == &window.id))
        .collect();

    Ok(snapshot)
}

fn split_quota_windows(
    groups: Vec<QuotaSummaryGroup>,
) -> (Vec<NamedRateWindow>, Vec<NamedRateWindow>, bool) {
    let mut indexed_groups = groups.into_iter().enumerate().collect::<Vec<_>>();
    let has_gemini_group = indexed_groups
        .iter()
        .any(|(_, group)| group_rank(group) == 0);
    indexed_groups.sort_by_key(|(index, group)| (group_rank(group), *index));

    let mut primary_windows = Vec::new();
    let mut extra_windows = Vec::new();
    let mut used_window_ids = HashSet::new();
    let mut group_scope_counts = HashMap::<String, usize>::new();

    for (pos, (_, group)) in indexed_groups.into_iter().enumerate() {
        let base_scope = group_scope(&group);
        let occurrence = group_scope_counts.entry(base_scope.clone()).or_default();
        let scoped_group = if *occurrence == 0 {
            base_scope
        } else {
            format!("{base_scope}-{}", *occurrence)
        };
        *occurrence += 1;
        let windows = group_quota_windows(&group, &scoped_group, &mut used_window_ids);
        if pos == 0 {
            primary_windows = windows;
        } else {
            extra_windows.extend(windows);
        }
    }

    (primary_windows, extra_windows, has_gemini_group)
}

fn group_quota_windows(
    group: &QuotaSummaryGroup,
    group_scope: &str,
    used_window_ids: &mut HashSet<String>,
) -> Vec<NamedRateWindow> {
    let group_title = group_title(group);
    let mut buckets = group.buckets.iter().enumerate().collect::<Vec<_>>();
    buckets.sort_by_key(|(index, bucket)| (bucket_kind(bucket), *index));

    let mut windows = Vec::new();
    for (_, bucket) in buckets {
        let Some(bucket_id) = non_empty(bucket.bucket_id.as_deref().or(bucket.id.as_deref()))
        else {
            continue;
        };
        let kind = bucket_kind(bucket);
        let title = format!("{} {}", group_title, bucket_title(bucket, kind));
        let remaining = bucket.resolved_remaining_fraction();
        let usage_known = !bucket.disabled.unwrap_or(false) && remaining.is_some();
        let used_percent = remaining
            .map(|fraction| 100.0 - (fraction * 100.0).clamp(0.0, 100.0))
            .unwrap_or(0.0);
        let window_minutes = match kind {
            BucketKind::Session => Some(SESSION_MINUTES),
            BucketKind::Weekly => Some(WEEKLY_MINUTES),
            BucketKind::Other => None,
        };
        let reset = bucket.reset_time.as_deref().and_then(parse_reset_time);
        let window = RateWindow::with_details(
            used_percent,
            window_minutes,
            reset,
            bucket.description.clone(),
        );
        let base_id = format!("{WINDOW_ID_PREFIX}{group_scope}-{bucket_id}");
        let id = unique_window_id(base_id, group_scope, used_window_ids);
        windows.push(NamedRateWindow::new(id, title, window).with_usage_known(usage_known));
    }
    windows
}

fn unique_window_id(
    base_id: String,
    group_scope: &str,
    used_window_ids: &mut HashSet<String>,
) -> String {
    if used_window_ids.insert(base_id.clone()) {
        return base_id;
    }

    let mut ordinal = 1;
    loop {
        let candidate = format!("{base_id}-{group_scope}-{ordinal}");
        if used_window_ids.insert(candidate.clone()) {
            return candidate;
        }
        ordinal += 1;
    }
}

fn most_constrained_named(windows: &[NamedRateWindow], minutes: u32) -> Option<&NamedRateWindow> {
    windows
        .iter()
        .filter(|row| row.usage_known && row.window.window_minutes == Some(minutes))
        .max_by(|left, right| {
            left.window
                .used_percent
                .partial_cmp(&right.window.used_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.id.cmp(&left.id))
        })
}

/// The group's trimmed name and its rank: 0 for Gemini, 1 for Claude/GPT,
/// 2 for everything else.
fn group_family(group: &QuotaSummaryGroup) -> (Option<&str>, u8) {
    let title = non_empty(group.display_name.as_deref().or(group.name.as_deref()));
    let lower = title.unwrap_or_default().to_ascii_lowercase();
    let rank = if lower.contains("gemini") {
        0
    } else if lower.contains("claude") || lower.contains("gpt") {
        1
    } else {
        2
    };
    (title, rank)
}

fn group_rank(group: &QuotaSummaryGroup) -> u8 {
    group_family(group).1
}

fn group_title(group: &QuotaSummaryGroup) -> String {
    match group_family(group) {
        (_, 0) => "Gemini".into(),
        (_, 1) => "Claude/GPT".into(),
        (title, _) => title.unwrap_or("Quota").to_string(),
    }
}

fn group_scope(group: &QuotaSummaryGroup) -> String {
    slug(&group_title(group))
}

fn bucket_kind(bucket: &QuotaSummaryBucket) -> BucketKind {
    // An explicit, non-empty `window` replaces the id/name candidates entirely.
    let raw_candidates = match non_empty(bucket.window.as_deref()) {
        Some(window) => [Some(window), None],
        None => [
            bucket.bucket_id.as_deref().or(bucket.id.as_deref()),
            bucket.display_name.as_deref().or(bucket.name.as_deref()),
        ],
    };
    let mut candidates = Vec::new();
    for raw in raw_candidates.into_iter().flatten() {
        let normalized = raw.trim().to_ascii_lowercase().replace('_', "-");
        if normalized.is_empty() {
            continue;
        }
        candidates.push(normalized.clone());
        if let Some(stripped) = normalized.strip_suffix(" limit") {
            candidates.push(stripped.to_string());
        }
    }
    const SESSION_ALIASES: [&str; 5] = ["session", "5h", "5-hour", "five hour", "five-hour"];
    if candidates.iter().any(|candidate| {
        SESSION_ALIASES
            .iter()
            .any(|alias| candidate == alias || candidate.ends_with(&format!("-{alias}")))
    }) {
        BucketKind::Session
    } else if candidates
        .iter()
        .any(|candidate| candidate == "weekly" || candidate.ends_with("-weekly"))
    {
        BucketKind::Weekly
    } else {
        BucketKind::Other
    }
}

fn bucket_title(bucket: &QuotaSummaryBucket, kind: BucketKind) -> String {
    match kind {
        BucketKind::Session => "5-hour".into(),
        BucketKind::Weekly => "weekly".into(),
        BucketKind::Other => non_empty(bucket.display_name.as_deref().or(bucket.name.as_deref()))
            .or_else(|| non_empty(bucket.bucket_id.as_deref().or(bucket.id.as_deref())))
            .unwrap_or("quota")
            .to_string(),
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn parse_reset_time(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.with_timezone(&Utc))
        .or_else(|| {
            let seconds = raw.parse::<f64>().ok()?;
            if !seconds.is_finite() {
                return None;
            }
            let whole = seconds.trunc();
            if whole < i64::MIN as f64 || whole > i64::MAX as f64 {
                return None;
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "finite epoch seconds are range-checked before this conversion"
            )]
            let whole_seconds = whole as i64;
            Utc.timestamp_opt(whole_seconds, 0).single()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_gemini_and_third_party_rate_windows() {
        let data = br#"{
          "response": {"groups": [
            {"displayName":"Claude and GPT","buckets":[
              {"bucketId":"3p-5h","displayName":"5-hour limit","remainingFraction":0.2},
              {"bucketId":"3p-weekly","displayName":"Weekly","remainingFraction":0.8}
            ]},
            {"displayName":"Gemini Models","buckets":[
              {"bucketId":"gemini-5h","displayName":"5h","remainingFraction":0.9},
              {"bucketId":"gemini-weekly","displayName":"Weekly","remainingFraction":0.1}
            ]}
          ]}
        }"#;
        let snapshot = parse_usage_snapshot(data).unwrap();
        assert!((snapshot.primary.used_percent - 10.0).abs() < 0.001);
        assert_eq!(snapshot.primary.window_minutes, Some(300));
        assert_eq!(snapshot.primary_label.as_deref(), Some("Gemini 5h"));
        let weekly = snapshot.secondary.unwrap();
        assert!((weekly.used_percent - 90.0).abs() < 0.001);
        assert_eq!(weekly.window_minutes, Some(10_080));
        assert_eq!(snapshot.secondary_label.as_deref(), Some("Gemini Weekly"));
        assert_eq!(snapshot.extra_rate_windows.len(), 2);
        assert_eq!(snapshot.extra_rate_windows[0].title, "Claude/GPT 5-hour");
        assert!((snapshot.extra_rate_windows[0].window.used_percent - 80.0).abs() < 0.001);
        assert_eq!(snapshot.extra_rate_windows[1].title, "Claude/GPT weekly");
        assert!((snapshot.extra_rate_windows[1].window.used_percent - 20.0).abs() < 0.001);
    }

    #[test]
    fn duplicate_bucket_ids_across_groups_remain_distinct() {
        let data = br#"{
          "groups": [
            {"displayName":"Claude and GPT","buckets":[
              {"bucketId":"weekly","displayName":"Weekly","remainingFraction":0.4}
            ]},
            {"displayName":"Gemini Models","buckets":[
              {"bucketId":"weekly","displayName":"Weekly","remainingFraction":0.8}
            ]}
          ]
        }"#;

        let snapshot = parse_usage_snapshot(data).unwrap();

        assert_eq!(snapshot.extra_rate_windows.len(), 1);
        assert_eq!(snapshot.extra_rate_windows[0].title, "Claude/GPT weekly");
        assert!(
            snapshot.extra_rate_windows[0]
                .id
                .contains("-claude-gpt-weekly")
        );
    }

    #[test]
    fn missing_gemini_cadence_stays_missing_when_third_party_has_it() {
        let data = br#"{
          "groups": [
            {"displayName":"Gemini Models","buckets":[
              {"bucketId":"gemini-5h","displayName":"5h","remainingFraction":0.9}
            ]},
            {"displayName":"Claude and GPT","buckets":[
              {"bucketId":"3p-weekly","displayName":"Weekly","remainingFraction":0.2}
            ]}
          ]
        }"#;

        let snapshot = parse_usage_snapshot(data).unwrap();

        assert_eq!(snapshot.primary_label.as_deref(), Some("Gemini 5h"));
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.extra_rate_windows.len(), 1);
        assert_eq!(snapshot.extra_rate_windows[0].title, "Claude/GPT weekly");
    }

    #[test]
    fn unknown_extra_group_remains_an_extra_window() {
        let data = br#"{
          "groups": [
            {"displayName":"Gemini Models","buckets":[
              {"bucketId":"gemini-5h","displayName":"5h","remainingFraction":0.9}
            ]},
            {"displayName":"Experimental","buckets":[
              {"bucketId":"experimental-daily","displayName":"Daily","remainingFraction":0.4}
            ]}
          ]
        }"#;

        let snapshot = parse_usage_snapshot(data).unwrap();

        assert_eq!(snapshot.primary_label.as_deref(), Some("Gemini 5h"));
        assert_eq!(snapshot.extra_rate_windows.len(), 1);
        assert_eq!(snapshot.extra_rate_windows[0].title, "Experimental Daily");
        assert_eq!(snapshot.extra_rate_windows[0].window.window_minutes, None);
    }

    #[test]
    fn exhausted_third_party_window_does_not_mask_active_gemini_quota() {
        let data = br#"{
          "response": {"groups": [
            {"displayName":"Gemini Models","buckets":[
              {"bucketId":"gemini-5h","displayName":"5h","remainingFraction":0.93},
              {"bucketId":"gemini-weekly","displayName":"Weekly","remainingFraction":0.12}
            ]},
            {"displayName":"Claude and GPT models","buckets":[
              {"bucketId":"3p-5h","displayName":"5-hour limit","disabled":true,"remainingFraction":1.0},
              {"bucketId":"3p-weekly","displayName":"Weekly","remainingFraction":0.0}
            ]}
          ]}
        }"#;
        let snapshot = parse_usage_snapshot(data).unwrap();
        assert!((snapshot.primary.used_percent - 7.0).abs() < 0.001);
        assert_eq!(snapshot.primary_label.as_deref(), Some("Gemini 5h"));
        let weekly = snapshot.secondary.unwrap();
        assert!((weekly.used_percent - 88.0).abs() < 0.001);
        assert_eq!(snapshot.secondary_label.as_deref(), Some("Gemini Weekly"));
        assert!(!weekly.is_exhausted());
        assert_eq!(snapshot.extra_rate_windows.len(), 2);
        assert_eq!(snapshot.extra_rate_windows[1].title, "Claude/GPT weekly");
        assert_eq!(snapshot.extra_rate_windows[1].window.used_percent, 100.0);
    }

    #[test]
    fn nested_oneof_remaining_and_disabled_buckets_preserve_unknown_state() {
        let data = br#"{
          "groups": [{"displayName":"Gemini","buckets":[
            {"bucketId":"gemini_session","displayName":"Session","remaining":{"case":"remainingFraction","value":0.25}},
            {"bucketId":"gemini-weekly","displayName":"Weekly","disabled":true,"remainingFraction":0.01},
            {"bucketId":"future","displayName":"Daily"}
          ]}]
        }"#;
        let snapshot = parse_usage_snapshot(data).unwrap();
        assert!((snapshot.primary.used_percent - 75.0).abs() < 0.001);
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.extra_rate_windows.len(), 2);
        assert!(!snapshot.extra_rate_windows[0].usage_known);
        assert!(!snapshot.extra_rate_windows[1].usage_known);
    }

    #[test]
    fn rejects_empty_or_unusable_quota_summary() {
        assert!(parse_usage_snapshot(br#"{}"#).is_err());
        assert!(parse_usage_snapshot(
            br#"{"groups":[{"displayName":"Gemini","buckets":[{"bucketId":"weekly","displayName":"Weekly"}]}]}"#
        )
        .is_err());
    }

    #[test]
    fn parses_structured_cli_usage_report_with_snake_case_fields() {
        let data = br#"{
          "status": "SUCCESS",
          "command": {
            "name": "usage",
            "data": {
              "groups": [{
                "name": "Gemini Models",
                "buckets": [
                  {"id":"gemini-5h","name":"Five Hour Limit Remaining","remaining_fraction":0.6,"reset_time":"2026-09-20T12:34:56Z"},
                  {"id":"gemini-weekly","name":"Weekly Limit Remaining","remaining_fraction":0.8}
                ]
              }]
            }
          }
        }"#;

        let snapshot = parse_cli_usage_report(data).expect("CLI report");
        assert_eq!(snapshot.primary_label.as_deref(), Some("Gemini 5h"));
        assert!((snapshot.primary.used_percent - 40.0).abs() < 0.001);
        assert_eq!(
            snapshot.primary.resets_at.map(|value| value.to_rfc3339()),
            Some("2026-09-20T12:34:56+00:00".to_string())
        );
        assert_eq!(snapshot.secondary_label.as_deref(), Some("Gemini Weekly"));
        assert!((snapshot.secondary.expect("weekly").used_percent - 20.0).abs() < 0.001);
    }

    #[test]
    fn rejects_unsuccessful_or_unrelated_cli_reports() {
        assert!(
            parse_cli_usage_report(
                br#"{"status":"ERROR","command":{"name":"usage","data":{"groups":[]}}}"#
            )
            .is_err()
        );
        assert!(
            parse_cli_usage_report(
                br#"{"status":"SUCCESS","command":{"name":"models","data":{"groups":[]}}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn group_rank_title_and_scope_follow_the_group_family() {
        // (displayName, name, rank, title, scope)
        let cases = [
            (Some("Gemini Pro"), None, 0, "Gemini", "gemini"),
            (None, Some("claude & GPT"), 1, "Claude/GPT", "claude-gpt"),
            (
                Some("OpenAI gpt-oss"),
                Some("Gemini"),
                1,
                "Claude/GPT",
                "claude-gpt",
            ),
            (Some("  "), Some("Gemini"), 2, "Quota", "quota"),
            (
                Some(" Third Party! "),
                None,
                2,
                "Third Party!",
                "third-party",
            ),
            (None, None, 2, "Quota", "quota"),
        ];
        for (display_name, name, rank, title, scope) in cases {
            let group: QuotaSummaryGroup = serde_json::from_value(serde_json::json!({
                "displayName": display_name,
                "name": name,
            }))
            .unwrap();
            assert_eq!(group_rank(&group), rank, "{display_name:?} {name:?}");
            assert_eq!(group_title(&group), title, "{display_name:?} {name:?}");
            assert_eq!(group_scope(&group), scope, "{display_name:?} {name:?}");
        }
    }

    fn single_bucket_snapshot(bucket_id: &str, display_name: &str, window: &str) -> UsageSnapshot {
        let data = format!(
            r#"{{"groups":[{{"displayName":"Gemini Models","buckets":[
              {{"bucketId":"{bucket_id}","displayName":"{display_name}","window":"{window}","remainingFraction":0.8}}
            ]}}]}}"#
        );
        parse_usage_snapshot(data.as_bytes()).unwrap()
    }

    #[test]
    fn explicit_window_sets_cadence_for_opaque_bucket_ids() {
        let weekly = single_bucket_snapshot("gemini-allowance", "Limit Remaining", "weekly");
        assert_eq!(
            weekly.secondary.unwrap().window_minutes,
            Some(WEEKLY_MINUTES)
        );
        assert!(weekly.primary.is_informational);

        let session = single_bucket_snapshot("gemini-allowance", "Limit Remaining", "5h");
        assert_eq!(session.primary.window_minutes, Some(SESSION_MINUTES));
    }

    #[test]
    fn explicit_window_replaces_legacy_bucket_names() {
        let weekly = single_bucket_snapshot("gemini-5h", "Five Hour Limit", "weekly");
        assert_eq!(
            weekly.secondary.unwrap().window_minutes,
            Some(WEEKLY_MINUTES)
        );
        assert!(weekly.extra_rate_windows.is_empty());
        assert!(weekly.primary.is_informational);

        let unknown = single_bucket_snapshot("gemini-5h", "Five Hour Limit", "unknown");
        assert!(unknown.secondary.is_none());
        assert!(unknown.primary.is_informational);
        assert_eq!(unknown.extra_rate_windows.len(), 1);
        assert_eq!(unknown.extra_rate_windows[0].window.window_minutes, None);
    }

    #[test]
    fn blank_explicit_window_falls_back_to_bucket_names() {
        let data = br#"{"groups":[{"displayName":"Gemini","buckets":[
          {"bucketId":"gemini-5h","displayName":"Five Hour Limit","window":"  ","remainingFraction":0.5}
        ]}]}"#;
        let snapshot = parse_usage_snapshot(data).unwrap();
        assert_eq!(snapshot.primary.window_minutes, Some(SESSION_MINUTES));
    }

    #[test]
    fn weekly_only_starter_groups_stay_visible_without_a_five_hour_window() {
        let data = br#"{"groups":[
          {"displayName":"Gemini Models","buckets":[
            {"bucketId":"gemini-weekly","displayName":"Weekly Limit","window":"weekly","remainingFraction":1.0}]},
          {"displayName":"Claude and GPT models","buckets":[
            {"bucketId":"3p-weekly","displayName":"Weekly Limit","window":"weekly","remainingFraction":1.0}]}
        ]}"#;
        let snapshot = parse_usage_snapshot(data).unwrap();
        assert!(snapshot.primary.is_informational);
        let secondary = snapshot.secondary.as_ref().unwrap();
        assert_eq!(secondary.window_minutes, Some(WEEKLY_MINUTES));
        assert_eq!(snapshot.secondary_label.as_deref(), Some("Gemini Weekly"));
        assert_eq!(snapshot.extra_rate_windows.len(), 1);
        assert_eq!(snapshot.extra_rate_windows[0].title, "Claude/GPT weekly");
        assert_eq!(
            snapshot.extra_rate_windows[0].window.window_minutes,
            Some(WEEKLY_MINUTES)
        );
    }
}
