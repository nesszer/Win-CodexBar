/// Each reason maps to one fixed, redacted log code.
macro_rules! reset_diagnostic_reasons {
    ($($variant:ident => $code:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(super) enum ResetDiagnosticReason {
            $($variant,)*
        }

        impl ResetDiagnosticReason {
            pub(super) const fn code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)*
                }
            }
        }
    };
}

reset_diagnostic_reasons! {
    CandidateCreated => "candidateCreated",
    SourceNotExactOAuth => "sourceNotExactOAuth",
    MissingPreviousSnapshot => "missingPreviousSnapshot",
    MissingWeeklyWindow => "missingWeeklyWindow",
    ResetThresholdMismatch => "resetThresholdMismatch",
    InvalidResetBoundary => "invalidResetBoundary",
    InconsistentResetBoundary => "inconsistentResetBoundary",
    UnsupportedResetBoundary => "unsupportedResetBoundary",
    PlanMismatch => "planMismatch",
    PlanChanged => "planChanged",
    MissingCreditInventory => "missingCreditInventory",
    ChangedCreditInventory => "changedCreditInventory",
    EvidenceVersionMismatch => "evidenceVersionMismatch",
    FutureCandidate => "futureCandidate",
    ExpiredCandidate => "expiredCandidate",
    StaleObservation => "staleObservation",
    MinimumDelay => "minimumDelay",
    ConfirmedObservation => "confirmedObservation",
    StoreUnavailable => "storeUnavailable",
    StoreRequested => "storeRequested",
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
