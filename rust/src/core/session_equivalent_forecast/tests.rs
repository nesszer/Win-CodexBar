use super::*;
use crate::core::RateWindow;
use chrono::TimeZone;

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().unwrap()
}

fn entry(captured: i64, used: f64, resets: i64) -> PlanUtilizationHistoryEntry {
    PlanUtilizationHistoryEntry {
        captured_at: ts(captured),
        used_percent: used,
        resets_at: Some(ts(resets)),
    }
}

fn session_secs() -> i64 {
    i64::from(SESSION_WINDOW_MINUTES) * 60
}

fn weekly_secs() -> i64 {
    i64::from(WEEKLY_WINDOW_MINUTES) * 60
}

#[test]
fn persisted_series_map_by_name_in_session_then_weekly_order() {
    let mut store = SessionEquivalentHistoryStore::default();
    let scope = ForecastScope::new("pin-series", None);
    let persisted = |name: &str, used: f64| quota_burndown::PersistedPlanSeries {
        name: name.to_string(),
        window_minutes: 1,
        entries: vec![PersistedEntry::from_history(&entry(
            1_700_000_000,
            used,
            1_700_018_000,
        ))],
    };
    store.adopt_persisted(
        &scope,
        &[
            persisted("weekly", 40.0),
            persisted("monthly", 70.0),
            persisted("session", 10.0),
        ],
    );
    let shape: Vec<_> = store
        .histories(&scope)
        .iter()
        .map(|h| (h.name, h.window_minutes, h.entries[0].used_percent))
        .collect();
    assert_eq!(
        shape,
        vec![
            (PlanUtilizationSeriesName::Session, 300, 10.0),
            (PlanUtilizationSeriesName::Weekly, 10_080, 40.0),
        ]
    );

    let mut weekly_only = SessionEquivalentHistoryStore::default();
    weekly_only.record(
        &scope,
        None,
        Some(entry(1_700_000_000, 5.0, 1_700_600_000)),
        7,
    );
    let names: Vec<_> = weekly_only
        .histories(&scope)
        .iter()
        .map(|h| h.name)
        .collect();
    assert_eq!(names, vec![PlanUtilizationSeriesName::Weekly]);
}

