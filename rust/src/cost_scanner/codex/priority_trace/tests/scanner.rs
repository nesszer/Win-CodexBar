//! Scanner-level Priority pricing, translated from upstream
//! `CostUsageScannerPriorityTests` (v0.65.0), plus the Windows cache edges
//! (fork rows, a vanished database, budget pruning, the trace path).
//!
//! Upstream pins each fixture to a fixed local noon and passes `now`; the
//! Windows scanner always reads a window ending today, so these fixtures sit
//! just before the current time and "time passing" ages the persisted cache.

use super::*;
use crate::core::test_fixtures::test_file_usage;
use crate::core::{
    CodexPriorityTurnMetadata, CodexPriorityTurnsCursor, CodexSourcePricingEvidence,
    CodexSourceRowCache, CodexSourceUsageRow, CostUsageCache, CostUsageCacheBudget,
    CostUsageFileUsage, JsonlScanner, ProviderId,
};
use crate::cost_scanner::codex::ambient_codex_trace_database_path;
use crate::cost_scanner::codex::scan::{codex_priority_metadata_appeared, save_codex_cache};
use crate::cost_scanner::{
    CostScanOptions, CostScanStats, CostScanner, CostSummary, ModelPricingCompleteness,
};
use chrono::{DateTime, Duration, Local};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

type DayModels = HashMap<String, HashMap<String, Vec<i64>>>;

/// gpt-5.5 Standard and Priority cost of upstream's 100/20/10 request.
const GPT55_STANDARD: f64 = 80.0 * 5e-6 + 20.0 * 5e-7 + 10.0 * 3e-5;
const GPT55_PRIORITY: f64 = 80.0 * 1.25e-5 + 20.0 * 1.25e-6 + 10.0 * 7.5e-5;
/// gpt-5.4 Standard and Priority (2x) cost of the same request.
const GPT54_STANDARD: f64 = 80.0 * 2.5e-6 + 20.0 * 2.5e-7 + 10.0 * 1.5e-5;
const GPT54_PRIORITY: f64 = 80.0 * 5e-6 + 20.0 * 5e-7 + 10.0 * 3e-5;

/// An isolated Codex home: `sessions/`, `logs_2.sqlite` and a cache root.
struct Env {
    root: tempfile::TempDir,
    base: DateTime<Local>,
}

impl Env {
    fn new() -> Self {
        let now = Local::now();
        // Keep every fixture event on today's date and not after `now`.
        let day_start = now
            .date_naive()
            .and_hms_opt(0, 0, 1)
            .and_then(|start| start.and_local_timezone(Local).earliest())
            .unwrap_or(now);
        Self {
            root: tempfile::tempdir().unwrap(),
            base: (now - Duration::seconds(30)).max(day_start),
        }
    }

    fn at(&self, offset: i64) -> DateTime<Local> {
        self.base + Duration::seconds(offset)
    }

    fn iso(&self, offset: i64) -> String {
        self.at(offset).to_rfc3339()
    }

    fn epoch(&self, offset: i64) -> i64 {
        self.at(offset).timestamp()
    }

    fn day(&self) -> String {
        self.base.format("%Y-%m-%d").to_string()
    }

    fn sessions(&self) -> PathBuf {
        self.root.path().join("sessions")
    }

    fn db_path(&self) -> PathBuf {
        self.root.path().join(CODEX_TRACE_DATABASE_FILE)
    }

    fn cache_root(&self) -> PathBuf {
        self.root.path().join("cache")
    }

