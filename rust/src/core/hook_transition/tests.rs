use super::*;
use chrono::TimeZone;
use std::path::PathBuf;

const PROVIDER: &str = "codex";

fn lane_key(window: HookQuotaWindow, account: Option<&str>) -> HookQuotaLaneKey {
    HookQuotaLaneKey::new(PROVIDER, window, account.map(str::to_string), None)
}

fn rate_window(used_percent: f64, resets_at: Option<DateTime<Utc>>) -> RateWindow {
    let mut w = RateWindow::new(used_percent);
    w.window_minutes = Some(300);
    w.resets_at = resets_at;
    w
}

fn informational_window(used_percent: f64) -> RateWindow {
    let mut w = RateWindow::informational("placeholder");
    w.used_percent = used_percent;
    w
}

fn lane(
    used_percent: Option<f64>,
    key: HookQuotaLaneKey,
    resets_at: Option<DateTime<Utc>>,
    thresholds: &[f64],
    informational: bool,
) -> HookQuotaLaneObservation {
    let label = key.window.display_name().to_string();
    let account = key.account_discriminator.clone();
    HookQuotaLaneObservation {
        key,
        label,
        rate_window: used_percent.map(|p| {
            if informational {
                informational_window(p)
            } else {
                rate_window(p, resets_at)
            }
        }),
        fallback_thresholds: thresholds.to_vec(),
        account_display_name: account,
    }
}

fn observation(
    lanes: Vec<HookQuotaLaneObservation>,
    status: HookProviderStatus,
    refresh_failure: Option<&str>,
) -> HookProviderObservation {
    HookProviderObservation {
        provider: PROVIDER.into(),
        lanes,
        status,
        refresh_failure_status: refresh_failure.map(str::to_string),
        account_display_name: None,
        successful_usage: None,
        account_discriminator: None,
    }
}

fn rule(event: HookEventType, threshold: Option<f64>, provider: Option<&str>) -> HookRule {
    HookRule {
        enabled: true,
        event: Some(event),
        events: Vec::new(),
        provider: provider.map(str::to_string),
        threshold,
        executable: PathBuf::from("/bin/true"),
        arguments: Vec::new(),
        timeout_secs: 10,
    }
}

fn config(enabled: bool, rules: Option<Vec<HookRule>>) -> HooksConfig {
    HooksConfig {
        enabled,
        events: rules.unwrap_or_else(|| {
            vec![
                rule(HookEventType::QuotaLow, None, None),
                rule(HookEventType::QuotaReached, None, None),
                rule(HookEventType::QuotaReset, None, None),
                rule(HookEventType::ProviderUnavailable, None, None),
                rule(HookEventType::ProviderRecovered, None, None),
                rule(HookEventType::RefreshFailed, None, None),
            ]
        }),
    }
}

fn events_of(dispatches: &[HookDispatch]) -> Vec<HookEventType> {
    dispatches.iter().map(|d| d.event.event).collect()
}

#[test]
fn first_sample_establishes_baseline_without_firing() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(
                Some(95.0),
                lane_key(HookQuotaWindow::Session, None),
                None,
                &[0.8],
                false,
            )],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(dispatches.is_empty());
}

