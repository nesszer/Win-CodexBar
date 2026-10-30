use super::*;
use crate::core::{
    CodexSourcePricingEvidence, CodexSourceRowCache, CodexSourceUsageRow, CostUsageCache,
    RateWindow,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

fn utc(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn weekly_window(reset_at: DateTime<Utc>) -> RateWindow {
    let mut window = RateWindow::new(0.0);
    window.window_minutes = Some(7 * 24 * 60);
    window.resets_at = Some(reset_at);
    window
}

fn row(
    timestamp: DateTime<Utc>,
    input: i64,
    output: i64,
    pricing_model: Option<&str>,
) -> CodexSourceUsageRow {
    CodexSourceUsageRow {
        day_key: timestamp.format("%Y-%m-%d").to_string(),
        timestamp: Some(timestamp),
        model: "gpt-5.5".to_string(),
        input,
        cached: 0,
        output,
        reasoning: None,
        source_end_offset: 1,
        turn_id: None,
        pricing: CodexSourcePricingEvidence {
            pricing_model: pricing_model.map(str::to_string),
            pricing_mode: None,
        },
    }
}

fn cache_with_source_rows(rows: Vec<CodexSourceUsageRow>) -> CostUsageCache {
    let mut cache = CostUsageCache {
        scan_since_key: Some("2026-09-01".to_string()),
        scan_until_key: Some("2026-09-21".to_string()),
        ..CostUsageCache::default()
    };
    cache.codex_source_rows.insert(
        "session.jsonl".to_string(),
        CodexSourceRowCache {
            file_identity: "identity-1".to_string(),
            size: 1,
            mtime_unix_ms: 1,
            prefix_hash: 1,
            rows,
        },
    );
    cache
}

#[test]
fn assigns_rows_to_exact_half_open_boundaries() {
    let reset_at = utc("2026-09-21T12:00:00Z");
    let current_start = reset_at - Duration::days(7);
    let older_start = current_start - Duration::days(7);
    let cache = cache_with_source_rows(vec![
        row(older_start, 10, 1, Some("gpt-5.5")),
        row(current_start, 20, 2, Some("gpt-5.5")),
        row(reset_at, 30, 3, Some("gpt-5.5")),
    ]);

    let windows = codex_quota_windows_from_cache(
        &cache,
        Some(&weekly_window(reset_at)),
        &[],
        utc("2026-09-20T12:00:00Z"),
        2,
    );

    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].start, current_start);
    assert_eq!(windows[0].end, reset_at);
    assert_eq!(windows[0].total_tokens, Some(22));
    assert_eq!(windows[0].entry_count, 1);
    assert_eq!(windows[1].start, older_start);
    assert_eq!(windows[1].end, current_start);
    assert_eq!(windows[1].total_tokens, Some(11));
    assert_eq!(windows[1].entry_count, 1);
}

#[test]
fn deduplicates_source_rows_by_file_identity() {
    let reset_at = utc("2026-09-21T12:00:00Z");
    let timestamp = utc("2026-09-19T12:00:00Z");
    let source = CodexSourceRowCache {
        file_identity: "same-file".to_string(),
        size: 1,
        mtime_unix_ms: 1,
        prefix_hash: 1,
        rows: vec![row(timestamp, 7, 3, Some("gpt-5.5"))],
    };
    let mut cache = cache_with_source_rows(Vec::new());
    cache.codex_source_rows = HashMap::from([
        ("first-path".to_string(), source.clone()),
        ("duplicate-path".to_string(), source),
    ]);

    let windows = codex_quota_windows_from_cache(
        &cache,
        Some(&weekly_window(reset_at)),
        &[],
        utc("2026-09-20T12:00:00Z"),
        1,
    );

    assert_eq!(windows[0].total_tokens, Some(10));
    assert_eq!(windows[0].entry_count, 1);
}

#[test]
fn token_and_cost_completeness_are_independent_without_pricing() {
    let reset_at = utc("2026-09-21T12:00:00Z");
    let cache = cache_with_source_rows(vec![row(utc("2026-09-19T12:00:00Z"), 100, 25, None)]);

    let windows = codex_quota_windows_from_cache(
        &cache,
        Some(&weekly_window(reset_at)),
        &[],
        utc("2026-09-20T12:00:00Z"),
        1,
    );

    assert_eq!(windows[0].total_tokens, Some(125));
    assert_eq!(windows[0].total_cost_usd, None);
    assert!(windows[0].tokens_are_complete);
    assert!(!windows[0].cost_is_complete);
}

