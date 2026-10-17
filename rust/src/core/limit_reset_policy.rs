//! Provider rules for the limit reset detector (upstream CodexBar 0.74.1,
//! `UsageStore+LimitResetCelebration.swift`).
//!
//! Each provider whose reset evidence differs from the default declares its
//! rule as `LIMIT_RESET_POLICY` in its own module (`providers/<name>/`). The
//! exhaustive dispatch is `provider_factory::limit_reset_policy`, reached
//! through [`super::ProviderId::limit_reset_policy`].

/// How a weekly-lane sample decides that the limit was reset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WeeklyResetRule {
    /// Announce when usage crosses from above the reset threshold to at or
    /// below it.
    Standard,
    /// A weekly lane that already announced a reset waits for `count`
    /// consecutive above-threshold samples before it re-arms, so a usage
    /// bounce after a reset is not announced again.
    RecoveryObservations { count: u32 },
    /// The reset boundary must advance; usage up to `reset_threshold_percent`
    /// then counts as a reset. A newly advanced boundary is strong reset
    /// evidence, so modest usage is allowed when the first post-rollover sample
    /// arrives late.
    AdvancedBoundary {
        reset_threshold_percent: f64,
        requires_previous_boundary: bool,
    },
}

/// What the detector does with a sample that carries no account identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingIdentityRule {
    /// Track the provider as one shared account. Identity-less samples can be
    /// transient, so `requires_low_confirmation` asks for a second low sample
    /// before a reset is announced.
    Track { requires_low_confirmation: bool },
    /// Skip detection: the samples cannot be attributed to an owner.
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProviderLimitResetPolicy {
    /// Whether the provider's usage history can back a reset detector
    /// (upstream excludes some providers from plan-utilization history).
    pub supported: bool,
    pub weekly: WeeklyResetRule,
    pub missing_identity: MissingIdentityRule,
}

impl ProviderLimitResetPolicy {
    pub const DEFAULT: Self = Self {
        supported: true,
        weekly: WeeklyResetRule::Standard,
        missing_identity: MissingIdentityRule::Track {
            requires_low_confirmation: false,
        },
    };

    pub const UNSUPPORTED: Self = Self {
        supported: false,
        ..Self::DEFAULT
    };
}
