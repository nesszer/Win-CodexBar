//! Upstream `CodexMissingResetInventoryTests` (#4218 / issue #4210): a
//! confirmed advanced weekly reset publishes even when the previously
//! published snapshot carried no reset-credit inventory.
//!
//! Upstream's `previous-account` / `confirmation-account` rows have no
//! equivalent: the state is scoped per account (`scope_key`). Its
//! `invalid-credits` row (NaN timestamp) is not representable in chrono.
//! Upstream's `estimated` confidence maps to the exact-OAuth source flag.

use super::*;

fn at(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).unwrap()
}

fn previous_time() -> DateTime<Utc> {
    at(1_790_964_643)
}

fn initial_time() -> DateTime<Utc> {
    at(1_790_967_671)
}

fn previous_boundary() -> DateTime<Utc> {
    at(1_791_307_920)
}

fn fresh_boundary() -> DateTime<Utc> {
    at(1_791_569_581)
}

fn weekly_snapshot(time: DateTime<Utc>, used: f64, boundary: DateTime<Utc>) -> UsageSnapshot {
    let weekly = RateWindow::with_details(used, Some(WEEKLY_WINDOW_MINUTES), Some(boundary), None);
    let mut snapshot = UsageSnapshot::new(RateWindow::new(20.0)).with_secondary(weekly);
    snapshot.updated_at = time;
    snapshot.login_method = Some("pro".to_string());
    snapshot
}

fn credits_with(
    time: DateTime<Utc>,
    ids: &[&str],
    expiry_offset_seconds: i64,
) -> Option<CreditInventory> {
    let expires_at = fresh_boundary() + chrono::Duration::seconds(86_400 + expiry_offset_seconds);
    let reset = ResetCredits {
        available_count: 3,
        credits: ids
            .iter()
            .map(|id| ResetCredit {
                id: Some((*id).to_string()),
                reset_type: Some("codex_rate_limits".to_string()),
                status: Some("available".to_string()),
                expires_at: Some(expires_at.to_rfc3339()),
            })
            .collect(),
        observed_at: Some(time),
    };
    super::super::inventory(Some(&reset), time)
}

fn credits(time: DateTime<Utc>) -> Option<CreditInventory> {
    credits_with(time, &["a", "b", "c"], 0)
}

/// Published state whose last snapshot carried no credit inventory.
fn state_without_inventory(boundary: DateTime<Utc>) -> AccountState {
    let previous = weekly_snapshot(previous_time(), 99.0, boundary);
    AccountState {
        published_weekly: previous.secondary.clone(),
        published_at: previous.updated_at,
        plan: previous.login_method.clone(),
        credit_inventory: None,
        candidate: None,
    }
}

struct Reads {
    state: AccountState,
    initial: UsageSnapshot,
    initial_inventory: Option<CreditInventory>,
    confirmation: UsageSnapshot,
    confirmation_inventory: Option<CreditInventory>,
    exact_oauth: bool,
}

impl Reads {
    fn fresh(previous_boundary: DateTime<Utc>) -> Self {
        let confirmation_time = initial_time() + chrono::Duration::seconds(1);
        Self {
            state: state_without_inventory(previous_boundary),
            initial: weekly_snapshot(initial_time(), 1.0, fresh_boundary()),
            initial_inventory: credits(initial_time()),
            confirmation: weekly_snapshot(confirmation_time, 1.0, fresh_boundary()),
            confirmation_inventory: credits(confirmation_time),
            exact_oauth: true,
        }
    }

    fn confirm(&mut self) -> ConfirmationDecision {
        confirmation_decision(
            &mut self.state,
            &self.initial,
            self.initial_inventory.as_ref(),
            &self.confirmation,
            self.confirmation_inventory.as_ref(),
            self.exact_oauth,
            initial_time(),
        )
    }
}

#[test]
fn issue_4210_publishes_two_matching_fresh_reads_without_historical_inventory() {
    let mut reads = Reads::fresh(previous_boundary());
    assert_eq!(
        initial_decision(
            &mut reads.state,
            &reads.initial,
            reads.initial_inventory.as_ref(),
            true,
            initial_time(),
        ),
        InitialDecision::RequiresConfirmation
    );
    assert_eq!(reads.confirm(), ConfirmationDecision::Publish);
    assert!(reads.state.candidate.is_none());

    commit_publication(
        &mut reads.state,
        &reads.confirmation,
        reads.confirmation_inventory.clone(),
    );
    let published = reads.state.published_weekly.as_ref().unwrap();
    assert_eq!(published.used_percent, 1.0);
    assert_eq!(published.resets_at, Some(fresh_boundary()));
    assert_eq!(
        reads
            .state
            .credit_inventory
            .as_ref()
            .unwrap()
            .available_count,
        3
    );
    assert_eq!(reads.state.published_at, reads.confirmation.updated_at);
}

