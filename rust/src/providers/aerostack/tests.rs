use super::*;
use serde_json::json;

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

fn with_tokens(tokens: Value) -> Value {
    json!({ "usage": { "ai_tokens": tokens } })
}

fn assert_unrecognized(result: Result<ProviderFetchResult, ProviderError>) {
    match result {
        Err(ProviderError::Other(message)) => assert_eq!(message, UNRECOGNIZED),
        other => panic!("expected the unrecognized-response error, got {other:?}"),
    }
}

#[test]
fn full_payload_matches_upstream_rows() {
    let result = parse_usage(&json!({
        "usage": { "ai_tokens": { "used": 1_200, "limit": 5_000 } },
        "period": "2026-03",
        "tier": " Pro ",
    }))
    .unwrap();

    assert_eq!(
        rows(&result),
        vec![
            row("Tokens used", "1,200"),
            row("Allowance", "5,000"),
            row("Remaining", "3,800"),
            row("Period", "2026-03"),
        ]
    );
    assert!((result.usage.primary.used_percent - 24.0).abs() < 1e-9);
    assert!(!result.usage.primary.hidden_on_card);
    assert_eq!(result.usage.login_method.as_deref(), Some("Pro"));
}

#[test]
fn usage_above_allowance_adds_a_row_and_clamps_the_bar() {
    let result = parse_usage(&with_tokens(json!({ "used": 6_000, "limit": 5_000 }))).unwrap();
    assert_eq!(
        rows(&result),
        vec![
            row("Tokens used", "6,000"),
            row("Allowance", "5,000"),
            row("Remaining", "0"),
            row("Above allowance", "1,000"),
        ]
    );
    assert!((result.usage.primary.used_percent - 100.0).abs() < 1e-9);
}

#[test]
fn a_single_counter_or_zero_allowance_draws_no_bar() {
    for tokens in [
        json!({ "used": 1_000 }),
        json!({ "limit": 5_000, "used": null }),
        json!({ "used": 10, "limit": 0 }),
    ] {
        let result = parse_usage(&with_tokens(tokens)).unwrap();
        assert!(result.usage.primary.hidden_on_card);
    }
    assert_eq!(
        rows(&parse_usage(&with_tokens(json!({ "used": 1_000 }))).unwrap()),
        vec![row("Tokens used", "1,000")]
    );
}

#[test]
fn rejects_shapes_upstream_rejects() {
    assert_unrecognized(parse_usage(&json!({})));
    assert_unrecognized(parse_usage(&json!([])));
    assert_unrecognized(parse_usage(&json!({ "usage": { "ai_tokens": [] } })));
    assert_unrecognized(parse_usage(&with_tokens(json!({}))));
    assert_unrecognized(parse_usage(&with_tokens(
        json!({ "used": null, "limit": null }),
    )));
    assert_unrecognized(parse_usage(&with_tokens(json!({ "used": -1, "limit": 5 }))));
    assert_unrecognized(parse_usage(&with_tokens(
        json!({ "used": 1.5, "limit": 5 }),
    )));
    assert_unrecognized(parse_usage(&with_tokens(
        json!({ "used": "1", "limit": 5 }),
    )));
    assert_unrecognized(parse_usage(&with_tokens(
        json!({ "used": 9_007_199_254_740_992_u64 }),
    )));
}

#[test]
fn integral_floats_count_as_integers() {
    let result = parse_usage(&with_tokens(json!({ "used": 5.0, "limit": 10.0 }))).unwrap();
    assert!((result.usage.primary.used_percent - 50.0).abs() < 1e-9);

    let result = parse_usage(&with_tokens(json!({ "used": -0.0, "limit": 10 }))).unwrap();
    assert!(result.usage.primary.used_percent.abs() < 1e-9);
}

#[test]
fn period_must_be_a_calendar_month_when_present() {
    let with_period = |period: Value| {
        parse_usage(&json!({
            "usage": { "ai_tokens": { "used": 1, "limit": 2 } },
            "period": period,
        }))
    };
    assert!(with_period(json!(null)).is_ok());
    assert!(with_period(json!("2026-12")).is_ok());
    for bad in [
        json!("2026-13"),
        json!("2026-00"),
        json!("2026-3"),
        json!("2026/03"),
        json!("26-03"),
        json!(202603),
    ] {
        assert_unrecognized(with_period(bad));
    }
}

#[test]
fn tier_must_be_a_short_string_when_present() {
    let with_tier = |tier: Value| {
        parse_usage(&json!({
            "usage": { "ai_tokens": { "used": 1, "limit": 2 } },
            "tier": tier,
        }))
    };
    assert_eq!(with_tier(json!(null)).unwrap().usage.login_method, None);
    assert_eq!(with_tier(json!("   ")).unwrap().usage.login_method, None);
    assert!(with_tier(json!("x".repeat(120))).is_ok());
    assert_unrecognized(with_tier(json!("x".repeat(121))));
    assert_unrecognized(with_tier(json!(5)));
}
