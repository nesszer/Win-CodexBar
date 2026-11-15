use super::build::build_spend_trend;
use super::cache_key_tests::empty_summary;
use super::*;
use codexbar::cost_scanner::CostSummary;

fn activity_day(day: &str, total_tokens: Option<u64>) -> TokenActivityDay {
    TokenActivityDay {
        day: day.to_string(),
        total_tokens,
        is_scanned: true,
    }
}

fn trend_row(provider_id: &str, currency: &str, daily: &[(&str, f64)]) -> UsageSpendRow {
    UsageSpendRow {
        provider_id: provider_id.to_string(),
        display_name: provider_id.to_uppercase(),
        seven_day: None,
        thirty_day: None,
        seven_day_estimate: None,
        thirty_day_estimate: None,
        seven_day_tokens: None,
        thirty_day_tokens: None,
        seven_day_tokens_lower_bound: false,
        thirty_day_tokens_lower_bound: false,
        period_cost: None,
        period_tokens: None,
        currency: currency.to_string(),
        source: "fixture".to_string(),
        included_in_overview: provider_id != "pi",
        daily: daily
            .iter()
            .map(|(day, amount)| UsageSpendDailyPoint {
                day: (*day).to_string(),
                amount: *amount,
            })
            .collect(),
        refreshing: false,
        stale_updated_at: None,
    }
}

fn cached_summary(key: &str, owner: Option<UsageSpendRefreshOwner>) -> CachedUsageSpendSummary {
    CachedUsageSpendSummary {
        key: key.to_string(),
        summary: empty_summary(),
        refresh_owner: owner,
    }
}

#[test]
fn only_one_summary_build_runs_at_a_time() {
    let mut coordinator = UsageSpendCoordinator::default();

    assert!(matches!(
        coordinator.claim_build("key:a", false),
        UsageSpendBuildClaim::Build
    ));
    // Nothing to serve yet: a second request waits instead of scanning again.
    assert!(matches!(
        coordinator.claim_build("key:b", false),
        UsageSpendBuildClaim::Wait
    ));

    coordinator.release_build();
    assert!(matches!(
        coordinator.claim_build("key:b", false),
        UsageSpendBuildClaim::Build
    ));
}

#[test]
fn request_during_a_build_reuses_the_last_summary_without_its_owner() {
    let mut coordinator = UsageSpendCoordinator::default();
    let owner = coordinator.begin("key:old".to_string());
    coordinator.cache = Some(cached_summary("key:old", Some(owner)));
    assert!(matches!(
        coordinator.claim_build("key:new", false),
        UsageSpendBuildClaim::Build
    ));

    // The provider snapshots moved on (new key) while the build runs.
    match coordinator.claim_build("key:newer", false) {
        UsageSpendBuildClaim::Reuse(built) => {
            assert_eq!(built.key, "key:old");
            assert!(built.refresh_owner.is_none());
        }
        other => panic!("expected the stale summary, got {other:?}"),
    }
    assert!(coordinator.building);
}

#[test]
fn forced_request_waits_for_the_running_build() {
    let mut coordinator = UsageSpendCoordinator {
        cache: Some(cached_summary("key:a", None)),
        ..UsageSpendCoordinator::default()
    };
    assert!(matches!(
        coordinator.claim_build("key:b", false),
        UsageSpendBuildClaim::Build
    ));

    assert!(matches!(
        coordinator.claim_build("key:a", true),
        UsageSpendBuildClaim::Wait
    ));
}

#[test]
fn matching_cache_is_reused_even_while_building() {
    let mut coordinator = UsageSpendCoordinator {
        cache: Some(cached_summary("key:a", None)),
        ..UsageSpendCoordinator::default()
    };
    assert!(matches!(
        coordinator.claim_build("key:b", false),
        UsageSpendBuildClaim::Build
    ));

    match coordinator.claim_build("key:a", false) {
        UsageSpendBuildClaim::Reuse(built) => assert_eq!(built.key, "key:a"),
        other => panic!("expected the cached summary, got {other:?}"),
    }
}

#[test]
fn invalidated_owner_clears_orphaned_indexing_activity() {
    let mut coordinator = UsageSpendCoordinator::default();
    let owner = coordinator.begin("account:old".to_string());

    assert!(coordinator.clear_if_indexing(&owner));
    assert!(!coordinator.is_current(&owner));
}

#[test]
fn old_owner_cleanup_cannot_clear_a_replacement() {
    let mut coordinator = UsageSpendCoordinator::default();
    let old = coordinator.begin("account:old".to_string());
    let replacement = coordinator.begin("account:new".to_string());

    assert!(!coordinator.clear_if_indexing(&old));
    assert!(coordinator.is_current(&replacement));
}

