//! Parser for the authenticated Mistral subscription page.
//!
//! The page embeds a small React Flight stream: `self.__next_f.push` calls
//! whose second element is a chunk of the Flight text. This parser scans the
//! markers, joins the chunks, and reads the `0:<hex>:<JSON>` lines. Keep it
//! local to the Mistral provider so a change in that page format cannot
//! affect other providers or the shared snapshot contract.

use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SubscriptionBudgets {
    pub(super) api: Option<SubscriptionBudget>,
    pub(super) vibe: Option<SubscriptionBudget>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SubscriptionBudget {
    pub(super) used_percent: f64,
    pub(super) limit: f64,
    pub(super) currency: String,
    pub(super) resets_at: Option<DateTime<Utc>>,
}

impl SubscriptionBudget {
    pub(super) fn used_amount(&self) -> f64 {
        self.limit * (self.used_percent / 100.0)
    }

    pub(super) fn remaining_amount(&self) -> f64 {
        (self.limit - self.used_amount()).max(0.0)
    }
}

pub(super) fn parse(html: &str) -> Result<SubscriptionBudgets, String> {
    let stream = flight_chunks(html).join("");
    let mut matches = Vec::new();
    collect_models(stream.as_bytes(), &mut |value| {
        collect_budgets(value, &mut matches);
    })?;

    match matches.as_slice() {
        [] => Err("Mistral subscription budgets were not found".into()),
        [budgets] => Ok(budgets.clone()),
        _ => Err("Mistral subscription budgets were ambiguous".into()),
    }
}

fn flight_chunks(html: &str) -> Vec<String> {
    let bytes = html.as_bytes();
    let marker = b"self.__next_f.push(";
    let mut cursor = 0;
    let mut chunks = Vec::new();

    while cursor < bytes.len() {
        let Some(offset) = find_bytes(&bytes[cursor..], marker) else {
            break;
        };
        let marker_start = cursor + offset;
        let mut start = marker_start + marker.len();
        while start < bytes.len() && bytes[start].is_ascii_whitespace() {
            start += 1;
        }
        if start >= bytes.len() || bytes[start] != b'[' {
            cursor = marker_start + marker.len();
            continue;
        }
        let Some(end) = json_container_end(bytes, start) else {
            cursor = start + 1;
            continue;
        };
        if let Ok(Value::Array(values)) = serde_json::from_slice::<Value>(&bytes[start..end])
            && values.first().and_then(Value::as_u64) == Some(1)
            && let Some(chunk) = values.get(1).and_then(Value::as_str)
        {
            chunks.push(chunk.to_string());
        }
        cursor = end;
    }

    chunks
}

fn collect_models(data: &[u8], body: &mut impl FnMut(Value)) -> Result<(), String> {
    // Advance past a scanned line, stopping at the final newlineless tail.
    let advance = |line_end: usize| line_end.min(data.len().saturating_sub(1) + 1) + 1;
    let mut cursor = 0;
    while cursor < data.len() {
        let line_end = data[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(data.len(), |offset| cursor + offset);
        let line = &data[cursor..line_end];
        let colon = line
            .iter()
            .position(|byte| *byte == b':')
            .filter(|colon| *colon != 0 && line[..*colon].iter().all(u8::is_ascii_hexdigit));
        let Some(colon) = colon else {
            cursor = advance(line_end);
            continue;
        };

        let start = cursor + colon + 1;
        let payload = data[start..line_end].trim_ascii();
        if matches!(payload.first(), Some(b'[' | b'{'))
            && let Ok(value) = serde_json::from_slice::<Value>(payload)
        {
            body(value);
        }
        cursor = advance(line_end);
    }
    Ok(())
}

fn collect_budgets(value: Value, matches: &mut Vec<SubscriptionBudgets>) {
    match value {
        Value::Object(object) => {
            if let Some(Value::Object(budget)) = object.get("budget") {
                let candidate = SubscriptionBudgets {
                    api: budget.get("api_budget").and_then(parse_budget),
                    vibe: budget.get("vibe_budget").and_then(parse_budget),
                };
                if candidate.api.is_some() || candidate.vibe.is_some() {
                    matches.push(candidate);
                }
            }
            for child in object.into_values() {
                collect_budgets(child, matches);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_budgets(child, matches);
            }
        }
        _ => {}
    }
}

fn parse_budget(value: &Value) -> Option<SubscriptionBudget> {
    let object = value.as_object()?;
    let used_percent = number(object.get("usage_percentage")?)?;
    let limit = number(object.get("initial_budget")?)?;
    let currency = object
        .get("currency")
        .and_then(Value::as_str)?
        .trim()
        .to_uppercase();
    if !used_percent.is_finite() || used_percent < 0.0 || !limit.is_finite() || limit <= 0.0 {
        return None;
    }
    if currency.is_empty() {
        return None;
    }
    let used_amount = limit * (used_percent / 100.0);
    if !used_amount.is_finite() {
        return None;
    }
    let resets_at = object
        .get("reset_at")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    Some(SubscriptionBudget {
        used_percent,
        limit,
        currency,
        resets_at,
    })
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn json_container_end(data: &[u8], start: usize) -> Option<usize> {
    let mut closers = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut index = start;
    while index < data.len() {
        let byte = data[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'[' => closers.push(b']'),
                b'{' => closers.push(b'}'),
                b']' | b'}' => {
                    if closers.last().copied() != Some(byte) {
                        return None;
                    }
                    closers.pop();
                    if closers.is_empty() {
                        return Some(index + 1);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flight_html(model: &str) -> String {
        let row = format!("0:{model}\n");
        format!(
            "<script>self.__next_f.push([1, {}])</script>",
            serde_json::to_string(&row).unwrap()
        )
    }

    #[test]
    fn parses_api_and_vibe_budgets_from_flight_data() {
        let html = flight_html(
            r#"{"budget":{"api_budget":{"usage_percentage":25,"initial_budget":100,"currency":"usd","reset_at":"2026-09-30T00:00:00Z"},"vibe_budget":{"usage_percentage":50,"initial_budget":20,"currency":"EUR","reset_at":null}}}"#,
        );
        let budgets = parse(&html).unwrap();
        assert_eq!(
            budgets.api.as_ref().map(|budget| budget.used_percent),
            Some(25.0)
        );
        assert_eq!(
            budgets.api.as_ref().map(|budget| budget.currency.as_str()),
            Some("USD")
        );
        assert_eq!(
            budgets
                .vibe
                .as_ref()
                .map(|budget| budget.remaining_amount()),
            Some(10.0)
        );
    }

    #[test]
    fn ignores_invalid_budget_values() {
        let html = flight_html(
            r#"{"budget":{"api_budget":{"usage_percentage":-1,"initial_budget":100,"currency":"USD"}}}"#,
        );
        assert!(parse(&html).is_err());
    }

    #[test]
    fn handles_brackets_inside_flight_strings() {
        let html = flight_html(
            r#"{"note":"fake [brackets]", "budget":{"api_budget":{"usage_percentage":1,"initial_budget":10,"currency":"USD"}}}"#,
        );
        assert!(parse(&html).unwrap().api.is_some());
    }
}
