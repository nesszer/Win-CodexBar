//! Plan-change baseline tests (upstream 0.69.0 #4088,
//! `CodexPlanTransitionPublicationTests`).

use super::*;

fn plan_snapshot(plan: Option<&str>, used: f64, captured_minutes: i64) -> UsageSnapshot {
    let mut snapshot = snapshot(used, 9, captured_minutes);
    snapshot.login_method = plan.map(str::to_string);
    snapshot
}

/// Plus subscription with a stale 80% weekly baseline that resets in one day.
fn plus_baseline() -> AccountState {
    let mut previous = snapshot(80.0, 1, 0);
    previous.login_method = Some("ChatGPT Plus".to_string());
    AccountState {
        published_weekly: previous.secondary.clone(),
        published_at: previous.updated_at,
        plan: previous.login_method.clone(),
        credit_inventory: Some(inventory("credit-a")),
        candidate: None,
    }
}

#[test]
fn plan_upgrade_starts_a_new_baseline_and_publishes_the_new_plan() {
    let mut state = plus_baseline();
    let inv = inventory("credit-a");
    let initial = plan_snapshot(Some("ChatGPT Pro"), 0.0, 10);
    let confirmation = plan_snapshot(Some("ChatGPT Pro"), 0.0, 11);
    assert_eq!(
        initial_decision(&mut state, &initial, Some(&inv), true, now()),
        InitialDecision::RequiresConfirmation
    );
    assert!(state.published_weekly.is_none());
    assert!(state.credit_inventory.is_none());
    assert!(state.candidate.is_none());
    assert_eq!(
        confirm(&mut state, &initial, &confirmation, Some(&inv)),
        ConfirmationDecision::Publish
    );
}

#[test]
fn same_plan_near_zero_reading_keeps_the_previous_weekly_pinned() {
    // Identical to the upgrade scenario, but the plan did not change: the old
    // weekly window stays pinned until the confirmation is trustworthy.
    let mut state = plus_baseline();
    let inv = inventory("credit-a");
    let initial = plan_snapshot(Some("ChatGPT Plus"), 0.0, 10);
    let confirmation = plan_snapshot(Some("ChatGPT Plus"), 0.0, 11);
    assert_eq!(
        initial_decision(&mut state, &initial, Some(&inv), true, now()),
        InitialDecision::RequiresConfirmation
    );
    assert!(state.published_weekly.is_some());
    assert_eq!(
        confirm(&mut state, &initial, &confirmation, Some(&inv)),
        ConfirmationDecision::Preserve
    );
}

#[test]
fn plan_upgrade_does_not_pin_the_previous_plan_weekly_window() {
    let mut state = plus_baseline();
    let current = plan_snapshot(Some("ChatGPT Pro"), 5.0, 10);
    assert_eq!(
        initial_decision(&mut state, &current, None, true, now()),
        InitialDecision::Publish
    );
    let preserved = preserve_weekly(&state, current.clone());
    let used = |snapshot: &UsageSnapshot| snapshot.secondary.as_ref().map(|w| w.used_percent);
    assert_eq!(used(&preserved), Some(5.0));
    assert_eq!(used(&preserved), used(&current));
}

#[test]
fn plan_change_discards_a_pending_candidate() {
    let mut state = plus_baseline();
    state.candidate = Some(pending_candidate(RateWindow::new(0.0), "ChatGPT Plus"));
    let current = plan_snapshot(Some("ChatGPT Pro"), 5.0, 10);
    assert_eq!(
        initial_decision(&mut state, &current, None, true, now()),
        InitialDecision::Publish
    );
    assert!(state.candidate.is_none());
}

#[test]
fn same_unknown_stale_or_non_oauth_plans_keep_the_baseline() {
    let cases: [(&str, Option<&str>, bool); 4] = [
        (
            "same plan with case and spacing",
            Some(" chatgpt plus "),
            true,
        ),
        ("unknown fresh plan", None, true),
        ("blank fresh plan", Some("  "), true),
        ("not exact OAuth", Some("ChatGPT Pro"), false),
    ];
    for (name, plan, exact_oauth) in cases {
        let mut state = plus_baseline();
        let current = plan_snapshot(plan, 5.0, 10);
        initial_decision(&mut state, &current, None, exact_oauth, now());
        assert!(state.published_weekly.is_some(), "{name}");
        assert!(state.credit_inventory.is_some(), "{name}");
    }

    let mut unknown_stored = plus_baseline();
    unknown_stored.plan = None;
    let fresh = plan_snapshot(Some("ChatGPT Pro"), 5.0, 10);
    initial_decision(&mut unknown_stored, &fresh, None, true, now());
    assert!(
        unknown_stored.published_weekly.is_some(),
        "unknown stored plan"
    );

    let mut older = plus_baseline();
    let stale = plan_snapshot(Some("ChatGPT Pro"), 5.0, -1);
    initial_decision(&mut older, &stale, None, true, now());
    assert!(older.published_weekly.is_some(), "older observation");
}

