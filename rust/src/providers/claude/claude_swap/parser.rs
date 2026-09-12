//! Schema-v1 parsing for `cswap --list --json` and `cswap --switch-to --json`.
//!
//! Parsing is intentionally strict: an unsupported schema, a malformed row, or
//! disagreeing active-account fields fail the whole payload instead of leaking
//! a partial snapshot.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::sanitize::{MAX_DIAGNOSTIC_CHARS, MAX_LABEL_CHARS, sanitize_display};
use super::{
    ClaudeSwapAccountList, ClaudeSwapAccountRow, ClaudeSwapError, ClaudeSwapScopedWindow,
    ClaudeSwapSwitchResult, ClaudeSwapUsageStatus, ClaudeSwapUsageWindow,
};

fn as_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value.as_object()
}

fn finite_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

fn non_negative_slot(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|number| {
        if number == 0 {
            None
        } else {
            u32::try_from(number).ok()
        }
    })
}

fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn trimmed_non_empty(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|text| sanitize_display(text, MAX_LABEL_CHARS))
        .unwrap_or_default()
}

fn non_empty_display_string(value: Option<&Value>) -> Option<String> {
    let text = trimmed_non_empty(value);
    if text.is_empty() { None } else { Some(text) }
}

fn parse_window(
    raw: Option<&Value>,
    slot: u32,
    name: &str,
) -> Result<Option<ClaudeSwapUsageWindow>, ClaudeSwapError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let object = as_object(raw).ok_or_else(|| {
        ClaudeSwapError::MalformedShape(format!("slot {slot} {name} window is not an object"))
    })?;
    let percent = object.get("pct").and_then(finite_number).ok_or_else(|| {
        ClaudeSwapError::MalformedShape(format!(
            "slot {slot} {name} percent is not a finite number"
        ))
    })?;
    let resets_at = match object.get("resetsAt") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(parse_timestamp(text).ok_or_else(|| {
            ClaudeSwapError::MalformedShape(format!(
                "slot {slot} {name} resetsAt is not a timestamp"
            ))
        })?),
        Some(_) => {
            return Err(ClaudeSwapError::MalformedShape(format!(
                "slot {slot} {name} resetsAt is not a timestamp"
            )));
        }
    };
    Ok(Some(ClaudeSwapUsageWindow {
        used_percent: percent.clamp(0.0, 100.0),
        resets_at,
    }))
}

/// `usage.scoped` is additive schema-v1 data. Malformed or future scope rows are
/// ignored so they cannot suppress otherwise valid account-wide usage.
fn parse_scoped(raw: Option<&Value>) -> Vec<ClaudeSwapScopedWindow> {
    let Some(rows) = raw.and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let object = row.as_object()?;
            let name = object
                .get("name")
                .map(|value| trimmed_non_empty(Some(value)))?;
            if name.is_empty() {
                return None;
            }
            let percent = object.get("pct").and_then(finite_number)?;
            let resets_at = match object.get("resetsAt") {
                None | Some(Value::Null) => None,
                Some(Value::String(text)) => Some(parse_timestamp(text)?),
                Some(_) => return None,
            };
            Some(ClaudeSwapScopedWindow {
                name,
                used_percent: percent.clamp(0.0, 100.0),
                resets_at,
            })
        })
        .collect()
}

fn parse_row(
    object: &serde_json::Map<String, Value>,
) -> Result<ClaudeSwapAccountRow, ClaudeSwapError> {
    let number = object
        .get("number")
        .and_then(non_negative_slot)
        .ok_or_else(|| {
            ClaudeSwapError::MalformedShape("account row has no numeric slot".to_string())
        })?;
    let is_active = object
        .get("active")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ClaudeSwapError::MalformedShape(format!("slot {number} has no active flag"))
        })?;
    let raw_status = object
        .get("usageStatus")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ClaudeSwapError::MalformedShape(format!("slot {number} has no usageStatus"))
        })?;
    let usage = object.get("usage").and_then(Value::as_object);
    Ok(ClaudeSwapAccountRow {
        number,
        email: trimmed_non_empty(object.get("email")),
        organization_name: trimmed_non_empty(object.get("organizationName")),
        alias: non_empty_display_string(object.get("alias")),
        is_active,
        usage_status: ClaudeSwapUsageStatus::from_raw(raw_status),
        five_hour: parse_window(usage.and_then(|u| u.get("fiveHour")), number, "fiveHour")?,
        seven_day: parse_window(usage.and_then(|u| u.get("sevenDay")), number, "sevenDay")?,
        scoped: parse_scoped(usage.and_then(|u| u.get("scoped"))),
        usage_fetched_at: object
            .get("usageFetchedAt")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
    })
}

