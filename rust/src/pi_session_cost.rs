//! Pi-compatible + OMP agent session cost scan (upstream #2269).
//!
//! Resolves Pi-family session roots and walks their JSONL files, attributing
//! openai-codex / anthropic assistant rows into cost summaries without
//! double-counting the same entry id across shared files.

use chrono::{DateTime, Duration, Local, Utc};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::agent_sessions::pi_family::roots::{
    EnvMap, PiProfile, omp_all_profile_roots, omp_default_profile_root, omp_named_profile_root,
    omp_profile_selector, pi_settings_session_directory,
};
use crate::core::CostUsagePricing;
use crate::cost_scanner::{CostSummary, ModelPricingCompleteness};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiMappedProvider {
    Codex,
    Claude,
}

#[derive(Debug, Default)]
pub struct PiDailyScan {
    pub costs: std::collections::HashMap<String, f64>,
    pub tokens: std::collections::HashMap<String, u64>,
    pub unpriced_days: HashSet<String>,
    pub history_coverage_established: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PiScanEvidence {
    pub complete: bool,
}

impl Default for PiScanEvidence {
    fn default() -> Self {
        Self { complete: true }
    }
}

/// Session roots to scan: `.pi` and `.omp` under the user home.
pub fn pi_compatible_session_roots(home: Option<PathBuf>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| home.clone());
    let environment: EnvMap = std::env::vars().collect();
    pi_compatible_session_roots_for(&home, &cwd, &environment)
}

fn pi_compatible_session_roots_for(home: &Path, cwd: &Path, environment: &EnvMap) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let pi_root = environment
        .get("PI_CODING_AGENT_SESSION_DIR")
        .and_then(|value| resolve_environment_path(value, cwd))
        .or_else(|| {
            environment
                .get("PI_CODING_AGENT_DIR")
                .and_then(|value| resolve_environment_path(value, cwd))
                .map(|root| root.join("sessions"))
        })
        .or_else(|| pi_settings_session_directory(cwd, home))
        .unwrap_or_else(|| home.join(".pi").join("agent").join("sessions"));
    roots.push(pi_root);

    match omp_profile_selector(environment) {
        PiProfile::Invalid => {}
        PiProfile::Named(profile) => {
            if let Some(root) = omp_named_profile_root(&profile, environment, cwd, home) {
                roots.push(root);
            }
        }
        PiProfile::Default => {
            if let Some(root) = omp_default_profile_root(environment, cwd, home) {
                roots.push(root);
            }
            roots.extend(omp_all_profile_roots(environment, home));
        }
    }

    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|root| {
            let key = std::fs::canonicalize(root)
                .unwrap_or_else(|_| root.clone())
                .to_string_lossy()
                .to_ascii_lowercase();
            seen.insert(key)
        })
        .collect()
}

fn resolve_environment_path(value: &str, cwd: &Path) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    Some(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}

pub fn scan_pi_compatible_into(
    summary: &mut CostSummary,
    target: PiMappedProvider,
    days: u32,
    cancel: Option<&AtomicBool>,
    seen_entries: &mut HashSet<String>,
) -> PiScanEvidence {
    scan_roots_into(
        summary,
        days,
        cancel,
        seen_entries,
        pi_compatible_session_roots(dirs::home_dir()),
        Some(target),
    )
}

/// Scan Pi and OMP history as one standalone provider-owned source.
///
/// The compatible Codex/Claude paths above intentionally project these rows
/// into their native summaries. This path keeps the same parser and pricing,
/// but accepts both mapped providers and shares one deduplication set across
/// both roots so standalone Pi history is not double-counted.
pub fn scan_pi_into(
    summary: &mut CostSummary,
    days: u32,
    cancel: Option<&AtomicBool>,
    seen_entries: &mut HashSet<String>,
) -> PiScanEvidence {
    scan_roots_into(
        summary,
        days,
        cancel,
        seen_entries,
        pi_compatible_session_roots(dirs::home_dir()),
        None,
    )
}

