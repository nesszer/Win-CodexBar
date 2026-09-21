//! JSONL record parsing for the Muse local token history scan.

use super::*;

pub(crate) fn integer(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}
pub(crate) fn counter(value: &serde_json::Map<String, Value>, names: &[&str]) -> Option<u64> {
    let mut found = None;
    for name in names {
        if let Some(raw) = value.get(*name) {
            let parsed = integer(Some(raw))?;
            if found.is_some_and(|previous| previous != parsed) {
                return None;
            }
            found = Some(parsed);
        }
    }
    Some(found.unwrap_or(0))
}
pub(crate) fn has_token_fields(value: &Value) -> bool {
    [
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "cache_read_tokens",
        "cached_input_tokens",
        "cached_tokens",
        "cache_write_tokens",
        "reasoning_tokens",
    ]
    .iter()
    .any(|key| value.get(*key).is_some())
}
pub(crate) fn parse_line(line: &[u8]) -> Result<Option<Event>, ()> {
    let Ok(object) = serde_json::from_slice::<Value>(line) else {
        return Err(());
    };
    let valid_envelope = object.get("schema_version").and_then(Value::as_u64) == Some(1)
        && object.get("record_type").and_then(Value::as_str) == Some("event")
        && object.get("payload_type").and_then(Value::as_str) == Some("runtime.session")
        && object.get("payload_schema_version").and_then(Value::as_u64) == Some(1);
    if !valid_envelope {
        return Err(());
    }
    let event = object
        .get("payload")
        .ok_or(())?
        .get("event")
        .and_then(Value::as_object);
    let Some(event) = event else { return Err(()) };
    let kind = event
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(
        kind,
        "resource_usage_sampled" | "workflow_child_lifecycle" | "goal_usage_attribution"
    ) {
        return Ok(None);
    }
    if !matches!(kind, "model_completed" | "automated_review_completed") {
        return if event.get("usage").is_some_and(has_token_fields) {
            Err(())
        } else {
            Ok(None)
        };
    }
    let usage = event.get("usage").and_then(Value::as_object).ok_or(())?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or(())?;
    let micros = integer(object.get("recorded_at"))
        .filter(|v| *v > 0)
        .ok_or(())?;
    let input = integer(usage.get("input_tokens")).ok_or(())?;
    let output = integer(usage.get("output_tokens")).ok_or(())?;
    let total = input.checked_add(output).ok_or(())?;
    if usage.get("total_tokens").is_some() && integer(usage.get("total_tokens")) != Some(total) {
        return Err(());
    }
    let cache_read = counter(
        usage,
        &["cache_read_tokens", "cached_input_tokens", "cached_tokens"],
    )
    .ok_or(())?;
    let cache_write = counter(usage, &["cache_write_tokens"]).ok_or(())?;
    let reasoning = counter(usage, &["reasoning_tokens"]).ok_or(())?;
    if cache_read > input || cache_write > input || reasoning > output {
        return Err(());
    }
    let model = event
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| {
            event
                .get("model")
                .and_then(|v| v.get("model_id"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let seconds = micros / 1_000_000;
    let nanos = u32::try_from((micros % 1_000_000) * 1_000).map_err(|_| ())?;
    let seconds = i64::try_from(seconds).map_err(|_| ())?;
    let timestamp = Utc.timestamp_opt(seconds, nanos).single().ok_or(())?;
    Ok(Some(Event {
        id: id.to_string(),
        day: timestamp.with_timezone(&Local).date_naive().to_string(),
        model,
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        total_tokens: total,
    }))
}

pub(crate) fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let mut saw_input = false;
    let mut discarding = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(saw_input.then_some(if discarding { Vec::new() } else { line }));
        }
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let segment_len = newline.unwrap_or(chunk.len());
        let consumed_len = segment_len + usize::from(newline.is_some());
        saw_input = true;
        if !discarding {
            if line.len().saturating_add(consumed_len) <= max_bytes {
                line.extend_from_slice(&chunk[..segment_len]);
                if newline.is_some() {
                    line.push(b'\n');
                }
            } else {
                line.clear();
                discarding = true;
            }
        }
        reader.consume(consumed_len);
        if newline.is_some() {
            return Ok(Some(if discarding { Vec::new() } else { line }));
        }
    }
}
