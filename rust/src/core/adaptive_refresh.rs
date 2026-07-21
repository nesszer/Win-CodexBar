//! Adaptive refresh decision table (upstream AdaptiveRefreshPolicyCore).
//!
//! Pure policy — platform adapters supply thermal / low-power / activity inputs.

use std::time::Duration;

/// Thermal pressure signal after platform normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalPressure {
    Nominal,
    Constrained,
}

/// Why a given delay was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptiveRefreshReason {
    RecentInteraction,
    CodingActivity,
    Warm,
    Idle,
    LongIdle,
    Constrained,
}

/// Inputs for one adaptive-refresh decision.
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveRefreshInput {
    /// Age of the last menu/panel open, if known.
    pub last_menu_open_age: Option<Duration>,
    /// Age of the last coding-activity signal, if known.
    pub last_coding_activity_age: Option<Duration>,
    pub low_power_mode_enabled: bool,
    pub thermal_pressure: ThermalPressure,
}

/// Result of the decision table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptiveRefreshDecision {
    pub delay: Duration,
    pub reason: AdaptiveRefreshReason,
}

const RECENT_INTERACTION_THRESHOLD: Duration = Duration::from_secs(5 * 60);
const WARM_THRESHOLD: Duration = Duration::from_secs(60 * 60);
const IDLE_THRESHOLD: Duration = Duration::from_secs(4 * 60 * 60);
const CODING_ACTIVITY_THRESHOLD: Duration = Duration::from_secs(5 * 60);

const RECENT_INTERACTION_DELAY: Duration = Duration::from_secs(2 * 60);
const WARM_DELAY: Duration = Duration::from_secs(5 * 60);
const IDLE_DELAY: Duration = Duration::from_secs(15 * 60);
const LONG_IDLE_DELAY: Duration = Duration::from_secs(30 * 60);
const CONSTRAINED_DELAY: Duration = Duration::from_secs(30 * 60);
const CODING_ACTIVITY_DELAY_CAP: Duration = Duration::from_secs(5 * 60);

/// Representative cadence for consumers that need one interval but cannot access live state.
pub const NOMINAL_INTERVAL_FOR_HEURISTICS: Duration = Duration::from_secs(5 * 60);

/// Compute the next adaptive refresh delay from activity / power inputs.
pub fn next_delay(input: AdaptiveRefreshInput) -> AdaptiveRefreshDecision {
    if input.low_power_mode_enabled || input.thermal_pressure == ThermalPressure::Constrained {
        return AdaptiveRefreshDecision {
            delay: CONSTRAINED_DELAY,
            reason: AdaptiveRefreshReason::Constrained,
        };
    }

    let base = menu_activity_decision(input.last_menu_open_age);
    if let Some(age) = input.last_coding_activity_age
        && age < CODING_ACTIVITY_THRESHOLD
        && base.delay > CODING_ACTIVITY_DELAY_CAP
    {
        return AdaptiveRefreshDecision {
            delay: CODING_ACTIVITY_DELAY_CAP,
            reason: AdaptiveRefreshReason::CodingActivity,
        };
    }
    base
}

fn menu_activity_decision(last_menu_open_age: Option<Duration>) -> AdaptiveRefreshDecision {
    let Some(age) = last_menu_open_age else {
        return AdaptiveRefreshDecision {
            delay: LONG_IDLE_DELAY,
            reason: AdaptiveRefreshReason::LongIdle,
        };
    };

    if age <= RECENT_INTERACTION_THRESHOLD {
        return AdaptiveRefreshDecision {
            delay: RECENT_INTERACTION_DELAY,
            reason: AdaptiveRefreshReason::RecentInteraction,
        };
    }
    if age <= WARM_THRESHOLD {
        return AdaptiveRefreshDecision {
            delay: WARM_DELAY,
            reason: AdaptiveRefreshReason::Warm,
        };
    }
    if age < IDLE_THRESHOLD {
        return AdaptiveRefreshDecision {
            delay: IDLE_DELAY,
            reason: AdaptiveRefreshReason::Idle,
        };
    }
    AdaptiveRefreshDecision {
        delay: LONG_IDLE_DELAY,
        reason: AdaptiveRefreshReason::LongIdle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        menu_age: Option<Duration>,
        coding_age: Option<Duration>,
        low_power: bool,
        thermal: ThermalPressure,
    ) -> AdaptiveRefreshInput {
        AdaptiveRefreshInput {
            last_menu_open_age: menu_age,
            last_coding_activity_age: coding_age,
            low_power_mode_enabled: low_power,
            thermal_pressure: thermal,
        }
    }

    #[test]
    fn constrained_or_low_power_is_30m() {
        let d = next_delay(input(
            Some(Duration::from_secs(10)),
            None,
            true,
            ThermalPressure::Nominal,
        ));
        assert_eq!(d.delay, Duration::from_secs(30 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::Constrained);

        let d = next_delay(input(
            Some(Duration::from_secs(10)),
            None,
            false,
            ThermalPressure::Constrained,
        ));
        assert_eq!(d.delay, Duration::from_secs(30 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::Constrained);
    }

    #[test]
    fn recent_menu_open_is_2m() {
        let d = next_delay(input(
            Some(Duration::from_secs(4 * 60)),
            None,
            false,
            ThermalPressure::Nominal,
        ));
        assert_eq!(d.delay, Duration::from_secs(2 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::RecentInteraction);
    }

    #[test]
    fn warm_is_5m() {
        let d = next_delay(input(
            Some(Duration::from_secs(30 * 60)),
            None,
            false,
            ThermalPressure::Nominal,
        ));
        assert_eq!(d.delay, Duration::from_secs(5 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::Warm);
    }

    #[test]
    fn idle_under_4h_is_15m() {
        let d = next_delay(input(
            Some(Duration::from_secs(2 * 60 * 60)),
            None,
            false,
            ThermalPressure::Nominal,
        ));
        assert_eq!(d.delay, Duration::from_secs(15 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::Idle);
    }

    #[test]
    fn long_idle_is_30m() {
        let d = next_delay(input(
            Some(Duration::from_secs(5 * 60 * 60)),
            None,
            false,
            ThermalPressure::Nominal,
        ));
        assert_eq!(d.delay, Duration::from_secs(30 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::LongIdle);

        let d = next_delay(input(None, None, false, ThermalPressure::Nominal));
        assert_eq!(d.delay, Duration::from_secs(30 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::LongIdle);
    }

    #[test]
    fn coding_activity_caps_long_idle_at_5m() {
        let d = next_delay(input(
            Some(Duration::from_secs(5 * 60 * 60)),
            Some(Duration::from_secs(60)),
            false,
            ThermalPressure::Nominal,
        ));
        assert_eq!(d.delay, Duration::from_secs(5 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::CodingActivity);
    }

    #[test]
    fn coding_activity_does_not_raise_recent_delay() {
        let d = next_delay(input(
            Some(Duration::from_secs(30)),
            Some(Duration::from_secs(10)),
            false,
            ThermalPressure::Nominal,
        ));
        assert_eq!(d.delay, Duration::from_secs(2 * 60));
        assert_eq!(d.reason, AdaptiveRefreshReason::RecentInteraction);
    }
}
