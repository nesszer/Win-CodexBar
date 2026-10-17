use super::*;
use serde_json::json;

fn fixture() -> Value {
    json!({
        "credits": 1034,
        "plan_credits": 997,
        "purchased_credits": 37,
        "is_free_tier": true,
        "credits_reset_at": "2026-11-04T12:00:00Z",
        "total_requests": 3,
        "api_key": "private-response-key",
        "email": "private-response@example.test",
        "github_username": "private-response-user",
        "last_login_method": "private-response-login"
    })
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String)> {
    result
        .display_details
        .iter()
        .map(|d| {
            assert_eq!(d.section_title(), Some(SECTION_TITLE));
            (d.title().to_string(), d.value().to_string())
        })
        .collect()
}

fn row(label: &str, value: &str) -> (String, String) {
    (label.to_string(), value.to_string())
}

fn assert_unrecognized(result: Result<ProviderFetchResult, ProviderError>) {
    match result {
        Err(ProviderError::Other(message)) => assert_eq!(message, UNRECOGNIZED),
        other => panic!("expected the unrecognized-response error, got {other:?}"),
    }
}

#[test]
fn free_tier_preserves_separate_balances_and_supplied_reset() {
    let result = parse_usage(&fixture()).unwrap();

    assert!(result.usage.primary.hidden_on_card);
    assert_eq!(result.usage.login_method.as_deref(), Some("Free tier"));
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("Available credits: 1,034")
    );
    assert_eq!(
        rows(&result),
        vec![
            row("Available credits", "1,034"),
            row("Plan credits", "997"),
            row("Purchased credits", "37"),
            row("Monthly reset", "2026-11-04 12:00:00 UTC"),
        ]
    );
}

#[test]
fn pay_as_you_go_keeps_zero_credits_and_no_reset() {
    let result = parse_usage(&json!({
        "credits": 0,
        "plan_credits": 0,
        "purchased_credits": 0,
        "is_free_tier": false
    }))
    .unwrap();

    assert_eq!(result.usage.login_method.as_deref(), Some("Pay as you go"));
    assert_eq!(
        rows(&result),
        vec![
            row("Available credits", "0"),
            row("Plan credits", "0"),
            row("Purchased credits", "0"),
        ]
    );
}

#[test]
fn missing_account_fields_stay_unavailable_without_inferred_balance() {
    let result = parse_usage(&json!({
        "purchased_credits": 37,
        "plan_credits": null,
        "credits_reset_at": null
    }))
    .unwrap();

    assert_eq!(rows(&result), vec![row("Purchased credits", "37")]);
    assert_eq!(result.usage.login_method.as_deref(), Some("API"));
}

#[test]
fn free_tier_without_a_reset_does_not_invent_one() {
    let result = parse_usage(&json!({
        "credits": 37.25,
        "is_free_tier": true,
        "credits_reset_at": null
    }))
    .unwrap();

    assert_eq!(rows(&result), vec![row("Available credits", "37.25")]);
}

#[test]
fn offset_resets_normalize_to_utc() {
    let result = parse_usage(&json!({
        "credits": 1034,
        "plan_credits": 997,
        "purchased_credits": 37,
        "is_free_tier": true,
        "credits_reset_at": "2026-11-04T14:00:00.125+02:00"
    }))
    .unwrap();

    let rows = rows(&result);
    assert_eq!(rows.last().unwrap().1, "2026-11-04 12:00:00.125 UTC");
}

#[test]
fn malformed_supplied_balances_fail() {
    for value in [json!(-1), json!(true), json!([]), json!({}), json!("37")] {
        let result = parse_usage(&json!({ "credits": value, "is_free_tier": true }));
        assert_unrecognized(result);
    }

    for value in [json!(-1), json!(true), json!([]), json!({}), json!("97")] {
        let result = parse_usage(&json!({ "plan_credits": value, "is_free_tier": true }));
        assert_unrecognized(result);
    }

    for value in [json!(-1), json!(true), json!([]), json!({}), json!("37")] {
        let result = parse_usage(&json!({ "purchased_credits": value, "is_free_tier": true }));
        assert_unrecognized(result);
    }
}