#[test]
fn record_provider_windows_filters_and_clamps_both_windows() {
    let now = ts(1_700_000_000);
    let window = |minutes: u32, used: f64| RateWindow {
        used_percent: used,
        window_minutes: Some(minutes),
        resets_at: Some(ts(1_700_018_000)),
        ..RateWindow::new(0.0)
    };
    let recorded = |provider: &str| {
        global_history_store()
            .lock()
            .unwrap()
            .histories(&ForecastScope::new(provider, None))
            .iter()
            .map(|h| {
                (
                    h.name,
                    h.entries.iter().map(|e| e.used_percent).collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };

    record_provider_windows(
        "pin-rec-a",
        None,
        &window(300, 120.0),
        Some(&window(10_080, -3.0)),
        now,
    );
    assert_eq!(
        recorded("pin-rec-a"),
        vec![
            (PlanUtilizationSeriesName::Session, vec![100.0]),
            (PlanUtilizationSeriesName::Weekly, vec![0.0]),
        ]
    );

    let informational = RateWindow {
        is_informational: true,
        ..window(10_080, 50.0)
    };
    record_provider_windows(
        "pin-rec-b",
        None,
        &window(300, 20.0),
        Some(&informational),
        now,
    );
    record_provider_windows(
        "pin-rec-b",
        None,
        &window(300, 30.0),
        Some(&window(43_200, 50.0)),
        now,
    );
    record_provider_windows(
        "pin-rec-b",
        None,
        &window(300, 40.0),
        Some(&window(10_080, f64::NAN)),
        now,
    );
    assert_eq!(
        recorded("pin-rec-b"),
        vec![(PlanUtilizationSeriesName::Session, vec![20.0, 30.0, 40.0])]
    );

    let informational_session = RateWindow {
        is_informational: true,
        ..window(300, 20.0)
    };
    record_provider_windows(
        "pin-rec-c",
        None,
        &informational_session,
        Some(&window(10_080, 50.0)),
        now,
    );
    record_provider_windows(
        "pin-rec-c",
        None,
        &window(60, 20.0),
        Some(&window(10_080, 50.0)),
        now,
    );
    record_provider_windows("pin-rec-c", None, &window(300, f64::INFINITY), None, now);
    assert!(recorded("pin-rec-c").is_empty());
}

/// Build three completed full-burn sessions with aligned weekly observations.
fn three_sample_histories(
    base: i64,
) -> (
    Vec<PlanUtilizationSeriesHistory>,
    DateTime<Utc>,
    DateTime<Utc>,
) {
    let s = session_secs();
    let w = weekly_secs();
    let weekly_reset = base + w;

    let mut session_entries = Vec::new();
    let mut weekly_entries = Vec::new();

    // Three completed sessions ending at base+s, base+2s, base+3s.
    for i in 0..3 {
        let start = base + i * s;
        let end = start + s;
        // Window-start weekly anchor (session used 0).
        weekly_entries.push(entry(start - 30, 10.0 + i as f64 * 5.0, weekly_reset));
        // Mid / end session captures with exact weekly alignment.
        session_entries.push(entry(start + 60, 40.0, end));
        weekly_entries.push(entry(start + 60, 12.0 + i as f64 * 5.0, weekly_reset));
        session_entries.push(entry(end - 60, 100.0, end));
        weekly_entries.push(entry(end - 60, 15.0 + i as f64 * 5.0, weekly_reset));
        // End-of-window weekly for max>=100 path.
        weekly_entries.push(entry(end, 15.5 + i as f64 * 5.0, weekly_reset));
    }

    // Current open session.
    let current_start = base + 3 * s;
    let current_end = current_start + s;
    let now_secs = current_start + 30 * 60;
    session_entries.push(entry(now_secs, 20.0, current_end));
    weekly_entries.push(entry(now_secs, 30.0, weekly_reset));

    session_entries.sort_by_key(|e| e.captured_at);
    weekly_entries.sort_by_key(|e| e.captured_at);

    let histories = vec![
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Session,
            window_minutes: SESSION_WINDOW_MINUTES,
            entries: session_entries,
        },
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Weekly,
            window_minutes: WEEKLY_WINDOW_MINUTES,
            entries: weekly_entries,
        },
    ];
    (histories, ts(now_secs), ts(current_end))
}

#[test]
fn median_math_from_three_full_sessions() {
    let base = 1_700_000_000_i64;
    let (histories, now, current_end) = three_sample_histories(base);
    let estimate = SessionEquivalentBurnEstimator::estimate(
        &histories,
        Some(current_end),
        now,
        SessionEquivalentBurnEstimator::DEFAULT_SAMPLE_LIMIT,
    )
    .expect("estimate");
    assert_eq!(estimate.sample_count, 3);
    // Mid-session captures dominate when start/end anchors are optional:
    // weekly 12→15 over session 40→100 ⇒ 100 * 3 / 60 = 5.0
    // (window-start + end anchors yield the same full-allowance burn here).
    assert!(
        (estimate.median_weekly_percent_per_window - 5.0).abs() < 0.01,
        "median={}",
        estimate.median_weekly_percent_per_window
    );
}

#[test]
fn grouping_tolerance_merges_nearby_resets() {
    let s = session_secs();
    let base = 1_700_100_000_i64;
    let weekly_reset = base + weekly_secs();
    let end = base + s;

    // Nearby resets within ±120s must merge into one group (not out-of-order).
    let single_group = vec![
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Session,
            window_minutes: SESSION_WINDOW_MINUTES,
            entries: vec![
                entry(base + 60, 30.0, end),
                entry(base + 120, 60.0, end + 100),
                entry(base + 180, 90.0, end + 110),
            ],
        },
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Weekly,
            window_minutes: WEEKLY_WINDOW_MINUTES,
            entries: vec![
                entry(base - 30, 5.0, weekly_reset),
                entry(base + 60, 8.0, weekly_reset),
                entry(base + 120, 10.0, weekly_reset),
                entry(base + 180, 12.0, weekly_reset),
            ],
        },
    ];
    // One completed group → below minimum sample count, but not an ordering reject.
    assert!(
        SessionEquivalentBurnEstimator::estimate(&single_group, Some(ts(end + s)), ts(end + 60), 7)
            .is_none()
    );

    // Strictly decreasing resets_at across groups is rejected.
    let out_of_order = vec![
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Session,
            window_minutes: SESSION_WINDOW_MINUTES,
            entries: vec![
                entry(base + 60, 30.0, end + s),
                entry(base + 120, 60.0, end), // jumps backward beyond tolerance
            ],
        },
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Weekly,
            window_minutes: WEEKLY_WINDOW_MINUTES,
            entries: vec![
                entry(base + 60, 8.0, weekly_reset),
                entry(base + 120, 10.0, weekly_reset),
            ],
        },
    ];
    assert!(
        SessionEquivalentBurnEstimator::estimate(
            &out_of_order,
            Some(ts(end + 2 * s)),
            ts(end + s + 60),
            7
        )
        .is_none()
    );
}

