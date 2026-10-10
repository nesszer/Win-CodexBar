use super::*;
use chrono::TimeZone;

mod plan_change;

#[test]
fn reset_diagnostic_codes_are_fixed_and_redacted() {
    let codes = [
        ResetDiagnosticReason::CandidateCreated.code(),
        ResetDiagnosticReason::SourceNotExactOAuth.code(),
        ResetDiagnosticReason::ExpiredCandidate.code(),
        ResetDiagnosticReason::ChangedCreditInventory.code(),
        ResetDiagnosticReason::StoreRequested.code(),
        ResetDiagnosticReason::MissingPreviousSnapshot.code(),
        ResetDiagnosticReason::MissingWeeklyWindow.code(),
        ResetDiagnosticReason::ResetThresholdMismatch.code(),
        ResetDiagnosticReason::InvalidResetBoundary.code(),
        ResetDiagnosticReason::InconsistentResetBoundary.code(),
        ResetDiagnosticReason::UnsupportedResetBoundary.code(),
        ResetDiagnosticReason::PlanMismatch.code(),
        ResetDiagnosticReason::PlanChanged.code(),
        ResetDiagnosticReason::MissingCreditInventory.code(),
        ResetDiagnosticReason::EvidenceVersionMismatch.code(),
        ResetDiagnosticReason::FutureCandidate.code(),
        ResetDiagnosticReason::StaleObservation.code(),
        ResetDiagnosticReason::MinimumDelay.code(),
        ResetDiagnosticReason::ConfirmedObservation.code(),
        ResetDiagnosticReason::StoreUnavailable.code(),
    ];
    assert_eq!(
        codes,
        [
            "candidateCreated",
            "sourceNotExactOAuth",
            "expiredCandidate",
            "changedCreditInventory",
            "storeRequested",
            "missingPreviousSnapshot",
            "missingWeeklyWindow",
            "resetThresholdMismatch",
            "invalidResetBoundary",
            "inconsistentResetBoundary",
            "unsupportedResetBoundary",
            "planMismatch",
            "planChanged",
            "missingCreditInventory",
            "evidenceVersionMismatch",
            "futureCandidate",
            "staleObservation",
            "minimumDelay",
            "confirmedObservation",
            "storeUnavailable",
        ]
    );
    assert!(codes.iter().all(|code| {
        !code.contains('@') && !code.contains(':') && !code.contains('/') && !code.contains('\\')
    }));
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 25, 12, 0, 0).unwrap()
}

fn snapshot(used: f64, reset_days: i64, captured_minutes: i64) -> UsageSnapshot {
    let captured = now() + chrono::Duration::minutes(captured_minutes);
    let weekly = RateWindow::with_details(
        used,
        Some(7 * 24 * 60),
        Some(now() + chrono::Duration::days(reset_days)),
        None,
    );
    let mut snapshot = UsageSnapshot::new(RateWindow::new(20.0)).with_secondary(weekly);
    snapshot.updated_at = captured;
    snapshot.login_method = Some("ChatGPT Pro".to_string());
    snapshot
}

fn inventory(id: &str) -> CreditInventory {
    CreditInventory {
        available_count: 1,
        credits: vec![CreditIdentity {
            id: id.to_string(),
            reset_type: "weekly".to_string(),
            status: "available".to_string(),
            expires_at: Some(now() + chrono::Duration::days(3)),
        }],
    }
}

fn baseline() -> AccountState {
    let previous = snapshot(45.0, 2, 0);
    AccountState {
        published_weekly: previous.secondary.clone(),
        published_at: previous.updated_at,
        plan: previous.login_method.clone(),
        credit_inventory: Some(inventory("credit-a")),
        candidate: None,
    }
}