/// Scan standalone Pi/OMP history into daily cost and token buckets.
pub fn scan_pi_daily(days: u32, cancel: Option<&AtomicBool>) -> PiDailyScan {
    let cutoff = Utc::now() - Duration::days(days as i64);
    scan_pi_daily_from_roots(
        cutoff,
        cancel,
        pi_compatible_session_roots(dirs::home_dir()),
    )
}

fn scan_pi_daily_from_roots(
    cutoff: DateTime<Utc>,
    cancel: Option<&AtomicBool>,
    roots: Vec<PathBuf>,
) -> PiDailyScan {
    let mut result = PiDailyScan::default();
    let mut seen_entries = HashSet::new();
    let mut missing_timestamp = false;
    let mut evidence = PiScanEvidence::default();
    for root in roots {
        if cancelled(cancel) {
            break;
        }
        if !root.is_dir() {
            continue;
        }
        if !walk_jsonl(&root, cancel, &mut |path| {
            if cancelled(cancel) {
                return false;
            }
            let file = for_each_pi_entry(path, cutoff, None, &mut seen_entries, |entry| {
                let Some(timestamp) = entry.timestamp else {
                    missing_timestamp = true;
                    return;
                };
                let day = timestamp
                    .with_timezone(&Local)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string();
                if !entry.pricing_known {
                    result.unpriced_days.insert(day.clone());
                }
                *result.costs.entry(day.clone()).or_insert(0.0) += entry.cost;
                let tokens = entry.input.saturating_add(entry.output);
                let total = result.tokens.entry(day.clone()).or_insert(0);
                *total = total.saturating_add(tokens);
            });
            file.complete
        }) {
            evidence.complete = false;
        }
    }
    result.history_coverage_established =
        evidence.complete && !cancelled(cancel) && !missing_timestamp;
    result
}

fn scan_roots_into(
    summary: &mut CostSummary,
    days: u32,
    cancel: Option<&AtomicBool>,
    seen_entries: &mut HashSet<String>,
    roots: Vec<PathBuf>,
    target: Option<PiMappedProvider>,
) -> PiScanEvidence {
    let cutoff = Utc::now() - Duration::days(days as i64);
    let mut sessions = 0u32;
    let mut evidence = PiScanEvidence::default();
    for root in roots {
        if cancelled(cancel) {
            break;
        }
        if !root.is_dir() {
            continue;
        }
        if !walk_jsonl(&root, cancel, &mut |path| {
            if cancelled(cancel) {
                return false;
            }
            let before = seen_entries.len();
            let file = for_each_pi_entry(path, cutoff, target, seen_entries, |entry| {
                apply_entry(summary, &entry);
            });
            if file.counted > 0 || seen_entries.len() > before {
                sessions += 1;
            }
            file.complete
        }) {
            evidence.complete = false;
        }
    }
    summary.sessions_count = summary.sessions_count.saturating_add(sessions);
    evidence
}

struct PiEntry {
    timestamp: Option<DateTime<Utc>>,
    provider: PiMappedProvider,
    model: String,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create: u64,
    cost: f64,
    pricing_known: bool,
}

fn apply_entry(summary: &mut CostSummary, entry: &PiEntry) {
    if !entry.pricing_known {
        summary.unknown_models.insert(entry.model.clone());
        match &mut summary.model_pricing_completeness {
            ModelPricingCompleteness::Complete => {
                summary.model_pricing_completeness = ModelPricingCompleteness::Partial {
                    unpriced_models: vec![entry.model.clone()],
                };
            }
            ModelPricingCompleteness::Partial { unpriced_models } => {
                if !unpriced_models.contains(&entry.model) {
                    unpriced_models.push(entry.model.clone());
                }
            }
        }
    }
    summary.input_tokens += entry.input;
    summary.output_tokens += entry.output;
    summary.cached_tokens += entry.cache_read + entry.cache_create;
    summary.total_cost_usd += entry.cost;
    *summary.by_model.entry(entry.model.clone()).or_insert(0.0) += entry.cost;
    let tokens = summary
        .by_model_tokens
        .entry(entry.model.clone())
        .or_default();
    tokens.input_tokens += entry.input;
    tokens.output_tokens += entry.output;
    tokens.cached_tokens += entry.cache_read + entry.cache_create;
}

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
}

