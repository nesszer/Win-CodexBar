use super::*;

fn api() -> CursorApi {
    CursorApi::new()
}

fn parse_summary(json: &str) -> UsageSummary {
    serde_json::from_str(json).expect("fixture should parse")
}

#[test]
fn sand_usage_maps_to_weekly_extra_window() {
    let status = SandUsageStatus {
        current_period_start: Some("2026-08-18T00:00:00Z".into()),
        next_reset_timestamp_utc: Some("2026-08-25T00:00:00Z".into()),
        usage_percent: Some(37.5),
        has_non_zero_included_limit: Some(true),
        included_limit_zero: None,
        sand_trial_expires_at: None,
    };
    let row = status
        .to_window("2026-08-20T00:00:00Z".parse().unwrap())
        .expect("grok bot window");
    assert_eq!(row.id, "cursor-grok-bot");
    assert_eq!(row.title, "Grok Bot");
    assert!((row.window.used_percent - 37.5).abs() < 0.001);
    assert_eq!(row.window.window_minutes, Some(10080));
}

#[test]
fn sand_usage_hides_accounts_without_included_allowance() {
    let status = SandUsageStatus {
        current_period_start: None,
        next_reset_timestamp_utc: None,
        usage_percent: Some(0.0),
        has_non_zero_included_limit: Some(false),
        included_limit_zero: None,
        sand_trial_expires_at: None,
    };
    assert!(
        status
            .to_window("2026-08-20T00:00:00Z".parse().unwrap())
            .is_none()
    );
}

#[test]
fn sand_usage_maps_paid_allowance_from_explicit_zero_flag() {
    let status = SandUsageStatus {
        current_period_start: Some("2026-08-18T00:00:00Z".into()),
        next_reset_timestamp_utc: Some("2026-08-25T00:00:00Z".into()),
        usage_percent: Some(37.5),
        has_non_zero_included_limit: Some(false),
        included_limit_zero: Some(false),
        sand_trial_expires_at: None,
    };
    let row = status
        .to_window("2026-08-20T00:00:00Z".parse().unwrap())
        .expect("paid Grok Bot window");
    assert_eq!(row.window.window_minutes, Some(10080));
    assert_eq!(
        row.window.resets_at,
        Some("2026-08-25T00:00:00Z".parse().unwrap())
    );
}

#[test]
fn sand_usage_maps_an_active_trial_without_a_recurring_reset() {
    let status = SandUsageStatus {
        current_period_start: Some("2026-08-18T00:00:00Z".into()),
        next_reset_timestamp_utc: Some("2026-08-25T00:00:00Z".into()),
        usage_percent: Some(12.5),
        has_non_zero_included_limit: Some(false),
        included_limit_zero: Some(true),
        sand_trial_expires_at: Some("2026-08-28T00:00:00Z".into()),
    };
    let row = status
        .to_window("2026-08-20T00:00:00Z".parse().unwrap())
        .expect("active trial window");
    assert_eq!(row.window.resets_at, None);
    assert_eq!(row.window.window_minutes, None);
    assert!((row.window.used_percent - 12.5).abs() < 0.001);
}

#[test]
fn sand_usage_hides_expired_trial() {
    let status = SandUsageStatus {
        current_period_start: None,
        next_reset_timestamp_utc: None,
        usage_percent: Some(12.5),
        has_non_zero_included_limit: Some(false),
        included_limit_zero: Some(true),
        sand_trial_expires_at: Some("2026-08-19T00:00:00Z".into()),
    };
    assert!(
        status
            .to_window("2026-08-20T00:00:00Z".parse().unwrap())
            .is_none()
    );
}