#[test]
fn inventory_retains_consumed_status_rows_but_counts_only_available_credits() {
    let reset = ResetCredits {
        available_count: 1,
        credits: vec![
            ResetCredit {
                id: Some("available-a".into()),
                reset_type: Some("weekly".into()),
                status: Some("available".into()),
                expires_at: None,
            },
            ResetCredit {
                id: Some("redeeming-b".into()),
                reset_type: Some("weekly".into()),
                status: Some("redeeming".into()),
                expires_at: None,
            },
            ResetCredit {
                id: Some("redeemed-c".into()),
                reset_type: Some("weekly".into()),
                status: Some("redeemed".into()),
                expires_at: None,
            },
        ],
    };
    let inventory = super::inventory(Some(&reset), now()).expect("credit inventory");
    assert_eq!(inventory.available_count, 1);
    assert_eq!(inventory.credits.len(), 3);
    assert!(
        inventory
            .credits
            .iter()
            .any(|credit| credit.status == "redeeming")
    );
    assert!(
        inventory
            .credits
            .iter()
            .any(|credit| credit.status == "redeemed")
    );
}
#[test]
fn early_low_usage_requires_confirmation_without_spending_credit() {
    let mut state = baseline();
    let initial = snapshot(0.0, 9, 1);
    let inv = inventory("credit-a");
    assert_eq!(
        initial_decision(&mut state, &initial, Some(&inv), true, now()),
        InitialDecision::RequiresConfirmation
    );
    let confirmation = snapshot(0.0, 9, 2);
    assert_eq!(
        confirmation_decision(
            &mut state,
            &initial,
            Some(&inv),
            &confirmation,
            Some(&inv),
            true,
            now(),
        ),
        ConfirmationDecision::Preserve
    );
    assert!(state.candidate.is_some());
    assert_eq!(state.credit_inventory.as_ref().unwrap().available_count, 1);
}

#[test]
fn delayed_candidate_publishes_after_sixty_seconds_and_expires_after_thirty_minutes() {
    let mut state = baseline();
    let initial = snapshot(0.0, 9, 1);
    let confirmation = snapshot(0.0, 9, 2);
    let inv = inventory("credit-a");
    assert_eq!(
        confirmation_decision(
            &mut state,
            &initial,
            Some(&inv),
            &confirmation,
            Some(&inv),
            true,
            now(),
        ),
        ConfirmationDecision::Preserve
    );
    let current = snapshot(0.0, 9, 3);
    let candidate = state.candidate.clone().unwrap();
    assert_eq!(
        delayed_candidate_decision(
            &state,
            &candidate,
            &current,
            Some(&inv),
            true,
            now() + chrono::Duration::seconds(59),
        ),
        DelayedDecision::Retain
    );
    assert_eq!(
        delayed_candidate_decision(
            &state,
            &candidate,
            &current,
            Some(&inv),
            true,
            now() + chrono::Duration::seconds(60),
        ),
        DelayedDecision::Publish
    );
    assert_eq!(
        delayed_candidate_decision(
            &state,
            &candidate,
            &current,
            Some(&inv),
            true,
            now() + chrono::Duration::minutes(31),
        ),
        DelayedDecision::Discard
    );
}

#[test]
fn credits_only_refresh_retains_candidate_and_account_scope_hashes_differ() {
    let mut state = baseline();
    state.candidate = Some(DelayedCandidate {
        evidence_version: EVIDENCE_VERSION,
        first_observed_at: now(),
        created_at: now(),
        snapshot_updated_at: now(),
        weekly: snapshot(0.0, 9, 1).secondary.unwrap(),
        plan: Some("ChatGPT Pro".to_string()),
        inventory: inventory("credit-a"),
    });
    let mut credits_only = UsageSnapshot::new(RateWindow::new(20.0));
    credits_only.updated_at = now() + chrono::Duration::minutes(1);
    // A credits-only refresh has no weekly window and may omit both plan and
    // reset-credit inventory. It must not consume the pending evidence.
    let candidate_before = serde_json::to_value(&state.candidate).unwrap();
    assert_eq!(
        initial_decision(
            &mut state,
            &credits_only,
            None,
            true,
            now() + chrono::Duration::minutes(1),
        ),
        InitialDecision::Preserve
    );
    assert_eq!(
        serde_json::to_value(&state.candidate).unwrap(),
        candidate_before
    );
    assert_ne!(
        scope_key(Some("account-a"), Path::new("C:/a/auth.json")),
        scope_key(Some("account-b"), Path::new("C:/b/auth.json"))
    );
}