#[test]
fn settings_replacement_preserves_an_intentional_pause() {
    let mut coordinator = UsageSpendCoordinator::default();
    let old = coordinator.begin("settings:old".to_string());
    let replacement = coordinator.begin("settings:new".to_string());
    let status = codexbar::core::CachedCostReadStatus {
        codex_scan_pause_reason: Some(codexbar::core::CodexScanPauseReason::NoProgress),
        ..Default::default()
    };
    mark_refresh_paused_if_codex_scan_paused(
        &mut coordinator,
        &replacement,
        true,
        status.codex_scan_pause_reason.as_ref(),
    );

    assert!(!coordinator.clear_if_indexing(&old));
    assert_eq!(
        coordinator.current.as_ref().map(|(_, phase)| *phase),
        Some(UsageSpendRefreshPhase::Paused)
    );
}

#[test]
fn privacy_mode_is_part_of_usage_spend_cache_identity() {
    let public = usage_spend_cache_key_with_privacy(&[], "rolling:30", false, false, false);
    let private = usage_spend_cache_key_with_privacy(&[], "rolling:30", false, false, true);
    assert_ne!(public, private);
}

#[test]
fn selected_period_scan_reuses_fixed_windows_and_scans_other_periods() {
    let seven = CostSummary {
        total_cost_usd: 7.0,
        ..CostSummary::default()
    };
    let thirty = CostSummary {
        total_cost_usd: 30.0,
        ..CostSummary::default()
    };
    let mut scanned = false;
    let selected = selected_period_scan(CostReportingPeriod::Rolling(7), &seven, &thirty, || {
        scanned = true;
        CostSummary::default()
    });
    assert_eq!(selected.total_cost_usd, 7.0);
    assert!(!scanned);

    let selected = selected_period_scan(CostReportingPeriod::Rolling(90), &seven, &thirty, || {
        scanned = true;
        CostSummary {
            total_cost_usd: 90.0,
            ..CostSummary::default()
        }
    });
    assert_eq!(selected.total_cost_usd, 90.0);
    assert!(scanned);
}

fn identity(period: CostReportingPeriod, now: &str) -> String {
    let now = chrono::DateTime::parse_from_rfc3339(now)
        .expect("valid instant")
        .with_timezone(&Utc);
    period.identity(now, CostTimeZone::UTC)
}

#[test]
fn selected_period_is_part_of_usage_spend_cache_identity() {
    let now = "2026-09-15T12:00:00Z";
    let month = identity(CostReportingPeriod::MonthToDate, now);
    let rolling = identity(CostReportingPeriod::Rolling(30), now);
    let all = identity(CostReportingPeriod::AllAvailable, now);
    assert_ne!(month, rolling);
    assert_ne!(rolling, all);
    assert_ne!(
        usage_spend_cache_key_with_privacy(&[], &month, false, false, false),
        usage_spend_cache_key_with_privacy(&[], &rolling, false, false, false)
    );
}

#[test]
fn month_to_date_identity_changes_across_a_month_boundary() {
    let last_day = identity(CostReportingPeriod::MonthToDate, "2026-09-30T23:00:00Z");
    let first_day = identity(CostReportingPeriod::MonthToDate, "2026-10-01T01:00:00Z");
    assert_ne!(last_day, first_day);
    assert_ne!(
        usage_spend_cache_key_with_privacy(&[], &last_day, false, false, false),
        usage_spend_cache_key_with_privacy(&[], &first_day, false, false, false)
    );
}

#[test]
fn daily_period_cost_follows_the_selected_window() {
    let daily = ["2026-09-01", "2026-09-10", "2026-08-31"]
        .into_iter()
        .map(|day| super::super::bridge::CostDailyPointBridge {
            day: day.to_string(),
            amount: 1.0,
        })
        .collect::<Vec<_>>();
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-15T12:00:00Z")
        .expect("valid instant")
        .with_timezone(&Utc);
    assert_eq!(
        period_cost_from_daily(&daily, CostReportingPeriod::MonthToDate, now),
        Some(2.0)
    );
    assert_eq!(
        period_cost_from_daily(&daily, CostReportingPeriod::AllAvailable, now),
        Some(3.0)
    );
    assert_eq!(
        period_cost_from_daily(&daily, CostReportingPeriod::Rolling(3), now),
        None
    );
}

