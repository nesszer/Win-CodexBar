//! Schema-v1 parsing for `cswap --list --json` and `cswap --switch-to --json`.
//!
//! Parsing is intentionally strict: an unsupported schema, a malformed row, or
//! disagreeing active-account fields fail the whole payload instead of leaking
//! a partial snapshot.

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::sanitize::{MAX_DIAGNOSTIC_CHARS, MAX_LABEL_CHARS, sanitize_display};
use super::{
    ClaudeSwapAccountList, ClaudeSwapAccountRow, ClaudeSwapError, ClaudeSwapHistoricalUsage,
    ClaudeSwapScopedWindow, ClaudeSwapSpendWindow, ClaudeSwapSwitchResult,
    ClaudeSwapUsageMeasurement, ClaudeSwapUsageStatus, ClaudeSwapUsageWindow,
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

fn parse_spend(raw: Option<&Value>) -> Option<ClaudeSwapSpendWindow> {
    let object = raw?.as_object()?;
    let used = object.get("used").and_then(finite_number)?;
    let limit = object.get("limit").and_then(finite_number)?;
    let used_percent = object.get("pct").and_then(finite_number)?;
    if used < 0.0 || limit <= 0.0 {
        return None;
    }
    Some(ClaudeSwapSpendWindow {
        used,
        limit,
        used_percent: used_percent.clamp(0.0, 100.0),
        currency_code: non_empty_display_string(object.get("currency")),
        resets_at: match object.get("resetsAt") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => parse_timestamp(text),
            Some(_) => None,
        },
    })
}

/// History is additive evidence. A malformed historical window or spend value
/// is dropped without invalidating the row's valid live usage.
fn parse_last_good_usage(
    object: &serde_json::Map<String, Value>,
    slot: u32,
) -> Option<ClaudeSwapHistoricalUsage> {
    let raw = object.get("lastGoodUsage")?.as_object()?;
    let fetched_at = object
        .get("lastGoodFetchedAt")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)?;
    let measurement = ClaudeSwapUsageMeasurement {
        five_hour: parse_window(raw.get("fiveHour"), slot, "lastGoodUsage.fiveHour")
            .ok()
            .flatten(),
        seven_day: parse_window(raw.get("sevenDay"), slot, "lastGoodUsage.sevenDay")
            .ok()
            .flatten(),
        scoped: parse_scoped(raw.get("scoped")),
        spend: parse_spend(raw.get("spend")),
    };
    (!measurement.is_empty()).then_some(ClaudeSwapHistoricalUsage {
        measurement,
        fetched_at,
    })
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
    let usage_measurement = ClaudeSwapUsageMeasurement {
        five_hour: parse_window(usage.and_then(|u| u.get("fiveHour")), number, "fiveHour")?,
        seven_day: parse_window(usage.and_then(|u| u.get("sevenDay")), number, "sevenDay")?,
        scoped: parse_scoped(usage.and_then(|u| u.get("scoped"))),
        spend: parse_spend(usage.and_then(|u| u.get("spend"))),
    };
    Ok(ClaudeSwapAccountRow {
        number,
        email: trimmed_non_empty(object.get("email")),
        organization_name: trimmed_non_empty(object.get("organizationName")),
        alias: non_empty_display_string(object.get("alias")),
        is_active,
        usage_status: ClaudeSwapUsageStatus::from_raw(raw_status),
        usage: usage_measurement,
        usage_fetched_at: object
            .get("usageFetchedAt")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
        is_disabled: object
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        historical_usage: parse_last_good_usage(object, number),
    })
}

/// Strictly parse the schema-v1 `cswap --list --json` envelope.
/// Parse a schema-v1 object and surface a reported `error` envelope before
/// any command-specific field is read.
fn parse_envelope(raw: &str) -> Result<serde_json::Map<String, Value>, ClaudeSwapError> {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(raw) else {
        return Err(ClaudeSwapError::NotJsonObject);
    };

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
    Ok(object)
}

pub fn parse_account_list(raw: &str) -> Result<ClaudeSwapAccountList, ClaudeSwapError> {
    let object = parse_envelope(raw)?;

    let raw_accounts = object
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or_else(|| ClaudeSwapError::MalformedShape("missing accounts array".to_string()))?;
    let active_field = object.get("activeAccountNumber").ok_or_else(|| {
        ClaudeSwapError::MalformedShape("missing activeAccountNumber".to_string())
    })?;
    let supports_account_switching = match object.get("supportsAccountSwitching") {
        None => true,
        Some(Value::Bool(supported)) => *supported,
        Some(_) => {
            return Err(ClaudeSwapError::MalformedShape(
                "supportsAccountSwitching is not a boolean".to_string(),
            ));
        }
    };
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
        supports_account_switching,
    })
}

/// Strictly parse the schema-v1 `cswap --switch-to <slot> --json` envelope.
pub fn parse_switch_result(raw: &str) -> Result<ClaudeSwapSwitchResult, ClaudeSwapError> {
    let object = parse_envelope(raw)?;

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
#[path = "parser_tests.rs"]
mod tests;