#[test]
fn credits_only_refresh_candidate_survives_state_reload_until_full_usage() {
    let mut state = baseline();
    state.candidate = Some(DelayedCandidate {
        evidence_version: EVIDENCE_VERSION,
        first_observed_at: now(),
        created_at: now(),
        snapshot_updated_at: now(),
        weekly: snapshot(0.0, 9, 1).secondary.unwrap(),
        plan: Some("ChatGPT Pro".to_string()),
        inventory: inventory("credit-a"),
    });
    let candidate_before = serde_json::to_value(&state.candidate).unwrap();
    let mut credits_only = UsageSnapshot::new(RateWindow::new(20.0));
    credits_only.updated_at = now() + chrono::Duration::minutes(1);

    assert_eq!(
        initial_decision(
            &mut state,
            &credits_only,
            None,
            true,
            now() + chrono::Duration::minutes(1),
        ),
        InitialDecision::Preserve
    );

    // Model the StateFile envelope used by save/load without touching the
    // user's real LocalAppData during a unit test.
    let encoded = serde_json::to_vec(&StateFile {
        version: STATE_VERSION,
        accounts: HashMap::from([(String::from("scope"), state)]),
    })
    .unwrap();
    let mut reloaded_file: StateFile = serde_json::from_slice(&encoded).unwrap();
    let mut reloaded = reloaded_file.accounts.remove("scope").unwrap();
    assert_eq!(
        serde_json::to_value(&reloaded.candidate).unwrap(),
        candidate_before
    );

    let mut incompatible = reloaded.clone();
    let mut incompatible_usage = snapshot(0.0, 9, 3);
    incompatible_usage.login_method = Some("ChatGPT Plus".to_string());
    assert_eq!(
        initial_decision(
            &mut incompatible,
            &incompatible_usage,
            Some(&inventory("credit-a")),
            true,
            now() + chrono::Duration::seconds(60),
        ),
        InitialDecision::RequiresConfirmation
    );
    assert!(incompatible.candidate.is_none());

    let full_usage = snapshot(0.0, 9, 3);
    assert_eq!(
        initial_decision(
            &mut reloaded,
            &full_usage,
            Some(&inventory("credit-a")),
            true,
            now() + chrono::Duration::seconds(60),
        ),
        InitialDecision::Publish
    );
    assert!(reloaded.candidate.is_none());
}

#[test]
fn consumed_credit_allows_immediate_confirmation() {
    let mut state = baseline();
    let initial = snapshot(0.0, 2, 1);
    let confirmation = snapshot(0.0, 2, 2);
    let consumed = CreditInventory {
        available_count: 0,
        credits: Vec::new(),
    };
    assert_eq!(
        confirmation_decision(
            &mut state,
            &initial,
            Some(&consumed),
            &confirmation,
            Some(&consumed),
            true,
            now(),
        ),
        ConfirmationDecision::Publish
    );
}

const WEEK_SECONDS: i64 = 7 * 24 * 60 * 60;

/// Unused weekly window whose reset date sits `boundary_ahead` seconds after its own capture time.
fn rolling_snapshot(
    used: f64,
    window_minutes: u32,
    captured_seconds: i64,
    boundary_ahead: i64,
) -> UsageSnapshot {
    let captured = now() + chrono::Duration::seconds(captured_seconds);
    let weekly = RateWindow::with_details(
        used,
        Some(window_minutes),
        Some(captured + chrono::Duration::seconds(boundary_ahead)),
        None,
    );
    let mut snapshot = UsageSnapshot::new(RateWindow::new(20.0)).with_secondary(weekly);
    snapshot.updated_at = captured;
    snapshot.login_method = Some("ChatGPT Pro".to_string());
    snapshot
}

