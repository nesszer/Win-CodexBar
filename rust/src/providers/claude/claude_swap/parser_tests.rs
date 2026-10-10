use super::*;
use serde_json::json;

fn list_fixture() -> Value {
    json!({
        "schemaVersion": 1,
        "activeAccountNumber": 2,
        "accounts": [
            {
                "number": 1,
                "email": "same@example.com",
                "organizationName": "Work",
                "active": false,
                "usageStatus": "ok",
                "usage": {
                    "fiveHour": { "pct": 120.0, "resetsAt": "2026-09-12T01:00:00Z" },
                    "sevenDay": { "pct": 18.0 },
                    "scoped": [{ "name": "Fable only", "pct": 4.0 }]
                },
                "usageFetchedAt": "2026-09-12T00:30:00.000Z"
            },
            {
                "number": 2,
                "email": "same@example.com",
                "organizationName": "Personal",
                "active": true,
                "usageStatus": "ok",
                "usage": { "fiveHour": { "pct": 81.0 }, "sevenDay": { "pct": 18.0 } }
            },
            {
                "number": 3,
                "email": "expired@example.com",
                "organizationName": "",
                "alias": "Backup",
                "active": false,
                "usageStatus": "token_expired"
            }
        ]
    })
}

#[test]
fn parses_schema_v1_and_normalizes_windows() {
    let parsed = parse_account_list(&list_fixture().to_string()).unwrap();
    assert_eq!(parsed.active_account_number, Some(2));
    assert_eq!(parsed.accounts.len(), 3);
    let first = &parsed.accounts[0];
    assert_eq!(first.number, 1);
    assert_eq!(first.organization_name, "Work");
    // Out-of-range percentages are clamped like upstream.
    assert_eq!(first.usage.five_hour.as_ref().unwrap().used_percent, 100.0);
    assert!(first.usage.five_hour.as_ref().unwrap().resets_at.is_some());
    assert_eq!(first.usage.scoped[0].name, "Fable only");
    assert_eq!(first.usage_status, ClaudeSwapUsageStatus::Ok);
}

#[test]
fn switching_capability_defaults_to_true_and_accepts_booleans() {
    let parsed = parse_account_list(&list_fixture().to_string()).unwrap();
    assert!(parsed.supports_account_switching);
    for supported in [true, false] {
        let mut fixture = list_fixture();
        fixture["supportsAccountSwitching"] = json!(supported);
        let parsed = parse_account_list(&fixture.to_string()).unwrap();
        assert_eq!(parsed.supports_account_switching, supported);
    }
}

#[test]
fn rejects_non_boolean_switching_capability() {
    for value in [
        json!(null),
        json!(0),
        json!(1),
        json!("false"),
        json!([]),
        json!({}),
    ] {
        let mut fixture = list_fixture();
        fixture["supportsAccountSwitching"] = value.clone();
        assert!(
            matches!(
                parse_account_list(&fixture.to_string()),
                Err(ClaudeSwapError::MalformedShape(ref message))
                    if message == "supportsAccountSwitching is not a boolean"
            ),
            "{value} must be rejected"
        );
    }
}

#[test]
fn rejects_unknown_and_missing_schema_versions() {
    let mut unknown = list_fixture();
    unknown["schemaVersion"] = json!(2);
    assert!(matches!(
        parse_account_list(&unknown.to_string()),
        Err(ClaudeSwapError::UnsupportedSchemaVersion(2))
    ));

    let mut missing = list_fixture();
    missing.as_object_mut().unwrap().remove("schemaVersion");
    assert!(matches!(
        parse_account_list(&missing.to_string()),
        Err(ClaudeSwapError::MissingSchemaVersion)
    ));

    assert!(matches!(
        parse_account_list("not json"),
        Err(ClaudeSwapError::NotJsonObject)
    ));
    assert!(matches!(
        parse_account_list("[]"),
        Err(ClaudeSwapError::NotJsonObject)
    ));
}

#[test]
fn unknown_status_is_not_echoed_to_the_row() {
    let mut fixture = list_fixture();
    fixture["accounts"][2]["usageStatus"] = json!("super_secret_token\u{1b}]0;leak\u{07}");
    let parsed = parse_account_list(&fixture.to_string()).unwrap();
    let row = parsed.accounts.iter().find(|a| a.number == 3).unwrap();
    assert_eq!(row.usage_status, ClaudeSwapUsageStatus::Unknown);
    assert!(!row.usage_status.as_label().contains("super_secret_token"));
}

#[test]
fn external_labels_strip_escapes_and_respect_length_bounds() {
    let hostile = format!("\u{1b}[31mEvil\u{1b}[0m\n{}", "x".repeat(400));
    let mut fixture = list_fixture();
    fixture["accounts"][2]["alias"] = json!(hostile);
    let parsed = parse_account_list(&fixture.to_string()).unwrap();
    let alias = parsed
        .accounts
        .iter()
        .find(|a| a.number == 3)
        .unwrap()
        .alias
        .as_deref()
        .unwrap();
    assert!(!alias.contains('\u{1b}'));
    assert!(alias.contains("Evil"));
    assert!(alias.chars().count() <= MAX_LABEL_CHARS);
}