#[test]
fn invalid_account_shapes_and_dates_produce_safe_parse_failures() {
    for body in [
        json!({}),
        json!([]),
        json!({"credits": 1, "is_free_tier": "true"}),
        json!({"credits": 1, "is_free_tier": true, "credits_reset_at": 123}),
        json!({"credits": 1, "is_free_tier": true, "credits_reset_at": "2026-11-04"}),
        json!({"credits": 1, "is_free_tier": true, "credits_reset_at": "2026-99-99T12:00:00Z"}),
        json!({"credits": 1, "is_free_tier": true, "credits_reset_at": "2026-02-31T12:00:00Z"}),
    ] {
        let result = parse_usage(&body);
        assert_unrecognized(result);
    }
}

#[test]
fn integral_floats_count_as_credits() {
    let result = parse_usage(&json!({
        "credits": 5.0,
        "plan_credits": 10.0
    }))
    .unwrap();

    assert_eq!(
        rows(&result),
        vec![row("Available credits", "5"), row("Plan credits", "10"),]
    );
}

#[test]
fn negative_zero_becomes_positive_zero() {
    let result = parse_usage(&json!({
        "credits": -0.0,
        "plan_credits": 10
    }))
    .unwrap();

    // -0.0 should be treated as 0.0
    assert_eq!(
        rows(&result),
        vec![row("Available credits", "0"), row("Plan credits", "10"),]
    );
}

#[test]
fn fractional_credits_preserve_decimals() {
    let result = parse_usage(&json!({
        "credits": 1034.5,
        "plan_credits": 997.123456,
        "purchased_credits": 37.1
    }))
    .unwrap();

    assert_eq!(
        rows(&result),
        vec![
            row("Available credits", "1,034.5"),
            row("Plan credits", "997.123456"),
            row("Purchased credits", "37.1"),
        ]
    );
}

#[test]
fn at_least_one_balance_is_required() {
    assert_unrecognized(parse_usage(&json!({})));
    assert_unrecognized(parse_usage(&json!({"is_free_tier": true})));
    assert_unrecognized(parse_usage(&json!({
        "credits": null,
        "plan_credits": null,
        "purchased_credits": null
    })));
}

#[test]
fn eligibility_must_be_boolean_when_present() {
    assert!(parse_usage(&json!({ "credits": 1, "is_free_tier": null })).is_ok());
    assert!(parse_usage(&json!({ "credits": 1, "is_free_tier": true })).is_ok());
    assert!(parse_usage(&json!({ "credits": 1, "is_free_tier": false })).is_ok());
    assert_unrecognized(parse_usage(
        &json!({ "credits": 1, "is_free_tier": "true" }),
    ));
    assert_unrecognized(parse_usage(&json!({ "credits": 1, "is_free_tier": 1 })));
}

#[test]
fn response_secrets_are_never_exposed() {
    let result = parse_usage(&fixture()).unwrap();
    let display_str = format!("{:?}", result);
    assert!(!display_str.contains("private-response"));
}

#[test]
fn datetime_with_negative_offset_converts_correctly() {
    let result = parse_usage(&json!({
        "credits": 1,
        "is_free_tier": true,
        "credits_reset_at": "2026-11-04T10:00:00-02:00"
    }))
    .unwrap();

    let rows = rows(&result);
    assert_eq!(rows[1].1, "2026-11-04 12:00:00 UTC");
}

#[test]
fn datetime_with_utc_z_suffix_normalizes() {
    let result = parse_usage(&json!({
        "credits": 1,
        "is_free_tier": true,
        "credits_reset_at": "2026-11-04T12:00:00Z"
    }))
    .unwrap();

    let rows = rows(&result);
    assert_eq!(rows[1].1, "2026-11-04 12:00:00 UTC");
}