fn rolling_current(offset_seconds: i64) -> UsageSnapshot {
    rolling_snapshot(0.0, 7 * 24 * 60, offset_seconds, WEEK_SECONDS - 1)
}

/// Candidate created from two unused observations whose boundaries roll with capture time.
fn rolling_state() -> AccountState {
    let mut state = baseline();
    let inv = inventory("credit-a");
    let initial = rolling_snapshot(0.0, 7 * 24 * 60, 1, WEEK_SECONDS - 1);
    let confirmation = rolling_snapshot(0.0, 7 * 24 * 60, 2, WEEK_SECONDS - 2);
    assert_eq!(
        confirmation_decision(
            &mut state,
            &initial,
            Some(&inv),
            &confirmation,
            Some(&inv),
            true,
            now(),
        ),
        ConfirmationDecision::Preserve
    );
    assert!(state.candidate.is_some());
    state
}

fn rolling_decision(
    state: &AccountState,
    current: &UsageSnapshot,
    inv: &CreditInventory,
    age_seconds: i64,
) -> DelayedDecision {
    let candidate = state.candidate.clone().unwrap();
    delayed_candidate_decision(
        state,
        &candidate,
        current,
        Some(inv),
        true,
        now() + chrono::Duration::seconds(age_seconds),
    )
}

#[test]
fn unused_rolling_weekly_boundaries_confirm_across_refresh_intervals() {
    let inv = inventory("credit-a");
    for offset in [180, 300, 900] {
        let state = rolling_state();
        let current = rolling_current(offset);
        assert_eq!(
            rolling_decision(&state, &current, &inv, offset),
            DelayedDecision::Publish,
            "offset {offset}"
        );
    }
    let state = rolling_state();
    assert_eq!(
        rolling_decision(&state, &rolling_current(30), &inv, 30),
        DelayedDecision::Retain,
        "minimum age still applies"
    );
    // Equivalent boundaries keep working exactly as before.
    assert_eq!(
        rolling_decision(
            &state,
            &rolling_snapshot(0.0, 7 * 24 * 60, 120, WEEK_SECONDS - 118),
            &inv,
            120
        ),
        DelayedDecision::Publish
    );
}

#[test]
fn ordinary_publication_after_rolling_confirmation_is_unchanged() {
    let mut state = rolling_state();
    let ordinary = rolling_snapshot(2.0, 7 * 24 * 60, 300, WEEK_SECONDS - 1);
    assert_eq!(
        initial_decision(
            &mut state,
            &ordinary,
            Some(&inventory("credit-a")),
            true,
            now() + chrono::Duration::seconds(300),
        ),
        InitialDecision::Publish
    );
}

fn rolling_evaluation(
    state: &AccountState,
    current: &UsageSnapshot,
    inv: Option<&CreditInventory>,
    exact_oauth: bool,
    age_seconds: i64,
) -> (DelayedDecision, ResetDiagnosticReason) {
    let candidate = state.candidate.clone().unwrap();
    delayed_candidate_evaluation(
        state,
        &candidate,
        current,
        inv,
        exact_oauth,
        now() + chrono::Duration::seconds(age_seconds),
    )
}