fn walk_jsonl(
    root: &Path,
    cancel: Option<&AtomicBool>,
    on_file: &mut dyn FnMut(&Path) -> bool,
) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    let mut complete = true;
    for entry in entries {
        if cancelled(cancel) {
            return false;
        }
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let path = entry.path();
        if path.is_dir() {
            complete = walk_jsonl(&path, cancel, on_file) && complete;
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
        {
            complete = on_file(&path) && complete;
        }
    }
    complete
}

#[derive(Debug, Clone, Copy)]
struct PiFileScanResult {
    counted: u32,
    complete: bool,
}

fn for_each_pi_entry(
    path: &Path,
    cutoff: DateTime<Utc>,
    target: Option<PiMappedProvider>,
    seen: &mut HashSet<String>,
    mut on_entry: impl FnMut(PiEntry),
) -> PiFileScanResult {
    let Ok(file) = File::open(path) else {
        return PiFileScanResult {
            counted: 0,
            complete: false,
        };
    };
    let mut counted = 0u32;
    let mut complete = true;
    let mut session_id = None;
    let reader = BufReader::new(file);
    for (ordinal, line_result) in reader.lines().enumerate() {
        let Ok(line) = line_result else {
            complete = false;
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            complete = false;
            continue;
        };
        if session_id.is_none() {
            session_id = value
                .get("id")
                .and_then(|id| id.as_str())
                .filter(|_| value.get("type").and_then(Value::as_str) == Some("session"))
                .map(str::to_string);
        }
        let Some(entry) = parse_pi_assistant_entry_any(&value) else {
            if looks_like_usage_candidate(&value) {
                complete = false;
            }
            continue;
        };
        if entry.timestamp.is_none() {
            complete = false;
        }
        if let Some(ts) = entry_timestamp(&value)
            && ts < cutoff
        {
            continue;
        }
        if target.is_some_and(|target| entry.provider != target) {
            continue;
        }
        let entry_id = entry_dedup_key(&value, path, ordinal, session_id.as_deref());
        if !seen.insert(entry_id) {
            continue;
        }
        on_entry(entry);
        counted += 1;
    }
    PiFileScanResult { counted, complete }
}

fn looks_like_usage_candidate(value: &Value) -> bool {
    let message = value.get("message").unwrap_or(value);
    let role = message
        .get("role")
        .or_else(|| value.get("role"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let typ = value.get("type").and_then(Value::as_str).unwrap_or("");
    let assistant_shape = role.eq_ignore_ascii_case("assistant")
        || typ.eq_ignore_ascii_case("assistant")
        || typ.eq_ignore_ascii_case("message");
    assistant_shape
        && message
            .get("usage")
            .or_else(|| value.get("usage"))
            .is_some()
}

fn entry_dedup_key(value: &Value, path: &Path, ordinal: usize, session_id: Option<&str>) -> String {
    // Pi/OMP migrations can mirror the same event into both roots, so a
    // stable event id wins within the logical session. Scope it with the
    // session header when available: message IDs can be reused by separate
    // sessions. The file stem and physical line ordinal cover legacy rows
    // without a session header or stable message id.
    if let Some(id) = value
        .get("id")
        .or_else(|| value.get("messageId"))
        .or_else(|| value.pointer("/message/id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let scope = session_id
            .map(str::to_string)
            .or_else(|| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| path.display().to_string());
        return format!("{scope}#{id}");
    }
    format!("{}#{ordinal}", path.display())
}

fn entry_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let raw = value
        .get("timestamp")
        .or_else(|| value.get("createdAt"))
        .or_else(|| value.pointer("/message/timestamp"))
        .and_then(|v| v.as_str())?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            // epoch ms
            value
                .get("timestamp")
                .and_then(|v| v.as_i64())
                .and_then(DateTime::from_timestamp_millis)
        })
}