#[test]
fn long_provider_daily_history_does_not_widen_the_fixed_columns() {
    // Bedrock reports 14 months of daily spend; the 7d / 30d columns keep
    // their windows while the selected period still sees the whole history.
    let today = Utc::now().date_naive();
    let day = |ago: i64| {
        serde_json::json!({
            "day": (today - chrono::Duration::days(ago)).format("%Y-%m-%d").to_string(),
            "amount": 1.0,
        })
    };
    let cost: super::super::bridge::CostSnapshotBridge =
        serde_json::from_value(serde_json::json!({
            "used": 99.0,
            "period": "Monthly",
            "daily": [day(0), day(6), day(7), day(29), day(30), day(200)],
        }))
        .expect("cost fixture");
    let mut snapshot = ProviderUsageSnapshot::from_error(
        codexbar::core::ProviderId::Bedrock,
        codexbar::core::instantiate_provider(codexbar::core::ProviderId::Bedrock).metadata(),
        "unused".to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    snapshot.cost = Some(cost);

    let spend = cached_spend(
        Some(&snapshot),
        CostReportingPeriod::AllAvailable,
        Utc::now(),
    );
    assert_eq!(spend.seven_day, Some(2.0));
    assert_eq!(spend.thirty_day, Some(4.0));
    assert_eq!(spend.period_cost, Some(6.0));
}

#[test]
fn pi_history_is_an_alternate_view_not_a_shared_overview_source() {
    assert!(!include_in_shared_overview("pi", true, true));
    assert!(include_in_shared_overview("codex", true, false));
    assert!(include_in_shared_overview("claude", false, true));
    assert!(!include_in_shared_overview("codex", false, false));
}

#[test]
fn cost_tracking_off_summary_is_empty_and_flagged() {
    let summary = cost_tracking_off_summary(CostReportingPeriod::MonthToDate);
    assert!(!summary.cost_tracking_enabled);
    assert!(summary.rows.is_empty());
    assert!(summary.token_activity.is_empty());
    assert_eq!(summary.reporting_period, "month-to-date");
    assert_eq!(summary.contract.known_cost_usd, None);
    assert!(summary.contract.daily.is_empty());
    let json = serde_json::to_value(&summary).expect("serialize");
    assert_eq!(json["costTrackingEnabled"], serde_json::json!(false));
}

#[test]
fn spend_trend_publishes_each_usd_source_once_inside_the_window() {
    let now = chrono::Utc::now();
    let zone = codexbar::cost_reporting_period::cost_bucket_zone();
    let today = zone.date(now);
    let key = |offset: i64| {
        (today - chrono::Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string()
    };
    let mut codex = empty_summary().contract;
    codex.known_zero = true;
    codex.daily = vec![codexbar::spend_contract::SpendDailyPoint {
        day: key(1),
        cost_usd: Some(2.0),
        total_tokens: Some(10),
    }];
    let (old, recent) = (key(40), key(2));
    let rows = vec![
        trend_row("codex", "USD", &[(recent.as_str(), 99.0)]),
        trend_row("claude", "USD", &[]),
        trend_row(
            "openrouter",
            "USD",
            &[(old.as_str(), 5.0), (recent.as_str(), 3.0)],
        ),
        trend_row("mistral", "EUR", &[(recent.as_str(), 4.0)]),
        trend_row("pi", "USD", &[(recent.as_str(), 1.0)]),
    ];
    let claude = Some((false, vec![(key(0), Some(1.5))], Vec::new()));
    let trend = build_spend_trend(&rows, &codex, claude, CostReportingPeriod::Rolling(30), now);

    let ids: Vec<_> = trend.sources.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["claude", "codex", "openrouter"]);
    assert!(
        trend
            .sources
            .iter()
            .find(|s| s.id == "codex")
            .unwrap()
            .known_zero
    );
    // Codex comes from local logs, never the snapshot's daily series.
    let codex_days: Vec<_> = trend
        .daily
        .iter()
        .filter(|p| p.source_id == "codex")
        .map(|p| (p.day.clone(), p.cost_usd))
        .collect();
    assert_eq!(codex_days, vec![(key(1), Some(2.0))]);
    // The 40-day-old point falls outside a 30-day window.
    assert!(
        trend
            .daily
            .iter()
            .filter(|p| p.source_id == "openrouter")
            .all(|p| p.day == recent)
    );
    assert_eq!(trend.start_day, key(29));
    assert_eq!(trend.end_day, key(0));
}

#[test]
fn token_activity_keeps_listed_local_log_rows_only() {
    let listed: BTreeSet<String> = ["codex".to_string(), "pi".to_string()].into();
    let sources = token_activity_sources(
        &listed,
        vec![activity_day("2026-10-01", Some(5))],
        vec![activity_day("2026-10-01", Some(7))],
    );
    let ids: Vec<_> = sources
        .iter()
        .map(|source| source.provider_id.as_str())
        .collect();
    assert_eq!(ids, ["codex"]);

    let both: BTreeSet<String> = ["codex".to_string(), "claude".to_string()].into();
    let sources = token_activity_sources(&both, Vec::new(), vec![activity_day("d", None)]);
    let json = serde_json::to_value(&sources).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!([{
            "providerId": "claude",
            "days": [{"day": "d", "totalTokens": null, "isScanned": true}],
        }])
    );
}
