use super::{
    CostFetchFailure, ProviderLocalUsageSummary, cost_fetch_failure_allows_early_retry,
    estimate_note_for, local_usage_summary_from_cost_summary, local_usage_summary_period_identity,
    localized_estimate_note, muse_local_usage_summary, token_cost_cache_is_fresh,
};
use crate::commands::is_provider_cache_fresh;
use codexbar::cost_reporting_period::{CostReportingPeriod, CostTimeZone};
use codexbar::cost_scanner::{CostSummary, TodayUsage};
use codexbar::providers::muse::local_usage::{DailyUsage, Report};
use codexbar::settings::Language;
use codexbar::spend_contract::LocalHistoryCoverage;
use std::time::{Duration, Instant};

#[test]
fn token_cost_age_does_not_use_provider_quota_age() {
    let now = Instant::now();
    let token_loaded = now - Duration::from_secs(31);
    let provider_updated = now;
    assert!(!token_cost_cache_is_fresh(
        Some(token_loaded),
        now,
        Duration::from_secs(30)
    ));
    assert!(is_provider_cache_fresh(
        Some(provider_updated),
        Duration::from_secs(30)
    ));
}

#[test]
fn fast_cost_failures_allow_the_next_pass_to_retry() {
    assert!(cost_fetch_failure_allows_early_retry(
        CostFetchFailure::Failed
    ));
    assert!(!cost_fetch_failure_allows_early_retry(
        CostFetchFailure::TimedOut
    ));
}

#[test]
fn local_usage_summary_serializes_token_cost_timestamp() {
    let summary = ProviderLocalUsageSummary {
        today_cost: Some(1.0),
        thirty_day_cost: Some(2.0),
        thirty_day_tokens: Some(300),
        period_cost: Some(3.0),
        period_tokens: Some(400),
        reporting_period: "month-to-date".to_string(),
        latest_tokens: Some(40),
        top_model: Some("gpt-5".to_string()),
        estimate_note: "estimated".to_string(),
        token_cost_updated_at_ms: 1234,
        incomplete_request_count: None,
    };

    let json = serde_json::to_value(summary).expect("serialize summary");
    assert_eq!(
        json.get("tokenCostUpdatedAtMs").and_then(|v| v.as_i64()),
        Some(1234)
    );
}

#[test]
fn local_usage_cache_identity_uses_the_summary_period_and_timestamp() {
    let scanned_at = chrono::DateTime::parse_from_rfc3339("2026-09-30T23:00:00Z")
        .expect("valid timestamp")
        .with_timezone(&chrono::Utc);
    let summary = ProviderLocalUsageSummary {
        today_cost: None,
        thirty_day_cost: None,
        thirty_day_tokens: None,
        period_cost: None,
        period_tokens: None,
        reporting_period: "month-to-date".to_string(),
        latest_tokens: None,
        top_model: None,
        estimate_note: String::new(),
        token_cost_updated_at_ms: scanned_at.timestamp_millis(),
        incomplete_request_count: None,
    };

    assert_eq!(
        local_usage_summary_period_identity(&summary),
        Some(CostReportingPeriod::MonthToDate.identity(scanned_at, CostTimeZone::Local))
    );
}

#[test]
fn muse_local_usage_summary_exposes_complete_tokens_without_cost() {
    let report = Report {
        daily: vec![DailyUsage {
            day: "2026-09-20".to_string(),
            input_tokens: 10,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 12,
            request_count: 1,
            models: vec![("muse-spark-1.3".to_string(), 12)],
        }],
        total_tokens: Some(12),
        today_tokens: Some(12),
        session_count: 1,
        top_model: Some("muse-spark-1.3".to_string()),
        coverage: LocalHistoryCoverage::Complete,
    };
    let period = CostReportingPeriod::Rolling(30);
    let summary = muse_local_usage_summary(
        &report,
        &report,
        period,
        codexbar::settings::Language::default(),
    )
    .expect("complete history is visible");
    assert_eq!(summary.period_tokens, Some(12));
    assert_eq!(summary.reporting_period, "rolling:30");
    assert_eq!(summary.today_cost, None);
    assert_eq!(summary.thirty_day_cost, None);
    assert_eq!(summary.thirty_day_tokens, Some(12));
    assert_eq!(summary.latest_tokens, Some(12));
    assert_eq!(summary.top_model.as_deref(), Some("muse-spark-1.3"));

    let partial = Report {
        coverage: LocalHistoryCoverage::Partial,
        ..report
    };
    assert!(
        muse_local_usage_summary(
            &partial,
            &partial,
            period,
            codexbar::settings::Language::default()
        )
        .is_none()
    );
}