#[test]
fn insufficient_samples_returns_none() {
    let base = 1_700_200_000_i64;
    let s = session_secs();
    let weekly_reset = base + weekly_secs();
    let end = base + s;
    let histories = vec![
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Session,
            window_minutes: SESSION_WINDOW_MINUTES,
            entries: vec![entry(base + 60, 50.0, end), entry(base + 120, 80.0, end)],
        },
        PlanUtilizationSeriesHistory {
            name: PlanUtilizationSeriesName::Weekly,
            window_minutes: WEEKLY_WINDOW_MINUTES,
            entries: vec![
                entry(base - 30, 10.0, weekly_reset),
                entry(base + 60, 12.0, weekly_reset),
                entry(base + 120, 14.0, weekly_reset),
            ],
        },
    ];
    assert!(
        SessionEquivalentBurnEstimator::estimate(&histories, Some(ts(end + s)), ts(end + 60), 7)
            .is_none()
    );
}

#[test]
fn forecast_make_fractional_windows() {
    let now = ts(1_700_300_000);
    let session = RateWindow::with_details(
        25.0,
        Some(SESSION_WINDOW_MINUTES),
        Some(now + Duration::hours(3)),
        None,
    );
    let weekly = RateWindow::with_details(
        40.0,
        Some(WEEKLY_WINDOW_MINUTES),
        Some(now + Duration::days(3)),
        None,
    );
    let burn = SessionEquivalentBurnEstimate {
        median_weekly_percent_per_window: 12.0,
        sample_count: 3,
    };
    let forecast = SessionEquivalentForecast::make(&session, &weekly, &burn, None, now, None)
        .expect("forecast");
    // remaining weekly 60 / 12 = 5.0
    assert!((forecast.estimated_windows_to_exhaust_weekly - 5.0).abs() < 1e-9);
    assert!(forecast.available_windows_until_reset > 0.0);
    // Mirrors the production whole-window truncation.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "assertion mirrors the production whole-window truncation"
    )]
    let expected_windows = forecast.available_windows_until_reset.floor() as i64;
    assert_eq!(forecast.windows_until_reset, expected_windows);
    assert_eq!(forecast.sample_count, 3);
    assert!((forecast.weekly_used_percent - 40.0).abs() < 1e-9);
}