#[test]
fn test_cursor_build_result_with_lanes() {
    let json = r#"{
            "billingCycleStart": "2026-03-01T00:00:00Z",
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 1500,
                    "limit": 5000,
                    "totalPercentUsed": 30.0,
                    "autoPercentUsed": 20.0,
                    "apiPercentUsed": 10.0
                }
            }
        }"#;

    let summary = parse_summary(json);
    let result = api().build_result_with_team_budget(summary, None, None);

    assert!((result.primary.used_percent - 30.0).abs() < 0.01);

    let sec = result.secondary.expect("secondary should be present");
    assert!((sec.used_percent - 20.0).abs() < 0.01);
    assert!(sec.resets_at.is_some());

    let ms = result
        .model_specific
        .expect("model_specific should be present");
    assert!((ms.used_percent - 10.0).abs() < 0.01);
    assert!(ms.resets_at.is_some());

    assert!(result.cost.is_some());
    assert_eq!(result.plan_type.as_deref(), Some("Cursor Pro"));
}

#[test]
fn clamps_plan_usage_percent_at_100_when_over_limit() {
    // Upstream #2255: included usage past limit must not paint >100%.
    let json = r#"{
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 6000,
                    "limit": 5000,
                    "totalPercentUsed": 120.0,
                    "autoPercentUsed": 110.0,
                    "apiPercentUsed": 105.0
                }
            }
        }"#;
    let summary = parse_summary(json);
    let result = api().build_result_with_team_budget(summary, None, None);
    assert!((result.primary.used_percent - 100.0).abs() < 0.01);
    assert!((result.secondary.unwrap().used_percent - 100.0).abs() < 0.01);
    assert!((result.model_specific.unwrap().used_percent - 100.0).abs() < 0.01);
}

#[test]
fn test_cursor_build_result_prefers_api_percent_fields() {
    let json = r#"{
            "membershipType": "pro",
            "autoModelSelectedDisplayMessage": "You've used 13% of your included total usage",
            "individualUsage": {
                "plan": {
                    "used": 2000,
                    "limit": 2000,
                    "breakdown": {
                        "included": 2000,
                        "bonus": 580,
                        "total": 2580
                    },
                    "autoPercentUsed": 17.2,
                    "apiPercentUsed": 0,
                    "totalPercentUsed": 13.230769230769232
                }
            }
        }"#;

    let summary = parse_summary(json);
    let result = api().build_result_with_team_budget(summary, None, None);

    assert!((result.primary.used_percent - 13.230769230769232).abs() < 0.01);
    assert!((result.secondary.unwrap().used_percent - 17.2).abs() < 0.01);
    assert!((result.model_specific.unwrap().used_percent - 0.0).abs() < 0.01);

    let cost = result
        .cost
        .expect("plan usage should still produce cost snapshot");
    assert!((cost.used - 20.0).abs() < 0.01);
    assert_eq!(cost.limit, Some(20.0));
    assert_eq!(result.plan_type.as_deref(), Some("Cursor Pro"));
}

#[test]
fn test_cursor_build_result_cents_only() {
    let json = r#"{
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 2500,
                    "limit": 5000
                }
            }
        }"#;

    let summary = parse_summary(json);
    let result = api().build_result_with_team_budget(summary, None, None);

    assert!((result.primary.used_percent - 50.0).abs() < 0.01);
    assert!(result.secondary.is_none(), "no autoPercentUsed in payload");
    assert!(
        result.model_specific.is_none(),
        "no apiPercentUsed in payload"
    );
    assert!(result.cost.is_some());
}

#[test]
fn test_cursor_build_result_missing_plan() {
    let json = r#"{
            "membershipType": "hobby",
            "individualUsage": {}
        }"#;

    let summary = parse_summary(json);
    let result = api().build_result_with_team_budget(summary, None, None);

    assert!((result.primary.used_percent).abs() < 0.01);
    assert!(result.secondary.is_none());
    assert!(result.model_specific.is_none());
    assert!(result.cost.is_none());
}