/// Upstream `unused weekly boundaries advancing with observation time confirm
/// on later refreshes` rejection table. Account mismatch has no row: the state
/// is scoped per account (`scope_key`), so another account never sees this
/// candidate. Upstream's `confidenceNotExact` maps to the exact-OAuth source.
#[test]
fn rolling_weekly_confirmation_rejects_incompatible_observations() {
    use ResetDiagnosticReason as Reason;
    let inv = inventory("credit-a");
    let week_minutes = 7 * 24 * 60;
    let mut other_plan = rolling_current(300);
    other_plan.login_method = Some("different-plan".to_string());
    let cases: Vec<(&str, UsageSnapshot, Option<CreditInventory>, bool, Reason)> = vec![
        (
            "nonzero usage",
            rolling_snapshot(0.5, week_minutes, 300, WEEK_SECONDS - 1),
            Some(inv.clone()),
            true,
            Reason::InconsistentResetBoundary,
        ),
        (
            "wrong window minutes",
            rolling_snapshot(0.0, 300, 300, WEEK_SECONDS - 1),
            Some(inv.clone()),
            true,
            Reason::InconsistentResetBoundary,
        ),
        (
            "boundary just outside capture plus one week",
            rolling_snapshot(0.0, week_minutes, 300, WEEK_SECONDS + 121),
            Some(inv.clone()),
            true,
            Reason::InconsistentResetBoundary,
        ),
        (
            "boundary far from capture plus one week",
            rolling_snapshot(0.0, week_minutes, 300, 600_000),
            Some(inv.clone()),
            true,
            Reason::InconsistentResetBoundary,
        ),
        (
            "plan mismatch",
            other_plan,
            Some(inv.clone()),
            true,
            Reason::PlanMismatch,
        ),
        (
            "changed credit inventory",
            rolling_current(300),
            Some(inventory("different-credit")),
            true,
            Reason::ChangedCreditInventory,
        ),
        (
            "missing credit inventory",
            rolling_current(300),
            None,
            true,
            Reason::MissingCreditInventory,
        ),
        (
            "source not exact OAuth",
            rolling_current(300),
            Some(inv.clone()),
            false,
            Reason::SourceNotExactOAuth,
        ),
    ];
    for (name, current, current_inventory, exact_oauth, reason) in cases {
        let state = rolling_state();
        assert_eq!(
            rolling_evaluation(
                &state,
                &current,
                current_inventory.as_ref(),
                exact_oauth,
                300
            ),
            (DelayedDecision::Discard, reason),
            "{name}"
        );
    }
}

#[test]
fn rolling_weekly_confirmation_reports_confirmed_observation() {
    let inv = inventory("credit-a");
    for offset in [180, 300, 900] {
        let state = rolling_state();
        assert_eq!(
            rolling_evaluation(&state, &rolling_current(offset), Some(&inv), true, offset),
            (
                DelayedDecision::Publish,
                ResetDiagnosticReason::ConfirmedObservation
            ),
            "offset {offset}"
        );
    }
}

/// Upstream checks `abs(boundary - capturedAt - 604_800) < 120` on floating
/// seconds, so a reset 119.8 s off one week still rolls and 120 s does not.
#[test]
fn rolling_weekly_tolerance_uses_full_timestamp_precision() {
    let inv = inventory("credit-a");
    let week_minutes = 7 * 24 * 60;
    let offset_from_week = |millis: i64| {
        let mut current = rolling_snapshot(0.0, week_minutes, 300, WEEK_SECONDS);
        let captured = current.updated_at;
        if let Some(weekly) = current.secondary.as_mut() {
            weekly.resets_at = Some(
                captured
                    + chrono::Duration::seconds(WEEK_SECONDS)
                    + chrono::Duration::milliseconds(millis),
            );
        }
        current
    };
    for (millis, expected) in [
        (-119_800, DelayedDecision::Publish),
        (119_800, DelayedDecision::Publish),
        (-120_000, DelayedDecision::Discard),
        (-120_200, DelayedDecision::Discard),
        (120_200, DelayedDecision::Discard),
    ] {
        let state = rolling_state();
        assert_eq!(
            rolling_decision(&state, &offset_from_week(millis), &inv, 300),
            expected,
            "{millis} ms from one week"
        );
    }
}

