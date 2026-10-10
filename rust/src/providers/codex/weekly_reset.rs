use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::api::{ResetCredit, ResetCredits};
use crate::core::{RateWindow, UsageSnapshot};

const STATE_VERSION: u32 = 1;
const EVIDENCE_VERSION: u32 = 1;
const RESET_THRESHOLD: f64 = 1.0;
const RESET_TOLERANCE_SECONDS: i64 = 2 * 60;
const STABLE_BOUNDARY_TOLERANCE_SECONDS: i64 = 1;
const CANDIDATE_MINIMUM_AGE_SECONDS: i64 = 60;
const CANDIDATE_MAXIMUM_AGE_SECONDS: i64 = 30 * 60;
const WEEKLY_WINDOW_MINUTES: u32 = 7 * 24 * 60;
const WEEKLY_WINDOW_SECONDS: i64 = WEEKLY_WINDOW_MINUTES as i64 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StateFile {
    version: u32,
    accounts: HashMap<String, AccountState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountState {
    published_weekly: Option<RateWindow>,
    published_at: DateTime<Utc>,
    plan: Option<String>,
    credit_inventory: Option<CreditInventory>,
    candidate: Option<DelayedCandidate>,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            published_weekly: None,
            published_at: DateTime::<Utc>::UNIX_EPOCH,
            plan: None,
            credit_inventory: None,
            candidate: None,
        }
    }
}

impl AccountState {
    /// Whether a delayed reset candidate waits for revalidation.
    pub(super) fn has_delayed_candidate(&self) -> bool {
        self.candidate.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreditInventory {
    available_count: u32,
    credits: Vec<CreditIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreditIdentity {
    id: String,
    reset_type: String,
    status: String,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DelayedCandidate {
    evidence_version: u32,
    first_observed_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    snapshot_updated_at: DateTime<Utc>,
    weekly: RateWindow,
    plan: Option<String>,
    inventory: CreditInventory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InitialDecision {
    Publish,
    Preserve,
    RequiresConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfirmationDecision {
    Publish,
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DelayedDecision {
    Publish,
    Retain,
    Discard,
}

impl DelayedDecision {
    const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Retain => "retain",
            Self::Discard => "discard",
        }
    }
}

mod diagnostics;
use diagnostics::{ResetDiagnosticReason, log_reset_diagnostic};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetCreditEvidence {
    None,
    Consumed,
    NoAvailableCredits,
}

pub(super) fn scope_key(account_id: Option<&str>, auth_path: &Path) -> String {
    let raw = account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("account:{value}"))
        .unwrap_or_else(|| format!("home:{}", auth_path.to_string_lossy().to_lowercase()));
    let digest = Sha256::digest(raw.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// An unreadable, malformed or other-version state file reads as absent.
fn read_state_file(path: &Path) -> Option<StateFile> {
    crate::secure_file::read_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<StateFile>(&raw).ok())
        .filter(|file| file.version == STATE_VERSION)
}

pub(super) fn load(scope: &str) -> AccountState {
    state_path()
        .and_then(|path| read_state_file(&path))
        .and_then(|file| file.accounts.get(scope).cloned())
        .unwrap_or_default()
}

pub(super) fn save(scope: &str, state: &AccountState) {
    let skipped = || {
        log_reset_diagnostic(
            "candidatePersistence",
            "skipped",
            ResetDiagnosticReason::StoreUnavailable,
        );
    };
    let Some(path) = state_path() else {
        skipped();
        return;
    };
    let mut file = read_state_file(&path).unwrap_or_else(|| StateFile {
        version: STATE_VERSION,
        accounts: HashMap::new(),
    });
    file.accounts.insert(scope.to_string(), state.clone());
    if path
        .parent()
        .is_none_or(|parent| std::fs::create_dir_all(parent).is_err())
    {
        skipped();
        return;
    }
    if let Ok(raw) = serde_json::to_string_pretty(&file) {
        let _written = crate::secure_file::write_string(&path, &raw);
        log_reset_diagnostic(
            "candidatePersistence",
            "requested",
            ResetDiagnosticReason::StoreRequested,
        );
    }
}

fn state_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|root| root.join("CodexBar").join("codex-weekly-reset-v1.json"))
}

pub(super) fn inventory(
    reset: Option<&ResetCredits>,
    observed_at: DateTime<Utc>,
) -> Option<CreditInventory> {
    let reset = reset?;
    let mut credits = reset
        .credits
        .iter()
        .filter_map(|credit| credit_identity(credit, observed_at))
        .collect::<Vec<_>>();
    credits.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.reset_type.cmp(&right.reset_type))
            .then_with(|| left.status.cmp(&right.status))
            .then_with(|| left.expires_at.cmp(&right.expires_at))
    });
    let available = credits
        .iter()
        .filter(|credit| credit_identity_is_available(credit))
        .count();
    if available != usize::try_from(reset.available_count).ok()? {
        return None;
    }
    Some(CreditInventory {
        available_count: reset.available_count,
        credits,
    })
}

