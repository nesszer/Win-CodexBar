use super::*;

fn local_history(
    total_tokens: u64,
    coverage: codexbar::spend_contract::LocalHistoryCoverage,
    known_subtotal_usd: Option<f64>,
    unpriced: u32,
) -> codexbar::spend_contract::LocalTokenHistorySummary {
    codexbar::spend_contract::LocalTokenHistorySummary {
        total_tokens,
        session_count: if total_tokens > 0 { 1 } else { 0 },
        coverage,
        cost_estimate: codexbar::spend_contract::LocalCostEstimate {
            known_subtotal_usd,
            coverage: codexbar::spend_contract::CostCoverageCounts {
                estimated: if known_subtotal_usd.is_some() { 1 } else { 0 },
                unpriced,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
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

fn empty_summary() -> UsageSpendSummary {
    UsageSpendSummary {
        reporting_period: "rolling:30".to_string(),
        rows: Vec::new(),
        contract: SpendContract {
            provider_id: "codex".to_string(),
            reporting_period: "rolling:30".to_string(),
            history_days: 30,
            known_cost_usd: None,
            known_zero: false,
            provenance: codexbar::spend_contract::CostProvenance::Unknown,
            price_coverage: Default::default(),
            price_coverage_ratio: None,
            history_coverage_established: false,
            token_mix: Default::default(),
            token_total: None,
            conversation_count: 0,
            models: Vec::new(),
            projects: Vec::new(),
            conversations: Vec::new(),
            daily: Vec::new(),
            hourly_activity: Vec::new(),
            project_source_status: None,
            custom_pricing_active: false,
            imports: Vec::new(),
        },
        reporting_day: "2026-10-01".to_string(),
        dashboard_timezone: "UTC".to_string(),
    }
}

#[test]
fn only_a_summary_rebuild_may_refresh_opencodex_pricing() {
    let mut coordinator = UsageSpendCoordinator::default();
    assert!(coordinator.reusable("day|30", false).is_none());
    coordinator.cache = Some(CachedUsageSpendSummary {
        key: "day|30".to_string(),
        summary: empty_summary(),
        refresh_owner: None,
    });
    // A cached read starts no network work; a forced or new key rebuilds.
    assert!(coordinator.reusable("day|30", false).is_some());
    assert!(coordinator.reusable("day|30", true).is_none());
    assert!(coordinator.reusable("day|7", false).is_none());
}

#[test]
fn privacy_mode_is_part_of_usage_spend_cache_identity() {
    let public = usage_spend_cache_key_with_privacy(&[], "rolling:30", false, false, false);
    let private = usage_spend_cache_key_with_privacy(&[], "rolling:30", false, false, true);
    assert_ne!(public, private);
}

#[test]
fn pi_history_is_an_alternate_view_not_a_shared_overview_source() {
    assert!(!include_in_shared_overview("pi", true, true));
    assert!(include_in_shared_overview("codex", true, false));
    assert!(include_in_shared_overview("claude", false, true));
    assert!(!include_in_shared_overview("codex", false, false));
}

#[test]
fn antigravity_partial_history_exposes_only_the_known_subtotal() {
    use codexbar::spend_contract::LocalHistoryCoverage;

    let seven = local_history(100, LocalHistoryCoverage::Partial, Some(1.25), 0);
    let thirty = local_history(200, LocalHistoryCoverage::Partial, Some(2.50), 0);
    let spend = antigravity_spend_values(
        cached_spend(None, CostReportingPeriod::Rolling(30), chrono::Utc::now()),
        &seven,
        &thirty,
    );

    assert_eq!(spend.seven_day, None);
    assert_eq!(spend.thirty_day, None);
    assert_eq!(spend.seven_day_tokens, None);
    assert_eq!(spend.thirty_day_tokens, None);
    assert!(spend.source.contains("known API list-price subtotal"));
}

#[test]
fn antigravity_lower_bound_history_publishes_floors_not_exact_totals() {
    use codexbar::spend_contract::LocalHistoryCoverage;

    let mut seven = local_history(100, LocalHistoryCoverage::Partial, Some(1.25), 0);
    seven.lower_bound = true;
    let withheld = local_history(0, LocalHistoryCoverage::Partial, None, 0);
    let spend = antigravity_spend_values(
        cached_spend(None, CostReportingPeriod::Rolling(30), chrono::Utc::now()),
        &seven,
        &withheld,
    );

    assert_eq!(spend.seven_day, None);
    assert_eq!(spend.seven_day_tokens, Some(100));
    assert_eq!(spend.thirty_day, None);
    assert_eq!(spend.thirty_day_tokens, None);
}

#[test]
fn antigravity_complete_empty_history_is_a_known_zero() {
    use codexbar::spend_contract::LocalHistoryCoverage;

    let seven = local_history(0, LocalHistoryCoverage::Complete, None, 0);
    let thirty = local_history(0, LocalHistoryCoverage::Complete, None, 0);
    let spend = antigravity_spend_values(
        cached_spend(None, CostReportingPeriod::Rolling(30), chrono::Utc::now()),
        &seven,
        &thirty,
    );

    assert_eq!(spend.seven_day, Some(0.0));
    assert_eq!(spend.thirty_day, Some(0.0));
    assert_eq!(spend.seven_day_tokens, Some(0));
    assert_eq!(spend.thirty_day_tokens, Some(0));
    assert!(spend.source.contains("API list-price estimate"));
}