#[test]
fn missing_history_still_requires_compatible_complete_fresh_evidence() {
    type Mutation = fn(&mut Reads);
    let cases: [(&str, Mutation); 15] = [
        ("confirmation-plan", |reads| {
            reads.confirmation.login_method = Some("plus".to_string());
        }),
        ("missing-plan", |reads| {
            reads.confirmation.login_method = None
        }),
        ("blank-plans", |reads| {
            reads.state.plan = Some(" ".to_string());
            reads.initial.login_method = Some(" ".to_string());
            reads.confirmation.login_method = Some(" ".to_string());
        }),
        ("initial-credits", |reads| reads.initial_inventory = None),
        ("confirmation-credits", |reads| {
            reads.confirmation_inventory = None;
        }),
        ("changed-credit", |reads| {
            reads.confirmation_inventory =
                credits_with(reads.confirmation.updated_at, &["a", "b", "different"], 0);
        }),
        ("changed-expiry", |reads| {
            reads.confirmation_inventory =
                credits_with(reads.confirmation.updated_at, &["a", "b", "c"], 1);
        }),
        ("changed-multiplicity", |reads| {
            reads.initial_inventory = credits_with(reads.initial.updated_at, &["a", "a", "b"], 0);
            reads.confirmation_inventory =
                credits_with(reads.confirmation.updated_at, &["a", "b", "b"], 0);
        }),
        ("partial-inventory", |reads| {
            reads.confirmation_inventory =
                credits_with(reads.confirmation.updated_at, &["a", "b"], 0);
        }),
        ("stale-credits", |reads| {
            reads.confirmation_inventory = credits(previous_time());
        }),
        ("estimated", |reads| reads.exact_oauth = false),
        ("missing-weekly", |reads| {
            reads.confirmation.secondary = None
        }),
        ("historical-credits", |reads| {
            let stale = previous_time() - chrono::Duration::seconds(1);
            reads.initial_inventory = credits(stale);
            reads.confirmation_inventory = credits(stale);
        }),
        ("concurrent-credits", |reads| {
            reads.initial_inventory = credits(previous_time());
            reads.confirmation_inventory = credits(previous_time());
        }),
        ("historical-credits-unchanged-boundary", |reads| {
            reads.state = state_without_inventory(fresh_boundary());
            let stale = previous_time() - chrono::Duration::seconds(1);
            reads.initial_inventory = credits(stale);
            reads.confirmation_inventory = credits(stale);
        }),
    ];
    for (name, mutate) in cases {
        let mut reads = Reads::fresh(previous_boundary());
        mutate(&mut reads);
        assert_eq!(reads.confirm(), ConfirmationDecision::Preserve, "{name}");
        assert!(reads.state.candidate.is_none(), "{name}");
    }
}

#[test]
fn missing_history_can_retain_unchanged_boundary_evidence_for_delayed_confirmation() {
    let mut reads = Reads::fresh(fresh_boundary());
    assert_eq!(reads.confirm(), ConfirmationDecision::Preserve);
    assert!(reads.state.candidate.is_some());

    let later_time = initial_time() + chrono::Duration::seconds(61);
    let later = weekly_snapshot(later_time, 1.0, fresh_boundary());
    let later_inventory = credits(later_time);
    assert_eq!(
        initial_decision(
            &mut reads.state,
            &later,
            later_inventory.as_ref(),
            true,
            later_time,
        ),
        InitialDecision::Publish
    );
    commit_publication(&mut reads.state, &later, later_inventory);
    assert_eq!(reads.state.published_at, later_time);
    assert_eq!(
        reads
            .state
            .credit_inventory
            .as_ref()
            .unwrap()
            .available_count,
        3
    );
    assert!(reads.state.candidate.is_none());
}

#[test]
fn missing_history_delayed_candidate_rejects_stale_current_inventory() {
    let mut reads = Reads::fresh(fresh_boundary());
    assert_eq!(reads.confirm(), ConfirmationDecision::Preserve);
    let candidate = reads.state.candidate.clone().unwrap();
    let later_time = initial_time() + chrono::Duration::seconds(61);
    let later = weekly_snapshot(later_time, 1.0, fresh_boundary());
    assert_eq!(
        delayed_candidate_evaluation(
            &reads.state,
            &candidate,
            &later,
            credits(previous_time()).as_ref(),
            true,
            later_time,
        ),
        (
            DelayedDecision::Discard,
            ResetDiagnosticReason::NonMonotonicCreditObservationTime
        )
    );
}

#[test]
fn known_history_ignores_inventory_observed_before_the_stored_one() {
    let mut reads = Reads::fresh(previous_boundary());
    // A zero-credit stored inventory normally lets two fresh reads publish
    // early; a fresh read older than that inventory proves nothing.
    reads.state.credit_inventory = Some(CreditInventory {
        available_count: 0,
        credits: Vec::new(),
        observed_at: initial_time() + chrono::Duration::seconds(30),
    });
    assert_eq!(reads.confirm(), ConfirmationDecision::Preserve);

    let mut reads = Reads::fresh(previous_boundary());
    reads.state.credit_inventory = Some(CreditInventory {
        available_count: 0,
        credits: Vec::new(),
        observed_at: previous_time(),
    });
    assert_eq!(reads.confirm(), ConfirmationDecision::Publish);
}

#[test]
fn inventory_records_the_endpoint_observation_time() {
    let fetched = previous_time();
    let reset = ResetCredits {
        available_count: 0,
        credits: Vec::new(),
        observed_at: Some(fetched),
    };
    let inventory = super::super::inventory(Some(&reset), initial_time()).unwrap();
    assert_eq!(inventory.observed_at, fetched);
    let unknown = ResetCredits {
        observed_at: None,
        ..reset
    };
    let inventory = super::super::inventory(Some(&unknown), initial_time()).unwrap();
    assert_eq!(inventory.observed_at, initial_time());
}

#[test]
fn persisted_inventory_without_observation_time_decodes_as_epoch() {
    let inventory: CreditInventory =
        serde_json::from_str(r#"{"availableCount":0,"credits":[]}"#).unwrap();
    assert_eq!(inventory.observed_at, DateTime::<Utc>::UNIX_EPOCH);
}