fn credit_identity_is_available(credit: &CreditIdentity) -> bool {
    credit.status.is_empty() || credit.status.eq_ignore_ascii_case("available")
}

fn credit_identity(credit: &ResetCredit, observed_at: DateTime<Utc>) -> Option<CreditIdentity> {
    let id = credit.id.as_deref()?.trim();
    if id.is_empty() {
        return None;
    }
    let expires_at = credit
        .expires_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    if expires_at.is_some_and(|value| value <= observed_at) {
        return None;
    }
    Some(CreditIdentity {
        id: id.to_string(),
        reset_type: credit.reset_type.clone().unwrap_or_default(),
        status: credit
            .status
            .clone()
            .unwrap_or_else(|| "available".to_string()),
        expires_at,
    })
}

pub(super) fn initial_decision(
    state: &mut AccountState,
    current: &UsageSnapshot,
    current_inventory: Option<&CreditInventory>,
    exact_oauth: bool,
    observed_at: DateTime<Utc>,
) -> InitialDecision {
    if exact_oauth && plan_changed(state, current) {
        // A new subscription has its own quota baseline, not evidence of a
        // reset on the previous plan: drop the stored weekly window, pending
        // candidate, and credit inventory so the old plan cannot be pinned.
        log_reset_diagnostic("planBaseline", "reset", ResetDiagnosticReason::PlanChanged);
        *state = AccountState::default();
    }
    if let Some(candidate) = state.candidate.clone() {
        match delayed_candidate_decision(
            state,
            &candidate,
            current,
            current_inventory,
            exact_oauth,
            observed_at,
        ) {
            DelayedDecision::Publish => {
                state.candidate = None;
                return InitialDecision::Publish;
            }
            DelayedDecision::Retain => return InitialDecision::Preserve,
            DelayedDecision::Discard => state.candidate = None,
        }
    }

    let Some(current_weekly) = weekly(current) else {
        return if state.published_weekly.is_some() {
            InitialDecision::Preserve
        } else {
            InitialDecision::Publish
        };
    };
    if !current_weekly.used_percent.is_finite()
        || !is_valid_boundary(current_weekly, current.updated_at)
    {
        return InitialDecision::Preserve;
    }

    let Some(previous_weekly) = state.published_weekly.as_ref() else {
        return if current_weekly.used_percent <= RESET_THRESHOLD {
            InitialDecision::RequiresConfirmation
        } else {
            InitialDecision::Publish
        };
    };
    if current.updated_at <= state.published_at || !previous_weekly.used_percent.is_finite() {
        return InitialDecision::Preserve;
    }
    if boundary_moves_backward(previous_weekly, current_weekly) {
        return InitialDecision::Preserve;
    }
    if previous_weekly.used_percent > RESET_THRESHOLD
        && current_weekly.used_percent <= RESET_THRESHOLD
    {
        InitialDecision::RequiresConfirmation
    } else {
        InitialDecision::Publish
    }
}