#[test]
fn rolling_weekly_confirmation_rejects_a_boundary_that_moves_backward() {
    let inv = inventory("credit-a");
    let mut state = rolling_state();
    // Both windows stay within two minutes of capture plus one week, but the
    // later observation resets earlier than the candidate.
    let candidate = state.candidate.as_mut().unwrap();
    candidate.weekly.resets_at =
        Some(candidate.snapshot_updated_at + chrono::Duration::seconds(WEEK_SECONDS + 100));
    let current = rolling_snapshot(0.0, 7 * 24 * 60, 62, WEEK_SECONDS - 100);
    assert_eq!(
        rolling_decision(&state, &current, &inv, 62),
        DelayedDecision::Discard
    );
    // The same pair with a non-decreasing boundary confirms.
    let current = rolling_snapshot(0.0, 7 * 24 * 60, 62, WEEK_SECONDS + 100);
    assert_eq!(
        rolling_decision(&state, &current, &inv, 62),
        DelayedDecision::Publish
    );
}

#[test]
fn rolling_weekly_confirmation_rejects_nonzero_or_mismatched_candidate_window() {
    let inv = inventory("credit-a");
    for (name, mutate) in [
        (
            "candidate used",
            (|weekly: &mut RateWindow| weekly.used_percent = 0.5) as fn(&mut RateWindow),
        ),
        ("candidate window minutes", |weekly| {
            weekly.window_minutes = Some(300);
        }),
        ("candidate boundary not near a week", |weekly| {
            weekly.resets_at = weekly.resets_at.map(|at| at + chrono::Duration::hours(1));
        }),
    ] {
        let mut state = rolling_state();
        if let Some(candidate) = state.candidate.as_mut() {
            mutate(&mut candidate.weekly);
        }
        assert_eq!(
            rolling_decision(&state, &rolling_current(300), &inv, 300),
            DelayedDecision::Discard,
            "{name}"
        );
    }
}

#[test]
fn rolling_weekly_confirmation_keeps_inventory_and_expiry_guards() {
    let state = rolling_state();
    let current = rolling_current(300);
    assert_eq!(
        rolling_decision(&state, &current, &inventory("credit-b"), 300),
        DelayedDecision::Discard,
        "inventory changed"
    );
    let inv = inventory("credit-a");
    let current = rolling_current(31 * 60);
    assert_eq!(
        rolling_decision(&state, &current, &inv, 31 * 60),
        DelayedDecision::Discard,
        "candidate expired"
    );
}

