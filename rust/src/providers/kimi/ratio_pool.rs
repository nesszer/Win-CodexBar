//! Kimi Code API ratio-pool reconciliation (upstream #4311 fixing #4306,
//! building on 0.63.0 `fd2414d`, #3755).
//!
//! The usages endpoint can return a self-contradictory body: a legacy counter
//! and a ratio pool may describe the same quota window with different
//! readings (the #4306 response reported a 5-hour counter at 100/100 next to a
//! zero 5-hour ratio). When a reliable legacy counter has the same duration as
//! the ratio window and its reset is within two seconds of the ratio reset,
//! the more-exhausted reading wins. Equal readings keep the ratio metadata.
//! Mismatched durations or reset periods never borrow counts. A monthly pool
//! or unreliable weekly counters do not block the comparison.

use chrono::TimeDelta;

use super::{KimiRatioPool, KimiUsageDetail, RateWindow, format_usage_amount};

/// Upstream observed the legacy and ratio reset clocks about 1.45 s apart.
const MATCHING_RESET_TOLERANCE_SECS: i64 = 2;

/// Integer usage counters (upstream `KimiUsageSnapshot.usageCounts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UsageCounts {
    used: i64,
    limit: i64,
    reliable: bool,
}

/// `used` is authoritative and may exceed the limit during overage;
/// `remaining` only counts when it describes a valid balance. A valid limit
/// without usable counters is kept but marked unreliable.
fn usage_counts(detail: &KimiUsageDetail) -> Option<UsageCounts> {
    let limit = integer_counter(detail.limit.as_ref()).filter(|limit| *limit > 0)?;
    if let Some(used) = integer_counter(detail.used.as_ref()).filter(|used| *used >= 0) {
        return Some(UsageCounts {
            used,
            limit,
            reliable: true,
        });
    }
    if let Some(remaining) = integer_counter(detail.remaining.as_ref())
        .filter(|remaining| (0..=limit).contains(remaining))
    {
        return Some(UsageCounts {
            used: limit - remaining,
            limit,
            reliable: true,
        });
    }
    Some(UsageCounts {
        used: 0,
        limit,
        reliable: false,
    })
}

/// Upstream keeps counters as strings and reads them with `Int(_)`: integer
/// strings and integral JSON numbers count; fractions, padded or formatted
/// strings do not.
fn integer_counter(value: Option<&serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::Number(number) => number.as_i64().or_else(|| {
            let value = number.as_f64()?;
            let integral = value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < i64::MAX as f64;
            #[allow(
                clippy::cast_possible_truncation,
                reason = "finite integral value is bounded to the i64 range above"
            )]
            integral.then_some(value as i64)
        }),
        serde_json::Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

/// Resolve a ratio pool. When a reliable legacy counter describes the same
/// quota window, the more-exhausted reading wins (see
/// [`matching_count_window`]). `count_window_minutes` is the duration the
/// legacy counters report.
pub(super) fn resolved_ratio_window(
    pool: &KimiRatioPool,
    detail: Option<&KimiUsageDetail>,
    window_minutes: u32,
    count_window_minutes: Option<u32>,
) -> Option<RateWindow> {
    let ratio_window = pool.rate_window(window_minutes)?;
    let count_window =
        matching_count_window(&ratio_window, detail, window_minutes, count_window_minutes);
    Some(count_window.unwrap_or(ratio_window))
}