#[test]
fn unknown_plan_publication_preserves_the_last_known_plan() {
    for unknown_plan in [None, Some("  ")] {
        let mut state = plus_baseline();
        let inventory = inventory("credit-a");
        let unknown = plan_snapshot(unknown_plan, 50.0, 10);
        assert_eq!(
            initial_decision(&mut state, &unknown, Some(&inventory), true, now()),
            InitialDecision::Publish
        );
        commit_publication(&mut state, &unknown, Some(inventory));
        assert_eq!(state.plan.as_deref(), Some("ChatGPT Plus"));

        let changed = plan_snapshot(Some("ChatGPT Pro"), 50.0, 11);
        assert_eq!(
            initial_decision(&mut state, &changed, None, true, now()),
            InitialDecision::Publish
        );
        assert!(state.published_weekly.is_none());
        assert!(state.credit_inventory.is_none());
    }
}

#[test]
fn near_zero_confirmation_must_report_the_initial_plan() {
    let inv = inventory("credit-a");
    for confirmation_plan in [Some("ChatGPT Plus"), None] {
        for has_baseline in [false, true] {
            let mut state = if has_baseline {
                baseline()
            } else {
                AccountState::default()
            };
            let initial = plan_snapshot(Some("ChatGPT Pro"), 0.0, 10);
            let confirmation = plan_snapshot(confirmation_plan, 0.0, 11);
            assert_eq!(
                confirm(&mut state, &initial, &confirmation, Some(&inv)),
                ConfirmationDecision::Preserve,
                "{confirmation_plan:?} baseline {has_baseline}"
            );
            assert!(state.candidate.is_none());
        }
    }
}

#[test]
fn nonzero_confirmation_can_publish_its_own_plan() {
    let mut state = AccountState::default();
    let initial = plan_snapshot(Some("ChatGPT Pro"), 0.0, 10);
    let confirmation = plan_snapshot(Some("ChatGPT Plus"), 5.0, 11);
    assert_eq!(
        confirm(&mut state, &initial, &confirmation, None),
        ConfirmationDecision::Publish
    );
}

/// Upstream `new plan cannot borrow missing weekly usage from the old plan`: a
/// changed plan without a weekly window publishes as-is instead of showing the
/// previous plan's weekly window. The same plan still keeps the old window.
#[test]
fn new_plan_cannot_borrow_missing_weekly_usage_from_the_old_plan() {
    let mut state = plus_baseline();
    let mut current = plan_snapshot(Some("ChatGPT Pro"), 5.0, 10);
    current.secondary = None;
    assert_eq!(
        initial_decision(&mut state, &current, None, true, now()),
        InitialDecision::Publish
    );
    assert!(preserve_weekly(&state, current.clone()).secondary.is_none());
    commit_publication(&mut state, &current, None);
    assert!(state.published_weekly.is_none());

    let mut same_plan = plus_baseline();
    let mut same = plan_snapshot(Some("ChatGPT Plus"), 5.0, 10);
    same.secondary = None;
    assert_eq!(
        initial_decision(&mut same_plan, &same, None, true, now()),
        InitialDecision::Preserve
    );
    let shown = preserve_weekly(&same_plan, same);
    assert_eq!(
        shown.secondary.map(|weekly| weekly.used_percent),
        Some(80.0)
    );
}

/// Upstream `new token plan replaces previous plan quota baseline` for 0 % and
/// 5 %: the new plan publishes its own usage and reset, which become the
/// stored baseline, and no reset candidate survives.
#[test]
fn new_plan_replaces_the_previous_plan_quota_baseline() {
    for used in [0.0, 5.0] {
        let mut state = plus_baseline();
        let inv = inventory("credit-a");
        let current = plan_snapshot(Some("ChatGPT Pro"), used, 10);
        let confirmation = plan_snapshot(Some("ChatGPT Pro"), used, 11);
        let published = match initial_decision(&mut state, &current, Some(&inv), true, now()) {
            InitialDecision::Publish => current.clone(),
            InitialDecision::RequiresConfirmation => {
                assert_eq!(
                    confirm(&mut state, &current, &confirmation, Some(&inv)),
                    ConfirmationDecision::Publish,
                    "{used}% confirmation"
                );
                confirmation.clone()
            }
            InitialDecision::Preserve => panic!("{used}%: previous plan baseline pinned"),
        };
        commit_publication(&mut state, &published, Some(inv));
        let weekly = state.published_weekly.as_ref().expect("new baseline");
        assert_eq!(weekly.used_percent, used);
        assert_eq!(
            weekly.resets_at,
            current
                .secondary
                .as_ref()
                .and_then(|window| window.resets_at)
        );
        assert_eq!(state.plan.as_deref(), Some("ChatGPT Pro"));
        assert!(state.candidate.is_none());
    }
}

/// Upstream `same or unknown token plan cannot discard previous quota
/// evidence`: a near-zero reading with the same (any case or spacing) or a
/// blank plan still needs a later confirmation, so the old weekly stays shown.
#[test]
fn same_or_unknown_plan_cannot_discard_previous_quota_evidence() {
    for plan in ["ChatGPT Plus", " CHATGPT PLUS ", ""] {
        let mut state = plus_baseline();
        let inv = inventory("credit-a");
        let current = plan_snapshot(Some(plan), 0.0, 10);
        assert_eq!(
            initial_decision(&mut state, &current, Some(&inv), true, now()),
            InitialDecision::RequiresConfirmation,
            "{plan:?}"
        );
        assert_eq!(
            confirm(&mut state, &current, &current, Some(&inv)),
            ConfirmationDecision::Preserve,
            "{plan:?}"
        );
        let shown = preserve_weekly(&state, current);
        assert_eq!(
            shown.secondary.map(|weekly| weekly.used_percent),
            Some(80.0),
            "{plan:?}"
        );
    }
}