pub(super) fn confirmation_decision(
    state: &mut AccountState,
    initial: &UsageSnapshot,
    initial_inventory: Option<&CreditInventory>,
    confirmation: &UsageSnapshot,
    confirmation_inventory: Option<&CreditInventory>,
    exact_oauth: bool,
    observed_at: DateTime<Utc>,
) -> ConfirmationDecision {
    let Some(initial_weekly) = weekly(initial) else {
        return ConfirmationDecision::Preserve;
    };
    let Some(confirmation_weekly) = weekly(confirmation) else {
        return ConfirmationDecision::Preserve;
    };
    if confirmation.updated_at <= initial.updated_at
        || !initial_weekly.used_percent.is_finite()
        || !confirmation_weekly.used_percent.is_finite()
        || !is_valid_boundary(initial_weekly, initial.updated_at)
        || !is_valid_boundary(confirmation_weekly, confirmation.updated_at)
        || boundary_distance_seconds(initial_weekly, confirmation_weekly).abs()
            >= RESET_TOLERANCE_SECONDS
    {
        return ConfirmationDecision::Preserve;
    }
    if confirmation_weekly.used_percent > RESET_THRESHOLD {
        return ConfirmationDecision::Publish;
    }
    // A near-zero reading is only trusted when both observations report the
    // same plan; a plan flip between them is not a confirmation.
    if initial_weekly.used_percent > RESET_THRESHOLD
        || normalized_plan(initial.login_method.as_deref())
            != normalized_plan(confirmation.login_method.as_deref())
    {
        return ConfirmationDecision::Preserve;
    }

    let Some(previous_weekly) = state.published_weekly.as_ref() else {
        return ConfirmationDecision::Publish;
    };
    let credit_evidence = reset_credit_evidence(
        state.credit_inventory.as_ref(),
        initial_inventory,
        confirmation_inventory,
        observed_at,
    );
    if let Some(previous_boundary) = previous_weekly.resets_at {
        if confirmation.updated_at
            < previous_boundary - chrono::Duration::seconds(RESET_TOLERANCE_SECONDS)
            && credit_evidence == ResetCreditEvidence::None
        {
            maybe_store_delayed_candidate(
                state,
                initial,
                confirmation,
                confirmation_inventory,
                exact_oauth,
                observed_at,
            );
            return ConfirmationDecision::Preserve;
        }
        let initial_advance = boundary_distance_seconds(previous_weekly, initial_weekly);
        let confirmation_advance = boundary_distance_seconds(previous_weekly, confirmation_weekly);
        if initial_advance < RESET_TOLERANCE_SECONDS
            || confirmation_advance < RESET_TOLERANCE_SECONDS
        {
            return if credit_evidence == ResetCreditEvidence::Consumed {
                ConfirmationDecision::Publish
            } else {
                maybe_store_delayed_candidate(
                    state,
                    initial,
                    confirmation,
                    confirmation_inventory,
                    exact_oauth,
                    observed_at,
                );
                ConfirmationDecision::Preserve
            };
        }
    }
    ConfirmationDecision::Publish
}

fn maybe_store_delayed_candidate(
    state: &mut AccountState,
    initial: &UsageSnapshot,
    confirmation: &UsageSnapshot,
    confirmation_inventory: Option<&CreditInventory>,
    exact_oauth: bool,
    observed_at: DateTime<Utc>,
) {
    match delayed_candidate_admission(
        state,
        initial,
        confirmation,
        confirmation_inventory,
        exact_oauth,
        observed_at,
    ) {
        Ok(candidate) => {
            state.candidate = Some(candidate);
            log_reset_diagnostic(
                "candidateCreation",
                "created",
                ResetDiagnosticReason::CandidateCreated,
            );
        }
        Err(reason) => log_reset_diagnostic("candidateCreation", "rejected", reason),
    }
}

/// The checks run in a fixed order; the first failure is the logged reason.
fn delayed_candidate_admission(
    state: &AccountState,
    initial: &UsageSnapshot,
    confirmation: &UsageSnapshot,
    confirmation_inventory: Option<&CreditInventory>,
    exact_oauth: bool,
    observed_at: DateTime<Utc>,
) -> Result<DelayedCandidate, ResetDiagnosticReason> {
    use ResetDiagnosticReason as Reason;
    if !exact_oauth {
        return Err(Reason::SourceNotExactOAuth);
    }
    if !plans_match(state.plan.as_deref(), initial, confirmation) {
        return Err(Reason::PlanMismatch);
    }
    let previous_weekly = state
        .published_weekly
        .as_ref()
        .ok_or(Reason::MissingPreviousSnapshot)?;
    let initial_weekly = weekly(initial).ok_or(Reason::MissingWeeklyWindow)?;
    let confirmation_weekly = weekly(confirmation).ok_or(Reason::MissingWeeklyWindow)?;
    let previous_inventory = state
        .credit_inventory
        .as_ref()
        .ok_or(Reason::MissingCreditInventory)?;
    let confirmation_inventory = confirmation_inventory.ok_or(Reason::MissingCreditInventory)?;
    if previous_inventory.available_count == 0 || previous_inventory != confirmation_inventory {
        return Err(Reason::ChangedCreditInventory);
    }
    if !supported_delayed_boundary(previous_weekly, initial_weekly)
        || !supported_delayed_boundary(previous_weekly, confirmation_weekly)
    {
        return Err(Reason::UnsupportedResetBoundary);
    }
    if boundary_distance_seconds(initial_weekly, confirmation_weekly).abs()
        >= RESET_TOLERANCE_SECONDS
    {
        return Err(Reason::InconsistentResetBoundary);
    }
    Ok(DelayedCandidate {
        evidence_version: EVIDENCE_VERSION,
        first_observed_at: initial.updated_at,
        created_at: observed_at,
        snapshot_updated_at: confirmation.updated_at,
        weekly: confirmation_weekly.clone(),
        plan: confirmation.login_method.clone(),
        inventory: confirmation_inventory.clone(),
    })
}

