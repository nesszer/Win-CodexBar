use super::*;

fn parse_with_seat(json: &str, seat: Option<f64>) -> Result<UsageSnapshot, ProviderError> {
    let response: CopilotUsageResponse = serde_json::from_str(json).unwrap();
    snapshot_from_response_with_seat_entitlement(response, seat)
}

fn parse_snapshot(json: &str) -> UsageSnapshot {
    parse_with_seat(json, None).unwrap()
}

fn parse_snapshot_result(json: &str) -> Result<UsageSnapshot, ProviderError> {
    parse_with_seat(json, None)
}

fn seat_window(usage: &UsageSnapshot) -> Option<&NamedRateWindow> {
    usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == SEAT_CREDIT_WINDOW_ID)
}

fn assert_used(window: &RateWindow, expected: f64) {
    assert!(
        (window.used_percent - expected).abs() < 0.001,
        "{} != {expected}",
        window.used_percent
    );
}

#[test]
fn paid_plan_parses_premium_and_chat_quotas() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "pro",
                "quota_reset_date": "2026-06-01",
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 300,
                        "remaining": 240,
                        "percent_remaining": 80,
                        "quota_id": "premium_interactions"
                    },
                    "chat": {
                        "entitlement": 1000,
                        "remaining": 900,
                        "percent_remaining": 90,
                        "quota_id": "chat"
                    }
                }
            }"#,
    );

    assert_eq!(usage.login_method.as_deref(), Some("Copilot Pro"));
    assert_used(&usage.primary, 20.0);
    assert_used(&usage.secondary.unwrap(), 10.0);
}

#[test]
fn limited_user_quotas_parse_free_schema() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "free",
                "monthly_quotas": {
                    "completions": 2000,
                    "chat": "50"
                },
                "limited_user_quotas": {
                    "completions": "1000",
                    "chat": 10
                }
            }"#,
    );

    assert_eq!(usage.login_method.as_deref(), Some("Copilot Free"));
    assert_used(&usage.primary, 50.0);
    assert_used(&usage.secondary.unwrap(), 80.0);
}

#[test]
fn derives_missing_percent_and_accepts_numeric_strings() {
    let usage = parse_snapshot(
        r#"{
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": "100",
                        "remaining": "25",
                        "quota_id": "premium_interactions"
                    }
                }
            }"#,
    );

    assert_used(&usage.primary, 75.0);
}

#[test]
fn ignores_placeholders_and_does_not_promote_chat_to_premium() {
    let usage = parse_snapshot(
        r#"{
                "quota_snapshots": {
                    "premium_interactions": {
                        "percent_remaining": 0,
                        "quota_id": ""
                    },
                    "chat": {
                        "entitlement": 100,
                        "remaining": 75,
                        "percent_remaining": 75,
                        "quota_id": "chat"
                    }
                }
            }"#,
    );

    assert_used(&usage.primary, 25.0);
    assert!(usage.secondary.is_none());
}

#[test]
fn drops_business_token_billing_zero_entitlement_quotas() {
    let err = parse_snapshot_result(
        r#"{
                "copilot_plan": "business",
                "token_based_billing": true,
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 0,
                        "remaining": 0,
                        "percent_remaining": 100,
                        "quota_id": "premium_interactions"
                    },
                    "chat": {
                        "entitlement": 0,
                        "remaining": 0,
                        "percent_remaining": 100,
                        "quota_id": "chat"
                    },
                    "completions": {
                        "entitlement": 0,
                        "remaining": 0,
                        "percent_remaining": 100,
                        "quota_id": "completions"
                    }
                }
            }"#,
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("token-based billing usage is unavailable")
    );
}

#[test]
fn keeps_percent_only_quota_snapshots_available() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "business",
                "quota_snapshots": {
                    "chat": {
                        "percent_remaining": 40,
                        "quota_id": "chat"
                    }
                }
            }"#,
    );

    assert_eq!(usage.login_method.as_deref(), Some("Copilot Business"));
    assert_used(&usage.primary, 60.0);
    assert!(usage.secondary.is_none());
}

#[test]
fn keeps_fully_consumed_positive_entitlement_quota() {
    let usage = parse_snapshot(
        r#"{
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 500,
                        "remaining": 0,
                        "percent_remaining": 0,
                        "quota_id": "premium_interactions"
                    }
                }
            }"#,
    );

    assert_used(&usage.primary, 100.0);
}

#[test]
fn keeps_additional_budget_as_extra_window() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "pro",
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 500,
                        "remaining": 250,
                        "quota_id": "premium_interactions"
                    },
                    "additional_budget": {
                        "entitlement": 100,
                        "remaining": 25,
                        "quota_id": "additional_budget"
                    }
                }
            }"#,
    );

    assert_used(&usage.primary, 50.0);
    assert_eq!(usage.extra_rate_windows.len(), 1);
    assert_eq!(usage.extra_rate_windows[0].id, "additional-budget");
    assert_eq!(usage.extra_rate_windows[0].title, "Additional Budget");
    assert_used(&usage.extra_rate_windows[0].window, 75.0);
}

#[test]
fn preserves_over_quota_percent_remaining() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "pro",
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 500,
                        "remaining": -75,
                        "percent_remaining": -15,
                        "quota_id": "premium_interactions"
                    }
                }
            }"#,
    );

    assert_eq!(usage.login_method.as_deref(), Some("Copilot Pro"));
    assert_used(&usage.primary, 115.0);
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("115% used")
    );
    assert!(usage.primary.is_exhausted());
}