#[test]
fn quota_low_fires_once_on_upward_crossing() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );

    let crossing = detector.evaluate(
        &observation(
            vec![lane(Some(85.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert_eq!(events_of(&crossing), vec![HookEventType::QuotaLow]);

    let persisting = detector.evaluate(
        &observation(
            vec![lane(Some(90.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(persisting.is_empty());
}

#[test]
fn quota_low_dispatches_only_rule_whose_threshold_crossed() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(
        true,
        Some(vec![
            rule(HookEventType::QuotaLow, Some(0.5), None),
            rule(HookEventType::QuotaLow, Some(0.8), None),
        ]),
    );
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(60.0), key.clone(), None, &[], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );

    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(85.0), key, None, &[], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert_eq!(dispatches.len(), 1);
    let rules = dispatches[0].rules.as_ref().expect("narrowed rules");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].threshold, Some(0.8));
}

#[test]
fn quota_reached_fires_on_upward_edge_only() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(90.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );

    let reached = detector.evaluate(
        &observation(
            vec![lane(Some(100.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(
        reached
            .iter()
            .any(|d| d.event.event == HookEventType::QuotaReached)
    );

    let still_full = detector.evaluate(
        &observation(
            vec![lane(Some(100.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(
        !still_full
            .iter()
            .any(|d| d.event.event == HookEventType::QuotaReached)
    );
}

#[test]
fn quota_reached_never_fires_for_weekly_lane() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Weekly, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(90.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(100.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(
        !dispatches
            .iter()
            .any(|d| d.event.event == HookEventType::QuotaReached)
    );
}

#[test]
fn quota_reset_fires_when_reset_boundary_advances() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let first = Utc.timestamp_opt(1_000_000, 0).unwrap();
    let second = first + chrono::Duration::seconds(18_000);

    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(100.0), key.clone(), Some(first), &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(0.0), key, Some(second), &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert_eq!(events_of(&dispatches), vec![HookEventType::QuotaReset]);
}

#[test]
fn quota_reset_fires_on_usage_drop_without_boundary() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(95.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(10.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert_eq!(events_of(&dispatches), vec![HookEventType::QuotaReset]);
}

#[test]
fn reset_suppresses_depletion_edge_in_same_poll() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(95.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(5.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(
        !dispatches
            .iter()
            .any(|d| d.event.event == HookEventType::QuotaReached)
    );
    assert!(
        !dispatches
            .iter()
            .any(|d| d.event.event == HookEventType::QuotaLow)
    );
}

#[test]
fn provider_status_fires_outage_and_recovery_edges() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let _ = detector.evaluate(&observation(vec![], HookProviderStatus::None, None), &cfg);

    let outage = detector.evaluate(&observation(vec![], HookProviderStatus::Major, None), &cfg);
    assert_eq!(events_of(&outage), vec![HookEventType::ProviderUnavailable]);

    let persisting = detector.evaluate(
        &observation(vec![], HookProviderStatus::Critical, None),
        &cfg,
    );
    assert!(persisting.is_empty());

    let recovered = detector.evaluate(&observation(vec![], HookProviderStatus::None, None), &cfg);
    assert_eq!(
        events_of(&recovered),
        vec![HookEventType::ProviderRecovered]
    );
}

#[test]
fn unknown_and_maintenance_never_flip_status_state() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let _ = detector.evaluate(&observation(vec![], HookProviderStatus::None, None), &cfg);

    assert!(
        detector
            .evaluate(
                &observation(vec![], HookProviderStatus::Unknown, None),
                &cfg
            )
            .is_empty()
    );
    assert!(
        detector
            .evaluate(
                &observation(vec![], HookProviderStatus::Maintenance, None),
                &cfg
            )
            .is_empty()
    );

    let outage = detector.evaluate(&observation(vec![], HookProviderStatus::Major, None), &cfg);
    assert_eq!(events_of(&outage), vec![HookEventType::ProviderUnavailable]);
}

#[test]
fn first_definite_status_does_not_fire() {
    let mut detector = HookTransitionDetector::new();
    let dispatches = detector.evaluate(
        &observation(vec![], HookProviderStatus::Critical, None),
        &config(true, None),
    );
    assert!(dispatches.is_empty());
}

#[test]
fn refresh_failure_emits_coarse_status_only() {
    let mut detector = HookTransitionDetector::new();
    let dispatches = detector.evaluate(
        &observation(vec![], HookProviderStatus::Unknown, Some("timeout")),
        &config(true, None),
    );
    assert_eq!(events_of(&dispatches), vec![HookEventType::RefreshFailed]);
    assert_eq!(dispatches[0].event.status.as_deref(), Some("timeout"));
}

#[test]
fn refresh_failure_does_not_disturb_quota_baselines() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let _ = detector.evaluate(
        &observation(vec![], HookProviderStatus::Unknown, Some("offline")),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(85.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(
        dispatches
            .iter()
            .any(|d| d.event.event == HookEventType::QuotaLow)
    );
}

#[test]
fn disabled_hooks_produce_no_events() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(false, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(95.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(dispatches.is_empty());
}

#[test]
fn configuration_change_clears_baselines() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    detector.reset_if_configuration_changed(1);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    detector.reset_if_configuration_changed(2);
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(95.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(dispatches.is_empty());
}

#[test]
fn synthetic_placeholder_lane_never_fires() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(100.0), key, None, &[0.8], true)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(dispatches.is_empty());
}

#[test]
fn disappearing_lane_resets_baseline() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let _ = detector.evaluate(
        &observation(vec![], HookProviderStatus::Unknown, None),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(95.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(dispatches.is_empty());
}

#[test]
fn accounts_on_same_provider_track_independently() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(true, None);
    let first = lane_key(HookQuotaWindow::Session, Some("a@example.com"));
    let second = lane_key(HookQuotaWindow::Session, Some("b@example.com"));
    let _ = detector.evaluate(
        &observation(
            vec![
                lane(Some(50.0), first.clone(), None, &[0.8], false),
                lane(Some(50.0), second.clone(), None, &[0.8], false),
            ],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![
                lane(Some(85.0), first, None, &[0.8], false),
                lane(Some(55.0), second, None, &[0.8], false),
            ],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].event.event, HookEventType::QuotaLow);
}

#[test]
fn quota_low_respects_explicit_rule_threshold_over_fallback() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(
        true,
        Some(vec![rule(HookEventType::QuotaLow, Some(0.9), None)]),
    );
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let below = detector.evaluate(
        &observation(
            vec![lane(Some(85.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(below.is_empty());
    let above = detector.evaluate(
        &observation(
            vec![lane(Some(95.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert_eq!(events_of(&above), vec![HookEventType::QuotaLow]);
}

#[test]
fn quota_low_ignores_rules_scoped_to_another_provider() {
    let mut detector = HookTransitionDetector::new();
    let cfg = config(
        true,
        Some(vec![rule(HookEventType::QuotaLow, None, Some("claude"))]),
    );
    let key = lane_key(HookQuotaWindow::Session, None);
    let _ = detector.evaluate(
        &observation(
            vec![lane(Some(50.0), key.clone(), None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    let dispatches = detector.evaluate(
        &observation(
            vec![lane(Some(95.0), key, None, &[0.8], false)],
            HookProviderStatus::Unknown,
            None,
        ),
        &cfg,
    );
    assert!(dispatches.is_empty());
}

#[test]
fn rate_limiter_suppresses_duplicate_refresh_failed() {
    use super::super::hooks::HookRateLimiter;
    use std::time::Duration;

    let limiter = HookRateLimiter::new(Duration::from_secs(600));
    let event = HookEvent::new(HookEventType::RefreshFailed, "codex").with_status("timeout");
    assert!(limiter.allow(&event, None));
    assert!(!limiter.allow(&event, None));

    // Quota events are not rate-limited by HookEventType::is_rate_limited.
    assert!(!HookEventType::QuotaLow.is_rate_limited());
    assert!(HookEventType::UsageUpdated.is_rate_limited());
    assert!(HookEventType::RefreshFailed.is_rate_limited());
    assert!(HookEventType::ProviderUnavailable.is_rate_limited());
}
