//! Tests for the OpenRouter provider.

use super::*;

// Regression guard for the `/auth/credits` 404 bug: the base must be the
// bare `/api/v1` prefix. Credits and key live on DIFFERENT subpaths, so a
// base that bakes in `/auth` (or anything else) silently breaks one of them.
#[test]
fn api_base_is_bare_v1_prefix() {
    assert_eq!(OPENROUTER_API_BASE, "https://openrouter.ai/api/v1");
}

// Credits endpoint: `/api/v1/credits` (verified HTTP 200 against live API).
// The old base `.../api/v1/auth` produced `/api/v1/auth/credits` -> 404.
#[test]
fn credits_url_resolves_to_canonical_path() {
    let url = format!("{}/credits", OPENROUTER_API_BASE);
    assert_eq!(url, "https://openrouter.ai/api/v1/credits");
}

// Key introspection endpoint: `/api/v1/key` (verified HTTP 200), matching
// upstream's `{base}/key` append. (OpenRouter also aliases `/auth/key`, but
// we mirror upstream's canonical path.)
#[test]
fn key_url_resolves_to_canonical_path() {
    let url = format!("{}/key", OPENROUTER_API_BASE);
    assert_eq!(url, "https://openrouter.ai/api/v1/key");
}

#[test]
fn usage_dashboard_opens_activity_history() {
    assert_eq!(
        OpenRouterProvider::new().metadata().dashboard_url,
        Some("https://openrouter.ai/activity")
    );
}

#[test]
fn deprecated_rate_limit_metadata_is_ignored() {
    let response: KeyResponse = serde_json::from_value(serde_json::json!({
        "data": {
            "rate_limit": "deprecated",
            "is_management_key": true,
            "usage": 0.0
        }
    }))
    .expect("deprecated rate_limit must not invalidate /key");

    assert_eq!(response.data.is_management_key, Some(true));
    assert_eq!(response.data.usage, Some(0.0));
}

// ── F14: server-reported current-period remaining drives the key meter ──

fn key_data(
    limit: Option<f64>,
    remaining: Option<f64>,
    reset: Option<&str>,
    usage: Option<f64>,
    daily: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
) -> KeyData {
    KeyData {
        limit,
        limit_remaining: remaining,
        limit_reset: reset.map(str::to_string),
        usage,
        usage_daily: daily,
        usage_weekly: weekly,
        usage_monthly: monthly,
        is_management_key: None,
    }
}

fn key_quota_percent(key_data: KeyData) -> Option<f64> {
    let mut usage = UsageSnapshot::new(RateWindow::new(0.0));
    OpenRouterProvider::add_key_quota(&mut usage, &key_data);
    usage.secondary.map(|window| window.used_percent)
}

#[test]
fn key_limit_copy_stays_distinct_from_account_balance() {
    let provider = OpenRouterProvider::new();
    assert_eq!(provider.metadata.weekly_label, "API key limit");

    let credits = CreditsData {
        total_credits: 5.0,
        total_usage: 3.1,
    };
    let mut usage = OpenRouterProvider::build_credits_usage(&credits);
    OpenRouterProvider::add_key_quota(
        &mut usage,
        &key_data(
            Some(30.0),
            Some(30.0),
            Some("monthly"),
            Some(0.0),
            None,
            None,
            Some(0.0),
        ),
    );
    assert_eq!(usage.login_method.as_deref(), Some("$1.90 balance"));
    let key = usage.secondary.expect("key spending cap");
    assert_eq!(key.used_percent, 0.0);
    assert_eq!(
        key.reset_description.as_deref(),
        Some("$0.00/$30.00 spending cap · Spending cap, not balance")
    );
}