#[test]
fn local_usage_keeps_thirty_day_fixed_while_period_follows_selection() {
    let thirty = CostSummary {
        total_cost_usd: 30.0,
        input_tokens: 3_000,
        output_tokens: 0,
        ..CostSummary::default()
    };
    let month = CostSummary {
        total_cost_usd: 5.0,
        input_tokens: 400,
        output_tokens: 100,
        ..CostSummary::default()
    };
    let summary = local_usage_summary_from_cost_summary(
        "claude",
        &thirty,
        CostReportingPeriod::MonthToDate,
        Some(&month),
        Some(&TodayUsage {
            cost_usd: Some(1.25),
            tokens: 42,
        }),
    )
    .expect("usage is visible");
    assert_eq!(summary.today_cost, Some(1.25));
    assert_eq!(summary.latest_tokens, Some(42));
    assert_eq!(summary.thirty_day_cost, Some(30.0));
    assert_eq!(summary.thirty_day_tokens, Some(3_000));
    assert_eq!(summary.period_cost, Some(5.0));
    assert_eq!(summary.period_tokens, Some(500));
    assert_eq!(summary.reporting_period, "month-to-date");
}

#[test]
fn japanese_estimate_note_is_localized() {
    assert_eq!(
        localized_estimate_note("codex", Language::Japanese),
        "ローカルログから推定したもので、請求書と異なる場合があります"
    );
    assert_eq!(
        localized_estimate_note("claude", Language::Japanese),
        "ClaudeのローカルログからAPIレートで推定したもので、トークン総数が請求書と異なる場合があります"
    );
}

#[test]
fn english_estimate_note_is_localized() {
    assert_eq!(
        localized_estimate_note("codex", Language::English),
        "Estimated from local logs; may differ from your bill"
    );
    assert_eq!(
        localized_estimate_note("claude", Language::English),
        "Estimated from local Claude logs at API rates; token totals may differ from your bill"
    );
}

#[test]
fn recorded_opencode_spend_replaces_the_estimate_note() {
    let native = CostSummary::default();
    let recorded = CostSummary {
        recorded_cost_rows: 2,
        ..CostSummary::default()
    };
    assert_eq!(
        estimate_note_for("codex", &native, Language::English),
        "Estimated from local logs; may differ from your bill"
    );
    assert_eq!(
        estimate_note_for("codex", &recorded, Language::English),
        "Estimated from local logs, plus recorded OpenCode spend; may differ from your bill"
    );
}

#[test]
fn local_usage_today_hides_known_zero_and_unknown_cost_but_keeps_tokens() {
    let thirty = CostSummary {
        total_cost_usd: 30.0,
        input_tokens: 3_000,
        ..CostSummary::default()
    };
    let summary_for = |today: Option<&TodayUsage>| {
        local_usage_summary_from_cost_summary(
            "claude",
            &thirty,
            CostReportingPeriod::Rolling(30),
            None,
            today,
        )
        .expect("usage is visible")
    };
    let zero = summary_for(Some(&TodayUsage {
        cost_usd: Some(0.0),
        tokens: 0,
    }));
    assert_eq!((zero.today_cost, zero.latest_tokens), (None, None));
    let unpriced = summary_for(Some(&TodayUsage {
        cost_usd: None,
        tokens: 1_500,
    }));
    assert_eq!(
        (unpriced.today_cost, unpriced.latest_tokens),
        (None, Some(1_500))
    );
    let absent = summary_for(None);
    assert_eq!((absent.today_cost, absent.latest_tokens), (None, None));
}

#[test]
fn a_scan_that_finishes_after_track_costs_went_off_is_not_cached() {
    use super::local_usage_cache::{
        cached_provider_local_usage_summary, store_scanned_local_usage_summary,
    };
    let summary = || ProviderLocalUsageSummary {
        today_cost: Some(1.25),
        thirty_day_cost: Some(12.5),
        thirty_day_tokens: Some(42_000),
        period_cost: Some(12.5),
        period_tokens: Some(42_000),
        reporting_period: "month-to-date".to_string(),
        latest_tokens: None,
        top_model: None,
        estimate_note: "estimated".to_string(),
        token_cost_updated_at_ms: 1234,
        incomplete_request_count: None,
    };

    store_scanned_local_usage_summary("track-off-race", Some(summary()), false);
    assert!(cached_provider_local_usage_summary("track-off-race").is_none());

    store_scanned_local_usage_summary("track-on-scan", Some(summary()), true);
    assert_eq!(
        cached_provider_local_usage_summary("track-on-scan").and_then(|s| s.today_cost),
        Some(1.25)
    );
}