#[test]
fn falls_back_to_legacy_daily_cache_when_source_rows_are_absent() {
    let reset_at = utc("2026-09-21T12:00:00Z");
    let mut cache = CostUsageCache {
        scan_since_key: Some("2026-09-01".to_string()),
        scan_until_key: Some("2026-09-21".to_string()),
        ..CostUsageCache::default()
    };
    cache.days = HashMap::from([(
        "2026-09-17".to_string(),
        HashMap::from([("gpt-5.5".to_string(), vec![100, 0, 25])]),
    )]);

    let windows = codex_quota_windows_from_cache(
        &cache,
        Some(&weekly_window(reset_at)),
        &[],
        utc("2026-09-20T12:00:00Z"),
        1,
    );

    assert_eq!(windows[0].total_tokens, Some(125));
    assert!(windows[0].total_cost_usd.is_some());
    assert!(windows[0].tokens_are_complete);
    assert!(windows[0].cost_is_complete);
    assert_eq!(windows[0].entry_count, 1);
}

#[test]
fn missing_live_reset_returns_no_windows() {
    let cache = cache_with_source_rows(vec![row(
        utc("2026-09-19T12:00:00Z"),
        100,
        25,
        Some("gpt-5.5"),
    )]);
    let live_window = RateWindow::new(0.0);

    assert!(
        codex_quota_windows_from_cache(
            &cache,
            Some(&live_window),
            &[],
            utc("2026-09-20T12:00:00Z"),
            1,
        )
        .is_empty()
    );
}

#[test]
fn moving_future_reset_forecasts_do_not_create_two_minute_history_windows() {
    let reset_at = utc("2026-10-29T17:20:00Z");
    let now = utc("2026-10-23T06:00:00Z");
    let mut cache = cache_with_source_rows(vec![row(
        utc("2026-10-21T12:00:00Z"),
        100,
        25,
        Some("gpt-5.5"),
    )]);
    cache.scan_since_key = Some("2026-09-23".to_string());
    cache.scan_until_key = Some("2026-10-23".to_string());
    let observations = [reset_at - Duration::minutes(2), reset_at];

    let windows = codex_quota_windows_from_cache(
        &cache,
        Some(&weekly_window(reset_at)),
        &observations,
        now,
        4,
    );
    assert_eq!(windows.len(), 4);
    for window in &windows {
        assert_eq!(window.end - window.start, Duration::days(7), "{window:?}");
    }
    assert_eq!(windows[1].total_tokens, Some(125));
}

#[test]
fn sparse_future_forecasts_do_not_invent_historical_resets() {
    let end = utc("2026-10-29T17:20:00Z");
    let boundaries = quota_boundaries(
        &[end - Duration::days(2), end - Duration::days(1), end],
        end,
        Duration::days(7),
        4,
        utc("2026-10-23T06:00:00Z"),
    );
    assert!(
        boundaries
            .windows(2)
            .all(|pair| pair[1] - pair[0] == Duration::days(7))
    );
}

#[test]
fn expired_forecast_jitter_stays_one_boundary_after_rollover() {
    let reset = utc("2026-10-29T17:21:00Z");
    let observations: Vec<_> = (0..=12)
        .map(|minute| reset - Duration::minutes(minute))
        .collect();
    let boundaries = quota_boundaries(
        &observations,
        reset + Duration::days(7),
        Duration::days(7),
        4,
        reset + Duration::days(1),
    );
    assert!(
        boundaries
            .windows(2)
            .all(|pair| pair[1] - pair[0] == Duration::days(7))
    );
}

#[test]
fn keeps_irregular_past_reset_without_inventing_a_short_tail() {
    let end = utc("2026-10-29T17:21:00Z");
    let past = utc("2026-10-14T03:35:09Z");
    let boundaries = quota_boundaries(
        &[past, end - Duration::days(2), end],
        end,
        Duration::days(7),
        4,
        utc("2026-10-23T06:00:00Z"),
    );
    assert!(boundaries.contains(&past));
    let current_start = boundaries
        .iter()
        .position(|date| *date == end - Duration::days(7))
        .unwrap();
    assert_eq!(boundaries[current_start - 1], past);
}

#[test]
fn fills_multiple_missing_weeks_between_past_observations() {
    let end = utc("2026-10-29T17:21:00Z");
    let boundaries = quota_boundaries(
        &[end - Duration::days(28)],
        end,
        Duration::days(7),
        4,
        utc("2026-10-23T06:00:00Z"),
    );
    assert_eq!(boundaries.len(), 5);
    assert!(
        boundaries
            .windows(2)
            .all(|pair| pair[1] - pair[0] == Duration::days(7))
    );
}