#[test]
fn reported_error_envelope_is_sanitized_and_bounded() {
    let raw = json!({
        "schemaVersion": 1,
        "error": {
            "type": "\u{1b}[31mBad\u{07}",
            "message": format!("\u{1b}]0;leak\u{07}{}", "y".repeat(900))
        }
    });
    match parse_account_list(&raw.to_string()) {
        Err(ClaudeSwapError::ReportedError { kind, message }) => {
            assert!(!kind.contains('\u{1b}'));
            assert!(!message.contains('\u{1b}'));
            assert!(message.chars().count() <= MAX_DIAGNOSTIC_CHARS);
        }
        other => panic!("expected reported error, got {other:?}"),
    }
}

#[test]
fn surfaces_error_envelope_instead_of_partial_accounts() {
    let raw = json!({
        "schemaVersion": 1,
        "error": { "type": "LockHeld", "message": "another cswap is running" }
    });
    match parse_account_list(&raw.to_string()) {
        Err(ClaudeSwapError::ReportedError { kind, message }) => {
            assert_eq!(kind, "LockHeld");
            assert!(message.contains("another cswap"));
        }
        other => panic!("expected reported error, got {other:?}"),
    }
}

#[test]
fn rejects_disagreeing_active_fields_and_duplicate_slots() {
    let mut disagree = list_fixture();
    disagree["activeAccountNumber"] = json!(1);
    assert!(matches!(
        parse_account_list(&disagree.to_string()),
        Err(ClaudeSwapError::MalformedShape(_))
    ));

    let mut duplicate = list_fixture();
    {
        let accounts = duplicate["accounts"].as_array_mut().unwrap();
        accounts[1]["number"] = json!(1);
        accounts[0]["active"] = json!(false);
    }
    duplicate["activeAccountNumber"] = json!(null);
    assert!(matches!(
        parse_account_list(&duplicate.to_string()),
        Err(ClaudeSwapError::MalformedShape(_))
    ));
}

#[test]
fn ignores_malformed_scoped_rows_without_dropping_valid_windows() {
    let mut fixture = list_fixture();
    fixture["accounts"][0]["usage"]["scoped"] = json!([
        { "name": "Fable only", "pct": 4.0 },
        { "name": "", "pct": 9.0 },
        { "pct": 3.0 },
        "nonsense",
        { "name": "Broken reset", "pct": 2.0, "resetsAt": "not-a-date" }
    ]);
    let parsed = parse_account_list(&fixture.to_string()).unwrap();
    let scoped = &parsed.accounts[0].usage.scoped;
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].name, "Fable only");
    assert!(parsed.accounts[0].usage.five_hour.is_some());
}

#[test]
fn parses_spend_disabled_and_last_good_usage_as_additive_data() {
    let mut fixture = list_fixture();
    fixture["accounts"][0]["disabled"] = json!(true);
    fixture["accounts"][0]["usage"]["spend"] = json!({
        "used": 12.5,
        "limit": 50.0,
        "pct": 25.0,
        "currency": " USD ",
        "resetsAt": "2026-09-13T00:00:00Z"
    });
    fixture["accounts"][0]["lastGoodUsage"] = json!({
        "fiveHour": { "pct": 44.0 },
        "sevenDay": { "pct": 19.0 },
        "spend": { "used": 8.0, "limit": 40.0, "pct": 20.0, "currency": "EUR" }
    });
    fixture["accounts"][0]["lastGoodFetchedAt"] = json!("2026-09-12T00:45:00Z");
    fixture["accounts"][2]["usageStatus"] = json!("foreign_credential");

    let parsed = parse_account_list(&fixture.to_string()).unwrap();
    let first = &parsed.accounts[0];
    assert!(first.is_disabled);
    assert_eq!(
        first.usage.spend.as_ref().unwrap().currency_code.as_deref(),
        Some("USD")
    );
    assert_eq!(first.usage.spend.as_ref().unwrap().used, 12.5);
    let history = first.historical_usage.as_ref().unwrap();
    assert_eq!(
        history.measurement.five_hour.as_ref().unwrap().used_percent,
        44.0
    );
    assert_eq!(
        history
            .measurement
            .spend
            .as_ref()
            .unwrap()
            .currency_code
            .as_deref(),
        Some("EUR")
    );
    assert_eq!(history.fetched_at.to_rfc3339(), "2026-09-12T00:45:00+00:00");
    assert_eq!(
        parsed.accounts[2].usage_status,
        ClaudeSwapUsageStatus::ForeignCredential
    );
}

