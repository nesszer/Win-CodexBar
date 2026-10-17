//! Focused tests for each weekly-reset guard and rejection reason: exact-OAuth
//! gating, plan match, positive credit inventory consistency, credits observed
//! after the last publish, and per-account scoping. Each test pins one guard
//! so a regression names the guard it broke.

use super::*;

fn weekly_at(
    updated_at: DateTime<Utc>,
    used: f64,
    boundary: DateTime<Utc>,
    plan: &str,
) -> UsageSnapshot {
    let weekly = RateWindow::with_details(used, Some(WEEKLY_WINDOW_MINUTES), Some(boundary), None);
    let mut snapshot = UsageSnapshot::new(RateWindow::new(20.0)).with_secondary(weekly);
    snapshot.updated_at = updated_at;
    snapshot.login_method = Some(plan.to_string());
    snapshot
}

fn credit(id: &str) -> CreditIdentity {
    CreditIdentity {
        id: id.to_string(),
        reset_type: "weekly".to_string(),
        status: "available".to_string(),
        expires_at: Some(now() + chrono::Duration::days(3)),
    }
}

/// Inventory listing `credits` as available credits, with an explicit
/// `available_count` so incomplete inventories can be expressed.
fn inventory_with(
    credits: &[&str],
    available_count: u32,
    observed_at: DateTime<Utc>,
) -> CreditInventory {
    CreditInventory {
        available_count,
        credits: credits.iter().map(|id| credit(id)).collect(),
        observed_at,
    }
}

/// Stored state with a published weekly window and optional credit history.
fn state_with_history(
    inventory: Option<CreditInventory>,
    published_at: DateTime<Utc>,
    plan: &str,
) -> AccountState {
    AccountState {
        published_weekly: Some(RateWindow::with_details(
            45.0,
            Some(WEEKLY_WINDOW_MINUTES),
            Some(now() + chrono::Duration::days(2)),
            None,
        )),
        published_at,
        plan: Some(plan.to_string()),
        credit_inventory: inventory,
        candidate: None,
    }
}

#[test]
fn exact_oauth_gate_keeps_non_oauth_readings_out_of_candidates() {
    let initial = snapshot(0.0, 9, 1);
    let confirmation = snapshot(0.0, 9, 2);
    let inv = inventory("credit-a");

    // Control: exact OAuth readings store a delayed candidate.
    let mut exact_state = baseline();
    assert_eq!(
        initial_decision(&mut exact_state, &initial, Some(&inv), true, now()),
        InitialDecision::RequiresConfirmation
    );
    assert_eq!(
        confirmation_decision(
            &mut exact_state,
            &initial,
            Some(&inv),
            &confirmation,
            Some(&inv),
            true,
            now(),
        ),
        ConfirmationDecision::Preserve
    );
    assert!(exact_state.has_delayed_candidate());

    // The same readings from a non-exact source store nothing.
    let mut estimated_state = baseline();
    initial_decision(&mut estimated_state, &initial, Some(&inv), false, now());
    assert_eq!(
        confirmation_decision(
            &mut estimated_state,
            &initial,
            Some(&inv),
            &confirmation,
            Some(&inv),
            false,
            now(),
        ),
        ConfirmationDecision::Preserve
    );
    assert!(!estimated_state.has_delayed_candidate());

    // Revalidation of an exact candidate from a non-exact source discards it.
    let candidate = exact_state.candidate.clone().unwrap();
    let later = now() + chrono::Duration::seconds(61);
    let current = snapshot(0.0, 9, 3);
    assert_eq!(
        delayed_candidate_evaluation(&exact_state, &candidate, &current, Some(&inv), false, later,),
        (
            DelayedDecision::Discard,
            ResetDiagnosticReason::SourceNotExactOAuth
        )
    );
    // Control: the same revalidation from exact OAuth publishes.
    assert_eq!(
        delayed_candidate_evaluation(&exact_state, &candidate, &current, Some(&inv), true, later,),
        (
            DelayedDecision::Publish,
            ResetDiagnosticReason::ConfirmedObservation
        )
    );
}