/// Upstream `resolvedRatioWindow`: the counters replace the ratio only when
/// they report the same duration, are reliable, reset within
/// [`MATCHING_RESET_TOLERANCE_SECS`] of the ratio reset, and show a strictly
/// higher percent. Equal readings keep the ratio window and its metadata.
fn matching_count_window(
    ratio_window: &RateWindow,
    detail: Option<&KimiUsageDetail>,
    window_minutes: u32,
    count_window_minutes: Option<u32>,
) -> Option<RateWindow> {
    if count_window_minutes != Some(window_minutes) {
        return None;
    }

    let detail = detail?;
    let counts = usage_counts(detail).filter(|counts| counts.reliable)?;
    let count_reset = detail
        .reset_time
        .as_ref()
        .and_then(super::parse_kimi_timestamp)?;
    let ratio_reset = ratio_window.resets_at?;
    if (count_reset - ratio_reset).abs() > TimeDelta::seconds(MATCHING_RESET_TOLERANCE_SECS) {
        return None;
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "quota counters are far below 2^52; the percent is display-only"
    )]
    let (used, limit) = (counts.used as f64, counts.limit as f64);
    let counts_percent = (used / limit * 100.0).clamp(0.0, 100.0);
    if counts_percent <= ratio_window.used_percent {
        return None;
    }
    Some(RateWindow::with_details(
        counts_percent,
        Some(window_minutes),
        Some(count_reset),
        Some(format!(
            "{}/{} credits",
            format_usage_amount(used),
            format_usage_amount(limit)
        )),
    ))
}

#[cfg(test)]
mod tests {
    //! Mirrors upstream `KimiRatioPoolTests` (0.60.5 #3694 and 0.63.0
    //! `fd2414d`). Upstream reports an absent lane as `nil`; Win-CodexBar keeps
    //! an informational weekly placeholder instead (`assert_weekly_absent`).
    //! Count descriptions read "credits" where upstream says "requests" or
    //! "Rate: N/M per 5 hours".

    use super::super::code_api::{MISSING_WEEKLY_DESCRIPTION, snapshot_from_code_api_response};
    use super::super::{
        KimiCodeApiUsageResponse, KimiSubscriptionStatsResponse, MONTHLY_WINDOW_ID, ProviderError,
        UsageSnapshot, apply_subscription_windows,
    };
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn try_parse(value: Value) -> Result<UsageSnapshot, ProviderError> {
        let response: KimiCodeApiUsageResponse =
            serde_json::from_value(value).expect("fixture parses");
        snapshot_from_code_api_response(response)
    }

    fn parse(value: Value) -> UsageSnapshot {
        try_parse(value).expect("fixture has a supported quota window")
    }

    fn at(text: &str) -> Option<DateTime<Utc>> {
        Some(
            DateTime::parse_from_rfc3339(text)
                .expect("valid fixture timestamp")
                .with_timezone(&Utc),
        )
    }

    fn close(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 0.000_01
    }

    /// Upstream `usage.primary == nil`.
    fn assert_weekly_absent(snapshot: &UsageSnapshot) {
        let weekly = &snapshot.primary;
        assert!(weekly.is_informational, "absent weekly stays informational");
        assert!(!weekly.usage_known);
        assert_eq!(weekly.window_minutes, None);
        assert_eq!(
            weekly.reset_description.as_deref(),
            Some(MISSING_WEEKLY_DESCRIPTION)
        );
    }

    fn rate_limit_percent(snapshot: &UsageSnapshot) -> f64 {
        snapshot
            .secondary
            .as_ref()
            .expect("rate-limit lane is present")
            .used_percent
    }

    fn weekly_fixture(usage: Value, weekly_pool: Value) -> Value {
        json!({ "usage": usage, "usages": { "limit_7d": weekly_pool } })
    }