#[test]
fn work_days_reduces_available_windows_vs_wall_clock() {
    // Friday 18:00 UTC → Monday 18:00 UTC (weekend in between).
    let friday = Utc
        .with_ymd_and_hms(2024, 6, 14, 18, 0, 0)
        .single()
        .unwrap();
    assert_eq!(friday.weekday(), Weekday::Fri);
    let monday = Utc
        .with_ymd_and_hms(2024, 6, 17, 18, 0, 0)
        .single()
        .unwrap();

    let wall = effective_remaining_seconds(friday, monday, None);
    let work5 = effective_remaining_seconds(friday, monday, Some(5));
    assert!((wall - 3.0 * 24.0 * 3600.0).abs() < 1.0);
    // Fri 18→24 = 6h, Mon 0→18 = 18h → 24h work seconds (no Sat/Sun).
    assert!((work5 - 24.0 * 3600.0).abs() < 1.0);
    assert!(work5 < wall);

    let session = RateWindow::with_details(
        10.0,
        Some(SESSION_WINDOW_MINUTES),
        Some(friday + Duration::hours(4)),
        None,
    );
    let weekly = RateWindow::with_details(20.0, Some(WEEKLY_WINDOW_MINUTES), Some(monday), None);
    let burn = SessionEquivalentBurnEstimate {
        median_weekly_percent_per_window: 10.0,
        sample_count: 4,
    };
    let wall_f =
        SessionEquivalentForecast::make(&session, &weekly, &burn, None, friday, None).unwrap();
    let work_f =
        SessionEquivalentForecast::make(&session, &weekly, &burn, None, friday, Some(5)).unwrap();
    assert!(work_f.available_windows_until_reset < wall_f.available_windows_until_reset);
    // Mirrors the production whole-window truncation.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "assertion mirrors the production whole-window truncation"
    )]
    let expected_work_windows = work_f.available_windows_until_reset.floor() as i64;
    assert_eq!(work_f.windows_until_reset, expected_work_windows);
}

#[test]
fn work_days_out_of_range_uses_wall_clock() {
    let now = ts(1_700_400_000);
    let reset = now + Duration::days(2);
    let wall = effective_remaining_seconds(now, reset, None);
    assert!((effective_remaining_seconds(now, reset, Some(1)) - wall).abs() < 1e-9);
    assert!((effective_remaining_seconds(now, reset, Some(7)) - wall).abs() < 1e-9);
}

#[test]
fn history_ring_retains_latest_samples() {
    let mut store = SessionEquivalentHistoryStore::default();
    let scope = ForecastScope::new("claude", None);
    let base = ts(1_700_500_000);
    for i in 0..30 {
        store.record(
            &scope,
            Some(PlanUtilizationHistoryEntry {
                captured_at: base + Duration::minutes(i),
                used_percent: i as f64,
                resets_at: Some(base + Duration::hours(5)),
            }),
            None,
            7,
        );
    }
    let h = store.histories(&scope);
    assert_eq!(h.len(), 1);
    assert!(h[0].entries.len() <= 7 * 8);
    assert!((h[0].entries.last().unwrap().used_percent - 29.0).abs() < 1e-9);
}

/// Two accounts on one provider must not share burn samples.
///
/// Regression: history was keyed by `provider_id` alone, so switching the active
/// account blended both accounts' observations into one ring and produced a median
/// drawn from a mixture of plans.
#[test]
fn history_is_isolated_per_account() {
    let mut store = SessionEquivalentHistoryStore::default();
    let base = ts(1_700_500_000);
    let alice = ForecastScope::new("codex", Some("alice@example.com"));
    let bob = ForecastScope::new("codex", Some("bob@example.com"));

    for i in 0..5 {
        store.record(
            &alice,
            Some(PlanUtilizationHistoryEntry {
                captured_at: base + Duration::minutes(i),
                used_percent: 10.0,
                resets_at: Some(base + Duration::hours(5)),
            }),
            None,
            7,
        );
    }
    store.record(
        &bob,
        Some(PlanUtilizationHistoryEntry {
            captured_at: base + Duration::minutes(99),
            used_percent: 90.0,
            resets_at: Some(base + Duration::hours(5)),
        }),
        None,
        7,
    );

    let alice_hist = store.histories(&alice);
    let bob_hist = store.histories(&bob);
    assert_eq!(alice_hist[0].entries.len(), 5);
    assert_eq!(bob_hist[0].entries.len(), 1);
    assert!(
        alice_hist[0]
            .entries
            .iter()
            .all(|e| (e.used_percent - 10.0).abs() < 1e-9),
        "bob's 90% sample leaked into alice's history"
    );
    assert!((bob_hist[0].entries[0].used_percent - 90.0).abs() < 1e-9);

    // A provider with no account discriminator is its own bucket, not a catch-all.
    assert!(
        store
            .histories(&ForecastScope::new("codex", None))
            .is_empty()
    );
}