#[test]
fn drops_invalid_additive_history_and_spend_without_dropping_live_usage() {
    let mut fixture = list_fixture();
    fixture["accounts"][0]["usage"]["spend"] = json!({
        "used": -1.0,
        "limit": 50.0,
        "pct": 25.0
    });
    fixture["accounts"][0]["lastGoodUsage"] = json!({
        "fiveHour": { "pct": "not-a-number" },
        "scoped": [{ "name": "valid scope", "pct": 3.0 }]
    });
    fixture["accounts"][0]["lastGoodFetchedAt"] = json!("not-a-date");
    let parsed = parse_account_list(&fixture.to_string()).unwrap();
    let first = &parsed.accounts[0];
    assert!(first.usage.five_hour.is_some());
    assert!(first.usage.spend.is_none());
    assert!(first.historical_usage.is_none());

    fixture["accounts"][0]["lastGoodFetchedAt"] = json!("2026-09-12T00:45:00Z");
    let parsed = parse_account_list(&fixture.to_string()).unwrap();
    let history = parsed.accounts[0].historical_usage.as_ref().unwrap();
    assert!(history.measurement.five_hour.is_none());
    assert_eq!(history.measurement.scoped.len(), 1);
}

#[test]
fn missing_spend_currency_remains_unknown() {
    let mut fixture = list_fixture();
    fixture["accounts"][0]["usage"]["spend"] = json!({
        "used": 2.0,
        "limit": 20.0,
        "pct": 10.0
    });
    let parsed = parse_account_list(&fixture.to_string()).unwrap();
    assert_eq!(
        parsed.accounts[0]
            .usage
            .spend
            .as_ref()
            .unwrap()
            .currency_code,
        None
    );
}

#[test]
fn list_and_switch_share_the_envelope_checks() {
    const NOT_OBJECT: &str = "claude-swap returned output that is not a JSON object.";
    const NO_SCHEMA: &str = "claude-swap output has no schemaVersion field.";
    const SCHEMA_2: &str =
        "claude-swap output uses unsupported schema version 2; CodexBar supports version 1.";
    const UNKNOWN: &str = "claude-swap reported Error: unknown error";
    let rows: [(&str, &str, &str); 12] = [
        ("not json", NOT_OBJECT, NOT_OBJECT),
        ("[]", NOT_OBJECT, NOT_OBJECT),
        (r#""text""#, NOT_OBJECT, NOT_OBJECT),
        ("{}", NO_SCHEMA, NO_SCHEMA),
        (r#"{"schemaVersion": "1"}"#, NO_SCHEMA, NO_SCHEMA),
        (r#"{"schemaVersion": 1.5}"#, NO_SCHEMA, NO_SCHEMA),
        (r#"{"schemaVersion": 2}"#, SCHEMA_2, SCHEMA_2),
        (
            r#"{"schemaVersion": 2, "error": {"type": "LockHeld"}}"#,
            SCHEMA_2,
            SCHEMA_2,
        ),
        (r#"{"schemaVersion": 1, "error": {}}"#, UNKNOWN, UNKNOWN),
        (
            r#"{"schemaVersion": 1, "error": {"type": "", "message": ""}}"#,
            UNKNOWN,
            UNKNOWN,
        ),
        (
            r#"{"schemaVersion": 1, "error": {"type": "LockHeld", "message": "busy"}}"#,
            "claude-swap reported LockHeld: busy",
            "claude-swap reported LockHeld: busy",
        ),
        (
            r#"{"schemaVersion": 1, "error": "text"}"#,
            "claude-swap output is malformed: missing accounts array",
            "claude-swap output is malformed: missing switched flag",
        ),
    ];
    for (raw, list_expected, switch_expected) in rows {
        let list = parse_account_list(raw).unwrap_err().to_string();
        let switch = parse_switch_result(raw).unwrap_err().to_string();
        assert_eq!(list, list_expected, "{raw}");
        assert_eq!(switch, switch_expected, "{raw}");
    }
}

#[test]
fn switch_result_requires_matching_target_slot() {
    let raw = json!({
        "schemaVersion": 1,
        "switched": true,
        "from": { "number": 2 },
        "to": { "number": 3 },
        "reason": "switched"
    });
    let parsed = parse_switch_result(&raw.to_string()).unwrap();
    assert!(parsed.switched);
    assert_eq!(parsed.from_account_number, Some(2));
    assert_eq!(parsed.to_account_number, 3);
    assert!(validate_switch_target(3, &parsed).is_ok());

    let wrong = json!({
        "schemaVersion": 1,
        "switched": true,
        "from": { "number": 1 },
        "to": { "number": 2 },
        "reason": "switched"
    });
    let wrong = parse_switch_result(&wrong.to_string()).unwrap();
    assert!(matches!(
        validate_switch_target(3, &wrong),
        Err(ClaudeSwapError::MismatchedTarget {
            expected: 3,
            actual: 2
        })
    ));
}

#[test]
fn switch_result_rejects_missing_reason_and_bad_schema() {
    let missing_reason = json!({
        "schemaVersion": 1,
        "switched": true,
        "from": { "number": 1 },
        "to": { "number": 2 }
    });
    assert!(matches!(
        parse_switch_result(&missing_reason.to_string()),
        Err(ClaudeSwapError::MalformedShape(_))
    ));

    let bad_schema = json!({
        "schemaVersion": 9,
        "switched": true,
        "from": { "number": 1 },
        "to": { "number": 2 },
        "reason": "switched"
    });
    assert!(matches!(
        parse_switch_result(&bad_schema.to_string()),
        Err(ClaudeSwapError::UnsupportedSchemaVersion(9))
    ));
}
