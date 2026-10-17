//! `credits.monthlyCreditsGranted` sizing (upstream #3939), ported from
//! `CommandCodeUsageFetcherTests` at v0.66.0.

use super::*;
use serde_json::json;

const ANSWERED: SubscriptionLookup = SubscriptionLookup::Answered;
const UNAVAILABLE: SubscriptionLookup = SubscriptionLookup::Unavailable;

fn go_plan_subscription() -> Value {
    json!({"data": {"planId": "individual-go", "currentPeriodEnd": "2026-10-01T12:00:00Z"}})
}

/// Upstream `parses granted monthly credits` fixture, verbatim.
fn granted_fixture() -> Value {
    serde_json::from_str(
        r#"{"credits":{"monthlyCredits":4,"purchasedCredits":0,"premiumMonthlyCredits":0,
        "opensourceMonthlyCredits":4,"monthlyCreditsGranted":10}}"#,
    )
    .unwrap()
}

/// Upstream `subscription failure ... sizes the monthly window from granted
/// credits` payload: granted 10 beside rolling five-hour and weekly limits.
fn granted_with_rolling_limits(remaining: f64) -> Value {
    json!({
        "credits": {
            "monthlyCredits": remaining,
            "purchasedCredits": 0,
            "premiumMonthlyCredits": 0,
            "opensourceMonthlyCredits": remaining,
            "monthlyCreditsGranted": 10
        },
        "windowLimits": {
            "fiveHour": {"used": 2.5, "cap": 10, "resetAt": 0},
            "weekly": {"used": 30, "cap": 100, "resetAt": 0}
        }
    })
}

#[test]
fn granted_credits_size_the_monthly_window_without_a_plan() {
    let result = result_from_payloads(&granted_fixture(), None, ANSWERED).unwrap();

    // No windowLimits: the monthly window is the only lane, so it is primary.
    assert_eq!(result.usage.primary.used_percent, 60.0);
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("$6.00 of $10.00")
    );
    let cost = result.cost.expect("monthly credits cost");
    assert_eq!(cost.used, 6.0);
    assert_eq!(cost.limit, Some(10.0));
}

#[test]
fn credits_without_a_grant_keep_the_free_tier_reading() {
    let credits = json!({"credits": {"monthlyCredits": 4, "opensourceMonthlyCredits": 4}});
    let result = result_from_payloads(&credits, None, ANSWERED).unwrap();

    assert_eq!(result.usage.primary.used_percent, 0.0);
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("$4.00 remaining")
    );
    let cost = result.cost.expect("monthly credits cost");
    assert_eq!(cost.used, 0.0);
    assert_eq!(cost.limit, Some(4.0));
}

#[test]
fn granted_credits_win_over_the_plan_catalog() {
    let credits = json!({"credits": {
        "monthlyCredits": 9, "purchasedCredits": 0, "premiumMonthlyCredits": 0,
        "opensourceMonthlyCredits": 9, "monthlyCreditsGranted": 12
    }});
    let result = result_from_payloads(&credits, Some(&go_plan_subscription()), ANSWERED).unwrap();

    assert!((result.usage.primary.used_percent - 25.0).abs() < 1e-9);
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Go · $3.00 of $12.00")
    );
    let cost = result.cost.expect("monthly credits cost");
    assert_eq!(cost.used, 3.0);
    assert_eq!(cost.limit, Some(12.0));
    // The period end still comes from the subscription.
    assert_eq!(
        result.usage.primary.resets_at.map(|ts| ts.to_rfc3339()),
        Some("2026-10-01T12:00:00+00:00".to_string())
    );
}

#[test]
fn granted_credits_accept_numeric_strings() {
    let credits = json!({"credits": {"monthlyCredits": "4", "monthlyCreditsGranted": " 10 "}});
    let result = result_from_payloads(&credits, None, ANSWERED).unwrap();
    assert_eq!(result.usage.primary.used_percent, 60.0);
}

