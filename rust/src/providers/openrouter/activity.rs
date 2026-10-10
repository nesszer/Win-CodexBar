use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

use crate::core::{CostDailyPoint, CostSnapshot, ProviderError};
use crate::spend_contract::CostProvenance;

const MAX_ACTIVITY_ROWS: usize = 20_000;
/// Distinct identity rows tracked for dedupe; bounds the `seen` map.
const MAX_DISTINCT_ROWS: usize = 10_000;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Aggregate of the deduplicated, in-window Activity rows (upstream
/// `activityDetails`: Tokens = prompt + completion, Requests, distinct Models).
/// Reasoning tokens are validated but never added to `tokens` a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ActivitySummary {
    pub(super) tokens: u64,
    pub(super) requests: u64,
    pub(super) models: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ActivityReport {
    pub(super) cost: CostSnapshot,
    pub(super) summary: ActivitySummary,
}

/// Add `amount` to `total`, rejecting aggregates beyond the JS safe-integer
/// range like upstream (`Number.isSafeInteger`).
fn checked_aggregate(total: &mut u64, amount: u64) -> Result<(), ProviderError> {
    *total = total
        .checked_add(amount)
        .filter(|sum| *sum <= MAX_SAFE_INTEGER)
        .ok_or_else(|| {
            ProviderError::Parse(
                "OpenRouter Activity aggregate must be within the safe integer range".into(),
            )
        })?;
    Ok(())
}

pub(super) fn parse_activity_cost(
    payloads: &[Value],
    now: DateTime<Utc>,
) -> Result<ActivityReport, ProviderError> {
    let latest_completed = now.date_naive() - Duration::days(1);
    let cutoff = latest_completed - Duration::days(29);
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut daily: BTreeMap<String, f64> = BTreeMap::new();
    let mut total = 0.0;
    let mut estimated_total = 0.0;
    let mut tokens = 0u64;
    let mut reasoning_tokens = 0u64;
    let mut requests_total = 0u64;
    let mut models: HashSet<String> = HashSet::new();
    let mut rows_seen = 0usize;

    for payload in payloads {
        let rows = payload
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ProviderError::Parse("OpenRouter activity.data must be an array".into())
            })?;
        rows_seen = rows_seen.saturating_add(rows.len());
        if rows_seen > MAX_ACTIVITY_ROWS {
            return Err(ProviderError::Parse(
                "OpenRouter activity.data exceeds 20000 rows".into(),
            ));
        }
        for (index, row) in rows.iter().enumerate() {
            let object = row
                .as_object()
                .ok_or_else(|| row_err(index, " must be an object"))?;
            let raw_day = object
                .get("date")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| row_err(index, ".date is missing"))?;
            let day = normalize_activity_day(raw_day)
                .ok_or_else(|| row_err(index, ".date must be YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"))?;
            let parsed_day = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
                .map_err(|_| row_err(index, ".date must be a real calendar date"))?;
            if parsed_day > latest_completed {
                return Err(row_err(index, ".date must be a completed UTC day"));
            }
            if parsed_day < cutoff {
                continue;
            }
            let model = object
                .get("model_permaslug")
                .or_else(|| object.get("model"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if model.len() > 64 {
                return Err(row_err(index, ".model exceeds 64 characters"));
            }
            let prompt = nonnegative_integer(object.get("prompt_tokens"), index, "prompt_tokens")?;
            let completion =
                nonnegative_integer(object.get("completion_tokens"), index, "completion_tokens")?;
            let reasoning = match object.get("reasoning_tokens") {
                Some(Value::Null) | None => 0,
                value => nonnegative_integer(value, index, "reasoning_tokens")?,
            };
            if prompt
                .checked_add(completion)
                .is_none_or(|total| total > MAX_SAFE_INTEGER)
            {
                return Err(row_err(index, " token total overflowed"));
            }
            let requests = nonnegative_integer(object.get("requests"), index, "requests")?;
            let metered = nonnegative_number(object.get("usage"), index, "usage")?;
            let estimated = match object.get("byok_usage_inference") {
                Some(Value::Null) | None => 0.0,
                value => nonnegative_number(value, index, "byok_usage_inference")?,
            };
            let cost = metered + estimated;
            if !cost.is_finite() {
                return Err(ProviderError::Parse(
                    "OpenRouter Activity spend overflowed".into(),
                ));
            }
            let identity = format!(
                "{day}|{model}|{}|{}|{}",
                object
                    .get("endpoint_id")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                object
                    .get("provider_name")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                object
                    .get("workspace_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            );
            let signature = format!(
                "{prompt}|{completion}|{reasoning}|{requests}|{metered:.12}|{estimated:.12}"
            );
            if let Some(existing) = seen.get(&identity) {
                if existing != &signature {
                    return Err(ProviderError::Parse(
                        "OpenRouter Activity contains conflicting duplicate rows".into(),
                    ));
                }
                continue;
            }
            seen.insert(identity, signature);
            if seen.len() > MAX_DISTINCT_ROWS {
                return Err(ProviderError::Parse(
                    "OpenRouter activity.data exceeds 10000 distinct rows".into(),
                ));
            }
            // Prompt plus completion only: reasoning tokens are a separate counter.
            checked_aggregate(&mut tokens, prompt + completion)?;
            checked_aggregate(&mut reasoning_tokens, reasoning)?;
            checked_aggregate(&mut requests_total, requests)?;
            if !model.is_empty() {
                models.insert(model.to_string());
            }
            total += cost;
            estimated_total += estimated;
            *daily.entry(day.to_string()).or_default() += cost;
        }
    }

    if !total.is_finite() || !estimated_total.is_finite() {
        return Err(ProviderError::Parse(
            "OpenRouter Activity spend overflowed".into(),
        ));
    }
    // Matches upstream's plugin snapshot mapper: any BYOK estimate makes the
    // window estimated, or mixed when metered spend is also present.
    let provenance = if estimated_total > 0.0 {
        if total - estimated_total > 0.0 {
            CostProvenance::Mixed
        } else {
            CostProvenance::ListPriceEstimate
        }
    } else {
        CostProvenance::VendorMetered
    };
    let cost = CostSnapshot::new(total, "USD", "Last 30 days (UTC)")
        .with_history_tokens(tokens)
        .with_provenance(provenance)
        .with_daily(
            daily
                .into_iter()
                .map(|(day, amount)| CostDailyPoint { day, amount })
                .collect(),
        )
        .always_visible();
    Ok(ActivityReport {
        cost,
        summary: ActivitySummary {
            tokens,
            requests: requests_total,
            models: models.len(),
        },
    })
}

fn normalize_activity_day(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    let shape_ok = match bytes.len() {
        10 => true,
        19 => {
            bytes[10] == b' '
                && bytes[13] == b':'
                && bytes[16] == b':'
                && bytes[11..13].iter().all(u8::is_ascii_digit)
                && bytes[14..16].iter().all(u8::is_ascii_digit)
                && bytes[17..19].iter().all(u8::is_ascii_digit)
        }
        _ => false,
    };
    if !shape_ok
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[0..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    Some(&raw[..10])
}

fn row_err(index: usize, detail: &str) -> ProviderError {
    ProviderError::Parse(format!("OpenRouter activity.data[{index}]{detail}"))
}

fn nonnegative_integer(
    value: Option<&Value>,
    index: usize,
    field: &str,
) -> Result<u64, ProviderError> {
    let value = value.ok_or_else(|| row_err(index, &format!(".{field} is missing")))?;
    let value = value
        .as_u64()
        .ok_or_else(|| row_err(index, &format!(".{field} must be a nonnegative integer")))?;
    if value > MAX_SAFE_INTEGER {
        return Err(row_err(
            index,
            &format!(".{field} must be a nonnegative safe integer"),
        ));
    }
    Ok(value)
}

fn nonnegative_number(
    value: Option<&Value>,
    index: usize,
    field: &str,
) -> Result<f64, ProviderError> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| row_err(index, &format!(".{field} must be finite and nonnegative")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-22T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn activity_row_errors_name_the_row_index_and_field() {
        let long_model = "m".repeat(65);
        let cases = [
            (serde_json::json!(5), " must be an object"),
            (serde_json::json!({}), ".date is missing"),
            (
                serde_json::json!({"date":"2026/08/21"}),
                ".date must be YYYY-MM-DD or YYYY-MM-DD HH:MM:SS",
            ),
            (
                serde_json::json!({"date":"2026-02-31"}),
                ".date must be a real calendar date",
            ),
            (
                serde_json::json!({"date":"2026-08-22"}),
                ".date must be a completed UTC day",
            ),
            (
                serde_json::json!({"date":"2026-08-21","model":long_model}),
                ".model exceeds 64 characters",
            ),
            (
                serde_json::json!({"date":"2026-08-21"}),
                ".prompt_tokens is missing",
            ),
            (
                serde_json::json!({"date":"2026-08-21","prompt_tokens":-1}),
                ".prompt_tokens must be a nonnegative integer",
            ),
            (
                serde_json::json!({"date":"2026-08-21","prompt_tokens":MAX_SAFE_INTEGER + 1}),
                ".prompt_tokens must be a nonnegative safe integer",
            ),
            (
                serde_json::json!({"date":"2026-08-21","prompt_tokens":MAX_SAFE_INTEGER,
                    "completion_tokens":1}),
                " token total overflowed",
            ),
            (
                serde_json::json!({"date":"2026-08-21","prompt_tokens":1,"completion_tokens":1,
                    "requests":1,"usage":-1.0}),
                ".usage must be finite and nonnegative",
            ),
        ];
        for (row, detail) in cases {
            let valid = serde_json::json!({"date":"2026-08-21","model":"m","prompt_tokens":1,
                "completion_tokens":1,"requests":1,"usage":0.1});
            let payload = serde_json::json!({ "data": [valid, row] });
            let error = parse_activity_cost(&[payload], now()).unwrap_err();
            assert!(
                matches!(&error, ProviderError::Parse(message)
                    if *message == format!("OpenRouter activity.data[1]{detail}")),
                "{detail}: {error:?}"
            );
        }
    }

    #[test]
    fn aggregates_metered_and_byok_spend_without_double_counting_latest_completed_day() {
        let history = serde_json::json!({"data":[
            {"date":"2026-08-20","model":"m0","prompt_tokens":5,"completion_tokens":2,"reasoning_tokens":0,"requests":1,"usage":0.5,"byok_usage_inference":0.0},
            {"date":"2026-08-21","model":"m1","prompt_tokens":10,"completion_tokens":5,"reasoning_tokens":2,"requests":1,"usage":1.25,"byok_usage_inference":0.25}
        ]});
        let latest_completed = serde_json::json!({"data":[
            {"date":"2026-08-21","model":"m1","prompt_tokens":10,"completion_tokens":5,"reasoning_tokens":2,"requests":1,"usage":1.25,"byok_usage_inference":0.25}
        ]});
        let report = parse_activity_cost(&[history, latest_completed], now()).unwrap();
        let cost = report.cost;
        // The CLI history line and the Activity tokens row read the same total.
        assert_eq!(cost.history_tokens, Some(report.summary.tokens));
        // The duplicated latest-completed row is counted once.
        assert_eq!(
            report.summary,
            ActivitySummary {
                tokens: 22,
                requests: 2,
                models: 2
            }
        );
        assert!((cost.used - 2.0).abs() < 1e-12);
        assert_eq!(cost.daily.len(), 2);
        assert_eq!(cost.period, "Last 30 days (UTC)");
    }

    #[test]
    fn preserves_reasoning_tokens_when_they_exceed_completion_tokens() {
        let payload = serde_json::json!({"data":[
            {"date":"2026-08-21","model":"reasoning-model","prompt_tokens":10,
             "completion_tokens":2,"reasoning_tokens":8,"requests":1,"usage":1.0}
        ]});

        let report = parse_activity_cost(&[payload], now()).unwrap();

        assert_eq!(report.cost.used, 1.0);
        assert_eq!(report.cost.daily.len(), 1);
        // Tokens stay prompt + completion; reasoning is not added again.
        assert_eq!(report.summary.tokens, 12);
    }

    #[test]
    fn rejects_conflicting_duplicate_activity_rows() {
        let a = serde_json::json!({"data":[
            {"date":"2026-08-21","model":"m","prompt_tokens":10,"completion_tokens":5,"requests":1,"usage":1.0}
        ]});
        let b = serde_json::json!({"data":[
            {"date":"2026-08-21","model":"m","prompt_tokens":11,"completion_tokens":5,"requests":1,"usage":1.0}
        ]});
        assert!(parse_activity_cost(&[a, b], now()).is_err());
    }

    #[test]
    fn filters_rows_outside_exact_30_day_window() {
        let payload = serde_json::json!({"data":[
            {"date":"2026-07-22","model":"old","prompt_tokens":10,"completion_tokens":5,"requests":1,"usage":99.0},
            {"date":"2026-07-23","model":"in","prompt_tokens":10,"completion_tokens":5,"requests":1,"usage":1.0}
        ]});
        let cost = parse_activity_cost(&[payload], now()).unwrap().cost;
        assert_eq!(cost.used, 1.0);
    }

    #[test]
    fn accepts_timestamp_shaped_activity_dates_and_normalizes_to_the_utc_day() {
        for date in ["2026-08-21", "2026-08-21 00:00:00"] {
            let payload = serde_json::json!({"data":[
                {"date":date,"model":"m","prompt_tokens":10,"completion_tokens":5,"requests":1,"usage":1.0}
            ]});
            let cost = parse_activity_cost(&[payload], now()).unwrap().cost;
            assert_eq!(cost.daily.len(), 1);
            assert_eq!(cost.daily[0].day, "2026-08-21");
        }
    }

    #[test]
    fn rejects_unsupported_or_impossible_activity_timestamp_dates() {
        for date in ["2026-08-21T00:00:00", "2026-02-31 00:00:00"] {
            let payload = serde_json::json!({"data":[
                {"date":date,"model":"m","prompt_tokens":10,"completion_tokens":5,"requests":1,"usage":1.0}
            ]});
            assert!(parse_activity_cost(&[payload], now()).is_err());
        }
    }

    #[test]
    fn rejects_activity_rows_from_an_incomplete_utc_day() {
        let payload = serde_json::json!({"data":[
            {"date":"2026-08-22","model":"today","prompt_tokens":10,
             "completion_tokens":5,"requests":1,"usage":1.0}
        ]});

        let error = parse_activity_cost(&[payload], now()).unwrap_err();

        assert!(error.to_string().contains("completed UTC day"));
    }

    #[test]
    fn rows_without_a_model_do_not_count_as_models() {
        let payload = serde_json::json!({"data":[
            {"date":"2026-08-21","prompt_tokens":1,"completion_tokens":1,"requests":1,"usage":0.1},
            {"date":"2026-08-21","model":"  ","endpoint_id":"e","prompt_tokens":1,"completion_tokens":1,"requests":1,"usage":0.1},
            {"date":"2026-08-21","model_permaslug":"a/b","prompt_tokens":1,"completion_tokens":1,"requests":1,"usage":0.1}
        ]});

        let summary = parse_activity_cost(&[payload], now()).unwrap().summary;

        assert_eq!(summary.models, 1);
        assert_eq!(summary.requests, 3);
    }

    #[test]
    fn aggregate_beyond_the_safe_integer_range_is_rejected() {
        let big = MAX_SAFE_INTEGER / 2 + 1;
        let payload = serde_json::json!({"data":[
            {"date":"2026-08-21","model":"a","prompt_tokens":big,"completion_tokens":0,"requests":1,"usage":0.1},
            {"date":"2026-08-20","model":"b","prompt_tokens":big,"completion_tokens":0,"requests":1,"usage":0.1}
        ]});

        let error = parse_activity_cost(&[payload], now()).unwrap_err();

        assert!(error.to_string().contains("safe integer"));
    }

    #[test]
    fn records_token_total_and_cost_provenance() {
        use crate::spend_contract::CostProvenance;

        let cases = [
            (1.25, 0.0, CostProvenance::VendorMetered),
            (0.0, 0.75, CostProvenance::ListPriceEstimate),
            (1.25, 0.75, CostProvenance::Mixed),
        ];
        for (usage, byok, expected) in cases {
            let history = serde_json::json!({"data":[
                {"date":"2026-08-17","model":"m","prompt_tokens":10,"completion_tokens":5,
                 "reasoning_tokens":2,"requests":1,"usage":usage,"byok_usage_inference":byok}
            ]});
            let cost = parse_activity_cost(&[history], now()).unwrap().cost;
            assert_eq!(cost.history_tokens, Some(15));
            assert_eq!(cost.provenance, Some(expected));
            assert_eq!(cost.used, usage + byok);
        }
    }

    #[test]
    fn empty_activity_is_a_reported_zero_with_zero_tokens() {
        use crate::spend_contract::CostProvenance;

        let cost = parse_activity_cost(&[serde_json::json!({"data":[]})], now())
            .unwrap()
            .cost;
        assert_eq!(cost.used, 0.0);
        assert_eq!(cost.history_tokens, Some(0));
        assert_eq!(cost.provenance, Some(CostProvenance::VendorMetered));
    }

    #[test]
    fn duplicate_rows_do_not_double_count_tokens() {
        let row = serde_json::json!({"date":"2026-08-21","model":"m","prompt_tokens":10,
            "completion_tokens":5,"requests":1,"usage":1.0});
        let history = serde_json::json!({"data":[row.clone()]});
        let latest = serde_json::json!({"data":[row]});
        let cost = parse_activity_cost(&[history, latest], now()).unwrap().cost;
        assert_eq!(cost.history_tokens, Some(15));
    }
}