#[test]
fn derives_over_quota_percent_from_negative_remaining() {
    let usage = parse_snapshot(
        r#"{
                "quota_snapshots": {
                    "chat": {
                        "entitlement": 500,
                        "remaining": -75,
                        "quota_id": "chat"
                    }
                }
            }"#,
    );

    assert_used(&usage.primary, 115.0);
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("115% used")
    );
}

#[test]
fn normalizes_enterprise_hosts() {
    assert_eq!(
        normalized_api_host(Some("github.com")),
        "api.github.com".to_string()
    );
    assert_eq!(
        normalized_api_host(Some("github.example.com")),
        "api.github.example.com".to_string()
    );
    assert_eq!(
        normalized_api_host(Some("api.github.example.com")),
        "api.github.example.com".to_string()
    );
}

// ── A15: credits_used counter for token-billed seats (upstream #2613) ───

#[test]
fn decodes_credits_used_as_number_or_string() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "business",
                "quota_reset_date": "2026-06-01",
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 300,
                        "remaining": 240,
                        "percent_remaining": 80,
                        "quota_id": "premium_interactions",
                        "credits_used": "1234.56"
                    }
                }
            }"#,
    );
    let extra = &usage.extra_rate_windows;
    assert!(
        extra.iter().any(|w| w.id == "ai-credits"
            && w.window.reset_description.as_deref() == Some("1234.56 AI credits used")),
        "{extra:?}"
    );
}

#[test]
fn configured_seat_allowance_adds_a_numeric_credit_window() {
    let usage = parse_with_seat(
        r#"{
                "copilot_plan": "business",
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 300,
                        "remaining": 240,
                        "percent_remaining": 80,
                        "quota_id": "premium_interactions",
                        "credits_used": 50
                    }
                }
            }"#,
        Some(200.0),
    )
    .unwrap();

    let seat = seat_window(&usage).expect("configured seat-credit window");
    assert_used(&seat.window, 25.0);
    assert!(!seat.window.is_informational);
    assert_eq!(seat.title, "Credits used");
}

#[test]
fn missing_primary_quota_is_informational_when_seat_credit_is_available() {
    let usage = parse_with_seat(
        r#"{
                "copilot_plan": "business",
                "quota_snapshots": {
                    "additional_budget": {
                        "credits_used": 50
                    }
                }
            }"#,
        Some(200.0),
    )
    .unwrap();

    assert!(usage.primary.is_informational);
    assert!(seat_window(&usage).is_some());
}

#[test]
fn non_finite_derived_seat_credit_percentage_is_omitted() {
    let usage = parse_with_seat(
        r#"{
                "copilot_plan": "business",
                "quota_snapshots": {
                    "premium_interactions": {
                        "credits_used": 1e308
                    }
                }
            }"#,
        Some(1e-308),
    )
    .unwrap();

    assert!(seat_window(&usage).is_none());
}

#[test]
fn invalid_seat_allowance_keeps_credit_progress_unknown() {
    let usage = parse_with_seat(
        r#"{
                "copilot_plan": "business",
                "token_based_billing": true,
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 0,
                        "remaining": 0,
                        "credits_used": 50
                    }
                }
            }"#,
        Some(0.0),
    )
    .unwrap();

    assert!(usage.primary.is_informational);
    assert!(seat_window(&usage).is_none());
}

#[test]
fn zero_entitlement_business_seat_surfaces_credits_counter() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "business",
                "token_based_billing": true,
                "quota_reset_date": "2026-06-01",
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 0,
                        "remaining": 0,
                        "percent_remaining": 100,
                        "quota_id": "premium_interactions",
                        "credits_used": 1234
                    }
                }
            }"#,
    );
    // Not an error anymore: informational counter row without a fake bar.
    assert!(usage.primary.is_informational);
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("1234 AI credits used")
    );
    assert!(usage.primary.resets_at.is_some());
    assert_eq!(usage.login_method.as_deref(), Some("Copilot Business"));
}

#[test]
fn placeholder_snapshot_still_carries_its_credits_counter() {
    // Upstream carriesCreditsCounter: a placeholder cannot become a window,
    // but its absolute counter is real consumption and must survive.
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "business",
                "token_based_billing": true,
                "quota_snapshots": {
                    "premium_interactions": {
                        "entitlement": 0,
                        "remaining": 0,
                        "percent_remaining": 0,
                        "quota_id": "",
                        "placeholder": true,
                        "credits_used": 42.5
                    }
                }
            }"#,
    );
    assert!(usage.primary.is_informational);
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("42.50 AI credits used")
    );
}

#[test]
fn premium_credits_counter_wins_over_chat() {
    let usage = parse_snapshot(
        r#"{
                "copilot_plan": "pro",
                "quota_snapshots": {
                    "chat": {
                        "entitlement": 100,
                        "remaining": 75,
                        "percent_remaining": 75,
                        "quota_id": "chat",
                        "credits_used": 1
                    },
                    "premium_interactions": {
                        "entitlement": 300,
                        "remaining": 240,
                        "percent_remaining": 80,
                        "quota_id": "premium_interactions",
                        "credits_used": 7
                    }
                }
            }"#,
    );
    let credits_row = usage
        .extra_rate_windows
        .iter()
        .find(|w| w.id == "ai-credits")
        .expect("ai-credits row");
    assert_eq!(
        credits_row.window.reset_description.as_deref(),
        Some("7 AI credits used")
    );
    // Windows still render normally next to the counter.
    assert!(credits_row.window.is_informational);
    assert_used(&usage.primary, 20.0);
}

#[test]
fn business_seat_without_credits_keeps_existing_error() {
    let err = parse_snapshot_result(
        r#"{
                "copilot_plan": "business",
                "token_based_billing": true
            }"#,
    );
    assert!(err.is_err());
}
