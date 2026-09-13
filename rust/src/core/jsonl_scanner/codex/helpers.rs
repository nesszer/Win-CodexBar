use super::CodexTotals;
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use serde::Deserialize;
use serde_json::Value;
use std::io::BufRead;

pub(super) const CODEX_JSONL_MAX_LINE_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
struct CodexFastLine<'a> {
    #[serde(rename = "type", borrow)]
    event_type: Option<&'a str>,
    #[serde(default, borrow)]
    timestamp: Option<&'a str>,
    #[serde(default, borrow)]
    payload: Option<CodexFastPayload<'a>>,
    #[serde(default, borrow)]
    event_msg: Option<CodexFastPayload<'a>>,
    #[serde(default, borrow)]
    model: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexFastPayload<'a> {
    #[serde(rename = "type", borrow)]
    pub(super) payload_type: Option<&'a str>,
    #[serde(default, borrow)]
    pub(super) model: Option<&'a str>,
    #[serde(default, borrow)]
    pub(super) model_name: Option<&'a str>,
    #[serde(default, borrow)]
    pub(super) info: Option<CodexFastInfo<'a>>,
    #[serde(default)]
    pub(super) input_tokens: Option<i64>,
    #[serde(default)]
    pub(super) cached_input_tokens: Option<i64>,
    #[serde(default)]
    pub(super) cache_read_input_tokens: Option<i64>,
    #[serde(default)]
    pub(super) output_tokens: Option<i64>,
    #[serde(default)]
    pub(super) reasoning_output_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexFastInfo<'a> {
    #[serde(default, borrow)]
    pub(super) model: Option<&'a str>,
    #[serde(default, borrow)]
    pub(super) model_name: Option<&'a str>,
    #[serde(default)]
    pub(super) total_token_usage: Option<CodexFastTotals>,
    #[serde(default)]
    pub(super) last_token_usage: Option<CodexFastTotals>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(super) struct CodexFastTotals {
    #[serde(default)]
    pub(super) input_tokens: i64,
    #[serde(default)]
    pub(super) cached_input_tokens: Option<i64>,
    #[serde(default)]
    pub(super) cache_read_input_tokens: Option<i64>,
    #[serde(default)]
    pub(super) output_tokens: i64,
    #[serde(default)]
    pub(super) reasoning_output_tokens: Option<i64>,
}

#[allow(
    clippy::large_enum_variant,
    reason = "keeping the borrowed fast payload inline avoids heap allocation in the JSONL scan hot path"
)]
pub(super) enum CodexFastEvent<'a> {
    TurnContext {
        model: Option<&'a str>,
    },
    TokenCount {
        timestamp: &'a str,
        payload: CodexFastPayload<'a>,
    },
}