    fn mixed_international_response() -> Value {
        json!({
            "usage": {
                "limit": "100",
                "used": "19",
                "remaining": "81",
                "resetTime": "2026-09-19T16:45:59.449979Z"
            },
            "limits": [{
                "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                "detail": {
                    "limit": "100",
                    "used": "1",
                    "remaining": "99",
                    "resetTime": "2026-09-19T14:45:59.449979Z"
                }
            }],
            "usages": {
                "limit_5h": { "used_ratio": 0, "reset_time": "2026-09-19T14:45:58Z" },
                "limit_7d": { "used_ratio": 0, "reset_time": "2026-09-19T16:45:58Z" }
            }
        })
    }

    fn subscription_stats(value: Value) -> KimiSubscriptionStatsResponse {
        serde_json::from_value(value).expect("subscription fixture parses")
    }

    // Upstream: `reported ratio pools retain missing weekly quota and monthly
    // identity`.
    #[test]
    fn reported_ratio_pools_retain_missing_weekly_quota_and_monthly_identity() {
        let snapshot = parse(json!({
            "limits": [{
                "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                "detail": { "limit": "100", "used": "25", "remaining": "75" }
            }],
            "usages": {
                "limit_5h": { "used_ratio": 0, "reset_time": "2026-09-16T20:15:44Z" },
                "limit_month_total": {
                    "used_ratio": 0.0056,
                    "reset_time": "2026-10-17T00:00:00Z"
                },
                "limit_month_code": { "used_ratio": 0, "reset_time": "2026-10-17T00:00:00Z" }
            }
        }));
        assert_weekly_absent(&snapshot);
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.used_percent, 0.0);
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert_eq!(rate_limit.resets_at, at("2026-09-16T20:15:44Z"));
        assert_eq!(rate_limit.reset_description, None);
        assert!(snapshot.tertiary.is_none());
        let [monthly] = snapshot.extra_rate_windows.as_slice() else {
            panic!("only the monthly lane is an extra");
        };
        assert_eq!(monthly.id, MONTHLY_WINDOW_ID);
        assert_eq!(monthly.title, "Total usage");
        assert!(close(monthly.window.used_percent, 0.56));
        assert_eq!(monthly.window.window_minutes, Some(43_200));
        assert_eq!(monthly.window.resets_at, at("2026-10-17T00:00:00Z"));
        // Upstream leaves the login method empty without a plan; the fetch
        // replaces this parse-time label with the source label.
        assert_eq!(snapshot.login_method.as_deref(), Some("Code API"));
    }

    // Upstream: `ratio weekly and session retain established lane ordering`.
    #[test]
    fn ratio_weekly_and_session_retain_established_lane_ordering() {
        let snapshot = parse(json!({
            "usages": {
                "limit_7d": { "used_ratio": 0.125, "reset_time": "2026-09-20T00:00:00Z" },
                "limit_5h": { "used_ratio": 0.625 }
            }
        }));
        assert_eq!(snapshot.primary.used_percent, 12.5);
        assert_eq!(snapshot.primary.window_minutes, Some(10_080));
        assert_eq!(snapshot.primary.resets_at, at("2026-09-20T00:00:00Z"));
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.used_percent, 62.5);
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert!(snapshot.extra_rate_windows.is_empty());
    }

    // Audit gap: a response with only the weekly pool shows that lane alone.
    #[test]
    fn weekly_only_pool_shows_only_the_weekly_lane() {
        let snapshot = parse(json!({ "usages": { "limit_7d": { "used_ratio": 0.25 } } }));
        assert!(!snapshot.primary.is_informational);
        assert_eq!(snapshot.primary.used_percent, 25.0);
        assert_eq!(snapshot.primary.window_minutes, Some(10_080));
        assert!(snapshot.secondary.is_none());
        assert!(snapshot.extra_rate_windows.is_empty());
    }

    // Upstream: `count rate window remains usable without legacy weekly
    // usage` (upstream text "Rate: 25/100 per 5 hours").
    #[test]
    fn count_rate_window_remains_usable_without_legacy_weekly_usage() {
        let snapshot = parse(json!({
            "limits": [{
                "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                "detail": { "limit": "100", "used": "25", "remaining": "75" }
            }]
        }));
        assert_weekly_absent(&snapshot);
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.used_percent, 25.0);
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert_eq!(
            rate_limit.reset_description.as_deref(),
            Some("25/100 credits")
        );
    }

    // Audit gap: a legacy 5-hour count window next to a monthly-only pool
    // keeps both lanes.
    #[test]
    fn count_rate_window_and_monthly_only_pool_keep_both_lanes() {
        let snapshot = parse(json!({
            "limits": [{
                "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                "detail": { "limit": "100", "used": "25" }
            }],
            "usages": { "limit_month_total": { "used_ratio": 0.5 } }
        }));
        assert_weekly_absent(&snapshot);
        assert_eq!(rate_limit_percent(&snapshot), 25.0);
        let [monthly] = snapshot.extra_rate_windows.as_slice() else {
            panic!("monthly lane");
        };
        assert_eq!(monthly.id, MONTHLY_WINDOW_ID);
        assert_eq!(monthly.window.used_percent, 50.0);
    }

    // Upstream: `monthly only response does not invent code windows`.
    #[test]
    fn monthly_only_response_does_not_invent_code_windows() {
        let snapshot = parse(json!({ "usages": { "limit_month_total": { "used_ratio": 1.05 } } }));
        assert_weekly_absent(&snapshot);
        assert!(snapshot.secondary.is_none());
        assert!(snapshot.tertiary.is_none());
        let [monthly] = snapshot.extra_rate_windows.as_slice() else {
            panic!("monthly lane");
        };
        assert_eq!(monthly.id, MONTHLY_WINDOW_ID);
        assert_eq!(monthly.window.used_percent, 100.0);
        assert!(monthly.window.usage_known);
    }

    // Upstream: `invalid ratio cannot suppress usable legacy counts` (upstream
    // text "25/100 requests").
    #[test]
    fn invalid_ratio_cannot_suppress_usable_legacy_counts() {
        let snapshot = parse(json!({
            "usage": { "limit": "100", "used": "25" },
            "usages": { "limit_7d": { "used_ratio": -0.5 } }
        }));
        assert!(!snapshot.primary.is_informational);
        assert_eq!(snapshot.primary.used_percent, 25.0);
        assert_eq!(snapshot.primary.window_minutes, Some(10_080));
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("25/100 credits")
        );
    }

    // Upstream: `web enrichment preserves authoritative API pools` for web
    // status 200 and 503. A 200 merges the subscription stats; any other
    // non-auth status makes `fetch_subscription_for_enrichment_result` return
    // `Ok(None)`, so `fetch_via_code_api` keeps the snapshot as parsed.
    #[test]
    fn web_enrichment_preserves_authoritative_api_pools() {
        let api = parse(json!({
            "usages": {
                "limit_5h": { "used_ratio": 0.1 },
                "limit_month_total": { "used_ratio": 0.42 }
            }
        }));
        let stats =
            subscription_stats(json!({ "subscriptionBalance": { "amountUsedRatio": 0.99 } }));
        for (status, usage) in [
            (200, apply_subscription_windows(api.clone(), &stats)),
            (503, api),
        ] {
            assert_weekly_absent(&usage);
            assert!(
                close(rate_limit_percent(&usage), 10.0),
                "web status {status}"
            );
            let [monthly] = usage.extra_rate_windows.as_slice() else {
                panic!("one monthly lane after web status {status}");
            };
            assert_eq!(monthly.id, MONTHLY_WINDOW_ID);
            assert!(
                close(monthly.window.used_percent, 42.0),
                "web status {status}"
            );
        }
    }

    // Upstream `codeUsagePools?.monthly?.window(...) ?? subscriptionBalance`:
    // without a usable API monthly pool, web enrichment still fills the lane.
    #[test]
    fn web_enrichment_fills_a_missing_or_invalid_api_monthly_pool() {
        let stats = subscription_stats(json!({
            "subscriptionBalance": {
                "amountUsedRatio": 0.99,
                "expireTime": "2026-10-17T00:00:00Z"
            }
        }));
        for pools in [
            json!({ "limit_5h": { "used_ratio": 0.1 } }),
            json!({ "limit_5h": { "used_ratio": 0.1 }, "limit_month_total": { "used_ratio": -1 } }),
        ] {
            let usage = apply_subscription_windows(parse(json!({ "usages": pools })), &stats);
            let [monthly] = usage.extra_rate_windows.as_slice() else {
                panic!("subscription monthly lane");
            };
            assert_eq!(monthly.id, MONTHLY_WINDOW_ID);
            assert!(close(monthly.window.used_percent, 99.0));
            assert_eq!(monthly.window.resets_at, at("2026-10-17T00:00:00Z"));
        }
    }

    // Upstream: `unrecognized or empty quotas do not succeed as unused`.
    #[test]
    fn unrecognized_or_empty_quotas_do_not_succeed_as_unused() {
        for fixture in [
            json!({}),
            json!({ "usages": {} }),
            json!({ "usages": { "limit_5h": {} } }),
        ] {
            assert!(
                matches!(
                    try_parse(fixture.clone()),
                    Err(ProviderError::Parse(message))
                        if message == "No supported quota windows in Code usage response"
                ),
                "fixture {fixture}"
            );
        }
    }

    // Upstream: `zero ratio placeholder does not hide matching nonzero counts`.
    #[test]
    fn zero_ratio_placeholder_does_not_hide_matching_nonzero_counts() {
        for reset in ["2026-09-19T16:45:58Z", "2026-09-19T16:45:59Z"] {
            let snapshot = parse(weekly_fixture(
                json!({
                    "limit": "100",
                    "used": "19",
                    "remaining": "81",
                    "resetTime": "2026-09-19T16:45:59.449979Z"
                }),
                json!({ "used_ratio": 0, "reset_time": reset }),
            ));
            assert_eq!(snapshot.primary.used_percent, 19.0, "ratio reset {reset}");
        }
    }

    // Upstream: `zero ratio after a different reset stays authoritative`.
    #[test]
    fn zero_ratio_after_a_different_reset_stays_authoritative() {
        let snapshot = parse(weekly_fixture(
            json!({
                "limit": "100",
                "used": "19",
                "remaining": "81",
                "resetTime": "2026-09-19T16:45:59Z"
            }),
            json!({ "used_ratio": 0, "reset_time": "2026-09-26T16:45:59Z" }),
        ));
        assert_eq!(snapshot.primary.used_percent, 0.0);
    }

    // Upstream: `mixed international response retains the used weekly and
    // session quotas`.
    #[test]
    fn mixed_international_response_retains_the_used_weekly_and_session_quotas() {
        let snapshot = parse(mixed_international_response());
        assert_eq!(snapshot.primary.used_percent, 19.0);
        assert_eq!(snapshot.primary.window_minutes, Some(10_080));
        assert_eq!(
            snapshot.primary.resets_at,
            at("2026-09-19T16:45:59.449979Z")
        );
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.used_percent, 1.0);
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert_eq!(rate_limit.resets_at, at("2026-09-19T14:45:59.449979Z"));
        assert!(snapshot.tertiary.is_none());
        assert!(snapshot.extra_rate_windows.is_empty());
    }

    // Upstream #4311: `matching nonzero ratios and counts use the more exhausted
    // reading`. Counts at 19% beat an 18.69% ratio; a 50% ratio beats counts at
    // 19%; an equal 19% ratio keeps the ratio window and its metadata.
    #[test]
    fn matching_nonzero_ratios_and_counts_use_the_more_exhausted_reading() {
        for (ratio, expected, description) in [
            (0.1869, 19.0, Some("19/100 credits")),
            (0.19, 19.0, None),
            (0.5, 50.0, None),
        ] {
            let snapshot = parse(weekly_fixture(
                json!({ "limit": "100", "used": "19", "resetTime": "2026-09-19T16:45:59Z" }),
                json!({ "used_ratio": ratio, "reset_time": "2026-09-19T16:45:59Z" }),
            ));
            assert!(
                close(snapshot.primary.used_percent, expected),
                "ratio {ratio}"
            );
            assert_eq!(
                snapshot.primary.reset_description.as_deref(),
                description,
                "ratio {ratio}"
            );
        }
    }

    // Upstream #4311: `monthly ratio accounts also reconcile matching weekly
    // counters`. The monthly pool no longer blocks the weekly comparison.
    #[test]
    fn monthly_ratio_accounts_also_reconcile_matching_weekly_counters() {
        let snapshot = parse(json!({
            "usage": { "limit": "100", "used": "19", "resetTime": "2026-09-19T16:45:59Z" },
            "usages": {
                "limit_7d": { "used_ratio": 0, "reset_time": "2026-09-19T16:45:59Z" },
                "limit_month_total": { "used_ratio": 0.0313 }
            }
        }));
        assert_eq!(snapshot.primary.used_percent, 19.0);
        let monthly = snapshot.extra_rate_windows.first().expect("monthly pool");
        assert!(close(monthly.window.used_percent, 3.13));
    }

    // Upstream: `unmatched count resets cannot override a zero ratio`.
    #[test]
    fn unmatched_count_resets_cannot_override_a_zero_ratio() {
        for reset in [json!(null), json!("invalid"), json!("2026-09-19T16:46:02Z")] {
            let snapshot = parse(weekly_fixture(
                json!({ "limit": "100", "used": "19", "resetTime": reset }),
                json!({ "used_ratio": 0, "reset_time": "2026-09-19T16:45:59Z" }),
            ));
            assert_eq!(snapshot.primary.used_percent, 0.0, "count reset {reset}");
        }
    }

    // Upstream: `invalid or empty counts cannot override a zero ratio`.
    #[test]
    fn invalid_or_empty_counts_cannot_override_a_zero_ratio() {
        for used in ["0", "-1", "invalid"] {
            let snapshot = parse(weekly_fixture(
                json!({ "limit": "100", "used": used, "resetTime": "2026-09-19T16:45:59Z" }),
                json!({ "used_ratio": 0, "reset_time": "2026-09-19T16:45:59Z" }),
            ));
            assert_eq!(snapshot.primary.used_percent, 0.0, "used {used}");
        }
    }

    // Upstream: `different count window duration cannot override the session
    // ratio`.
    #[test]
    fn different_count_window_duration_cannot_override_the_session_ratio() {
        let mut response = mixed_international_response();
        response["limits"][0]["window"]["duration"] = json!(120);
        let snapshot = parse(response);
        assert_eq!(snapshot.primary.used_percent, 19.0);
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.used_percent, 0.0);
        assert_eq!(rate_limit.window_minutes, Some(300));
    }

    // Upstream `usageCounts`: an invalid `used` falls back to a valid
    // `remaining` balance, which is reliable evidence for both lanes.
    #[test]
    fn remaining_balance_recovers_invalid_used_counters() {
        let mut response = mixed_international_response();
        response["usage"]["used"] = json!("invalid");
        response["limits"][0]["detail"]["used"] = json!("-1");
        let snapshot = parse(response);
        assert_eq!(snapshot.primary.used_percent, 19.0);
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.used_percent, 1.0);
        assert_eq!(
            rate_limit.reset_description.as_deref(),
            Some("1/100 credits")
        );
    }

    // Upstream #4311 dropped the reliable-weekly-counters gate: a matching
    // 5-hour count overrides the zero session ratio whatever the weekly counts
    // report. The weekly lane still needs its own reliable counts to change.
    #[test]
    fn session_override_does_not_require_reliable_weekly_counters() {
        for weekly_usage in [
            None,
            Some(json!({ "limit": "100", "used": "invalid", "remaining": "101" })),
            Some(json!({ "limit": "0", "used": "19" })),
        ] {
            let mut response = mixed_international_response();
            match weekly_usage {
                Some(usage) => response["usage"] = usage,
                None => {
                    response
                        .as_object_mut()
                        .expect("fixture object")
                        .remove("usage");
                }
            }
            let snapshot = parse(response);
            assert_eq!(snapshot.primary.used_percent, 0.0);
            assert_eq!(snapshot.primary.window_minutes, Some(10_080));
            let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
            assert_eq!(rate_limit.used_percent, 1.0);
            assert_eq!(rate_limit.window_minutes, Some(300));
        }
    }

    // Upstream reads counters with `Int(_)`: fractional or padded values are
    // not reliable evidence, while integral JSON numbers are.
    #[test]
    fn counters_must_be_integers() {
        for used in [json!("19.5"), json!(" 19"), json!(19.5)] {
            let snapshot = parse(weekly_fixture(
                json!({ "limit": "100", "used": used, "resetTime": "2026-09-19T16:45:59Z" }),
                json!({ "used_ratio": 0, "reset_time": "2026-09-19T16:45:59Z" }),
            ));
            assert_eq!(snapshot.primary.used_percent, 0.0, "used {used}");
        }
        let snapshot = parse(weekly_fixture(
            json!({ "limit": 100.0, "used": 19, "resetTime": "2026-09-19T16:45:59Z" }),
            json!({ "used_ratio": 0, "reset_time": "2026-09-19T16:45:59Z" }),
        ));
        assert_eq!(snapshot.primary.used_percent, 19.0);
    }

    fn zero_session_ratio_with_legacy_window(window: Option<Value>) -> UsageSnapshot {
        let mut response = mixed_international_response();
        match window {
            Some(window) => response["limits"][0]["window"] = window,
            None => {
                response["limits"][0]
                    .as_object_mut()
                    .expect("limit object")
                    .remove("window");
            }
        }
        parse(response)
    }

    // Win-CodexBar tolerates a legacy limit without a window (upstream fails
    // to decode it); without a duration the counters cannot claim the lane.
    #[test]
    fn missing_legacy_window_does_not_override_zero_session_ratio() {
        let snapshot = zero_session_ratio_with_legacy_window(None);
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert_eq!(rate_limit.used_percent, 0.0);
    }

    #[test]
    fn unrecognized_legacy_window_does_not_override_zero_session_ratio() {
        let snapshot = zero_session_ratio_with_legacy_window(Some(json!({
            "duration": 300,
            "timeUnit": "TIME_UNIT_FORTNIGHT"
        })));
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert_eq!(rate_limit.used_percent, 0.0);
    }

    // Upstream #4311 (fixes #4306, fixture from the issue): the legacy 5-hour
    // counter is exhausted (100/100) while the matching 5-hour ratio reports 0
    // with the same reset. The more-exhausted reading wins even though a
    // monthly pool is present and weekly counters are absent. The monthly
    // Total pool is still reported.
    #[test]
    fn exhausted_legacy_counter_beats_zero_session_ratio_with_monthly_pool() {
        let snapshot = parse(json!({
            "limits": [{
                "window": { "duration": 300, "timeUnit": "TIME_UNIT_MINUTE" },
                "detail": {
                    "limit": "100",
                    "used": "100",
                    "resetTime": "2026-10-06T13:23:46.915474Z"
                }
            }],
            "usages": {
                "limit_5h": { "used_ratio": 0, "reset_time": "2026-10-06T13:23:46Z" },
                "limit_month_total": {
                    "used_ratio": 0.5531,
                    "reset_time": "2026-10-22T14:26:29Z"
                },
                "limit_month_code": { "used_ratio": 0, "reset_time": "2026-10-22T14:26:29Z" }
            }
        }));
        assert_weekly_absent(&snapshot);
        let rate_limit = snapshot.secondary.as_ref().expect("rate-limit lane");
        assert_eq!(rate_limit.used_percent, 100.0);
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert_eq!(rate_limit.resets_at, at("2026-10-06T13:23:46.915474Z"));
        assert_eq!(
            rate_limit.reset_description.as_deref(),
            Some("100/100 credits")
        );
        let [monthly] = snapshot.extra_rate_windows.as_slice() else {
            panic!("monthly Total lane");
        };
        assert_eq!(monthly.id, MONTHLY_WINDOW_ID);
        assert_eq!(monthly.title, "Total usage");
        assert!(close(monthly.window.used_percent, 55.31));
        assert_eq!(monthly.window.window_minutes, Some(43_200));
        assert_eq!(monthly.window.resets_at, at("2026-10-22T14:26:29Z"));
    }
}