#[test]
fn unusable_granted_credits_keep_the_free_tier_reading() {
    for granted in [json!(0), json!(-5), json!("Infinity"), json!("NaN")] {
        let credits = json!({"credits": {
            "monthlyCredits": 0, "purchasedCredits": 5, "monthlyCreditsGranted": granted
        }});
        let result = result_from_payloads(&credits, None, ANSWERED).unwrap();

        // A spendable balance keeps the monthly bar untouched, as with no grant.
        assert_eq!(result.usage.primary.used_percent, 0.0, "{granted}");
        assert_eq!(
            result.usage.login_method.as_deref(),
            Some("+ $5.00 credits"),
            "{granted}"
        );
    }
}

#[test]
fn unusable_granted_credits_fall_back_to_the_plan_catalog() {
    for granted in [json!(0), json!(-5), json!("Infinity")] {
        let credits = json!({"credits": {"monthlyCredits": 4, "monthlyCreditsGranted": granted}});
        let result =
            result_from_payloads(&credits, Some(&go_plan_subscription()), ANSWERED).unwrap();

        assert_eq!(result.usage.primary.used_percent, 60.0, "{granted}");
        assert_eq!(
            result.usage.login_method.as_deref(),
            Some("Go · $6.00 of $10.00"),
            "{granted}"
        );
    }
}

#[test]
fn failed_lookup_with_granted_credits_keeps_the_row_without_a_reset() {
    for (remaining, used_percent) in [(4.0, 60.0), (0.0, 100.0)] {
        let credits = granted_with_rolling_limits(remaining);
        let result = result_from_payloads(&credits, None, UNAVAILABLE).unwrap();
        let usage = result.usage;

        assert_eq!(usage.primary.used_percent, 25.0);
        assert_eq!(usage.secondary.expect("weekly").used_percent, 30.0);
        let monthly = usage.tertiary.expect("monthly window");
        assert!(
            (monthly.used_percent - used_percent).abs() < 1e-9,
            "{monthly:?}"
        );
        // The billing period end only comes from the subscription lookup.
        assert_eq!(monthly.resets_at, None);
    }
}

#[test]
fn failed_lookup_without_plan_or_grant_leaves_the_monthly_row_unavailable() {
    // Upstream `creditsJSON`: 8.7784 remaining, no grant, no rolling limits.
    let credits = json!({"credits": {
        "monthlyCredits": 8.7784, "purchasedCredits": 0, "premiumMonthlyCredits": 0,
        "opensourceMonthlyCredits": 8.7784
    }});
    let result = result_from_payloads(&credits, None, UNAVAILABLE).unwrap();
    assert!(result.usage.tertiary.is_none());

    // Purchased credits alone must not revive the free-tier reading either.
    let purchased = json!({"credits": {"monthlyCredits": 0, "purchasedCredits": 3}});
    let result = result_from_payloads(&purchased, None, UNAVAILABLE).unwrap();
    assert!(result.usage.tertiary.is_none());

    // The rolling rows stay available either way.
    let with_limits = json!({
        "credits": {"monthlyCredits": 8.7784},
        "windowLimits": {"fiveHour": {"used": 1, "cap": 4, "resetAt": 0}}
    });
    let result = result_from_payloads(&with_limits, None, UNAVAILABLE).unwrap();
    assert_eq!(result.usage.primary.used_percent, 25.0);
    assert!(result.usage.tertiary.is_none());
}

#[test]
fn answered_free_tier_lookup_keeps_the_untouched_monthly_row() {
    let credits = json!({
        "credits": {"monthlyCredits": 8.7784},
        "windowLimits": {"fiveHour": {"used": 1, "cap": 4, "resetAt": 0}}
    });
    let monthly = result_from_payloads(&credits, None, ANSWERED)
        .unwrap()
        .usage
        .tertiary
        .expect("free-tier monthly bar");
    assert_eq!(monthly.used_percent, 0.0);
}

