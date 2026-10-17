//! Spend-only Claude accounts (upstream #4480, fixes #4381).
//!
//! An account whose only limit is a monthly spend cap (for example an
//! Enterprise seat) has no session or weekly lane. A positive cap becomes the
//! primary meter: the used share of the cap, the calendar month it resets in
//! (midnight UTC on the 1st, inferred because Claude reports no cap reset),
//! and calendar-month pace. Uncapped balances and quota accounts are unchanged.

use chrono::{DateTime, Datelike, NaiveDate, Utc};

use crate::core::{CostSnapshot, ProviderPaceCapability, RateWindow, UsageSnapshot};

/// Upstream `ProviderPaceCapability.calendarMonthResetWindow`: the projected
/// meter is scored over the real month that ends at its reset.
pub(crate) const PACE_CAPABILITY: ProviderPaceCapability =
    ProviderPaceCapability::CALENDAR_MONTH_RESET_WINDOW;

/// Primary label for the projected meter; the shell localizes "Monthly".
pub(crate) const METER_LABEL: &str = "Monthly";

/// Midnight UTC on the first day of the month after `at`.
pub(crate) fn next_utc_month_start(at: DateTime<Utc>) -> DateTime<Utc> {
    let (year, month) = if at.month() == 12 {
        (at.year() + 1, 1)
    } else {
        (at.year(), at.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map_or(at, |start| start.and_utc())
}

/// The meter for a positive monthly cap, or `None` without a usable cap.
///
/// The reset is the observed cap reset when Claude reports one; otherwise it
/// is inferred as midnight UTC on the first after the observation.
pub(crate) fn monthly_cap_meter(cost: &CostSnapshot) -> Option<RateWindow> {
    let limit = cost
        .limit
        .filter(|limit| limit.is_finite() && *limit > 0.0)?;
    if !cost.used.is_finite() {
        return None;
    }
    let used_percent = (cost.used.max(0.0) / limit * 100.0).min(100.0);
    let resets_at = cost
        .resets_at
        .unwrap_or_else(|| next_utc_month_start(cost.updated_at));
    Some(RateWindow::with_details(
        used_percent,
        RateWindow::calendar_month_window_minutes(resets_at),
        Some(resets_at),
        None,
    ))
}

/// Project a spend-only account's capped spend into the primary meter.
///
/// Returns `true` when the projection applied. Quota accounts (a session,
/// weekly, model, or scoped-weekly lane present) and uncapped balances return
/// `false` and are left untouched.
pub(crate) fn project_spend_only(usage: &mut UsageSnapshot, cost: &mut CostSnapshot) -> bool {
    let has_quota_lane = !usage.primary.is_informational
        || usage.secondary.is_some()
        || usage.model_specific.is_some()
        || usage.tertiary.is_some()
        || !usage.extra_rate_windows.is_empty();
    if has_quota_lane {
        return false;
    }
    let Some(meter) = monthly_cap_meter(cost) else {
        return false;
    };
    usage.primary = meter;
    usage.primary_label = Some(METER_LABEL.to_string());
    cost.folded_into_primary = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::NamedRateWindow;
    use chrono::{Datelike, TimeZone};

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    fn spend_only_usage() -> UsageSnapshot {
        UsageSnapshot::new(RateWindow::no_active_session())
    }

    fn capped_cost(used: f64, limit: f64, observed: DateTime<Utc>) -> CostSnapshot {
        let mut cost = CostSnapshot::new(used, "USD", "Monthly").with_limit(limit);
        cost.updated_at = observed;
        cost
    }

    #[test]
    fn capped_spend_resets_at_the_next_utc_first_across_december() {
        let mut usage = spend_only_usage();
        let mut cost = capped_cost(250.0, 1000.0, at(2026, 12, 15, 10));

        assert!(project_spend_only(&mut usage, &mut cost));

        assert!(!usage.primary.is_informational);
        assert_eq!(usage.primary.resets_at, Some(at(2027, 1, 1, 0)));
        assert_eq!(usage.primary.window_minutes, Some(31 * 24 * 60));
        assert!((usage.primary.used_percent - 25.0).abs() < f64::EPSILON);
        assert_eq!(usage.primary_label.as_deref(), Some("Monthly"));
        assert!(cost.folded_into_primary);
        assert_eq!(cost.limit, Some(1000.0));
    }

    #[test]
    fn leap_february_rollover_scores_a_29_day_cycle() {
        let mut usage = spend_only_usage();
        let mut cost = capped_cost(500.0, 1000.0, at(2028, 2, 14, 9));

        assert!(project_spend_only(&mut usage, &mut cost));

        assert_eq!(usage.primary.resets_at, Some(at(2028, 3, 1, 0)));
        assert_eq!(usage.primary.window_minutes, Some(29 * 24 * 60));
    }

    #[test]
    fn common_february_rollover_scores_a_28_day_cycle() {
        let mut usage = spend_only_usage();
        let mut cost = capped_cost(500.0, 1000.0, at(2026, 2, 10, 9));

        assert!(project_spend_only(&mut usage, &mut cost));

        assert_eq!(usage.primary.resets_at, Some(at(2026, 3, 1, 0)));
        assert_eq!(usage.primary.window_minutes, Some(28 * 24 * 60));
    }

    #[test]
    fn next_utc_month_start_crosses_the_year_boundary() {
        assert_eq!(
            next_utc_month_start(at(2026, 12, 31, 23)),
            at(2027, 1, 1, 0)
        );
        assert_eq!(next_utc_month_start(at(2026, 1, 1, 0)), at(2026, 2, 1, 0));
    }

    #[test]
    fn an_observed_reset_is_kept_over_the_inferred_one() {
        let mut usage = spend_only_usage();
        let mut cost = capped_cost(100.0, 400.0, at(2026, 5, 20, 8));
        cost.resets_at = Some(at(2026, 6, 1, 0));

        assert!(project_spend_only(&mut usage, &mut cost));

        assert_eq!(usage.primary.resets_at, Some(at(2026, 6, 1, 0)));
    }

    #[test]
    fn projected_meter_paces_over_the_real_month_it_resets_in() {
        let now = at(2026, 2, 10, 12);
        let mut usage = spend_only_usage();
        let mut cost = capped_cost(100.0, 1000.0, now);
        assert!(project_spend_only(&mut usage, &mut cost));

        let resolved = PACE_CAPABILITY.resolved_reset_window_for_pace(&usage.primary);
        assert_eq!(resolved.window_minutes, Some(28 * 24 * 60));
        assert!(
            PACE_CAPABILITY
                .reset_window_pace(&usage.primary, now, None)
                .is_some()
        );
        assert_eq!(usage.primary.resets_at.map(|dt| dt.day()), Some(1));
    }

    #[test]
    fn quota_account_with_a_session_keeps_its_meter_and_cost() {
        let mut usage = UsageSnapshot::new(RateWindow::new(10.0));
        let mut cost = capped_cost(250.0, 1000.0, at(2026, 12, 15, 10));

        assert!(!project_spend_only(&mut usage, &mut cost));

        assert!((usage.primary.used_percent - 10.0).abs() < f64::EPSILON);
        assert!(usage.primary_label.is_none());
        assert!(!cost.folded_into_primary);
        assert_eq!(cost.limit, Some(1000.0));
    }

    #[test]
    fn weekly_or_model_lane_blocks_the_projection() {
        let mut weekly = spend_only_usage().with_secondary(RateWindow::new(40.0));
        let mut cost = capped_cost(250.0, 1000.0, at(2026, 12, 15, 10));
        assert!(!project_spend_only(&mut weekly, &mut cost));
        assert!(weekly.primary.is_informational);

        let mut model = spend_only_usage().with_model_specific(RateWindow::new(5.0));
        let mut cost = capped_cost(250.0, 1000.0, at(2026, 12, 15, 10));
        assert!(!project_spend_only(&mut model, &mut cost));
        assert!(model.primary.is_informational);
    }

    #[test]
    fn scoped_weekly_lane_blocks_the_projection() {
        let mut usage = spend_only_usage();
        usage.extra_rate_windows.push(NamedRateWindow::new(
            "claude-weekly-scoped-fable",
            "Fable only",
            RateWindow::new(7.0),
        ));
        let mut cost = capped_cost(250.0, 1000.0, at(2026, 12, 15, 10));

        assert!(!project_spend_only(&mut usage, &mut cost));
        assert!(usage.primary.is_informational);
    }

    #[test]
    fn uncapped_or_zero_cap_balance_is_not_projected() {
        let mut usage = spend_only_usage();
        let mut uncapped = CostSnapshot::new(12.0, "USD", "Monthly");
        uncapped.updated_at = at(2026, 12, 15, 10);
        assert!(!project_spend_only(&mut usage, &mut uncapped));
        assert!(usage.primary.is_informational);
        assert!(!uncapped.folded_into_primary);

        let mut zero = capped_cost(0.0, 0.0, at(2026, 12, 15, 10));
        assert!(!project_spend_only(&mut usage, &mut zero));
        assert!(usage.primary.is_informational);
    }
}
