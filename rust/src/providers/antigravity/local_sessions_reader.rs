use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use serde_json::Value;

use super::cost::estimate_cost_usd;
use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};

const MAX_SESSION_FILES: usize = 2048;
const MAX_SESSION_DISCOVERY_ENTRIES: usize = 16 * 1024;
const MAX_SESSION_FILE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SESSION_FILE_BYTES_U64: u64 = 32 * 1024 * 1024;
const MAX_TOTAL_SESSION_BYTES: usize = 128 * 1024 * 1024;
const MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;

enum BoundedJsonlLine {
    Record(Vec<u8>),
    Oversized,
    Truncated,
}

pub(super) fn tokscale_sessions_from_values(
    home: &Path,
    tokscale_config_dir: Option<&str>,
) -> PathBuf {
    let tokscale_base = clean_env_path(tokscale_config_dir)
        .unwrap_or_else(|| home.join(".config").join("tokscale"));
    tokscale_base.join("antigravity-cache").join("sessions")
}

pub(super) fn configured_tokscale_sessions(home: &Path) -> PathBuf {
    let tokscale = std::env::var("TOKSCALE_CONFIG_DIR").ok();
    tokscale_sessions_from_values(home, tokscale.as_deref())
}

fn clean_env_path(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(super) fn summarize_jsonl_at(
    tokscale_sessions: &Path,
    now: DateTime<Utc>,
    days: u32,
) -> LocalTokenHistorySummary {
    let (paths, truncated) = tokscale_paths(tokscale_sessions);
    summarize_jsonl_paths(&paths, now, days, truncated)
}

pub(super) fn summarize_jsonl_paths(
    paths: &[PathBuf],
    now: DateTime<Utc>,
    days: u32,
    truncated: bool,
) -> LocalTokenHistorySummary {
    if paths.is_empty() {
        LocalTokenHistorySummary::default()
    } else {
        summarize_paths(paths, now, days, truncated)
    }
}

fn tokscale_paths(base: &Path) -> (Vec<PathBuf>, bool) {
    let Ok(entries) = fs::read_dir(base) else {
        return (Vec::new(), false);
    };
    bounded_tokscale_paths(entries, MAX_SESSION_DISCOVERY_ENTRIES, MAX_SESSION_FILES)
}

fn bounded_tokscale_paths(
    entries: fs::ReadDir,
    max_entries: usize,
    max_files: usize,
) -> (Vec<PathBuf>, bool) {
    let mut paths = BinaryHeap::<Reverse<PathBuf>>::with_capacity(max_files);
    let mut truncated = false;

    for (entries_examined, entry) in entries.enumerate() {
        if entries_examined == max_entries {
            truncated = true;
            break;
        }

        let Ok(entry) = entry else {
            truncated = true;
            continue;
        };
        let path = entry.path();
        let is_jsonl = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"));
        if !is_jsonl {
            continue;
        }

        if paths.len() < max_files {
            paths.push(Reverse(path));
        } else {
            truncated = true;
            if let Some(Reverse(smallest)) = paths.peek()
                && path > *smallest
            {
                paths.pop();
                paths.push(Reverse(path));
            }
        }
    }

    let mut paths: Vec<_> = paths.into_iter().map(|Reverse(path)| path).collect();
    paths.sort();
    (paths, truncated)
}

pub(super) fn count_jsonl_sessions_at(base: &Path) -> usize {
    tokscale_paths(base).0.len()
}

fn summarize_paths(
    paths: &[PathBuf],
    now: DateTime<Utc>,
    days: u32,
    truncated: bool,
) -> LocalTokenHistorySummary {
    summarize_paths_with_budget(paths, now, days, truncated, MAX_TOTAL_SESSION_BYTES)
}

fn summarize_paths_with_budget(
    paths: &[PathBuf],
    now: DateTime<Utc>,
    days: u32,
    truncated: bool,
    total_byte_budget: usize,
) -> LocalTokenHistorySummary {
    let first_day = now.with_timezone(&Local).date_naive()
        - Duration::days(i64::from(days.clamp(1, 365).saturating_sub(1)));
    let mut total_tokens = 0_u64;
    let mut cost_estimate = crate::spend_contract::LocalCostEstimate::default();
    let mut sessions_with_usage = HashSet::new();
    let mut seen_response_ids = HashSet::new();
    let mut complete = !truncated;
    let mut remaining_total_bytes = total_byte_budget;

    for path in paths.iter().take(MAX_SESSION_FILES) {
        if remaining_total_bytes == 0 {
            complete = false;
            break;
        }
        let file = match File::open(path) {
            Ok(file) => file,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        match file.metadata() {
            Ok(metadata) if metadata.len() > MAX_SESSION_FILE_BYTES_U64 => complete = false,
            Ok(_) => {}
            Err(_) => complete = false,
        }
        let mut reader = BufReader::new(file);
        let mut remaining = MAX_SESSION_FILE_BYTES;
        let mut path_had_usage = false;
        let mut model = None::<String>;
        loop {
            let line = match read_bounded_jsonl_line(
                &mut reader,
                &mut remaining,
                &mut remaining_total_bytes,
            ) {
                Ok(Some(BoundedJsonlLine::Record(line))) => line,
                Ok(Some(BoundedJsonlLine::Oversized)) => {
                    complete = false;
                    continue;
                }
                Ok(Some(BoundedJsonlLine::Truncated)) => {
                    complete = false;
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    complete = false;
                    break;
                }
            };
            if line.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                complete = false;
                continue;
            };
            if !value.is_object() {
                complete = false;
                continue;
            }
            let kind = value.get("type").and_then(Value::as_str);
            if kind == Some("session_meta") {
                model = value
                    .get("modelId")
                    .or_else(|| value.get("model_id"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                continue;
            }
            if kind != Some("usage") && value.get("input").is_none() {
                continue;
            }
            if !has_valid_token_fields(&value) {
                complete = false;
                continue;
            }

            let Some(timestamp_ms) = value.get("timestamp").and_then(Value::as_i64) else {
                complete = false;
                continue;
            };
            let Some(at) = Utc.timestamp_millis_opt(timestamp_ms).single() else {
                complete = false;
                continue;
            };

            if let Some(response_id) = value
                .get("responseId")
                .or_else(|| value.get("response_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                && !seen_response_ids.insert(response_id.to_string())
            {
                continue;
            }

            if at > now || at.with_timezone(&Local).date_naive() < first_day {
                continue;
            }

            let input = token_field(&value, &["input"]);
            let output = token_field(&value, &["output"]);
            let cache_read = token_field(&value, &["cacheRead", "cache_read"]);
            let cache_write = token_field(&value, &["cacheWrite", "cache_write"]);
            let reasoning = token_field(
                &value,
                &["reasoning", "reasoningTokens", "reasoning_tokens"],
            );
            let Some(total) = [input, output, cache_read, cache_write, reasoning]
                .into_iter()
                .try_fold(0_u64, u64::checked_add)
            else {
                complete = false;
                continue;
            };
            if total == 0 {
                continue;
            }
            let Some(next_total_tokens) = total_tokens.checked_add(total) else {
                complete = false;
                continue;
            };
            total_tokens = next_total_tokens;
            cost_estimate.record_list_price(estimate_cost_usd(
                model.as_deref(),
                input,
                cache_read,
                cache_write,
                output.saturating_add(reasoning),
            ));
            path_had_usage = true;
        }
        if path_had_usage {
            sessions_with_usage.insert(path.clone());
        }
    }

    LocalTokenHistorySummary {
        total_tokens,
        session_count: sessions_with_usage.len(),
        coverage: if paths.is_empty() {
            LocalHistoryCoverage::Unavailable
        } else if complete {
            LocalHistoryCoverage::Complete
        } else {
            LocalHistoryCoverage::Partial
        },
        cost_estimate,
    }
}

fn read_bounded_jsonl_line<R: BufRead>(
    reader: &mut R,
    remaining_file_bytes: &mut usize,
    remaining_total_bytes: &mut usize,
) -> std::io::Result<Option<BoundedJsonlLine>> {
    if *remaining_file_bytes == 0 {
        return Ok(None);
    }
    if *remaining_total_bytes == 0 {
        return Ok(if reader.fill_buf()?.is_empty() {
            None
        } else {
            Some(BoundedJsonlLine::Truncated)
        });
    }
    let mut line = Vec::new();
    let mut saw_input = false;
    let mut discarding = false;

    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(saw_input.then_some(if discarding {
                BoundedJsonlLine::Oversized
            } else {
                BoundedJsonlLine::Record(line)
            }));
        }
        let bounded_len = chunk
            .len()
            .min(*remaining_file_bytes)
            .min(*remaining_total_bytes);
        if bounded_len == 0 {
            return Ok(None);
        }
        let bounded = &chunk[..bounded_len];
        let newline = bounded.iter().position(|byte| *byte == b'\n');
        let segment_end = newline.unwrap_or(bounded.len());
        let segment = &bounded[..segment_end];
        saw_input = saw_input || !segment.is_empty() || newline.is_some();
        if !discarding {
            if line.len().saturating_add(segment.len()) <= MAX_JSONL_LINE_BYTES {
                line.extend_from_slice(segment);
            } else {
                line.clear();
                discarding = true;
            }
        }
        let consumed = segment_end + usize::from(newline.is_some());
        reader.consume(consumed);
        *remaining_file_bytes = remaining_file_bytes.saturating_sub(consumed);
        *remaining_total_bytes = remaining_total_bytes.saturating_sub(consumed);
        if newline.is_some() {
            return Ok(Some(if discarding {
                BoundedJsonlLine::Oversized
            } else {
                BoundedJsonlLine::Record(line)
            }));
        }
        if *remaining_file_bytes == 0 || *remaining_total_bytes == 0 {
            let at_eof = reader.fill_buf()?.is_empty();
            return Ok(Some(if !at_eof {
                BoundedJsonlLine::Truncated
            } else if discarding {
                BoundedJsonlLine::Oversized
            } else {
                BoundedJsonlLine::Record(line)
            }));
        }
    }
}

fn token_field(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
        .unwrap_or(0)
}

fn has_valid_token_fields(value: &Value) -> bool {
    let mut has_token_field = false;
    for key in [
        "input",
        "output",
        "cacheRead",
        "cache_read",
        "cacheWrite",
        "cache_write",
        "reasoning",
        "reasoningTokens",
        "reasoning_tokens",
    ] {
        if let Some(field) = value.get(key) {
            if field.as_u64().is_none() {
                return false;
            }
            has_token_field = true;
        }
    }
    has_token_field
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokscale_discovery_bounds_entries_and_retained_paths() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..5 {
            fs::write(dir.path().join(format!("session-{index}.jsonl")), "").unwrap();
        }

        let (paths, truncated) = bounded_tokscale_paths(fs::read_dir(dir.path()).unwrap(), 2, 10);
        assert_eq!(paths.len(), 2);
        assert!(truncated);

        let (paths, truncated) = bounded_tokscale_paths(fs::read_dir(dir.path()).unwrap(), 10, 2);
        assert_eq!(paths.len(), 2);
        assert!(truncated);
        assert_eq!(paths[0].file_name().unwrap(), "session-3.jsonl");
        assert_eq!(paths[1].file_name().unwrap(), "session-4.jsonl");
    }

    #[test]
    fn mixed_known_and_unknown_models_keep_only_a_known_subtotal() {
        let dir = tempfile::tempdir().unwrap();
        let known = dir.path().join("known.jsonl");
        let unknown = dir.path().join("unknown.jsonl");
        fs::write(
            &known,
            concat!(
                "{\"type\":\"session_meta\",\"modelId\":\"claude-sonnet-4-6\"}\n",
                "{\"type\":\"usage\",\"responseId\":\"known\",\"timestamp\":1787572800000,\"input\":1000,\"output\":200}\n"
            ),
        )
        .unwrap();
        fs::write(
            &unknown,
            concat!(
                "{\"type\":\"session_meta\",\"modelId\":\"future-model\"}\n",
                "{\"type\":\"usage\",\"responseId\":\"unknown\",\"timestamp\":1787572800000,\"input\":500,\"output\":100}\n"
            ),
        )
        .unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();

        let summary = summarize_paths(&[known, unknown], now, 7, false);

        assert_eq!(summary.cost_estimate.coverage.estimated, 1);
        assert_eq!(summary.cost_estimate.coverage.unpriced, 1);
        assert!(summary.cost_estimate.known_subtotal_usd.is_some());
        assert_eq!(summary.total_usd(), None);
    }
    #[test]
    fn summarizes_tokscale_jsonl_and_deduplicates_response_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-a.jsonl");
        fs::write(&path, concat!(
            "{\"type\":\"session_meta\",\"modelId\":\"test-model-antigravity-a\"}\n",
            "{\"type\":\"usage\",\"responseId\":\"r1\",\"timestamp\":1787572800000,\"input\":100,\"output\":20,\"cacheRead\":10,\"cacheWrite\":5}\n",
            "{\"type\":\"usage\",\"response_id\":\"r1\",\"timestamp\":1787572800000,\"input\":100,\"output\":20}\n"
        )).unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();
        let summary = summarize_paths(&[path], now, 7, false);
        assert_eq!(summary.total_tokens, 135);
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    }

    #[test]
    fn truncated_or_unreadable_tokscale_history_is_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-a.jsonl");
        fs::write(
            &path,
            b"{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":10}\n",
        )
        .unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();
        let truncated = summarize_paths(std::slice::from_ref(&path), now, 7, true);
        assert_eq!(truncated.coverage, LocalHistoryCoverage::Partial);

        let missing = summarize_paths(&[dir.path().join("missing.jsonl")], now, 7, false);
        assert_eq!(missing.coverage, LocalHistoryCoverage::Partial);
    }
    #[test]
    fn excludes_usage_outside_requested_window() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-a.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":10,\"output\":5}\n",
                "{\"type\":\"usage\",\"timestamp\":1784894400000,\"input\":99,\"output\":99}\n"
            ),
        )
        .unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();
        let summary = summarize_paths(&[path], now, 7, false);
        assert_eq!(summary.total_tokens, 15);
        assert_eq!(summary.session_count, 1);
    }

    #[test]
    fn oversized_jsonl_line_marks_coverage_partial_and_next_row_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-a.jsonl");
        let mut text = format!(
            r#"{{"type":"usage","padding":"{}"}}"#,
            "x".repeat(MAX_JSONL_LINE_BYTES + 32)
        );
        text.push('\n');
        text.push_str(r#"{"type":"usage","timestamp":1787572800000,"input":10,"output":5}"#);
        text.push('\n');
        fs::write(&path, text).unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();
        let summary = summarize_paths(&[path], now, 7, false);
        assert_eq!(summary.total_tokens, 15);
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    }

    #[test]
    fn malformed_jsonl_record_marks_coverage_partial_and_next_row_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-a.jsonl");
        fs::write(
            &path,
            concat!(
                "{malformed json}\n",
                "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":\"invalid\"}\n",
                "{\"type\":\"usage\",\"input\":10}\n",
                "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":10,\"output\":5}\n"
            ),
        )
        .unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();

        let summary = summarize_paths(&[path], now, 7, false);

        assert_eq!(summary.total_tokens, 15);
        assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    }

    #[test]
    fn total_scan_byte_budget_stops_later_records_and_marks_partial() {
        let dir = tempfile::tempdir().unwrap();
        let first_path = dir.path().join("session-a.jsonl");
        let second_path = dir.path().join("session-b.jsonl");
        let first = "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":10}\n";
        fs::write(&first_path, first).unwrap();
        fs::write(
            &second_path,
            "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":20}\n",
        )
        .unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();

        let summary =
            summarize_paths_with_budget(&[first_path, second_path], now, 7, false, first.len());

        assert_eq!(summary.total_tokens, 10);
        assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    }

    #[test]
    fn token_sum_overflow_marks_coverage_partial_without_saturation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-a.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":18446744073709551615,\"output\":1}\n",
                "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":18446744073709551615}\n",
                "{\"type\":\"usage\",\"timestamp\":1787572800000,\"input\":1}\n"
            ),
        )
        .unwrap();
        let now = Utc.timestamp_millis_opt(1787576400000).single().unwrap();

        let summary = summarize_paths(&[path], now, 7, false);

        assert_eq!(summary.total_tokens, u64::MAX);
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.coverage, LocalHistoryCoverage::Partial);
    }
}