#[test]
fn failed_lookup_sizes_from_the_remembered_plan_when_the_grant_is_absent() {
    let cache = Mutex::new(CommandCodePlanCache::default());
    let now = DateTime::from_timestamp(1_780_000_000, 0).unwrap();
    let period_end = DateTime::from_timestamp(1_780_100_000, 0).unwrap();
    let remembered = json!({"data": {
        "planId": "individual-go", "currentPeriodEnd": period_end.to_rfc3339()
    }});
    resolve_subscription_payload_with_cache(Ok(remembered), "fp", now, &cache);
    let recovered =
        resolve_subscription_payload_with_cache(Err(ProviderError::Timeout), "fp", now, &cache);

    // No grant: the remembered plan sizes the row, usage stays fresh.
    let credits = json!({"credits": {"monthlyCredits": 4}});
    let monthly = result_from_payloads(&credits, recovered.as_ref(), UNAVAILABLE)
        .unwrap()
        .usage
        .primary;
    assert_eq!(monthly.used_percent, 60.0);
    assert_eq!(monthly.resets_at, Some(period_end));

    // A grant wins over the remembered plan.
    let granted = json!({"credits": {"monthlyCredits": 4, "monthlyCreditsGranted": 20}});
    let result = result_from_payloads(&granted, recovered.as_ref(), UNAVAILABLE).unwrap();
    assert_eq!(result.usage.primary.used_percent, 80.0);
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Go · $16.00 of $20.00")
    );
}

#[test]
fn failed_lookup_without_memory_still_honours_a_grant() {
    let cache = Mutex::new(CommandCodePlanCache::default());
    let now = DateTime::from_timestamp(1_780_000_000, 0).unwrap();
    let recovered =
        resolve_subscription_payload_with_cache(Err(ProviderError::Timeout), "fp", now, &cache);
    assert_eq!(recovered, None);

    let result = result_from_payloads(&granted_fixture(), recovered.as_ref(), UNAVAILABLE).unwrap();
    assert_eq!(result.usage.primary.used_percent, 60.0);
    assert_eq!(result.usage.primary.resets_at, None);
}

// ── Go v1 and unknown plan ids (upstream #4423) ───

fn subscription_for(plan_id: &str) -> Value {
    json!({"data": {"planId": plan_id, "currentPeriodEnd": "2026-10-01T12:00:00Z"}})
}

#[test]
fn go_v1_plan_sizes_the_monthly_window_from_the_catalog() {
    let credits = json!({"credits": {"monthlyCredits": 4, "purchasedCredits": 0}});
    let result = result_from_payloads(
        &credits,
        Some(&subscription_for("individual-go-v1")),
        ANSWERED,
    )
    .unwrap();

    assert_eq!(result.usage.primary.used_percent, 60.0);
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Go · $6.00 of $10.00")
    );
    let cost = result.cost.expect("monthly credits cost");
    assert_eq!(cost.used, 6.0);
    assert_eq!(cost.limit, Some(10.0));
}

#[test]
fn unknown_plan_keeps_api_usage_and_its_raw_label() {
    let credits = json!({
        "credits": {"monthlyCredits": 8, "monthlyCreditsGranted": 20},
        "windowLimits": {"fiveHour": {"used": 1, "cap": 4, "resetAt": 0}}
    });
    let result = result_from_payloads(
        &credits,
        Some(&subscription_for("individual-future")),
        ANSWERED,
    )
    .unwrap();
    let usage = result.usage;

    assert_eq!(usage.primary.used_percent, 25.0);
    let monthly = usage.tertiary.expect("monthly window from the API grant");
    assert_eq!(monthly.used_percent, 60.0);
    assert_eq!(
        usage.login_method.as_deref(),
        Some("individual-future · $12.00 of $20.00")
    );
    let cost = result.cost.expect("monthly credits cost");
    assert_eq!(cost.used, 12.0);
    assert_eq!(cost.limit, Some(20.0));
}

#[test]
fn unknown_plan_without_grant_has_no_monthly_percentage() {
    let credits = json!({"credits": {"monthlyCredits": 8.7784}});
    let result = result_from_payloads(
        &credits,
        Some(&subscription_for("individual-future")),
        ANSWERED,
    )
    .unwrap();

    // A known-but-unsized plan must not borrow the free-tier 0% bar.
    assert!(result.usage.tertiary.is_none());
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("individual-future · $8.78 remaining")
    );

    let empty = json!({"credits": {"monthlyCredits": 0}});
    let result = result_from_payloads(
        &empty,
        Some(&subscription_for("individual-future")),
        ANSWERED,
    )
    .unwrap();
    assert!(result.usage.tertiary.is_none());
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("individual-future · $0.00 remaining")
    );
}