#[test]
fn test_cursor_on_demand_as_cost() {
    let json = r#"{
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 800,
                    "limit": 5000,
                    "totalPercentUsed": 16.0
                },
                "onDemand": {
                    "enabled": true,
                    "used": 350,
                    "limit": 1000
                }
            }
        }"#;

    let summary = parse_summary(json);
    let result = api().build_result_with_team_budget(summary, None, None);

    assert!((result.primary.used_percent - 16.0).abs() < 0.01);
    let cost = result.cost.expect("cost should exist from on-demand usage");
    assert!((cost.used - 3.5).abs() < 0.01);
    assert_eq!(cost.limit, Some(10.0));
    assert_eq!(cost.period, "On-demand (billing cycle)");
}

#[test]
fn plan_cost_period_uses_billing_cycle_start() {
    let json = r#"{
            "billingCycleStart": "2026-03-01T00:00:00Z",
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 2500,
                    "limit": 5000
                }
            }
        }"#;
    let summary = parse_summary(json);
    let result = api().build_result_with_team_budget(summary, None, None);
    let cost = result.cost.expect("plan cost");
    assert!((cost.used - 25.0).abs() < 0.01);
    assert_eq!(cost.limit, Some(50.0));
    assert_eq!(
        cost.period,
        "Cursor and Third Party (since 2026-03-01T00:00:00Z)"
    );
}

#[test]
fn test_cursor_individual_overall_fallback() {
    let summary = parse_summary(r#"{"individualUsage":{"overall":{"used":2500,"limit":10000}}}"#);
    let result = api().build_result_with_team_budget(summary, None, None);
    assert!((result.primary.used_percent - 25.0).abs() < 0.01);
    assert_eq!(result.cost.unwrap().limit, Some(100.0));
}

#[test]
fn test_cursor_team_pooled_fallback() {
    let summary = parse_summary(r#"{"teamUsage":{"pooled":{"used":5000,"limit":10000}}}"#);
    let result = api().build_result_with_team_budget(summary, None, None);
    assert!((result.primary.used_percent - 50.0).abs() < 0.01);
    assert_eq!(result.cost.unwrap().used, 50.0);
}

#[test]
fn member_lookup_requires_nonempty_authenticated_email() {
    for email in [None, Some(String::new()), Some("  ".to_string())] {
        let user = UserInfo {
            email,
            email_verified: None,
            name: None,
            sub: None,
            created_at: None,
            updated_at: None,
            picture: None,
        };
        assert!(user.verified_email().is_none());
    }
}

#[test]
fn verified_team_budget_replaces_summary_plan_and_keeps_zero_summary_fallback() {
    let summary = parse_summary(
        r#"{
                "billingCycleStart":"2026-09-01T00:00:00Z",
                "billingCycleEnd":"2026-10-01T00:00:00Z",
                "membershipType":"enterprise",
                "individualUsage":{"plan":{"used":0,"limit":2000,"totalPercentUsed":0}}
            }"#,
    );
    let result = api().build_result_with_team_budget(
        summary,
        None,
        Some(CursorMemberBudget {
            used_usd: 13.12,
            limit_usd: 150.0,
        }),
    );
    assert!((result.primary.used_percent - 8.7466666667).abs() < 0.00001);
    let cost = result.cost.expect("verified member budget cost");
    assert!((cost.used - 13.12).abs() < 0.00001);
    assert_eq!(cost.limit, Some(150.0));

    let fallback_summary = parse_summary(
        r#"{
                "billingCycleStart":"2026-09-01T00:00:00Z",
                "billingCycleEnd":"2026-10-01T00:00:00Z",
                "membershipType":"enterprise",
                "individualUsage":{"plan":{"used":0,"limit":2000,"totalPercentUsed":0}}
            }"#,
    );
    let fallback = api().build_result_with_team_budget(fallback_summary, None, None);
    assert_eq!(fallback.primary.used_percent, 0.0);
    assert_eq!(
        fallback.cost.expect("summary fallback cost").limit,
        Some(20.0)
    );
}