#[test]
fn fractional_seconds_are_preserved() {
    let result = parse_usage(&json!({
        "credits": 1,
        "is_free_tier": true,
        "credits_reset_at": "2026-11-04T12:00:00.123Z"
    }))
    .unwrap();

    let rows = rows(&result);
    assert_eq!(rows[1].1, "2026-11-04 12:00:00.123 UTC");
}

#[test]
fn invalid_datetimes_fail() {
    for dt_str in [
        "2026-11-04",           // missing time
        "2026-11-04T12:00:00",  // missing timezone
        "2026-13-04T12:00:00Z", // invalid month
        "2026-11-31T12:00:00Z", // invalid day (November has 30)
        "2026-02-31T12:00:00Z", // invalid day (February)
        "not-a-date",
    ] {
        assert_unrecognized(parse_usage(&json!({
            "credits": 1,
            "is_free_tier": true,
            "credits_reset_at": dt_str
        })));
    }
}

#[test]
fn pay_as_you_go_does_not_show_reset() {
    let result = parse_usage(&json!({
        "credits": 100,
        "is_free_tier": false,
        "credits_reset_at": "2026-11-04T12:00:00Z"
    }))
    .unwrap();

    assert_eq!(rows(&result), vec![row("Available credits", "100")]);
}

#[test]
fn api_eligibility_does_not_show_reset() {
    let result = parse_usage(&json!({
        "credits": 100,
        "is_free_tier": null,
        "credits_reset_at": "2026-11-04T12:00:00Z"
    }))
    .unwrap();

    assert_eq!(rows(&result), vec![row("Available credits", "100")]);
}

#[test]
fn reset_crossing_month_and_year_boundaries_normalizes_to_utc() {
    for (reset, expected) in [
        ("2026-12-31T23:30:00-01:00", "2027-01-01 00:30:00 UTC"),
        ("2026-03-01T01:00:00+02:00", "2026-02-28 23:00:00 UTC"),
        ("2028-03-01T00:30:00+01:00", "2028-02-29 23:30:00 UTC"),
    ] {
        let result =
            parse_usage(&json!({"credits": 1, "is_free_tier": true, "credits_reset_at": reset}))
                .unwrap();
        assert_eq!(rows(&result).last().unwrap().1, expected, "{reset}");
    }
}

#[test]
fn reset_fraction_keeps_milliseconds_like_to_iso_string() {
    for (reset, expected) in [
        ("2026-11-04T12:00:00.000Z", "2026-11-04 12:00:00 UTC"),
        ("2026-11-04T12:00:00.5Z", "2026-11-04 12:00:00.500 UTC"),
        ("2026-11-04T12:00:00.123456Z", "2026-11-04 12:00:00.123 UTC"),
    ] {
        let result =
            parse_usage(&json!({"credits": 1, "is_free_tier": true, "credits_reset_at": reset}))
                .unwrap();
        assert_eq!(rows(&result).last().unwrap().1, expected, "{reset}");
    }
}

#[test]
fn malformed_reset_fails_even_when_not_shown() {
    for eligible in [json!(false), Value::Null] {
        assert_unrecognized(parse_usage(&json!({
            "credits": 1,
            "is_free_tier": eligible,
            "credits_reset_at": "2026-02-31T12:00:00Z"
        })));
    }
}

#[test]
fn only_a_rejected_key_reports_an_expired_session() {
    let provider = SofyaProvider::new();
    let kind = |status| provider.error_state_kind(&ProviderError::Other(status_message(status)));
    assert_eq!(status_message(401), "Sofya returned HTTP 401.");
    assert_eq!(kind(401), ProviderStateKind::ExpiredSession);
    for status in [403, 429, 503, 418] {
        assert_eq!(kind(status), ProviderStateKind::Unknown, "status {status}");
    }
}