#[test]
fn plans_match_requires_known_stored_plan_and_equal_observed_plans() {
    let boundary = now() + chrono::Duration::days(2);
    let pro = weekly_at(now(), 0.0, boundary, "ChatGPT Pro");
    let pro_spaced = weekly_at(now(), 0.0, boundary, " chatgpt pro ");
    let plus = weekly_at(now(), 0.0, boundary, "ChatGPT Plus");
    let mut unknown = pro.clone();
    unknown.login_method = None;

    // Trimmed and case-insensitive equality on all three plans.
    assert!(plans_match(Some("ChatGPT Pro"), &pro, &pro_spaced));
    // An unknown or blank stored plan never matches.
    assert!(!plans_match(None, &pro, &pro));
    assert!(!plans_match(Some("  "), &pro, &pro));
    // The observed plans must agree with each other.
    assert!(!plans_match(Some("ChatGPT Pro"), &pro, &plus));
    // The stored plan must agree with the observed plans.
    assert!(!plans_match(Some("ChatGPT Plus"), &pro, &pro));
    // A missing observed plan never matches.
    assert!(!plans_match(Some("ChatGPT Pro"), &unknown, &pro));
}

#[test]
fn candidate_requires_stored_plan_to_match_both_observations() {
    let initial = snapshot(0.0, 9, 1);
    let confirmation = snapshot(0.0, 9, 2);
    let inv = inventory("credit-a");

    for (stored_plan, expect_candidate) in [
        (Some("ChatGPT Pro"), true),
        (Some("ChatGPT Plus"), false),
        (None, false),
    ] {
        let mut state = baseline();
        state.plan = stored_plan.map(str::to_string);
        // Only the confirmation step runs: an initial read with a different
        // plan would reset the baseline before the candidate guard is reached.
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
            ConfirmationDecision::Preserve,
            "stored plan {stored_plan:?}"
        );
        assert_eq!(
            state.has_delayed_candidate(),
            expect_candidate,
            "stored plan {stored_plan:?}"
        );
    }
}

#[test]
fn positive_inventory_rejects_missing_inventory() {
    let t0 = now();
    let state = state_with_history(Some(inventory_with(&["a"], 1, t0)), t0, "ChatGPT Pro");
    let fresh = inventory_with(&["a"], 1, t0 + chrono::Duration::minutes(1));

    assert_eq!(
        positive_inventory_rejection(&state, None, Some(&fresh)),
        Some(ResetDiagnosticReason::MissingCreditInventory)
    );
    assert_eq!(
        positive_inventory_rejection(&state, Some(&fresh), None),
        Some(ResetDiagnosticReason::MissingCreditInventory)
    );
}

#[test]
fn positive_inventory_rejects_non_monotonic_observation_time() {
    let t0 = now();
    let state = state_with_history(Some(inventory_with(&["a"], 1, t0)), t0, "ChatGPT Pro");
    let before_stored = inventory_with(&["a"], 1, t0 - chrono::Duration::minutes(1));
    let later = inventory_with(&["a"], 1, t0 + chrono::Duration::minutes(1));
    let earlier = inventory_with(&["a"], 1, t0 + chrono::Duration::seconds(30));

    // A fresh read older than the stored inventory proves nothing.
    assert_eq!(
        positive_inventory_rejection(&state, Some(&before_stored), Some(&later)),
        Some(ResetDiagnosticReason::NonMonotonicCreditObservationTime)
    );
    // The confirmation read may not be older than the initial read.
    assert_eq!(
        positive_inventory_rejection(&state, Some(&later), Some(&earlier)),
        Some(ResetDiagnosticReason::NonMonotonicCreditObservationTime)
    );
}

#[test]
fn positive_inventory_rejects_inconsistent_available_count() {
    let t0 = now();
    let state = state_with_history(Some(inventory_with(&["a"], 1, t0)), t0, "ChatGPT Pro");
    let t1 = t0 + chrono::Duration::minutes(1);
    let t2 = t0 + chrono::Duration::minutes(2);

    // Zero available credits is not a positive inventory.
    let none_available = inventory_with(&[], 0, t1);
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&none_available),
            Some(&inventory_with(&["a"], 1, t2))
        ),
        Some(ResetDiagnosticReason::InconsistentAvailableCreditCount)
    );
    // The listed credits must match the stated count.
    let count_mismatch = inventory_with(&["a"], 2, t1);
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&count_mismatch),
            Some(&inventory_with(&["a"], 1, t2))
        ),
        Some(ResetDiagnosticReason::InconsistentAvailableCreditCount)
    );
}

