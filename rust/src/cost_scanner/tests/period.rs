//! Reporting-period behavior of the scanner: window resolution and the Codex
//! partition walk for month-to-date and all-available history.

use super::*;
use crate::cost_reporting_period::CostReportingPeriod;
use chrono::{Datelike, TimeZone};

fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, 0, 0).single().unwrap()
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// Writes one Codex session under the `YYYY/MM/DD` partition for `day`.
fn write_session_on(sessions_root: &Path, day: NaiveDate, input_tokens: u64) {
    let day_dir = partition_dir(sessions_root, day);
    std::fs::create_dir_all(&day_dir).unwrap();
    // Local noon keeps the record on `day` in the local zone.
    let local_noon = Local
        .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    let ts = local_noon.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let body = format!(
        r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{input_tokens},"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
    );
    std::fs::write(day_dir.join(format!("s-{input_tokens}.jsonl")), body).unwrap();
}

fn scan_input_tokens(period: CostReportingPeriod, sessions: &Path, cache: &Path) -> u64 {
    let scanner = CostScanner::for_period(period)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(cache)
        .with_sessions_dirs(vec![sessions.to_path_buf()]);
    let (summary, _) = scanner.scan_codex_detailed(None);
    summary.input_tokens
}

#[test]
fn for_period_rolling_matches_new() {
    assert_eq!(
        CostScanner::new(14).period(),
        CostScanner::for_period(CostReportingPeriod::Rolling(14)).period()
    );
}

#[test]
fn zero_day_scanner_still_resolves_a_one_day_window() {
    let now = Utc::now();
    let window = CostScanner::new(0).calendar_window(now, None);
    assert_eq!(window.days, 1);
    assert_eq!(window.start, window.end);
}

#[test]
fn transcript_window_rolling_keeps_the_legacy_shape() {
    let now = utc(2026, 5, 15, 12);
    let today = date(2026, 5, 15);
    let window = CostScanner::new(30).transcript_window(now, today);
    assert_eq!(window.start, date(2026, 4, 15));
    assert_eq!(window.end, today);
    assert_eq!(window.cutoff, now - Duration::days(30));
    assert_eq!(window.days, 30);
}

#[test]
fn transcript_window_month_to_date_starts_at_the_month() {
    let now = Utc::now();
    let scanner = CostScanner::for_period(CostReportingPeriod::MonthToDate);
    let window = scanner.transcript_window(now, now.date_naive());
    let local_today = Local::now().date_naive();
    assert_eq!(window.start.day(), 1);
    assert_eq!(window.start.month(), local_today.month());
    assert!(window.cutoff <= now);
    assert!(window.days >= 1 && window.days <= 31);
}

#[test]
fn calendar_window_all_uses_the_earliest_day_without_clamping() {
    let now = Utc::now();
    let earliest = date(2000, 1, 1);
    let scanner = CostScanner::for_period(CostReportingPeriod::AllAvailable);
    let window = scanner.calendar_window(now, Some(earliest));
    assert_eq!(window.start, earliest);
    assert!(window.days > 365 * 20);
}

#[test]
fn first_partition_skips_years_before_the_first_existing_one() {
    let root = tempfile::tempdir().unwrap();
    for name in ["2024", "2023", "0500", "abcd", "20240"] {
        std::fs::create_dir_all(root.path().join(name)).unwrap();
    }
    std::fs::write(root.path().join("1999"), b"file, not a partition").unwrap();
    assert_eq!(
        codex::first_codex_partition_date(&[root.path().to_path_buf()]),
        Some(date(2023, 1, 1))
    );
}

#[test]
fn first_partition_is_none_for_empty_or_missing_roots() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        codex::first_codex_partition_date(&[root.path().to_path_buf()]),
        None
    );
    assert_eq!(
        codex::first_codex_partition_date(&[root.path().join("missing")]),
        None
    );
}

#[test]
fn codex_all_available_reads_history_beyond_a_year() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let today = Local::now().date_naive();
    write_session_on(&sessions, today, 100);
    write_session_on(&sessions, today - Duration::days(500), 1_000);

    let rolling = scan_input_tokens(
        CostReportingPeriod::Rolling(30),
        &sessions,
        &root.path().join("cache-rolling"),
    );
    let all = scan_input_tokens(
        CostReportingPeriod::AllAvailable,
        &sessions,
        &root.path().join("cache-all"),
    );
    assert_eq!(rolling, 100, "rolling 30 excludes the 500 day old session");
    assert_eq!(all, 1_100, "all available includes the old partition");
}

#[test]
fn codex_month_to_date_excludes_the_previous_month() {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let today = Local::now().date_naive();
    let month_start = today.with_day(1).unwrap();
    write_session_on(&sessions, today, 100);
    write_session_on(&sessions, month_start - Duration::days(3), 1_000);

    let mtd = scan_input_tokens(
        CostReportingPeriod::MonthToDate,
        &sessions,
        &root.path().join("cache-mtd"),
    );
    assert_eq!(mtd, 100);
}