fn delayed_candidate_decision(
    state: &AccountState,
    candidate: &DelayedCandidate,
    current: &UsageSnapshot,
    current_inventory: Option<&CreditInventory>,
    exact_oauth: bool,
    observed_at: DateTime<Utc>,
) -> DelayedDecision {
    let (decision, reason) = delayed_candidate_evaluation(
        state,
        candidate,
        current,
        current_inventory,
        exact_oauth,
        observed_at,
    );
    log_reset_diagnostic("delayedCandidate", decision.diagnostic_code(), reason);
    decision
}

/// Pure delayed-candidate revalidation: the decision plus the fixed reason
/// code that `delayed_candidate_decision` logs (upstream `DelayedEvaluation`).
fn delayed_candidate_evaluation(
    state: &AccountState,
    candidate: &DelayedCandidate,
    current: &UsageSnapshot,
    current_inventory: Option<&CreditInventory>,
    exact_oauth: bool,
    observed_at: DateTime<Utc>,
) -> (DelayedDecision, ResetDiagnosticReason) {
    use DelayedDecision::{Discard, Publish, Retain};
    use ResetDiagnosticReason as Reason;

    let age = observed_at
        .signed_duration_since(candidate.created_at)
        .num_seconds();
    if candidate.evidence_version != EVIDENCE_VERSION {
        return (Discard, Reason::EvidenceVersionMismatch);
    }
    if age < 0 {
        return (Discard, Reason::FutureCandidate);
    }
    if age > CANDIDATE_MAXIMUM_AGE_SECONDS {
        return (Discard, Reason::ExpiredCandidate);
    }
    if !exact_oauth {
        return (Discard, Reason::SourceNotExactOAuth);
    }
    let Some(previous_weekly) = state.published_weekly.as_ref() else {
        return (Discard, Reason::MissingPreviousSnapshot);
    };
    let Some(current_weekly) = weekly(current) else {
        // Credits-only refreshes do not carry the weekly window (or necessarily
        // the plan/inventory fields). Preserve the candidate and let the next
        // complete usage observation validate it.
        return (Retain, Reason::MissingWeeklyWindow);
    };
    if !plans_match(state.plan.as_deref(), current, current) {
        return (Discard, Reason::PlanMismatch);
    }
    if previous_weekly.used_percent <= RESET_THRESHOLD
        || current_weekly.used_percent > RESET_THRESHOLD
    {
        return (Discard, Reason::ResetThresholdMismatch);
    }
    if current.updated_at <= candidate.snapshot_updated_at {
        return (Discard, Reason::StaleObservation);
    }
    if !is_valid_boundary(current_weekly, current.updated_at) {
        return (Discard, Reason::InvalidResetBoundary);
    }
    if !delayed_boundaries_consistent(candidate, current_weekly, current.updated_at) {
        return (Discard, Reason::InconsistentResetBoundary);
    }
    if !supported_delayed_boundary(previous_weekly, current_weekly) {
        return (Discard, Reason::UnsupportedResetBoundary);
    }
    let Some(current_inventory) = current_inventory else {
        return (Discard, Reason::MissingCreditInventory);
    };
    if current_inventory != &candidate.inventory {
        return (Discard, Reason::ChangedCreditInventory);
    }
    if age >= CANDIDATE_MINIMUM_AGE_SECONDS {
        (Publish, Reason::ConfirmedObservation)
    } else {
        (Retain, Reason::MinimumDelay)
    }
}

pub(super) fn commit_publication(
    state: &mut AccountState,
    snapshot: &UsageSnapshot,
    inventory: Option<CreditInventory>,
) {
    if let Some(weekly) = weekly(snapshot) {
        state.published_weekly = Some(weekly.clone());
        state.published_at = snapshot.updated_at;
        if normalized_plan(snapshot.login_method.as_deref()).is_some() {
            state.plan = snapshot.login_method.clone();
        }
        state.credit_inventory = inventory;
        state.candidate = None;
    }
}

pub(super) fn preserve_weekly(state: &AccountState, mut current: UsageSnapshot) -> UsageSnapshot {
    if let Some(previous) = state.published_weekly.clone() {
        current.secondary = Some(previous);
    }
    current
}

fn weekly(snapshot: &UsageSnapshot) -> Option<&RateWindow> {
    snapshot
        .secondary
        .as_ref()
        .filter(|window| window.usage_known())
}