#[test]
fn positive_inventory_rejects_changed_inventory_and_ignores_row_order() {
    let t0 = now();
    let t1 = t0 + chrono::Duration::minutes(1);
    let t2 = t0 + chrono::Duration::minutes(2);

    let state = state_with_history(Some(inventory_with(&["a"], 1, t0)), t0, "ChatGPT Pro");
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&inventory_with(&["b"], 1, t1)),
            Some(&inventory_with(&["a"], 1, t2)),
        ),
        Some(ResetDiagnosticReason::ChangedCreditInventory)
    );

    // Same credits in a different row order is the same inventory.
    let state = state_with_history(Some(inventory_with(&["a", "b"], 2, t0)), t0, "ChatGPT Pro");
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&inventory_with(&["b", "a"], 2, t1)),
            Some(&inventory_with(&["a", "b"], 2, t2)),
        ),
        None
    );
}

#[test]
fn positive_inventory_without_history_requires_reads_after_last_publish() {
    let published_at = now();
    let state = state_with_history(None, published_at, "ChatGPT Pro");
    let after = published_at + chrono::Duration::minutes(1);
    let later = published_at + chrono::Duration::minutes(2);
    let before = published_at - chrono::Duration::minutes(1);

    // Both fresh reads strictly after the last publish are accepted.
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&inventory_with(&["a"], 1, after)),
            Some(&inventory_with(&["a"], 1, later)),
        ),
        None
    );
    // A read at exactly the publish time is not newer than it.
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&inventory_with(&["a"], 1, published_at)),
            Some(&inventory_with(&["a"], 1, later)),
        ),
        Some(ResetDiagnosticReason::NonMonotonicCreditObservationTime)
    );
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&inventory_with(&["a"], 1, after)),
            Some(&inventory_with(&["a"], 1, published_at)),
        ),
        Some(ResetDiagnosticReason::NonMonotonicCreditObservationTime)
    );
    // A read from before the last publish is rejected even when the other is fresh.
    assert_eq!(
        positive_inventory_rejection(
            &state,
            Some(&inventory_with(&["a"], 1, before)),
            Some(&inventory_with(&["a"], 1, later)),
        ),
        Some(ResetDiagnosticReason::NonMonotonicCreditObservationTime)
    );
}

#[test]
fn different_account_never_reuses_another_accounts_state() {
    let auth_path = Path::new("C:/shared/auth.json");
    let account_a = scope_key(Some("account-a"), auth_path);
    let account_b = scope_key(Some("account-b"), auth_path);
    let home_only = scope_key(None, auth_path);
    assert_ne!(account_a, account_b);
    assert_ne!(account_a, home_only);
    assert_ne!(account_b, home_only);
    // The account id scopes the state, not the auth file location.
    assert_eq!(
        scope_key(Some("account-a"), Path::new("D:/elsewhere/auth.json")),
        account_a
    );

    let mut a_state = baseline();
    a_state.candidate = Some(DelayedCandidate {
        evidence_version: EVIDENCE_VERSION,
        first_observed_at: now(),
        created_at: now(),
        snapshot_updated_at: now(),
        weekly: snapshot(0.0, 9, 1).secondary.unwrap(),
        plan: Some("ChatGPT Pro".to_string()),
        inventory: inventory("credit-a"),
    });
    let mut accounts = HashMap::new();
    accounts.insert(account_a.clone(), a_state.clone());
    let file = StateFile {
        version: STATE_VERSION,
        accounts,
    };

    let loaded_a = account_state(file.clone(), &account_a);
    assert!(loaded_a.has_delayed_candidate());
    let stored_window = |state: &AccountState| {
        state
            .published_weekly
            .as_ref()
            .map(|window| (window.used_percent, window.resets_at))
    };
    assert!(stored_window(&loaded_a).is_some());
    assert_eq!(stored_window(&loaded_a), stored_window(&a_state));

    for scope in [&account_b, &home_only] {
        let loaded = account_state(file.clone(), scope);
        assert!(!loaded.has_delayed_candidate());
        assert!(loaded.published_weekly.is_none());
        assert!(loaded.credit_inventory.is_none());
        assert_eq!(loaded.published_at, DateTime::<Utc>::UNIX_EPOCH);
    }
}