    fn write_session(&self, name: &str, lines: &[Value]) -> PathBuf {
        let day_dir = self
            .sessions()
            .join(self.base.format("%Y").to_string())
            .join(self.base.format("%m").to_string())
            .join(self.base.format("%d").to_string());
        std::fs::create_dir_all(&day_dir).unwrap();
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        let path = day_dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn create_db(&self) -> TraceDb {
        TraceDb::in_dir(self.root.path(), &[])
    }

    fn scan(
        &self,
        options: CostScanOptions,
        database: &Path,
    ) -> (CostSummary, CostScanStats, CostUsageCache) {
        CostScanner::new(7)
            .with_options(options)
            .with_cache_root(self.cache_root())
            .with_sessions_dirs(vec![self.sessions()])
            .with_codex_trace_database(database)
            .scan_codex_detailed_with_cache(None)
    }

    /// Move the last full scan `secs` into the past, like upstream's later
    /// `now` argument, so the next debounced pass may rescan.
    fn age_cache(&self, secs: i64) {
        let root = self.cache_root();
        let mut cache = JsonlScanner::load_cache(ProviderId::Codex, Some(&root));
        cache.last_scan_unix_ms -= secs * 1000;
        JsonlScanner::save_cache(ProviderId::Codex, &mut cache, Some(&root));
    }
}

fn turn_context(timestamp: &str, model: &str) -> Value {
    json!({"type": "turn_context", "timestamp": timestamp, "payload": {"model": model}})
}

fn task_started(timestamp: &str, turn: &str) -> Value {
    json!({
        "type": "event_msg", "timestamp": timestamp,
        "payload": {"type": "task_started", "turn_id": turn}
    })
}

fn usage(input: i64, cached: i64, output: i64) -> Value {
    json!({"input_tokens": input, "cached_input_tokens": cached, "output_tokens": output})
}

fn token_count(timestamp: &str, input: i64, cached: i64, output: i64) -> Value {
    json!({
        "type": "event_msg", "timestamp": timestamp,
        "payload": {"type": "token_count", "info": {"last_token_usage": usage(input, cached, output)}}
    })
}

fn total_token_count(timestamp: &str, input: i64, cached: i64, output: i64) -> Value {
    json!({
        "type": "event_msg", "timestamp": timestamp,
        "payload": {"type": "token_count", "info": {"total_token_usage": usage(input, cached, output)}}
    })
}

/// Upstream `insertPriorityTrace` body.
fn trace_request(turn: &str, model: &str) -> String {
    format!(
        "thread_id=thread turn.id={turn} websocket request: \
         {{\"type\":\"response.create\",\"model\":\"{model}\",\"service_tier\":\"priority\"}}"
    )
}

/// Upstream completed-response body.
fn trace_completed(turn: &str, model: &str) -> String {
    format!(
        "thread_id=thread turn.id={turn} websocket event: \
         {{\"type\":\"response.completed\",\"response\":{{\"model\":\"{model}\"}}}}"
    )
}

/// One Standard turn, then one Priority turn, each a 100/20/10 request.
fn two_turn_session(env: &Env, model: &str) {
    env.write_session(
        "session.jsonl",
        &[
            turn_context(&env.iso(0), model),
            task_started(&env.iso(1), "standard-turn"),
            token_count(&env.iso(2), 100, 20, 10),
            task_started(&env.iso(3), "priority-turn"),
            token_count(&env.iso(3), 100, 20, 10),
        ],
    );
}

/// A single request inside `priority-turn`.
fn priority_session(env: &Env, model: &str, input: i64, cached: i64, output: i64) {
    env.write_session(
        "session.jsonl",
        &[
            turn_context(&env.iso(0), model),
            task_started(&env.iso(1), "priority-turn"),
            token_count(&env.iso(1), input, cached, output),
        ],
    );
}

/// A long Standard request, then a Priority turn whose first request is
/// over the Fast-lane limit and whose second one is under it.
fn long_context_session(env: &Env, model: &str) {
    env.write_session(
        "session.jsonl",
        &[
            turn_context(&env.iso(0), model),
            task_started(&env.iso(1), "standard-turn"),
            token_count(&env.iso(1), 272_001, 0, 10),
            task_started(&env.iso(2), "priority-turn"),
            token_count(&env.iso(2), 300_000, 0, 5),
            token_count(&env.iso(3), 100_001, 0, 5),
        ],
    );
}

#[track_caller]
fn assert_cost(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "cost {actual} != expected {expected}"
    );
}