#[test]
fn key_quota_can_stand_in_when_account_credits_are_unavailable() {
    let usage = OpenRouterProvider::build_key_fallback_usage(&key_data(
        Some(20.0),
        None,
        None,
        Some(5.0),
        None,
        None,
        None,
    ))
    .expect("usable key quota");

    assert!(usage.primary.is_informational);
    assert!(usage.primary_label.is_none());
    assert!(usage.login_method.is_none());
    let key_window = usage.secondary.expect("key spending cap");
    assert_eq!(key_window.used_percent, 25.0);
    assert_eq!(usage.secondary_label.as_deref(), Some("API key limit"));
    assert_eq!(
        key_window.reset_description.as_deref(),
        Some("$5.00/$20.00 spending cap · Account balance unavailable")
    );
}

#[test]
fn key_fallback_does_not_invent_usage_without_a_limit() {
    assert!(
        OpenRouterProvider::build_key_fallback_usage(&key_data(
            None,
            None,
            None,
            Some(5.0),
            None,
            None,
            None,
        ))
        .is_none()
    );
}

#[test]
fn fallback_preserves_key_quota_lane_across_recovery() {
    let credits = || {
        Ok(CreditsResponse {
            data: CreditsData {
                total_credits: 20.0,
                total_usage: 5.0,
            },
        })
    };
    let key = || key_data(Some(20.0), None, None, Some(5.0), None, None, None);

    let normal = OpenRouterProvider::resolve_usage(credits(), Some(key()))
        .expect("account credits should resolve");
    let fallback = OpenRouterProvider::resolve_usage(
        Err(ProviderError::Other("credits unavailable".to_string())),
        Some(key()),
    )
    .expect("key quota should resolve when credits are unavailable");
    let recovered = OpenRouterProvider::resolve_usage(credits(), Some(key()))
        .expect("account credits should recover");

    for usage in [&normal, &fallback, &recovered] {
        assert_eq!(usage.secondary_label.as_deref(), Some("API key limit"));
        assert_eq!(
            usage.secondary.as_ref().map(|window| window.used_percent),
            Some(25.0)
        );
    }
    assert!(!normal.primary.is_informational);
    assert!(fallback.primary.is_informational);
    assert!(!recovered.primary.is_informational);
    assert_eq!(normal.login_method.as_deref(), Some("$15.00 balance"));
    assert!(fallback.login_method.is_none());
    assert_eq!(recovered.login_method.as_deref(), Some("$15.00 balance"));
}

#[test]
fn uncapped_cost_prefers_monthly_key_usage_and_keeps_balance() {
    let credits = CreditsResponse {
        data: CreditsData {
            total_credits: 20.0,
            total_usage: 7.0,
        },
    };
    let key = key_data(Some(0.0), None, None, Some(5.0), None, None, Some(3.5));

    let cost = OpenRouterProvider::build_uncapped_cost(Some(&key), Some(&credits))
        .expect("uncapped key should expose spend");

    assert_eq!(cost.used, 3.5);
    assert_eq!(cost.period, "This month (API key)");
    assert_eq!(cost.balance, Some(13.0));
}

#[test]
fn capped_and_management_keys_do_not_create_payg_costs() {
    let credits = CreditsResponse {
        data: CreditsData {
            total_credits: 20.0,
            total_usage: 7.0,
        },
    };
    let capped = key_data(Some(10.0), None, None, Some(5.0), None, None, Some(3.5));
    assert!(OpenRouterProvider::build_uncapped_cost(Some(&capped), Some(&credits)).is_none());

    let mut management = key_data(Some(0.0), None, None, Some(5.0), None, None, Some(3.5));
    management.is_management_key = Some(true);
    assert!(OpenRouterProvider::build_uncapped_cost(Some(&management), Some(&credits)).is_none());
}