/// Upstream `persisted stale baseline recovers after delayed reset
/// confirmation across relaunch` for fixed and rolling boundaries, driven
/// through the decisions `CodexApi::fetch_usage` makes and the persisted
/// `StateFile` envelope (without touching the real LocalAppData).
#[test]
fn persisted_stale_baseline_recovers_after_delayed_reset_confirmation_across_relaunch() {
    for rolling_boundary in [false, true] {
        let at = |seconds: i64| now() + chrono::Duration::seconds(seconds);
        let prior_boundary = now() + chrono::Duration::days(2);
        let next_boundary = if rolling_boundary {
            at(WEEK_SECONDS - 301)
        } else {
            prior_boundary + chrono::Duration::seconds(WEEK_SECONDS)
        };
        let credits = CreditInventory {
            available_count: 1,
            credits: vec![CreditIdentity {
                id: "credit-a".to_string(),
                reset_type: "weekly".to_string(),
                status: "available".to_string(),
                expires_at: Some(next_boundary + chrono::Duration::days(1)),
            }],
        };
        let weekly_snapshot = |used: f64, boundary: DateTime<Utc>, updated_at: DateTime<Utc>| {
            let weekly =
                RateWindow::with_details(used, Some(WEEKLY_WINDOW_MINUTES), Some(boundary), None);
            let mut snapshot = UsageSnapshot::new(RateWindow::new(20.0)).with_secondary(weekly);
            snapshot.updated_at = updated_at;
            snapshot.login_method = Some("ChatGPT Pro".to_string());
            snapshot
        };
        let roll =
            |seconds: i64| chrono::Duration::seconds(if rolling_boundary { seconds } else { 0 });
        let prior = weekly_snapshot(81.0, prior_boundary, at(-360));
        let initial_low = weekly_snapshot(0.0, next_boundary, at(-300));
        let confirmed_low = weekly_snapshot(0.0, next_boundary + roll(1), at(-299));
        let later_low = weekly_snapshot(
            if rolling_boundary { 0.0 } else { 0.4 },
            next_boundary + roll(300),
            now(),
        );

        let mut state = AccountState::default();
        assert_eq!(
            initial_decision(&mut state, &prior, Some(&credits), true, prior.updated_at),
            InitialDecision::Publish,
            "rolling {rolling_boundary}: seed"
        );
        commit_publication(&mut state, &prior, Some(credits.clone()));

        // First low refresh: the confirmation fetch admits a delayed candidate
        // while the published weekly stays on the stale baseline.
        let first_observed = initial_low.updated_at;
        assert_eq!(
            initial_decision(
                &mut state,
                &initial_low,
                Some(&credits),
                true,
                first_observed
            ),
            InitialDecision::RequiresConfirmation,
            "rolling {rolling_boundary}: initial low"
        );
        assert_eq!(
            confirmation_decision(
                &mut state,
                &initial_low,
                Some(&credits),
                &confirmed_low,
                Some(&credits),
                true,
                first_observed,
            ),
            ConfirmationDecision::Preserve,
            "rolling {rolling_boundary}: confirmed low"
        );
        assert_eq!(state.published_at, prior.updated_at);
        let shown = preserve_weekly(&state, initial_low.clone());
        assert_eq!(shown.secondary.as_ref().unwrap().used_percent, 81.0);
        let admitted = state.candidate.clone().expect("delayed candidate admitted");
        assert_eq!(admitted.snapshot_updated_at, confirmed_low.updated_at);
        assert_eq!(admitted.created_at, initial_low.updated_at);
        let admitted_json = serde_json::to_value(&state.candidate).unwrap();

        // The credits phase of the same refresh keeps the admitted evidence.
        let mut credits_only = UsageSnapshot::new(RateWindow::new(20.0));
        credits_only.updated_at = at(-298);
        assert_eq!(
            initial_decision(&mut state, &credits_only, None, true, at(-298)),
            InitialDecision::Preserve
        );
        assert_eq!(
            serde_json::to_value(&state.candidate).unwrap(),
            admitted_json
        );

        // Relaunch: the account state round-trips through the StateFile envelope.
        let encoded = serde_json::to_vec(&StateFile {
            version: STATE_VERSION,
            accounts: HashMap::from([(String::from("scope"), state)]),
        })
        .unwrap();
        let mut reloaded_file: StateFile = serde_json::from_slice(&encoded).unwrap();
        let mut reloaded = reloaded_file.accounts.remove("scope").unwrap();
        assert_eq!(reloaded.published_at, prior.updated_at);
        assert_eq!(
            serde_json::to_value(&reloaded.candidate).unwrap(),
            admitted_json
        );

        // The later low refresh confirms the candidate and publishes itself.
        assert_eq!(
            initial_decision(
                &mut reloaded,
                &later_low,
                Some(&credits),
                true,
                later_low.updated_at
            ),
            InitialDecision::Publish,
            "rolling {rolling_boundary}: later low"
        );
        commit_publication(&mut reloaded, &later_low, Some(credits.clone()));
        let published = reloaded.published_weekly.as_ref().unwrap();
        let expected = later_low.secondary.as_ref().unwrap();
        assert_eq!(published.used_percent, expected.used_percent);
        assert_eq!(published.resets_at, expected.resets_at);
        assert_eq!(reloaded.published_at, later_low.updated_at);
        assert!(reloaded.candidate.is_none());
    }
}
