#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResetDiagnosticReason {
    CandidateCreated,
    SourceNotExactOAuth,
    MissingPreviousSnapshot,
    MissingWeeklyWindow,
    ResetThresholdMismatch,
    InvalidResetBoundary,
    InconsistentResetBoundary,
    UnsupportedResetBoundary,
    PlanMismatch,
    PlanChanged,
    MissingCreditInventory,
    ChangedCreditInventory,
    NonMonotonicCreditObservationTime,
    InconsistentAvailableCreditCount,
    EvidenceVersionMismatch,
    FutureCandidate,
    ExpiredCandidate,
    StaleObservation,
    MinimumDelay,
    ConfirmedObservation,
    StoreUnavailable,
    StoreRequested,
}

impl ResetDiagnosticReason {
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::CandidateCreated => "candidateCreated",
            Self::SourceNotExactOAuth => "sourceNotExactOAuth",
            Self::MissingPreviousSnapshot => "missingPreviousSnapshot",
            Self::MissingWeeklyWindow => "missingWeeklyWindow",
            Self::ResetThresholdMismatch => "resetThresholdMismatch",
            Self::InvalidResetBoundary => "invalidResetBoundary",
            Self::InconsistentResetBoundary => "inconsistentResetBoundary",
            Self::UnsupportedResetBoundary => "unsupportedResetBoundary",
            Self::PlanMismatch => "planMismatch",
            Self::PlanChanged => "planChanged",
            Self::MissingCreditInventory => "missingCreditInventory",
            Self::ChangedCreditInventory => "changedCreditInventory",
            Self::NonMonotonicCreditObservationTime => "nonMonotonicCreditObservationTime",
            Self::InconsistentAvailableCreditCount => "inconsistentAvailableCreditCount",
            Self::EvidenceVersionMismatch => "evidenceVersionMismatch",
            Self::FutureCandidate => "futureCandidate",
            Self::ExpiredCandidate => "expiredCandidate",
            Self::StaleObservation => "staleObservation",
            Self::MinimumDelay => "minimumDelay",
            Self::ConfirmedObservation => "confirmedObservation",
            Self::StoreUnavailable => "storeUnavailable",
            Self::StoreRequested => "storeRequested",
        }
    }
}

pub(super) fn log_reset_diagnostic(
    stage: &'static str,
    decision: &'static str,
    reason: ResetDiagnosticReason,
) {
    tracing::debug!(
        target: "codex_weekly_reset",
        stage,
        decision,
        reason = reason.code(),
        "Codex weekly-reset decision"
    );
}