#[track_caller]
fn assert_exact_cost(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "cost {actual} != expected {expected}"
    );
}

#[test]
fn astra_priority_traces_price_short_and_long_requests() {
    for (input, cached, output, expected) in [
        (100_000, 20_000, 10_000, 2.64),
        (300_000, 100_000, 20_000, 11.4),
    ] {
        let env = Env::new();
        priority_session(&env, "gpt-6-astra", input, cached, output);
        let db = env.create_db();
        db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-6-astra"))]);

        let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

        assert_exact_cost(summary.total_cost_usd, expected);
        assert_exact_cost(summary.by_speed["fast"], expected);
        assert_exact_cost(summary.by_model["gpt-6-astra-priority"], expected);
        assert_eq!(
            summary.by_speed_tokens["fast"].total(),
            u64::try_from(input + output).unwrap()
        );
    }
}

#[test]
fn gpt55_priority_turn_uses_priority_rates() {
    let env = Env::new();
    two_turn_session(&env, "gpt-5.5");
    let db = env.create_db();
    db.insert(&[(env.epoch(3), trace_request("priority-turn", "gpt-5.5"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    assert_cost(summary.total_cost_usd, GPT55_STANDARD + GPT55_PRIORITY);
    assert_cost(summary.by_model["gpt-5.5"], GPT55_STANDARD);
    assert_cost(summary.by_model["gpt-5.5-priority"], GPT55_PRIORITY);
    assert_cost(summary.by_speed["standard"], GPT55_STANDARD);
    assert_cost(summary.by_speed["fast"], GPT55_PRIORITY);
    assert_eq!(summary.by_speed_tokens["standard"].total(), 110);
    assert_eq!(summary.by_speed_tokens["fast"].total(), 110);
}

#[test]
fn gpt56_priority_turn_doubles_the_standard_brief_rate() {
    // The built-in table carries the post-repricing 4/0.4/20 per-million
    // Sol rates (the brief's 5/0.5/30 rates now apply only before 2026-08-21).
    let env = Env::new();
    priority_session(&env, "gpt-5.6-sol", 100_000, 20_000, 20_000);
    let db = env.create_db();
    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.6-sol"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    // Standard total $0.728; API Fast is 2x for GPT-5.6.
    assert_exact_cost(summary.by_speed["fast"], 1.456);
    assert_exact_cost(summary.by_model["gpt-5.6-sol-priority"], 1.456);
}

#[test]
fn cached_priority_surcharge_survives_a_missing_database() {
    let env = Env::new();
    priority_session(&env, "gpt-5.5", 100, 20, 10);
    let db = env.create_db();
    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.5"))]);
    let (first, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);
    assert_cost(first.total_cost_usd, GPT55_PRIORITY);

    let missing = env.root.path().join("missing.sqlite");
    let (cached, stats, _) = env.scan(CostScanOptions::default(), &missing);

    assert!(stats.used_cache_debounce);
    assert_cost(cached.total_cost_usd, GPT55_PRIORITY);
    assert_cost(cached.by_speed["fast"], GPT55_PRIORITY);
    assert_eq!(cached.by_speed_tokens["fast"].total(), 110);
}

#[test]
fn appearing_database_bypasses_the_scan_debounce() {
    let env = Env::new();
    priority_session(&env, "gpt-5.5", 100, 20, 10);
    let db_path = env.db_path();
    let (first, _, cache) = env.scan(CostScanOptions::app_driven(), &db_path);
    assert_cost(first.total_cost_usd, GPT55_STANDARD);
    assert_eq!(
        cache.codex_priority_metadata_key,
        Some(format!("missing:{}", db_path.to_string_lossy()))
    );

    let db = env.create_db();
    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.5"))]);
    let (rescanned, stats, _) = env.scan(CostScanOptions::default(), &db.path);

    assert!(!stats.used_cache_debounce);
    assert_cost(rescanned.total_cost_usd, GPT55_PRIORITY);
}

#[test]
fn unrelated_wal_changes_keep_the_scan_debounce() {
    let env = Env::new();
    priority_session(&env, "gpt-5.5", 100, 20, 10);
    let db = env.create_db();
    let (first, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);
    assert_cost(first.total_cost_usd, GPT55_STANDARD);

    let mut wal = db.path.clone().into_os_string();
    wal.push("-wal");
    std::fs::write(PathBuf::from(wal), b"wal-changed").unwrap();
    let (cached, stats, _) = env.scan(CostScanOptions::default(), &db.path);

    assert!(stats.used_cache_debounce);
    assert_cost(cached.total_cost_usd, GPT55_STANDARD);
}

#[test]
fn new_priority_turn_reprices_the_cached_file() {
    let env = Env::new();
    priority_session(&env, "gpt-5.5", 100, 20, 10);
    let db = env.create_db();
    let (first, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);
    assert_cost(first.total_cost_usd, GPT55_STANDARD);

    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.5"))]);
    env.age_cache(61);
    let (repriced, stats, _) = env.scan(CostScanOptions::default(), &db.path);

    assert!(!stats.used_cache_debounce);
    assert_cost(repriced.total_cost_usd, GPT55_PRIORITY);
}

#[test]
fn gpt54_priority_turn_uses_priority_rates() {
    let env = Env::new();
    two_turn_session(&env, "gpt-5.4");
    let db = env.create_db();
    db.insert(&[(env.epoch(3), trace_request("priority-turn", "gpt-5.4"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    assert_cost(summary.total_cost_usd, GPT54_STANDARD + GPT54_PRIORITY);
    assert_cost(summary.by_model["gpt-5.4"], GPT54_STANDARD);
    assert_cost(summary.by_model["gpt-5.4-priority"], GPT54_PRIORITY);
}

#[test]
fn priority_alias_is_priced_with_the_completed_response_model() {
    let env = Env::new();
    priority_session(&env, "codex-auto-review", 100, 20, 10);
    let db = env.create_db();
    db.insert(&[
        (
            env.epoch(1),
            trace_request("priority-turn", "codex-auto-review"),
        ),
        (env.epoch(1), trace_completed("priority-turn", "gpt-5.4")),
    ]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    assert_cost(summary.total_cost_usd, GPT54_PRIORITY);
    assert_cost(summary.by_model["gpt-5.4-priority"], GPT54_PRIORITY);
    assert_cost(summary.by_speed["fast"], GPT54_PRIORITY);
    assert_eq!(summary.by_speed_tokens["fast"].total(), 110);
    assert_eq!(
        summary.model_pricing_completeness,
        ModelPricingCompleteness::Complete
    );
}

#[test]
fn completed_response_model_prices_the_priority_turn() {
    let env = Env::new();
    priority_session(&env, "gpt-5.4", 100, 20, 10);
    let db = env.create_db();
    db.insert(&[
        (env.epoch(1), trace_request("priority-turn", "gpt-5.4")),
        (env.epoch(1), trace_completed("priority-turn", "gpt-5.5")),
    ]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    assert_cost(summary.total_cost_usd, GPT55_PRIORITY);
    assert_cost(summary.by_model["gpt-5.5-priority"], GPT55_PRIORITY);
    assert!(!summary.by_model.contains_key("gpt-5.4-priority"));
}

#[test]
fn cached_priority_alias_is_repriced_when_the_completion_arrives() {
    let env = Env::new();
    priority_session(&env, "codex-auto-review", 100, 20, 10);
    let db = env.create_db();
    db.insert(&[(
        env.epoch(1),
        trace_request("priority-turn", "codex-auto-review"),
    )]);
    let (first, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);
    // Upstream reports no total; the Windows summary labels it partial.
    assert!(first.total_cost_usd.abs() < 1e-12);
    match &first.model_pricing_completeness {
        ModelPricingCompleteness::Partial { unpriced_models } => {
            assert!(
                unpriced_models
                    .iter()
                    .any(|model| model == "codex-auto-review"),
                "{unpriced_models:?}"
            );
        }
        ModelPricingCompleteness::Complete => panic!("an unpriced alias must be partial"),
    }

    db.insert(&[(env.epoch(1), trace_completed("priority-turn", "gpt-5.4"))]);
    env.age_cache(61);
    let (repriced, stats, _) = env.scan(CostScanOptions::default(), &db.path);

    assert!(!stats.used_cache_debounce);
    assert_cost(repriced.total_cost_usd, GPT54_PRIORITY);
    assert_cost(repriced.by_speed["fast"], GPT54_PRIORITY);
    assert_eq!(repriced.by_speed_tokens["fast"].total(), 110);
    assert_eq!(
        repriced.model_pricing_completeness,
        ModelPricingCompleteness::Complete
    );
}

#[test]
fn unpriced_priority_alias_falls_back_to_the_session_model() {
    let env = Env::new();
    priority_session(&env, "gpt-5.4", 100, 20, 10);
    let db = env.create_db();
    db.insert(&[(
        env.epoch(1),
        trace_request("priority-turn", "codex-auto-review"),
    )]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    assert_cost(summary.total_cost_usd, GPT54_PRIORITY);
    assert_cost(summary.by_model["gpt-5.4-priority"], GPT54_PRIORITY);
    assert_eq!(summary.by_speed_tokens["fast"].total(), 110);
}

#[test]
fn missing_trace_database_keeps_the_base_cost() {
    let env = Env::new();
    priority_session(&env, "gpt-5.5", 100, 20, 10);
    let missing = env.root.path().join("missing.sqlite");

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &missing);

    assert_cost(summary.total_cost_usd, GPT55_STANDARD);
    assert_cost(summary.by_model["gpt-5.5"], GPT55_STANDARD);
    assert!(!summary.by_model.contains_key("gpt-5.5-priority"));
    assert!(!summary.by_speed.contains_key("fast"));
    // Windows deviation: the speed split is always derived from the priced
    // model, so the base cost shows as Standard; upstream leaves both
    // buckets empty when no trace metadata exists.
    assert_cost(summary.by_speed["standard"], GPT55_STANDARD);
}

#[test]
fn priority_row_without_fast_rates_keeps_the_base_cost() {
    let env = Env::new();
    priority_session(&env, "gpt-5.4-nano", 100, 20, 10);
    let db = env.create_db();
    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.4-nano"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    let expected = 80.0 * 2e-7 + 20.0 * 2e-8 + 10.0 * 1.25e-6;
    assert_cost(summary.total_cost_usd, expected);
    assert_cost(summary.by_model["gpt-5.4-nano"], expected);
    // Windows deviation: the day cache keys a row by its priced model, so a
    // Priority row priced at the base rate lands in the Standard bucket;
    // upstream attributes it to the priority bucket. Totals match.
    assert_eq!(summary.by_speed_tokens["standard"].total(), 110);
}

#[test]
fn long_context_priority_rows_skip_the_surcharge() {
    let env = Env::new();
    long_context_session(&env, "gpt-5.5");
    let db = env.create_db();
    db.insert(&[(env.epoch(2), trace_request("priority-turn", "gpt-5.5"))]);

    let (_, _, cache) = env.scan(CostScanOptions::app_driven(), &db.path);

    // Only the second Priority request fits the Fast lane. (The Windows
    // gpt-5.5 table has no long-context tier, so its cost is asserted on
    // gpt-5.6-sol below, which prices both tiers like upstream gpt-5.5.)
    let models = &cache.days[&env.day()];
    assert_eq!(models["gpt-5.5"][..3], [572_001, 0, 15]);
    assert_eq!(models["gpt-5.5-priority"][..3], [100_001, 0, 5]);
}

#[test]
fn long_context_rows_use_long_rates_and_short_priority_rows_use_fast_rates() {
    let env = Env::new();
    long_context_session(&env, "gpt-5.6-sol");
    let db = env.create_db();
    db.insert(&[(env.epoch(2), trace_request("priority-turn", "gpt-5.6-sol"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    let standard_turn = 272_001.0 * 8e-6 + 10.0 * 3e-5;
    let standard_first_row = 300_000.0 * 8e-6 + 5.0 * 3e-5;
    let priority_second_row = (100_001.0 * 4e-6 + 5.0 * 2e-5) * 2.0;
    assert_cost(
        summary.total_cost_usd,
        standard_turn + standard_first_row + priority_second_row,
    );
    assert_cost(summary.by_speed["fast"], priority_second_row);
}

#[test]
fn gpt56_long_context_priority_row_keeps_the_long_base_cost() {
    let env = Env::new();
    priority_session(&env, "gpt-5.6-sol", 272_001, 100_000, 5);
    let db = env.create_db();
    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.6-sol"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    let expected = 172_001.0 * 8e-6 + 100_000.0 * 8e-7 + 5.0 * 3e-5;
    assert_cost(summary.total_cost_usd, expected);
    assert_cost(summary.by_model["gpt-5.6-sol"], expected);
    assert!(!summary.by_speed.contains_key("fast"));
    // Same bucket deviation as the gpt-5.4-nano case above.
    assert_eq!(summary.by_speed_tokens["standard"].total(), 272_006);
}

#[test]
fn cached_reads_do_not_count_toward_the_fast_lane_limit() {
    let env = Env::new();
    priority_session(&env, "gpt-5.5", 200_000, 100_000, 5);
    let db = env.create_db();
    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.5"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    // Cached input is a subset of input, so the 272K limit applies to the
    // 200K input alone and only the 100K uncached input pays the input rate.
    let expected = 100_000.0 * 1.25e-5 + 100_000.0 * 1.25e-6 + 5.0 * 7.5e-5;
    assert_cost(summary.total_cost_usd, expected);
    assert_cost(summary.by_speed["fast"], expected);
    assert_eq!(summary.by_speed_tokens["fast"].total(), 200_005);
}

#[test]
fn cumulative_totals_do_not_trigger_long_context_pricing() {
    let env = Env::new();
    env.write_session(
        "session.jsonl",
        &[
            turn_context(&env.iso(0), "gpt-5.5"),
            task_started(&env.iso(1), "standard-turn"),
            total_token_count(&env.iso(1), 120_000, 60_000, 100),
            total_token_count(&env.iso(2), 240_000, 120_000, 200),
            task_started(&env.iso(3), "priority-turn"),
            total_token_count(&env.iso(3), 360_000, 180_000, 300),
        ],
    );
    let db = env.create_db();
    db.insert(&[(env.epoch(3), trace_request("priority-turn", "gpt-5.5"))]);

    let (summary, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);

    let standard_row = 60_000.0 * 5e-6 + 60_000.0 * 5e-7 + 100.0 * 3e-5;
    let priority_row = 60_000.0 * 1.25e-5 + 60_000.0 * 1.25e-6 + 100.0 * 7.5e-5;
    assert_cost(summary.total_cost_usd, 2.0 * standard_row + priority_row);
}

fn session_meta(timestamp: &str, session_id: &str, forked_from: Option<&str>) -> Value {
    let mut payload = json!({"session_id": session_id});
    if let Some(parent) = forked_from {
        payload["forked_from_id"] = json!(parent);
    }
    json!({"type": "session_meta", "timestamp": timestamp, "payload": payload})
}

fn model_total(timestamp: &str, input: i64, output: i64) -> Value {
    json!({
        "type": "event_msg", "timestamp": timestamp,
        "payload": {"type": "token_count", "info": {
            "model": "gpt-5.5",
            "total_token_usage": {
                "input_tokens": input, "cached_input_tokens": 0, "output_tokens": output
            }
        }}
    })
}

fn set_modified_ago(path: &Path, secs: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn fork_child_growth_is_priced_from_trace_evidence() {
    let env = Env::new();
    let parent = env.write_session(
        "parent.jsonl",
        &[
            session_meta(&env.iso(0), "parent-id", None),
            model_total(&env.iso(0), 1_000, 5),
        ],
    );
    let child = env.write_session(
        "child.jsonl",
        &[
            session_meta(&env.iso(2), "child-id", Some("parent-id")),
            model_total(&env.iso(3), 1_000, 5),
            task_started(&env.iso(4), "child-fast"),
            model_total(&env.iso(5), 1_400, 45),
        ],
    );
    set_modified_ago(&parent, 10);
    set_modified_ago(&child, 5);
    let db = env.create_db();
    db.insert(&[(env.epoch(4), trace_request("child-fast", "gpt-5.5"))]);
    let mut options = CostScanOptions::app_driven();
    options.prefer_newest_codex_sessions_first = false;

    let (_, _, cache) = env.scan(options, &db.path);

    // The child inherits the parent's 1000/5 baseline; only its growth
    // belongs to the Priority turn.
    let models = &cache.days[&env.day()];
    assert_eq!(models["gpt-5.5-priority"][..3], [400, 0, 40]);
    assert_eq!(models["gpt-5.5"][..3], [1_000, 0, 5]);
}

#[test]
fn vanished_database_keeps_its_evidence_and_metadata_key() {
    let env = Env::new();
    priority_session(&env, "gpt-5.5", 100, 20, 10);
    let db = env.create_db();
    db.insert(&[(env.epoch(1), trace_request("priority-turn", "gpt-5.5"))]);
    let (first, _, _) = env.scan(CostScanOptions::app_driven(), &db.path);
    assert_cost(first.total_cost_usd, GPT55_PRIORITY);

    std::fs::rename(&db.path, env.root.path().join("moved.sqlite")).unwrap();
    let (rescanned, stats, cache) = env.scan(CostScanOptions::app_driven(), &db.path);

    assert!(!stats.used_cache_debounce);
    assert_cost(rescanned.total_cost_usd, GPT55_PRIORITY);
    // Validation stays pending, so the key still names the observed database.
    assert_eq!(
        cache.codex_priority_metadata_key,
        Some(format!("sqlite:{}", db.path.to_string_lossy()))
    );
}

fn file_usage(days: DayModels) -> CostUsageFileUsage {
    CostUsageFileUsage {
        parsed_bytes: Some(10),
        ..test_file_usage(10, days)
    }
}

fn one_day(day: &str, model: &str, tokens: [i64; 3]) -> DayModels {
    HashMap::from([(
        day.to_string(),
        HashMap::from([(model.to_string(), tokens.to_vec())]),
    )])
}

fn standard_row(day: &str, input: i64, turn: &str) -> CodexSourceUsageRow {
    CodexSourceUsageRow {
        day_key: day.to_string(),
        timestamp: None,
        model: "gpt-5.5".to_string(),
        input,
        cached: 0,
        output: 5,
        reasoning: None,
        source_end_offset: 10,
        turn_id: Some(turn.to_string()),
        pricing: CodexSourcePricingEvidence {
            pricing_model: Some("gpt-5.5".to_string()),
            pricing_mode: Some("standard".to_string()),
        },
    }
}

#[track_caller]
fn assert_only_day(days: &DayModels, day: &str, model: &str, tokens: [i64; 3]) {
    assert_eq!(days.len(), 1, "{days:?}");
    let models = &days[day];
    assert_eq!(models.len(), 1, "{models:?}");
    assert_eq!(models[model][..3], tokens);
}

#[test]
fn budget_pruning_rebuilds_priority_days_from_retained_files() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let sessions = home.join("sessions");
    let key = |name: &str| sessions.join(name).to_string_lossy().into_owned();
    let mut cache = CostUsageCache::default();
    for index in 0..CostUsageCacheBudget::MAX_FILE_ENTRIES {
        cache.files.insert(
            key(&format!("filler-{index}.jsonl")),
            file_usage(HashMap::new()),
        );
    }
    let old = key("old.jsonl");
    let new = key("new.jsonl");
    let mut cursor = CodexPriorityTurnsCursor {
        database_path: home
            .join(CODEX_TRACE_DATABASE_FILE)
            .to_string_lossy()
            .into_owned(),
        ..CodexPriorityTurnsCursor::default()
    };
    for (row_id, path, day, input, turn) in [
        (1, &old, "2026-08-01", 100, "turn-old"),
        (2, &new, "2026-09-15", 50, "turn-new"),
    ] {
        cache.files.insert(
            path.clone(),
            file_usage(one_day(day, "gpt-5.5", [input, 0, 5])),
        );
        cache.codex_source_rows.insert(
            path.clone(),
            CodexSourceRowCache {
                file_identity: String::new(),
                size: 10,
                mtime_unix_ms: 0,
                prefix_hash: 0,
                rows: vec![standard_row(day, input, turn)],
            },
        );
        cursor.request_sources.insert(
            turn.to_string(),
            BTreeMap::from([(
                row_id,
                CodexPriorityTurnMetadata {
                    turn_id: turn.to_string(),
                    model: Some("gpt-5.5".to_string()),
                    ..CodexPriorityTurnMetadata::default()
                },
            )]),
        );
        cache
            .days
            .extend(one_day(day, "gpt-5.5-priority", [input, 0, 5]));
    }
    cache.codex_priority_turns_cursor = Some(cursor);
    cache.scan_since_key = Some("2026-09-01".to_string());
    cache.scan_until_key = Some("2026-09-30".to_string());
    let cache_root = home.join("cache");

    save_codex_cache(&mut cache, Some(&cache_root));

    // Pruning drops the fillers and the out-of-window file; subtracting the
    // old file's plain totals cannot undo its overlay, so the aggregate is
    // rebuilt from the retained file instead.
    assert_eq!(cache.files.len(), 1);
    assert!(cache.files.contains_key(&new));
    assert_only_day(&cache.days, "2026-09-15", "gpt-5.5-priority", [50, 0, 5]);
    let reloaded = JsonlScanner::load_cache(ProviderId::Codex, Some(&cache_root));
    assert_only_day(&reloaded.days, "2026-09-15", "gpt-5.5-priority", [50, 0, 5]);
}

#[test]
fn priority_metadata_appears_only_for_a_new_database() {
    let appeared = codex_priority_metadata_appeared;
    assert!(!appeared(None, None));
    // The first scan has nothing to compare against.
    assert!(!appeared(None, Some("sqlite:/a")));
    assert!(appeared(Some("missing:/a"), Some("sqlite:/a")));
    assert!(appeared(Some("sqlite:/a"), Some("sqlite:/b")));
    assert!(!appeared(Some("sqlite:/a"), Some("sqlite:/a")));
    // A database that went away keeps the cached evidence.
    assert!(!appeared(Some("sqlite:/a"), Some("missing:/a")));
    assert!(!appeared(Some("sqlite:/a"), None));
}

#[test]
fn trace_database_path_follows_codex_home() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let codex_home = root.path().join("codex-home");
    let default_path = home.join(".codex").join(CODEX_TRACE_DATABASE_FILE);

    assert_eq!(
        ambient_codex_trace_database_path(
            Some(format!("  {}  ", codex_home.display())),
            Some(home.clone())
        ),
        Some(codex_home.join(CODEX_TRACE_DATABASE_FILE))
    );
    assert_eq!(
        ambient_codex_trace_database_path(Some("   ".to_string()), Some(home.clone())),
        Some(default_path.clone())
    );
    assert_eq!(
        ambient_codex_trace_database_path(None, Some(home)),
        Some(default_path)
    );
    assert_eq!(ambient_codex_trace_database_path(None, None), None);

    // Tests and injected session roots never read the ambient database.
    assert_eq!(CostScanner::new(7).codex_trace_database_path(), None);
    assert_eq!(
        CostScanner::new(7)
            .with_sessions_dirs(vec![root.path().join("sessions")])
            .codex_trace_database_path(),
        None
    );
    let configured = root.path().join("configured.sqlite");
    assert_eq!(
        CostScanner::new(7)
            .with_codex_trace_database(&configured)
            .codex_trace_database_path(),
        Some(configured)
    );
}