/// Strictly parse the schema-v1 `cswap --list --json` envelope.
pub fn parse_account_list(raw: &str) -> Result<ClaudeSwapAccountList, ClaudeSwapError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| ClaudeSwapError::NotJsonObject)?;
    let object = value.as_object().ok_or(ClaudeSwapError::NotJsonObject)?;

    let schema_version = object
        .get("schemaVersion")
        .and_then(Value::as_i64)
        .ok_or(ClaudeSwapError::MissingSchemaVersion)?;
    if schema_version != 1 {
        return Err(ClaudeSwapError::UnsupportedSchemaVersion(schema_version));
    }
    if let Some(error) = object.get("error").and_then(Value::as_object) {
        let kind = sanitize_display(
            error.get("type").and_then(Value::as_str).unwrap_or("Error"),
            MAX_LABEL_CHARS,
        );
        let message = sanitize_display(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error"),
            MAX_DIAGNOSTIC_CHARS,
        );
        return Err(ClaudeSwapError::ReportedError {
            kind: if kind.is_empty() {
                "Error".to_string()
            } else {
                kind
            },
            message: if message.is_empty() {
                "unknown error".to_string()
            } else {
                message
            },
        });
    }

    let raw_accounts = object
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or_else(|| ClaudeSwapError::MalformedShape("missing accounts array".to_string()))?;
    let active_field = object.get("activeAccountNumber").ok_or_else(|| {
        ClaudeSwapError::MalformedShape("missing activeAccountNumber".to_string())
    })?;
    let active_account_number = match active_field {
        Value::Null => None,
        Value::Number(_) => Some(non_negative_slot(active_field).ok_or_else(|| {
            ClaudeSwapError::MalformedShape(
                "activeAccountNumber is not a numeric slot or null".to_string(),
            )
        })?),
        _ => {
            return Err(ClaudeSwapError::MalformedShape(
                "activeAccountNumber is not a numeric slot or null".to_string(),
            ));
        }
    };

    let mut seen = std::collections::HashSet::new();
    let mut accounts = Vec::with_capacity(raw_accounts.len());
    for raw_row in raw_accounts {
        let object = raw_row.as_object().ok_or_else(|| {
            ClaudeSwapError::MalformedShape("account row is not an object".to_string())
        })?;
        let account = parse_row(object)?;
        if !seen.insert(account.number) {
            return Err(ClaudeSwapError::MalformedShape(format!(
                "duplicate account slot {}",
                account.number
            )));
        }
        accounts.push(account);
    }

    let active_slots = accounts
        .iter()
        .filter(|account| account.is_active)
        .map(|account| account.number)
        .collect::<Vec<_>>();
    let expected_active = active_account_number
        .map(|slot| vec![slot])
        .unwrap_or_default();
    if active_slots != expected_active {
        return Err(ClaudeSwapError::MalformedShape(
            "active account fields disagree".to_string(),
        ));
    }

    Ok(ClaudeSwapAccountList {
        active_account_number,
        accounts,
    })
}

/// Strictly parse the schema-v1 `cswap --switch-to <slot> --json` envelope.
pub fn parse_switch_result(raw: &str) -> Result<ClaudeSwapSwitchResult, ClaudeSwapError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| ClaudeSwapError::NotJsonObject)?;
    let object = value.as_object().ok_or(ClaudeSwapError::NotJsonObject)?;

    let schema_version = object
        .get("schemaVersion")
        .and_then(Value::as_i64)
        .ok_or(ClaudeSwapError::MissingSchemaVersion)?;
    if schema_version != 1 {
        return Err(ClaudeSwapError::UnsupportedSchemaVersion(schema_version));
    }
    if let Some(error) = object.get("error").and_then(Value::as_object) {
        let kind = sanitize_display(
            error.get("type").and_then(Value::as_str).unwrap_or("Error"),
            MAX_LABEL_CHARS,
        );
        let message = sanitize_display(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error"),
            MAX_DIAGNOSTIC_CHARS,
        );
        return Err(ClaudeSwapError::ReportedError {
            kind: if kind.is_empty() {
                "Error".to_string()
            } else {
                kind
            },
            message: if message.is_empty() {
                "unknown error".to_string()
            } else {
                message
            },
        });
    }

    let switched = object
        .get("switched")
        .and_then(Value::as_bool)
        .ok_or_else(|| ClaudeSwapError::MalformedShape("missing switched flag".to_string()))?;
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .map(|text| sanitize_display(text, MAX_DIAGNOSTIC_CHARS))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ClaudeSwapError::MalformedShape("missing reason".to_string()))?;

    let from_account_number = parse_switch_slot(object.get("from"), "from", true)?;
    let to_account_number = parse_switch_slot(object.get("to"), "to", false)?.ok_or_else(|| {
        ClaudeSwapError::MalformedShape("to account has no numeric slot".to_string())
    })?;

    Ok(ClaudeSwapSwitchResult {
        switched,
        from_account_number,
        to_account_number,
        reason,
    })
}

fn parse_switch_slot(
    raw: Option<&Value>,
    field: &str,
    allows_null: bool,
) -> Result<Option<u32>, ClaudeSwapError> {
    match raw {
        Some(Value::Null) if allows_null => Ok(None),
        Some(value) => {
            let object = value.as_object().ok_or_else(|| {
                ClaudeSwapError::MalformedShape(format!("missing {field} account"))
            })?;
            match object.get("number") {
                Some(Value::Null) if allows_null => Ok(None),
                Some(number) => non_negative_slot(number).map(Some).ok_or_else(|| {
                    ClaudeSwapError::MalformedShape(format!(
                        "{field} account number is not a positive slot"
                    ))
                }),
                None => Err(ClaudeSwapError::MalformedShape(format!(
                    "{field} account has no number"
                ))),
            }
        }
        None => {
            if allows_null {
                Ok(None)
            } else {
                Err(ClaudeSwapError::MalformedShape(format!(
                    "missing {field} account"
                )))
            }
        }
    }
}

/// The switch envelope must confirm the exact slot CodexBar requested.
pub fn validate_switch_target(
    requested: u32,
    parsed: &ClaudeSwapSwitchResult,
) -> Result<(), ClaudeSwapError> {
    if parsed.to_account_number != requested {
        return Err(ClaudeSwapError::MismatchedTarget {
            expected: requested,
            actual: parsed.to_account_number,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        assert_eq!(first.five_hour.as_ref().unwrap().used_percent, 100.0);
        assert!(first.five_hour.as_ref().unwrap().resets_at.is_some());
        assert_eq!(first.scoped[0].name, "Fable only");
        assert_eq!(first.usage_status, ClaudeSwapUsageStatus::Ok);
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
        let scoped = &parsed.accounts[0].scoped;
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].name, "Fable only");
        assert!(parsed.accounts[0].five_hour.is_some());
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
}