fn map_provider(raw: &str) -> Option<PiMappedProvider> {
    let n = raw.trim().to_ascii_lowercase();
    if n.contains("openai-codex") || n == "codex" || n == "openai" {
        return Some(PiMappedProvider::Codex);
    }
    if n.contains("anthropic") || n.contains("claude") {
        return Some(PiMappedProvider::Claude);
    }
    None
}

#[cfg(test)]
fn parse_pi_assistant_entry(value: &Value, target: PiMappedProvider) -> Option<PiEntry> {
    let entry = parse_pi_assistant_entry_any(value)?;
    (entry.provider == target).then_some(entry)
}

fn parse_pi_assistant_entry_any(value: &Value) -> Option<PiEntry> {
    // Accept either flat or nested { message: {...} } pi-compatible rows.
    let message = value.get("message").unwrap_or(value);
    let role = message
        .get("role")
        .or_else(|| value.get("role"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !role.eq_ignore_ascii_case("assistant") {
        // Some pi rows use type=message with role nested differently.
        let typ = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if typ != "assistant" && typ != "message" {
            return None;
        }
        if !role.is_empty() && !role.eq_ignore_ascii_case("assistant") {
            return None;
        }
    }

    let provider_raw = message
        .get("provider")
        .or_else(|| value.get("provider"))
        .or_else(|| message.get("api"))
        .or_else(|| value.get("api"))
        .and_then(|v| v.as_str())?;
    let mapped = map_provider(provider_raw)?;
    let model = message
        .get("model")
        .or_else(|| message.get("modelId"))
        .or_else(|| value.get("model"))
        .or_else(|| value.get("modelId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();

    let usage = message
        .get("usage")
        .or_else(|| value.get("usage"))
        .cloned()
        .unwrap_or(Value::Null);

    let input = num(
        &usage,
        &["input", "inputTokens", "input_tokens", "promptTokens"],
    );
    let output = num(
        &usage,
        &[
            "output",
            "outputTokens",
            "output_tokens",
            "completionTokens",
        ],
    );
    let cache_read = num(
        &usage,
        &[
            "cacheRead",
            "cache_read",
            "cache_read_input_tokens",
            "cached",
        ],
    );
    let cache_create = num(
        &usage,
        &[
            "cacheWrite",
            "cache_write",
            "cache_creation_input_tokens",
            "cacheCreate",
        ],
    );
    if input == 0 && output == 0 && cache_read == 0 && cache_create == 0 {
        return None;
    }

    let (cost, pricing_known) = match mapped {
        PiMappedProvider::Codex => match CostUsagePricing::codex_cost_usd_with_cache_write(
            &model,
            input,
            cache_read,
            cache_create,
            output,
        ) {
            Some(cost) => (cost, true),
            None => (0.0, false),
        },
        PiMappedProvider::Claude => {
            // Token counts come from API usage records and fit within i32;
            // the canonical Claude pricing table takes i32 per-token counts.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "token counts clamped to i32::MAX above"
            )]
            #[allow(
                clippy::cast_possible_wrap,
                reason = "token counts are non-negative; wrapping is impossible"
            )]
            let (input, cache_read, cache_create, output) = (
                input.min(i32::MAX as u64) as i32,
                cache_read.min(i32::MAX as u64) as i32,
                cache_create.min(i32::MAX as u64) as i32,
                output.min(i32::MAX as u64) as i32,
            );
            if let Some(cost) =
                CostUsagePricing::claude_cost_usd(&model, input, cache_read, cache_create, output)
            {
                (cost, true)
            } else {
                // Preserve the legacy fallback amount for continuity, while
                // marking the model unknown so callers do not present it as
                // canonical pricing.
                let fallback = CostUsagePricing::claude_cost_usd(
                    "claude-sonnet-4-6",
                    input,
                    cache_read,
                    cache_create,
                    output,
                )
                .unwrap_or(0.0);
                (fallback, false)
            }
        }
    };

    Some(PiEntry {
        timestamp: entry_timestamp(value),
        provider: mapped,
        model,
        input,
        output,
        cache_read,
        cache_create,
        cost,
        pricing_known,
    })
}