fn is_valid_boundary(window: &RateWindow, captured_at: DateTime<Utc>) -> bool {
    window
        .resets_at
        .is_some_and(|boundary| boundary > captured_at)
}

fn boundary_distance_seconds(left: &RateWindow, right: &RateWindow) -> i64 {
    match (left.resets_at, right.resets_at) {
        (Some(left), Some(right)) => right.signed_duration_since(left).num_seconds(),
        _ => i64::MIN / 2,
    }
}

fn boundary_moves_backward(previous: &RateWindow, current: &RateWindow) -> bool {
    boundary_distance_seconds(previous, current) < -RESET_TOLERANCE_SECONDS
}

/// Delayed confirmation accepts equivalent boundaries, or an unused rolling
/// weekly window whose reset date advances with each zero-use observation.
fn delayed_boundaries_consistent(
    candidate: &DelayedCandidate,
    current_weekly: &RateWindow,
    current_updated_at: DateTime<Utc>,
) -> bool {
    if boundary_distance_seconds(&candidate.weekly, current_weekly).abs() < RESET_TOLERANCE_SECONDS
    {
        return true;
    }
    let (Some(candidate_boundary), Some(current_boundary)) =
        (candidate.weekly.resets_at, current_weekly.resets_at)
    else {
        return false;
    };
    is_unused_rolling_weekly(&candidate.weekly, candidate.snapshot_updated_at)
        && is_unused_rolling_weekly(current_weekly, current_updated_at)
        && current_boundary >= candidate_boundary
}

/// Zero usage, a seven-day window, and a reset within two minutes of one full
/// week after capture. Compared at full timestamp precision, like upstream's
/// floating-point interval check, so sub-second capture skew cannot flip the
/// two-minute edge.
fn is_unused_rolling_weekly(window: &RateWindow, captured_at: DateTime<Utc>) -> bool {
    window.used_percent == 0.0
        && window.window_minutes == Some(WEEKLY_WINDOW_MINUTES)
        && window.resets_at.is_some_and(|boundary| {
            let offset = boundary.signed_duration_since(captured_at)
                - chrono::TimeDelta::seconds(WEEKLY_WINDOW_SECONDS);
            offset.abs() < chrono::TimeDelta::seconds(RESET_TOLERANCE_SECONDS)
        })
}

fn supported_delayed_boundary(previous: &RateWindow, current: &RateWindow) -> bool {
    let distance = boundary_distance_seconds(previous, current);
    distance.abs() < STABLE_BOUNDARY_TOLERANCE_SECONDS || distance >= RESET_TOLERANCE_SECONDS
}

fn normalized_plan(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase)
}

/// True only when the stored and fresh plans are both known and differ;
/// an unknown plan never resets the baseline.
fn plan_changed(state: &AccountState, current: &UsageSnapshot) -> bool {
    if current.updated_at <= state.published_at {
        return false;
    }
    match (
        normalized_plan(state.plan.as_deref()),
        normalized_plan(current.login_method.as_deref()),
    ) {
        (Some(previous), Some(current)) => previous != current,
        _ => false,
    }
}

fn plans_match(previous: Option<&str>, left: &UsageSnapshot, right: &UsageSnapshot) -> bool {
    let previous = normalized_plan(previous);
    let left = normalized_plan(left.login_method.as_deref());
    let right = normalized_plan(right.login_method.as_deref());
    previous.is_some() && previous == left && left == right
}

fn reset_credit_evidence(
    previous: Option<&CreditInventory>,
    initial: Option<&CreditInventory>,
    confirmation: Option<&CreditInventory>,
    observed_at: DateTime<Utc>,
) -> ResetCreditEvidence {
    let (Some(previous), Some(initial), Some(confirmation)) = (previous, initial, confirmation)
    else {
        return ResetCreditEvidence::None;
    };
    if previous.available_count == 0 {
        return ResetCreditEvidence::NoAvailableCredits;
    }
    for prior in &previous.credits {
        let consumed_in = |current: &CreditInventory| {
            if let Some(credit) = current.credits.iter().find(|credit| credit.id == prior.id) {
                return credit.status.eq_ignore_ascii_case("redeeming")
                    || credit.status.eq_ignore_ascii_case("redeemed");
            }
            let still_valid = prior.expires_at.is_none_or(|expiry| expiry > observed_at);
            still_valid && current.available_count < previous.available_count
        };
        if consumed_in(initial) && consumed_in(confirmation) {
            return ResetCreditEvidence::Consumed;
        }
    }
    ResetCreditEvidence::None
}

#[cfg(test)]
mod tests;