#[test]
fn activity_cost_wins_while_uncapped_key_spend_windows_remain() {
    let credits = CreditsResponse {
        data: CreditsData {
            total_credits: 20.0,
            total_usage: 7.0,
        },
    };
    let key = key_data(
        Some(0.0),
        None,
        None,
        Some(5.0),
        Some(1.0),
        Some(2.0),
        Some(3.0),
    );
    let mut usage = OpenRouterProvider::build_credits_usage(&credits.data);
    OpenRouterProvider::apply_key_lanes(&mut usage, &key, "Spending cap, not balance");

    let activity = CostSnapshot::new(4.0, "USD", "Last 30 days (UTC)");
    let selected = Some(activity)
        .or(OpenRouterProvider::build_uncapped_cost(
            Some(&key),
            Some(&credits),
        ))
        .expect("Activity cost should be selected");

    assert_eq!(selected.used, 4.0);
    assert_eq!(selected.period, "Last 30 days (UTC)");
    for (id, expected) in [
        ("daily-spend", "$1.00 today"),
        ("weekly-spend", "$2.00 this week"),
        ("monthly-spend", "$3.00 this month"),
    ] {
        let window = usage
            .extra_rate_windows
            .iter()
            .find(|window| window.id == id)
            .expect("key spend window");
        assert_eq!(window.window.reset_description.as_deref(), Some(expected));
    }
}

#[test]
fn server_remaining_replaces_lifetime_usage_for_meter() {
    // limit 50, server says 12.50 left this period → 75% used, even though
    // cumulative lifetime usage would imply a different ratio.
    let pct = key_quota_percent(key_data(
        Some(50.0),
        Some(12.5),
        None,
        Some(40.0),
        None,
        None,
        None,
    ));
    assert_eq!(pct, Some(75.0));
}

#[test]
fn negative_server_remaining_reads_exhausted() {
    // Upstream: "treat negative remaining as exhausted quota".
    let pct = key_quota_percent(key_data(
        Some(50.0),
        Some(-3.0),
        None,
        Some(10.0),
        None,
        None,
        None,
    ));
    assert_eq!(pct, Some(100.0));
}

#[test]
fn above_limit_server_remaining_reads_zero() {
    // Inclusive [0, keyLimit] clamp: a server remaining above the
    // configured limit renders 0% used, not a suppressed meter.
    let pct = key_quota_percent(key_data(
        Some(50.0),
        Some(75.0),
        None,
        Some(10.0),
        None,
        None,
        None,
    ));
    assert_eq!(pct, Some(0.0));
}

#[test]
fn reset_window_usage_is_the_preferred_fallback() {
    // No remaining: `limit_reset: "monthly"` picks usage_monthly (25/50).
    let pct = key_quota_percent(key_data(
        Some(50.0),
        None,
        Some("monthly"),
        Some(40.0),
        Some(1.0),
        Some(2.0),
        Some(25.0),
    ));
    assert_eq!(pct, Some(50.0));
    // Case-insensitive reset label.
    let pct = key_quota_percent(key_data(
        Some(50.0),
        None,
        Some("WEEKLY"),
        Some(40.0),
        Some(1.0),
        Some(2.0),
        Some(25.0),
    ));
    assert_eq!(pct, Some(4.0));
}

#[test]
fn cumulative_usage_is_the_last_fallback() {
    let pct = key_quota_percent(key_data(
        Some(50.0),
        None,
        None,
        Some(20.0),
        Some(1.0),
        None,
        None,
    ));
    assert_eq!(pct, Some(40.0));
}

#[test]
fn no_usable_quota_source_hides_the_meter() {
    assert_eq!(
        key_quota_percent(key_data(Some(50.0), None, None, None, None, None, None)),
        None
    );
    assert_eq!(
        key_quota_percent(key_data(
            Some(0.0),
            Some(5.0),
            None,
            Some(1.0),
            None,
            None,
            None
        )),
        None
    );
    assert_eq!(
        key_quota_percent(key_data(None, Some(5.0), None, Some(1.0), None, None, None)),
        None
    );
}

#[test]
fn parsed_key_wire_fields_decode() {
    let parsed: KeyResponse = serde_json::from_str(
        r#"{"data":{"limit":50,"limit_remaining":12.5,"limit_reset":"monthly","usage":40,"usage_monthly":25}}"#,
    )
    .unwrap();
    assert_eq!(parsed.data.limit, Some(50.0));
    assert_eq!(parsed.data.limit_remaining, Some(12.5));
    assert_eq!(parsed.data.limit_reset.as_deref(), Some("monthly"));
}