fn num(usage: &Value, keys: &[&str]) -> u64 {
    for key in keys {
        if let Some(v) = usage.get(*key) {
            if let Some(n) = v.as_u64() {
                return n;
            }
            if let Some(n) = v.as_i64() {
                return n.max(0) as u64;
            }
            if let Some(n) = v.as_f64() {
                // Usage counts are whole tokens; dropping any fractional part
                // matches the previous cast behavior exactly.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "token counts are whole numbers; fractional part is rounding noise"
                )]
                return n.max(0.0) as u64;
            }
            if let Some(s) = v.as_str()
                && let Ok(n) = s.parse::<u64>()
            {
                return n;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn maps_openai_codex_and_anthropic_providers() {
        assert_eq!(
            map_provider("openai-codex-responses"),
            Some(PiMappedProvider::Codex)
        );
        assert_eq!(map_provider("anthropic"), Some(PiMappedProvider::Claude));
        assert_eq!(map_provider("google"), None);
    }

    #[test]
    fn parses_assistant_usage_row() {
        let raw = serde_json::json!({
            "id": "msg-1",
            "role": "assistant",
            "provider": "openai-codex",
            "model": "gpt-5",
            "timestamp": "2026-07-20T12:00:00Z",
            "usage": { "input": 100, "output": 20, "cacheRead": 10 }
        });
        let entry = parse_pi_assistant_entry(&raw, PiMappedProvider::Codex).unwrap();
        assert_eq!(entry.input, 100);
        assert_eq!(entry.output, 20);
        assert_eq!(entry.cache_read, 10);
        assert_eq!(entry.model, "gpt-5");
        assert!(entry.pricing_known);
    }

    #[test]
    fn parses_astra_cache_write_and_prices_it() {
        let raw = serde_json::json!({
            "id": "astra-msg-1",
            "role": "assistant",
            "provider": "openai-codex",
            "model": "gpt-6-astra",
            "usage": {
                "input": 1_000,
                "output": 100,
                "cacheRead": 200,
                "cacheWrite": 300
            }
        });
        let entry = parse_pi_assistant_entry(&raw, PiMappedProvider::Codex).unwrap();
        let expected = 500.0 * 1e-5 + 200.0 * 1e-6 + 300.0 * 1.25e-5 + 100.0 * 5e-5;
        assert_eq!(entry.cache_create, 300);
        assert!((entry.cost - expected).abs() < 1e-12);
        assert!(entry.pricing_known);
    }

    #[test]
    fn unknown_model_keeps_tokens_and_marks_pricing_incomplete() {
        let raw = serde_json::json!({
            "id": "unknown-model-1",
            "role": "assistant",
            "provider": "openai-codex",
            "model": "future-model-without-a-rate",
            "usage": { "input": 11, "output": 3 }
        });
        let entry = parse_pi_assistant_entry_any(&raw).unwrap();
        assert!(!entry.pricing_known);
        assert_eq!(entry.cost, 0.0);

        let mut summary = CostSummary::default();
        apply_entry(&mut summary, &entry);
        assert_eq!(summary.input_tokens, 11);
        assert_eq!(summary.output_tokens, 3);
        assert!(
            summary
                .unknown_models
                .contains("future-model-without-a-rate")
        );
        assert!(summary.model_pricing_completeness.is_partial());
    }

    #[test]
    fn standalone_parser_keeps_the_mapped_provider_for_mixed_history() {
        let codex = serde_json::json!({
            "id": "codex-1",
            "role": "assistant",
            "provider": "openai-codex",
            "model": "gpt-5",
            "usage": { "input": 100, "output": 20 }
        });
        let claude = serde_json::json!({
            "id": "claude-1",
            "role": "assistant",
            "provider": "anthropic",
            "model": "claude-sonnet-4-6",
            "usage": { "input": 100, "output": 20 }
        });

        assert_eq!(
            parse_pi_assistant_entry_any(&codex)
                .expect("Codex row should parse")
                .provider,
            PiMappedProvider::Codex
        );
        assert_eq!(
            parse_pi_assistant_entry_any(&claude)
                .expect("Claude row should parse")
                .provider,
            PiMappedProvider::Claude
        );
    }

    #[test]
    fn dedupes_shared_entry_ids_across_files() {
        let dir = tempdir().unwrap();
        let sessions = dir.path().join("agent").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let header = r#"{"type":"session","id":"session-1"}"#;
        let line = r#"{"id":"shared-1","role":"assistant","provider":"openai-codex","model":"gpt-5","timestamp":"2026-07-20T12:00:00Z","usage":{"input":50,"output":5}}"#;
        for name in ["a.jsonl", "b.jsonl"] {
            let mut f = File::create(sessions.join(name)).unwrap();
            writeln!(f, "{header}").unwrap();
            writeln!(f, "{line}").unwrap();
        }

        // Point home at temp so roots resolve under .omp
        let home = dir.path().to_path_buf();
        // Manually walk the sessions we created via for_each
        let mut seen = HashSet::new();
        let mut summary = CostSummary::default();
        let mut total = 0u32;
        for name in ["a.jsonl", "b.jsonl"] {
            total += for_each_pi_entry(
                &sessions.join(name),
                DateTime::parse_from_rfc3339("2026-07-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                Some(PiMappedProvider::Codex),
                &mut seen,
                |entry| apply_entry(&mut summary, &entry),
            )
            .counted;
        }
        assert_eq!(seen.len(), 1);
        assert_eq!(summary.input_tokens, 50);
        assert_eq!(total, 1); // second file deduped
        let _ = home;
    }

    #[test]
    fn same_message_id_in_distinct_sessions_is_not_deduped() {
        let dir = tempdir().unwrap();
        let sessions = dir.path().join("agent").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let line = r#"{"id":"reused-message","role":"assistant","provider":"openai-codex","model":"gpt-5","timestamp":"2026-07-20T12:00:00Z","usage":{"input":50,"output":5}}"#;
        for session in ["session-a", "session-b"] {
            let mut f = File::create(sessions.join(format!("{session}.jsonl"))).unwrap();
            writeln!(f, "{{\"type\":\"session\",\"id\":\"{session}\"}}").unwrap();
            writeln!(f, "{line}").unwrap();
        }

        let cutoff = DateTime::parse_from_rfc3339("2026-07-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut seen = HashSet::new();
        let mut summary = CostSummary::default();
        let mut total = 0u32;
        for session in ["session-a", "session-b"] {
            total += for_each_pi_entry(
                &sessions.join(format!("{session}.jsonl")),
                cutoff,
                Some(PiMappedProvider::Codex),
                &mut seen,
                |entry| apply_entry(&mut summary, &entry),
            )
            .counted;
        }
        assert_eq!(total, 2);
        assert_eq!(summary.input_tokens, 100);
    }

    #[test]
    fn configured_roots_include_explicit_pi_and_omp_profile_sources() {
        let home = tempdir().unwrap();
        let cwd = home.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let mut environment = EnvMap::new();
        environment.insert(
            "PI_CODING_AGENT_SESSION_DIR".to_string(),
            "pi-sessions".to_string(),
        );
        environment.insert("PI_CONFIG_DIR".to_string(), "config".to_string());
        environment.insert("OMP_PROFILE".to_string(), "work".to_string());

        let roots = pi_compatible_session_roots_for(home.path(), &cwd, &environment);
        let pi_root = cwd.join("pi-sessions");
        let omp_root = home
            .path()
            .join("config")
            .join("profiles")
            .join("work")
            .join("agent")
            .join("sessions");
        assert!(roots.iter().any(|root| root == &pi_root));
        assert!(roots.iter().any(|root| root == &omp_root));

        let duplicate = concat!(
            r#"{"type":"session","id":"session-1"}"#,
            "\n",
            r#"{"id":"shared","role":"assistant","provider":"openai-codex","model":"gpt-5","timestamp":"2026-07-20T12:00:00Z","usage":{"input":50,"output":5}}"#,
            "\n"
        );
        let unique = concat!(
            r#"{"type":"session","id":"session-2"}"#,
            "\n",
            r#"{"id":"unique","role":"assistant","provider":"anthropic","model":"claude-sonnet-4-6","timestamp":"2026-07-20T13:00:00Z","usage":{"input":70,"output":7}}"#,
            "\n"
        );
        std::fs::create_dir_all(&pi_root).unwrap();
        std::fs::create_dir_all(&omp_root).unwrap();
        std::fs::write(pi_root.join("session.jsonl"), duplicate).unwrap();
        std::fs::write(omp_root.join("session.jsonl"), duplicate).unwrap();
        std::fs::write(omp_root.join("unique.jsonl"), unique).unwrap();

        let scan = scan_pi_daily_from_roots(
            DateTime::parse_from_rfc3339("2026-07-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            None,
            roots,
        );
        assert!(scan.history_coverage_established);
        assert_eq!(scan.tokens.values().sum::<u64>(), 132);
    }

    #[test]
    fn malformed_usage_input_keeps_valid_rows_but_marks_source_incomplete() {
        let dir = tempdir().unwrap();
        let sessions = dir.path().join("agent").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let valid = r#"{"id":"valid","role":"assistant","provider":"openai-codex","model":"gpt-5","timestamp":"2026-07-20T12:00:00Z","usage":{"input":11,"output":3}}"#;
        std::fs::write(
            sessions.join("mixed.jsonl"),
            format!("{valid}\n{{\"role\":\"assistant\",\"usage\":\n"),
        )
        .unwrap();

        let scan = scan_pi_daily_from_roots(
            DateTime::parse_from_rfc3339("2026-07-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            None,
            vec![sessions.clone()],
        );
        assert!(!scan.history_coverage_established);
        assert_eq!(scan.tokens.values().sum::<u64>(), 14);

        let mut summary = CostSummary::default();
        let mut seen = HashSet::new();
        let evidence = scan_roots_into(&mut summary, 365, None, &mut seen, vec![sessions], None);
        assert!(!evidence.complete);
        assert_eq!(summary.input_tokens, 11);
    }

    #[test]
    fn standalone_daily_scan_dedupes_pi_and_omp_roots() {
        let dir = tempdir().unwrap();
        let pi_sessions = dir.path().join(".pi").join("agent").join("sessions");
        let omp_sessions = dir.path().join(".omp").join("agent").join("sessions");
        std::fs::create_dir_all(&pi_sessions).unwrap();
        std::fs::create_dir_all(&omp_sessions).unwrap();
        let codex = r#"{"id":"shared","role":"assistant","provider":"openai-codex","model":"gpt-5","timestamp":"2026-07-20T12:00:00Z","usage":{"input":50,"output":5}}"#;
        let claude = r#"{"id":"claude-only","role":"assistant","provider":"anthropic","model":"claude-sonnet-4-6","timestamp":"2026-07-20T13:00:00Z","usage":{"input":70,"output":7}}"#;
        std::fs::write(
            pi_sessions.join("one.jsonl"),
            format!("{codex}\n{claude}\n"),
        )
        .unwrap();
        std::fs::write(omp_sessions.join("one.jsonl"), format!("{codex}\n")).unwrap();

        let scan = scan_pi_daily_from_roots(
            DateTime::parse_from_rfc3339("2026-07-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            None,
            vec![pi_sessions, omp_sessions],
        );
        assert!(scan.history_coverage_established);
        assert_eq!(scan.tokens.values().sum::<u64>(), 132);
        assert_eq!(scan.tokens.len(), 1);
    }

    #[test]
    fn session_roots_include_pi_and_omp() {
        let home = PathBuf::from("/home/user");
        let roots = pi_compatible_session_roots_for(&home, &home, &EnvMap::new());
        assert!(
            roots
                .iter()
                .any(|p| p.ends_with(".pi/agent/sessions") || p.ends_with(".pi\\agent\\sessions"))
        );
        assert!(roots.iter().any(|p| p.ends_with(".omp/agent/sessions") || p.ends_with(".omp\\agent\\sessions")));
    }
}
