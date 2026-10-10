//! Monthly token-plan window (upstream 0.66.0, `TokenPlanMonthlyWindowTests`).

use super::*;
use chrono::TimeZone;

const MONTHLY: &str = r#"{"per1MonthPercentage":0.25,"per1MonthResetTime":1791043200000}"#;

fn usage_for(payload: &str, subscription: Option<&str>, quota: Option<&str>) -> UsageSnapshot {
    let provider = QwenCloudProvider::new();
    let snapshot = QwenCloudProvider::parse(
        payload.as_bytes(),
        subscription.map(str::as_bytes),
        quota.map(str::as_bytes),
    )
    .unwrap();
    provider.snapshot_to_usage(snapshot).unwrap()
}

#[test]
fn monthly_only_payload_uses_labelled_primary_bar_with_quota_detail() {
    let usage = usage_for(
        MONTHLY,
        Some(r#"{"data":{"specCode":"standard"}}"#),
        Some(r#"{"standard":{"monthly":45000}}"#),
    );
    assert_eq!(usage.primary.used_percent, 25.0);
    assert_eq!(usage.primary.window_minutes, Some(43_200));
    assert_eq!(
        usage.primary.resets_at,
        Utc.timestamp_millis_opt(1_791_043_200_000).single()
    );
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("11,250 / 45,000 credits used")
    );
    assert_eq!(usage.primary_label.as_deref(), Some("Monthly"));
    assert!(usage.secondary.is_none());
    assert!(usage.extra_rate_windows.is_empty());
    assert_eq!(usage.login_method.as_deref(), Some("Standard"));
}

#[test]
fn monthly_alongside_rolling_windows_is_an_extra_row() {
    let usage = usage_for(
        r#"{"per5HourPercentage":0.1,"per1WeekPercentage":0.2,"per1MonthPercentage":0.3}"#,
        None,
        None,
    );
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert_eq!(usage.primary_label, None);
    assert_eq!(
        usage.secondary.as_ref().and_then(|w| w.window_minutes),
        Some(10_080)
    );
    let [extra] = usage.extra_rate_windows.as_slice() else {
        panic!("expected exactly one extra window");
    };
    assert_eq!(extra.title, "Monthly");
    assert_eq!(extra.window.window_minutes, Some(43_200));
    assert!((extra.window.used_percent - 30.0).abs() < 1e-9);
}

#[test]
fn weekly_plus_monthly_keeps_weekly_primary_label() {
    let usage = usage_for(
        r#"{"per1WeekPercentage":0.2,"per1MonthPercentage":0.3}"#,
        None,
        None,
    );
    assert_eq!(usage.primary.window_minutes, Some(10_080));
    assert_eq!(usage.primary_label.as_deref(), Some("Weekly"));
    assert_eq!(usage.extra_rate_windows.len(), 1);
}