pub(super) fn model_evidence(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// When interleaved Ultra lineages reset cumulative counters, only count growth
/// above the historical high watermark so rewound branches do not re-add work.
pub(super) fn contained_total_delta(
    watermark: Option<&CodexTotals>,
    counted: Option<&CodexTotals>,
    current: &CodexTotals,
) -> CodexTotals {
    let water = watermark.cloned().unwrap_or(CodexTotals {
        input: 0,
        cached: 0,
        output: 0,
        reasoning: None,
    });
    let counted = counted.cloned().unwrap_or(CodexTotals {
        input: 0,
        cached: 0,
        output: 0,
        reasoning: None,
    });

    let component = |water: i64, counted: i64, current: i64| -> i64 {
        if current >= water {
            // Only growth above the historical high watermark counts.
            (current - water.max(counted)).max(0)
        } else {
            // Below watermark: rewind / interleaved lineage - do not re-add
            // mid-range climbs that would inflate totals after a fork reset.
            0
        }
    };

    CodexTotals {
        input: component(water.input, counted.input, current.input),
        cached: component(water.cached, counted.cached, current.cached),
        output: component(water.output, counted.output, current.output),
        reasoning: cumulative_reasoning_delta(
            Some(&counted),
            current.reasoning,
            component(water.output, counted.output, current.output),
        ),
    }
}

pub(super) fn cumulative_reasoning_delta(
    previous: Option<&CodexTotals>,
    current: Option<i64>,
    output_delta: i64,
) -> Option<i64> {
    let current = current?;
    let previous = match previous {
        Some(previous) => previous.reasoning?,
        None => 0,
    };
    Some(
        current
            .saturating_sub(previous)
            .max(0)
            .min(output_delta.max(0)),
    )
}

/// A bounded physical JSONL line.
///
/// Keeping the discarded case separate prevents callers from accidentally
/// treating an oversized prefix as a parseable empty line.
pub(super) enum BoundedJsonlLine {
    Retained {
        bytes: Vec<u8>,
        consumed: usize,
        terminated_by_newline: bool,
    },
    Discarded {
        consumed: usize,
        terminated_by_newline: bool,
    },
}

/// Read one JSONL line, discarding content when it exceeds `max_bytes`.
pub(super) fn read_bounded_jsonl_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<BoundedJsonlLine>> {
    read_bounded_jsonl_line_until(reader, max_bytes, None)
}

/// Read one JSONL line without consuming past a frozen logical target.
///
/// A target can end in the middle of a record while Codex is writing it. The
/// caller can then retain the last committed line boundary and retry the tail
/// after a later append, instead of publishing a partial record.
pub(super) fn read_bounded_jsonl_line_until<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
    max_total_bytes: Option<usize>,
) -> std::io::Result<Option<BoundedJsonlLine>> {
    let mut line = Vec::new();
    let mut saw_bytes = false;
    let mut discarding = false;
    let mut consumed_total = 0;
    let mut remaining_total = max_total_bytes;

    loop {
        if remaining_total == Some(0) {
            return Ok(saw_bytes.then_some(if discarding {
                BoundedJsonlLine::Discarded {
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            } else {
                BoundedJsonlLine::Retained {
                    bytes: line,
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            }));
        }

        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(saw_bytes.then_some(if discarding {
                BoundedJsonlLine::Discarded {
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            } else {
                BoundedJsonlLine::Retained {
                    bytes: line,
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            }));
        }

        let visible_len =
            remaining_total.map_or(chunk.len(), |remaining| remaining.min(chunk.len()));
        if visible_len == 0 {
            return Ok(saw_bytes.then_some(if discarding {
                BoundedJsonlLine::Discarded {
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            } else {
                BoundedJsonlLine::Retained {
                    bytes: line,
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            }));
        }

        let visible_chunk = &chunk[..visible_len];
        let newline = visible_chunk.iter().position(|byte| *byte == b'\n');
        let segment_end = newline.unwrap_or(visible_len);
        let segment = &visible_chunk[..segment_end];
        saw_bytes = true;

        if !discarding {
            let remaining = max_bytes.saturating_sub(line.len());
            if segment.len() <= remaining {
                line.extend_from_slice(segment);
            } else {
                line.clear();
                discarding = true;
            }
        }

        let consumed = segment_end + usize::from(newline.is_some());
        reader.consume(consumed);
        consumed_total += consumed;
        if let Some(remaining) = remaining_total.as_mut() {
            *remaining = remaining.saturating_sub(consumed);
        }
        if newline.is_some() {
            return Ok(Some(if discarding {
                BoundedJsonlLine::Discarded {
                    consumed: consumed_total,
                    terminated_by_newline: true,
                }
            } else {
                BoundedJsonlLine::Retained {
                    bytes: line,
                    consumed: consumed_total,
                    terminated_by_newline: true,
                }
            }));
        }

        if remaining_total == Some(0) {
            return Ok(Some(if discarding {
                BoundedJsonlLine::Discarded {
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            } else {
                BoundedJsonlLine::Retained {
                    bytes: line,
                    consumed: consumed_total,
                    terminated_by_newline: false,
                }
            }));
        }
    }
}

pub(super) fn parse_codex_fast_event(line: &str) -> Option<CodexFastEvent<'_>> {
    let parsed: CodexFastLine<'_> = serde_json::from_str(line).ok()?;
    match parsed.event_type? {
        "turn_context" => {
            let model = parsed
                .payload
                .as_ref()
                .and_then(|payload| {
                    payload.model.or(payload.model_name).or_else(|| {
                        payload
                            .info
                            .as_ref()
                            .and_then(|info| info.model.or(info.model_name))
                    })
                })
                .or(parsed.model);
            Some(CodexFastEvent::TurnContext { model })
        }
        "event_msg" => {
            let payload = parsed.payload.or(parsed.event_msg)?;
            (payload.payload_type == Some("token_count")).then_some(CodexFastEvent::TokenCount {
                timestamp: parsed.timestamp?,
                payload,
            })
        }
        _ => None,
    }
}

pub(super) fn is_candidate_codex_line(line: &str) -> bool {
    if !line.contains("\"type\":\"event_msg\"")
        && !line.contains("\"type\":\"turn_context\"")
        && !line.contains("\"turn_context\"")
        && !line.contains("\"event_msg\"")
    {
        return false;
    }

    !line.contains("\"type\":\"event_msg\"") || line.contains("\"token_count\"")
}

pub(super) fn codex_timestamp_day_key(timestamp: &str) -> Option<String> {
    parse_codex_timestamp(timestamp).map(|parsed| parsed.day_key())
}

#[derive(Debug, Clone)]
pub(super) struct ParsedCodexTimestamp {
    pub(super) parsed: Option<DateTime<FixedOffset>>,
    pub(super) fallback_day_key: String,
}

impl ParsedCodexTimestamp {
    pub(super) fn day_key(&self) -> String {
        self.parsed
            .as_ref()
            .map(|timestamp| {
                timestamp
                    .with_timezone(&Local)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string()
            })
            .unwrap_or_else(|| self.fallback_day_key.clone())
    }
}

pub(super) fn parse_codex_timestamp(timestamp: &str) -> Option<ParsedCodexTimestamp> {
    let bytes = timestamp.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let fallback_day_key = timestamp.get(..10)?;
    NaiveDate::parse_from_str(fallback_day_key, "%Y-%m-%d").ok()?;
    Some(ParsedCodexTimestamp {
        parsed: parse_rfc3339_timestamp(timestamp),
        fallback_day_key: fallback_day_key.to_string(),
    })
}

pub(super) fn parse_rfc3339_timestamp(timestamp: &str) -> Option<DateTime<FixedOffset>> {
    parse_native_rfc3339(timestamp).or_else(|| DateTime::parse_from_rfc3339(timestamp).ok())
}

pub(super) fn nonempty_json_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn session_meta_field(
    root: &Value,
    payload: Option<&Value>,
    keys: &[&str],
) -> Option<String> {
    payload
        .and_then(|payload| {
            keys.iter()
                .find_map(|key| nonempty_json_string(payload.get(*key)))
        })
        .or_else(|| {
            keys.iter()
                .find_map(|key| nonempty_json_string(root.get(*key)))
        })
}

/// Fast path for the RFC3339 spelling emitted by native Codex logs. Historical
/// spellings still fall through to chrono's parser, preserving old behavior.
fn parse_native_rfc3339(timestamp: &str) -> Option<DateTime<FixedOffset>> {
    let bytes = timestamp.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }

    let year = parse_ascii_number(bytes, 0, 4)?;
    if year < 1900 {
        return None;
    }
    let month = parse_ascii_number(bytes, 5, 2)?;
    let day = parse_ascii_number(bytes, 8, 2)?;
    let hour = parse_ascii_number(bytes, 11, 2)?;
    let minute = parse_ascii_number(bytes, 14, 2)?;
    let second = parse_ascii_number(bytes, 17, 2)?;
    if !(1..=12).contains(&month) || hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }

    let mut zone_index = 19;
    let mut nanoseconds = 0_u32;
    if bytes.get(zone_index) == Some(&b'.') {
        zone_index += 1;
        let fraction_start = zone_index;
        while bytes
            .get(zone_index)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            let digits = zone_index - fraction_start;
            if digits >= 9 {
                return None;
            }
            if digits < 3 {
                nanoseconds = nanoseconds * 10 + u32::from(bytes[zone_index] - b'0');
            }
            zone_index += 1;
        }
        let digits = zone_index - fraction_start;
        if digits == 0 {
            return None;
        }
        for _ in digits.min(3)..3 {
            nanoseconds *= 10;
        }
        for _ in 0..6 {
            nanoseconds *= 10;
        }
    }

    let offset_seconds = match bytes.get(zone_index) {
        Some(b'Z') if zone_index + 1 == bytes.len() => 0,
        Some(sign) if (*sign == b'+' || *sign == b'-') && zone_index + 6 == bytes.len() => {
            if bytes[zone_index + 3] != b':' {
                return None;
            }
            let hours = parse_ascii_number(bytes, zone_index + 1, 2)?;
            let minutes = parse_ascii_number(bytes, zone_index + 4, 2)?;
            if hours >= 24 || minutes >= 60 {
                return None;
            }
            let seconds = i32::try_from((hours * 60 + minutes) * 60).ok()?;
            if *sign == b'-' { -seconds } else { seconds }
        }
        _ => return None,
    };

    let year = i32::try_from(year).ok()?;
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let local = date.and_hms_nano_opt(hour, minute, second, nanoseconds)?;
    FixedOffset::east_opt(offset_seconds)
        .and_then(|offset| offset.from_local_datetime(&local).single())
}

fn parse_ascii_number(bytes: &[u8], start: usize, count: usize) -> Option<u32> {
    let slice = bytes.get(start..start.checked_add(count)?)?;
    let mut value = 0_u32;
    for byte in slice {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(*byte - b'0');
    }
    Some(value)
}

pub(super) fn bare_usage_totals(obj: &Value) -> Option<(CodexTotals, Option<String>)> {
    let usage = obj
        .get("usage")
        .or_else(|| obj.get("data").and_then(|v| v.get("usage")))
        .or_else(|| obj.get("result").and_then(|v| v.get("usage")))
        .or_else(|| obj.get("response").and_then(|v| v.get("usage")))?;
    // Token counts come from usage records and use i64, the canonical totals storage type.
    let input = ["input_tokens", "prompt_tokens", "input"]
        .into_iter()
        .find_map(|key| usage.get(key).and_then(Value::as_i64))
        .unwrap_or(0)
        .max(0);
    // Token counts come from usage records and use i64, the canonical totals storage type.
    let output = ["output_tokens", "completion_tokens", "output"]
        .into_iter()
        .find_map(|key| usage.get(key).and_then(Value::as_i64))
        .unwrap_or(0)
        .max(0);
    // Token counts come from usage records and use i64, the canonical totals storage type.
    let cached = [
        "cached_input_tokens",
        "cache_read_input_tokens",
        "cached_tokens",
    ]
    .into_iter()
    .filter_map(|key| usage.get(key).and_then(Value::as_i64))
    .max()
    .unwrap_or(0)
    .max(0);
    let reasoning = clamp_reasoning(optional_token_i64(usage, "reasoning_output_tokens"), output);
    if input == 0 && output == 0 && cached == 0 {
        return None;
    }
    let model = obj
        .get("model")
        .or_else(|| obj.get("data").and_then(|v| v.get("model")))
        .or_else(|| obj.get("result").and_then(|v| v.get("model")))
        .or_else(|| obj.get("response").and_then(|v| v.get("model")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    Some((
        CodexTotals {
            input,
            cached,
            output,
            reasoning,
        },
        model,
    ))
}

pub(super) fn token_count_payload(obj: &Value) -> Option<&Value> {
    if let Some(payload) = obj.get("payload")
        && payload.get("type").and_then(|v| v.as_str()) == Some("token_count")
    {
        return Some(payload);
    }

    let event_msg = obj.get("event_msg")?;
    (event_msg.get("type").and_then(|v| v.as_str()) == Some("token_count")).then_some(event_msg)
}

pub(super) fn read_token_totals(value: &Value) -> CodexTotals {
    // Token counts come from Codex usage records and use i64, which is
    // the canonical storage type of the totals table. Malformed negative
    // counts are clamped at the source so they can never lower the high
    // watermark and inflate a later `apply_totals_delta`.
    let cached = value
        .get("cached_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(
            value
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        )
        .max(0);
    let input = token_i64(value, "input_tokens").max(0);
    let output = token_i64(value, "output_tokens").max(0);
    CodexTotals {
        input,
        cached,
        output,
        reasoning: clamp_reasoning(optional_token_i64(value, "reasoning_output_tokens"), output),
    }
}

pub(super) fn codex_totals_from_fast(value: CodexFastTotals) -> CodexTotals {
    let input = value.input_tokens.max(0);
    let cached = value
        .cached_input_tokens
        .unwrap_or(0)
        .max(value.cache_read_input_tokens.unwrap_or(0))
        .max(0);
    let output = value.output_tokens.max(0);
    CodexTotals {
        input,
        cached,
        output,
        reasoning: clamp_reasoning(value.reasoning_output_tokens, output),
    }
}

pub(super) fn fast_totals_from_payload(value: &CodexFastPayload<'_>) -> CodexTotals {
    let input = value.input_tokens.unwrap_or(0).max(0);
    let cached = value
        .cached_input_tokens
        .unwrap_or(0)
        .max(value.cache_read_input_tokens.unwrap_or(0))
        .max(0);
    let output = value.output_tokens.unwrap_or(0).max(0);
    CodexTotals {
        input,
        cached,
        output,
        reasoning: clamp_reasoning(value.reasoning_output_tokens, output),
    }
}

fn token_i64(value: &Value, key: &str) -> i64 {
    // Token counts from usage records use i64, the canonical totals storage type.
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn optional_token_i64(value: &Value, key: &str) -> Option<i64> {
    // Token counts from usage records use i64, the canonical storage type.
    value.get(key).and_then(Value::as_i64)
}

pub(super) fn clamp_reasoning(reasoning: Option<i64>, output: i64) -> Option<i64> {
    reasoning.map(|tokens| tokens.max(0).min(output.max(0)))
}

pub(super) fn last_usage_delta(last: &Value) -> (i64, i64, i64, Option<i64>) {
    let totals = read_token_totals(last);
    (
        totals.input.max(0),
        totals.cached.max(0),
        totals.output.max(0),
        totals.reasoning,
    )
}

pub(super) fn fast_last_usage_delta(last: CodexFastTotals) -> (i64, i64, i64, Option<i64>) {
    let totals = codex_totals_from_fast(last);
    (
        totals.input.max(0),
        totals.cached.max(0),
        totals.output.max(0),
        totals.reasoning,
    )
}