#[test]
fn make_rejects_bad_windows_and_zero_remaining() {
    let now = ts(1_700_600_000);
    let burn = SessionEquivalentBurnEstimate {
        median_weekly_percent_per_window: 10.0,
        sample_count: 3,
    };
    let session = RateWindow::with_details(
        10.0,
        Some(SESSION_WINDOW_MINUTES),
        Some(now + Duration::hours(2)),
        None,
    );
    let exhausted = RateWindow::with_details(
        100.0,
        Some(WEEKLY_WINDOW_MINUTES),
        Some(now + Duration::days(2)),
        None,
    );
    assert!(
        SessionEquivalentForecast::make(&session, &exhausted, &burn, None, now, None).is_none()
    );

    let low_samples = SessionEquivalentBurnEstimate {
        median_weekly_percent_per_window: 10.0,
        sample_count: 2,
    };
    let weekly = RateWindow::with_details(
        50.0,
        Some(WEEKLY_WINDOW_MINUTES),
        Some(now + Duration::days(2)),
        None,
    );
    assert!(
        SessionEquivalentForecast::make(&session, &weekly, &low_samples, None, now, None).is_none()
    );
}

#[test]
fn retain_last_full_session_estimate_survives_idle_refresh() {
    let t0 = ts(1_700_000_000);
    let t1 = t0 + Duration::hours(1);
    let learned = retain_last_full_session_estimate(None, Some(12.5), t0)
        .expect("fresh estimate should stick");
    assert_eq!(learned.estimate, 12.5);
    assert_eq!(learned.updated_at, t0);

    // Idle refresh yields no fresh sample — keep the last learned value.
    let idle = retain_last_full_session_estimate(Some(learned), None, t1)
        .expect("idle refresh must keep last estimate");
    assert_eq!(idle.estimate, 12.5);
    assert_eq!(idle.updated_at, t0);

    // Non-positive / non-finite fresh values also keep previous.
    let still = retain_last_full_session_estimate(Some(idle), Some(0.0), t1)
        .expect("zero fresh must not clear");
    assert_eq!(still.estimate, 12.5);
    let still = retain_last_full_session_estimate(Some(still), Some(f64::NAN), t1)
        .expect("nan fresh must not clear");
    assert_eq!(still.estimate, 12.5);
}

#[test]
fn retain_last_full_session_estimate_replaced_by_fresh_sample() {
    let t0 = ts(1_700_000_000);
    let t1 = t0 + Duration::hours(2);
    let previous = retain_last_full_session_estimate(None, Some(8.0), t0).unwrap();
    let next = retain_last_full_session_estimate(Some(previous), Some(15.0), t1)
        .expect("fresh sample replaces previous");
    assert_eq!(next.estimate, 15.0);
    assert_eq!(next.updated_at, t1);
}

#[test]
fn make_keeps_forecast_when_session_reset_is_expired_idle() {
    let now = ts(1_700_600_000);
    let burn = SessionEquivalentBurnEstimate {
        median_weekly_percent_per_window: 10.0,
        sample_count: 3,
    };
    // Session reset already passed (idle) — forecast still derives from weekly.
    let session = RateWindow::with_details(
        0.0,
        Some(SESSION_WINDOW_MINUTES),
        Some(now - Duration::hours(1)),
        None,
    );
    let weekly = RateWindow::with_details(
        40.0,
        Some(WEEKLY_WINDOW_MINUTES),
        Some(now + Duration::days(3)),
        None,
    );
    let forecast = SessionEquivalentForecast::make(&session, &weekly, &burn, None, now, None)
        .expect("idle expired session should still forecast");
    assert!((forecast.estimated_windows_to_exhaust_weekly - 6.0).abs() < 1e-9);
}
