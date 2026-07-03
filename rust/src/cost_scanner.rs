//! Local cost-usage scanner for Codex and Claude
//!
//! Scans local JSONL log files to aggregate token usage and calculate costs

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::{CostUsageDayRange, CostUsagePricing, JsonlScanner};

/// Cost summary from scanning local logs
#[derive(Debug, Clone, Default)]
pub struct CostSummary {
    /// Total cost in USD for the period
    pub total_cost_usd: f64,
    /// Total input tokens
    pub input_tokens: u64,
    /// Total output tokens
    pub output_tokens: u64,
    /// Total cached input tokens
    pub cached_tokens: u64,
    /// Number of sessions/conversations scanned
    pub sessions_count: u32,
    /// Cost breakdown by model
    pub by_model: HashMap<String, f64>,
    /// Token breakdown by model
    pub by_model_tokens: HashMap<String, ModelTokenCounts>,
    /// Codex cost split by speed/tier when local logs expose it.
    pub by_speed: HashMap<String, f64>,
    /// Codex token split by speed/tier when local logs expose it.
    pub by_speed_tokens: HashMap<String, ModelTokenCounts>,
    /// Period start date
    pub period_start: Option<NaiveDate>,
    /// Period end date
    pub period_end: Option<NaiveDate>,
}

/// Per-model token counts
#[derive(Debug, Clone, Default)]
pub struct ModelTokenCounts {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
}

impl ModelTokenCounts {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

impl CostSummary {
    pub fn format_total(&self) -> String {
        format!("${:.2}", self.total_cost_usd)
    }
}

fn codex_speed_bucket(model: &str) -> &'static str {
    let normalized = model.to_ascii_lowercase();
    if normalized.contains("fast")
        || normalized.contains("priority")
        || normalized.contains("spark")
        || normalized.contains("smoke")
    {
        "fast"
    } else {
        "standard"
    }
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// Codex token pricing (per 1M tokens, as of 2024)
struct CodexPricing;

impl CodexPricing {
    fn cost_usd(model: &str, input: u64, cached: u64, output: u64) -> f64 {
        if let Some(cost) = CostUsagePricing::codex_cost_usd(model, input, cached, output) {
            return cost;
        }

        let (input_price, cached_price, output_price) = match model.to_lowercase().as_str() {
            m if m.contains("gpt-4o-mini") => (0.15, 0.075, 0.60),
            m if m.contains("gpt-4o") => (2.50, 1.25, 10.00),
            m if m.contains("gpt-4-turbo") => (10.00, 5.00, 30.00),
            m if m.contains("gpt-4") => (30.00, 15.00, 60.00),
            m if m.contains("o1-mini") => (3.00, 1.50, 12.00),
            m if m.contains("o1") => (15.00, 7.50, 60.00),
            _ => (2.50, 1.25, 10.00), // Default to GPT-4o
        };

        let cached = cached.min(input);
        let non_cached = input.saturating_sub(cached);
        let input_cost = (non_cached as f64 / 1_000_000.0) * input_price;
        let cached_cost = (cached as f64 / 1_000_000.0) * cached_price;
        let output_cost = (output as f64 / 1_000_000.0) * output_price;

        input_cost + cached_cost + output_cost
    }
}

/// Fallback Claude model used when a scanned model isn't in the canonical
/// pricing table (unknown or retired IDs). Prices as Sonnet 4.6.
const FALLBACK_CLAUDE_MODEL: &str = "claude-sonnet-4-6";

/// Claude cost calculation for the usage scanner.
///
/// Per-token rates come from the canonical `CostUsagePricing::claude_cost_usd`
/// table (the single source of truth for Claude pricing). The only
/// scanner-specific piece is the one-hour cache-write premium, which the
/// canonical cost function doesn't model: one-hour cache writes bill at 2x the
/// input rate.
struct ClaudePricing;

impl ClaudePricing {
    fn cost_usd_with_cache_ttl(
        model: &str,
        input: u64,
        cache_create: u64,
        cache_create_1h: u64,
        cache_read: u64,
        output: u64,
    ) -> f64 {
        let cache_create_1h = cache_create_1h.min(cache_create);
        let cache_create_5m = cache_create.saturating_sub(cache_create_1h);

        // Standard buckets (input, cache-read, 5-minute cache-write, output),
        // including any long-context tiering, come from the canonical table.
        // Unknown/retired models fall back to Sonnet pricing.
        let clamp = |v: u64| v.min(i32::MAX as u64) as i32;
        let base = CostUsagePricing::claude_cost_usd(
            model,
            clamp(input),
            clamp(cache_read),
            clamp(cache_create_5m),
            clamp(output),
        )
        .or_else(|| {
            CostUsagePricing::claude_cost_usd(
                FALLBACK_CLAUDE_MODEL,
                clamp(input),
                clamp(cache_read),
                clamp(cache_create_5m),
                clamp(output),
            )
        })
        .unwrap_or(0.0);

        // Scanner-specific: one-hour cache writes bill at 2x the input rate.
        let input_rate = CostUsagePricing::claude_input_cost_per_token(model)
            .or_else(|| CostUsagePricing::claude_input_cost_per_token(FALLBACK_CLAUDE_MODEL))
            .unwrap_or(0.0);

        base + (cache_create_1h as f64) * input_rate * 2.0
    }
}

/// JSONL event structures for Codex
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct CodexEvent {
    #[serde(rename = "type")]
    event_type: Option<String>,
    event_msg: Option<CodexEventMsg>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct CodexEventMsg {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

/// JSONL event structures for Claude
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ClaudeEvent {
    #[serde(rename = "type")]
    event_type: Option<String>,
    message: Option<ClaudeMessage>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Debug)]
struct ClaudeUsageRecord {
    model: String,
    timestamp: Option<DateTime<Utc>>,
    dedup_key: Option<String>,
    input: u64,
    output: u64,
    cache_create: u64,
    cache_read: u64,
    cost: f64,
}

/// Cost usage scanner
pub struct CostScanner {
    days: u32,
}

impl CostScanner {
    /// Create a new scanner for the last N days
    pub fn new(days: u32) -> Self {
        Self { days }
    }

    /// Scan Codex local logs
    pub fn scan_codex(&self) -> CostSummary {
        self.scan_codex_with_cancel(None)
    }

    /// Scan Codex local logs, stopping early when the caller cancels the scan.
    pub fn scan_codex_with_cancel(&self, cancel: Option<&AtomicBool>) -> CostSummary {
        let sessions_dir = self.get_codex_sessions_dir();
        if !sessions_dir.exists() {
            return CostSummary::default();
        }

        let mut summary = CostSummary::default();
        let today = Utc::now().date_naive();
        let start_date = today - Duration::days(self.days as i64);

        summary.period_start = Some(start_date);
        summary.period_end = Some(today);

        // Iterate through date-based directory structure
        for days_ago in 0..self.days {
            if is_cancelled(cancel) {
                break;
            }
            let date = today - Duration::days(days_ago as i64);
            let year = date.format("%Y").to_string();
            let month = date.format("%m").to_string();
            let day = date.format("%d").to_string();

            let day_dir = sessions_dir.join(&year).join(&month).join(&day);
            if !day_dir.exists() {
                continue;
            }

            if let Ok(entries) = fs::read_dir(&day_dir) {
                for entry in entries.flatten() {
                    if is_cancelled(cancel) {
                        break;
                    }
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "jsonl") {
                        self.parse_codex_file(&path, &mut summary, cancel);
                    }
                }
            }
        }

        summary
    }

    /// Scan Claude local logs
    pub fn scan_claude(&self) -> CostSummary {
        self.scan_claude_with_cancel(None)
    }

    /// Scan Claude local logs, stopping early when the caller cancels the scan.
    pub fn scan_claude_with_cancel(&self, cancel: Option<&AtomicBool>) -> CostSummary {
        let projects_dir = self.get_claude_projects_dir();
        if !projects_dir.exists() {
            return CostSummary::default();
        }

        let mut summary = CostSummary::default();
        let today = Utc::now().date_naive();
        let start_date = today - Duration::days(self.days as i64);
        let cutoff = Utc::now() - Duration::days(self.days as i64);

        summary.period_start = Some(start_date);
        summary.period_end = Some(today);

        // Walk through projects directory, de-duplicating usage records
        // that appear across multiple files.
        let mut seen = HashSet::new();
        let mut handle_file = |path: &Path| {
            self.parse_claude_file(path, &cutoff, &mut summary, &mut seen, cancel);
        };
        self.walk_claude_files(&projects_dir, &cutoff, cancel, &mut handle_file);

        summary
    }

    fn get_codex_sessions_dir(&self) -> PathBuf {
        if let Ok(codex_home) = std::env::var("CODEX_HOME") {
            let trimmed = codex_home.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join("sessions");
            }
        }

        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".codex")
            .join("sessions")
    }

    fn get_claude_projects_dir(&self) -> PathBuf {
        if let Ok(claude_config) = std::env::var("CLAUDE_CONFIG_DIR") {
            let trimmed = claude_config.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join("projects");
            }
        }

        // Try ~/.claude/projects first
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let claude_dir = home.join(".claude").join("projects");
        if claude_dir.exists() {
            return claude_dir;
        }

        // Fallback to ~/.config/claude/projects
        home.join(".config").join("claude").join("projects")
    }

    fn parse_codex_file(
        &self,
        path: &Path,
        summary: &mut CostSummary,
        cancel: Option<&AtomicBool>,
    ) {
        if is_cancelled(cancel) {
            return;
        }
        let today = Utc::now().date_naive();
        let start_date = today - Duration::days(self.days as i64);
        let range = CostUsageDayRange::new(start_date, today);
        let parse_result = match JsonlScanner::parse_codex_file(path, &range, 0, None, None) {
            Ok(result) => result,
            Err(_) => return,
        };

        let (session_cost, has_tokens) = add_codex_days_to_summary(summary, &parse_result.days);

        if has_tokens {
            summary.total_cost_usd += session_cost;
            summary.sessions_count += 1;
        }
    }

    fn walk_claude_files<F>(
        &self,
        dir: &Path,
        cutoff: &DateTime<Utc>,
        cancel: Option<&AtomicBool>,
        on_file: &mut F,
    ) where
        F: FnMut(&Path),
    {
        if is_cancelled(cancel) {
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            if is_cancelled(cancel) {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                self.walk_claude_files(&path, cutoff, cancel, on_file);
            } else if path.extension().is_some_and(|e| e == "jsonl") {
                // Check file modification time
                if let Ok(metadata) = fs::metadata(&path)
                    && let Ok(modified) = metadata.modified()
                {
                    let modified_dt: DateTime<Utc> = modified.into();
                    if modified_dt >= *cutoff {
                        on_file(&path);
                    }
                }
            }
        }
    }

    fn parse_claude_file(
        &self,
        path: &Path,
        cutoff: &DateTime<Utc>,
        summary: &mut CostSummary,
        seen: &mut HashSet<String>,
        cancel: Option<&AtomicBool>,
    ) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let reader = BufReader::new(file);
        let mut has_tokens = false;

        for line in reader.lines().map_while(Result::ok) {
            if is_cancelled(cancel) {
                return;
            }
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line)
                && let Some(record) = claude_usage_record_from_event(&event)
                && should_count_claude_record(&record, cutoff, seen)
            {
                add_claude_record_to_summary(summary, &record);
                has_tokens = true;
            }
        }

        if has_tokens {
            summary.sessions_count += 1;
        }
    }

    fn add_claude_file_daily_costs(
        &self,
        path: &Path,
        cutoff: &DateTime<Utc>,
        daily_costs: &mut HashMap<String, f64>,
        seen: &mut HashSet<String>,
        cancel: Option<&AtomicBool>,
    ) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            if is_cancelled(cancel) {
                return;
            }
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line)
                && let Some(record) = claude_usage_record_from_event(&event)
                && should_count_claude_record(&record, cutoff, seen)
                && let Some(timestamp) = record.timestamp
            {
                let date_str = timestamp
                    .with_timezone(&Local)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string();
                if let Some(cost) = daily_costs.get_mut(&date_str) {
                    *cost += record.cost;
                }
            }
        }
    }
}

fn claude_usage_record_from_event(event: &serde_json::Value) -> Option<ClaudeUsageRecord> {
    if event.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }

    let message = event.get("message")?;
    let usage = message.get("usage")?;
    let model = message
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("claude-3-5-sonnet");

    let input = usage
        .get("input_tokens")
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let cache_create = usage
        .get("cache_creation_input_tokens")
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    if input == 0 && output == 0 && cache_create == 0 && cache_read == 0 {
        return None;
    }

    let cache_create_1h = claude_one_hour_cache_creation_tokens(usage, cache_create);
    let cost = ClaudePricing::cost_usd_with_cache_ttl(
        model,
        input,
        cache_create,
        cache_create_1h,
        cache_read,
        output,
    );

    Some(ClaudeUsageRecord {
        model: model.to_string(),
        timestamp: claude_event_timestamp(event),
        dedup_key: claude_usage_dedup_key(event, message),
        input,
        output,
        cache_create,
        cache_read,
        cost,
    })
}

fn claude_event_timestamp(event: &serde_json::Value) -> Option<DateTime<Utc>> {
    let timestamp = event.get("timestamp").and_then(|t| t.as_str())?;
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|ts| ts.with_timezone(&Utc))
}

fn claude_usage_dedup_key(
    event: &serde_json::Value,
    message: &serde_json::Value,
) -> Option<String> {
    let message_id = message.get("id").and_then(|id| id.as_str());
    let request_id = event
        .get("requestId")
        .or_else(|| event.get("request_id"))
        .and_then(|id| id.as_str());

    match (message_id, request_id) {
        (Some(message_id), Some(request_id)) => Some(format!("{message_id}:{request_id}")),
        (Some(message_id), None) => Some(format!("message:{message_id}")),
        (None, Some(request_id)) => Some(format!("request:{request_id}")),
        (None, None) => None,
    }
}

fn should_count_claude_record(
    record: &ClaudeUsageRecord,
    cutoff: &DateTime<Utc>,
    seen: &mut HashSet<String>,
) -> bool {
    if let Some(timestamp) = record.timestamp
        && timestamp < *cutoff
    {
        return false;
    }

    if let Some(key) = &record.dedup_key
        && !seen.insert(key.clone())
    {
        return false;
    }

    true
}

fn add_claude_record_to_summary(summary: &mut CostSummary, record: &ClaudeUsageRecord) {
    summary.input_tokens += record.input;
    summary.output_tokens += record.output;
    summary.cached_tokens += record.cache_create + record.cache_read;
    summary.total_cost_usd += record.cost;

    *summary.by_model.entry(record.model.clone()).or_insert(0.0) += record.cost;

    let model_tokens = summary
        .by_model_tokens
        .entry(record.model.clone())
        .or_default();
    model_tokens.input_tokens += record.input;
    model_tokens.output_tokens += record.output;
    model_tokens.cached_tokens += record.cache_create + record.cache_read;
}

fn claude_one_hour_cache_creation_tokens(usage: &serde_json::Value, total: u64) -> u64 {
    usage
        .get("cache_creation")
        .and_then(|cache_creation| cache_creation.get("ephemeral_1h_input_tokens"))
        .and_then(|tokens| tokens.as_u64())
        .unwrap_or(0)
        .min(total)
}

type CodexDays = HashMap<String, HashMap<String, Vec<i32>>>;

fn add_codex_days_to_summary(summary: &mut CostSummary, days: &CodexDays) -> (f64, bool) {
    let mut total_cost = 0.0;
    let mut has_tokens = false;

    for models in days.values() {
        for (model, packed) in models {
            let input = packed.first().copied().unwrap_or(0).max(0) as u64;
            let cached = (packed.get(1).copied().unwrap_or(0).max(0) as u64).min(input);
            let output = packed.get(2).copied().unwrap_or(0).max(0) as u64;

            if input == 0 && cached == 0 && output == 0 {
                continue;
            }

            let cost = CodexPricing::cost_usd(model, input, cached, output);
            total_cost += cost;
            has_tokens = true;

            summary.input_tokens += input;
            summary.cached_tokens += cached;
            summary.output_tokens += output;
            *summary.by_model.entry(model.clone()).or_insert(0.0) += cost;

            let speed_bucket = codex_speed_bucket(model);
            *summary
                .by_speed
                .entry(speed_bucket.to_string())
                .or_insert(0.0) += cost;

            let model_tokens = summary.by_model_tokens.entry(model.clone()).or_default();
            model_tokens.input_tokens += input;
            model_tokens.output_tokens += output;
            model_tokens.cached_tokens += cached;

            let speed_tokens = summary
                .by_speed_tokens
                .entry(speed_bucket.to_string())
                .or_default();
            speed_tokens.input_tokens += input;
            speed_tokens.output_tokens += output;
            speed_tokens.cached_tokens += cached;
        }
    }

    (total_cost, has_tokens)
}

fn codex_days_cost(days: &CodexDays) -> f64 {
    let mut total_cost = 0.0;

    for models in days.values() {
        for (model, packed) in models {
            let input = packed.first().copied().unwrap_or(0).max(0) as u64;
            let cached = (packed.get(1).copied().unwrap_or(0).max(0) as u64).min(input);
            let output = packed.get(2).copied().unwrap_or(0).max(0) as u64;

            if input == 0 && cached == 0 && output == 0 {
                continue;
            }

            total_cost += CodexPricing::cost_usd(model, input, cached, output);
        }
    }

    total_cost
}

/// Check if any cost usage sources are available
#[allow(dead_code)]
pub fn has_cost_usage_sources() -> bool {
    let scanner = CostScanner::new(1);
    scanner.get_codex_sessions_dir().exists() || scanner.get_claude_projects_dir().exists()
}

/// Get daily cost history for the last N days
/// Returns Vec of (date_string, cost_usd) sorted by date
pub fn get_daily_cost_history(provider: &str, days: u32) -> Vec<(String, f64)> {
    let scanner = CostScanner::new(days);
    let today = Local::now().date_naive();
    let mut daily_costs: HashMap<String, f64> = HashMap::new();

    // Initialize all days with 0
    for days_ago in 0..days {
        let date = today - Duration::days(days_ago as i64);
        let date_str = date.format("%Y-%m-%d").to_string();
        daily_costs.insert(date_str, 0.0);
    }

    match provider {
        "codex" => {
            // Scan Codex logs by day
            let sessions_dir = scanner.get_codex_sessions_dir();
            if sessions_dir.exists() {
                for days_ago in 0..days {
                    let date = today - Duration::days(days_ago as i64);
                    let date_str = date.format("%Y-%m-%d").to_string();
                    let year = date.format("%Y").to_string();
                    let month = date.format("%m").to_string();
                    let day = date.format("%d").to_string();

                    let day_dir = sessions_dir.join(&year).join(&month).join(&day);
                    if day_dir.exists() {
                        let mut day_cost = 0.0;
                        if let Ok(entries) = fs::read_dir(&day_dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().is_some_and(|e| e == "jsonl") {
                                    let range = CostUsageDayRange::new(date, date);
                                    day_cost += scan_codex_file_cost_for_range(&path, &range);
                                }
                            }
                        }
                        daily_costs.insert(date_str, day_cost);
                    }
                }
            }
        }
        "claude" => {
            // Real per-day breakdown: walk the project logs once,
            // de-duplicating records across files.
            let projects_dir = scanner.get_claude_projects_dir();
            if projects_dir.exists() {
                let cutoff = Utc::now() - Duration::days(days as i64);
                let mut seen = HashSet::new();
                let mut handle_file = |path: &Path| {
                    scanner.add_claude_file_daily_costs(
                        path,
                        &cutoff,
                        &mut daily_costs,
                        &mut seen,
                        None,
                    );
                };
                scanner.walk_claude_files(&projects_dir, &cutoff, None, &mut handle_file);
            }
        }
        _ => {}
    }

    // Convert to sorted vector
    let mut result: Vec<(String, f64)> = daily_costs.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

/// Scan a single Codex file and return its cost
#[cfg(test)]
fn scan_codex_file_cost(path: &Path) -> f64 {
    let today = Utc::now().date_naive();
    let range = CostUsageDayRange::new(today - Duration::days(30), today);
    scan_codex_file_cost_for_range(path, &range)
}

fn scan_codex_file_cost_for_range(path: &Path, range: &CostUsageDayRange) -> f64 {
    let parse_result = match JsonlScanner::parse_codex_file(path, range, 0, None, None) {
        Ok(result) => result,
        Err(_) => return 0.0,
    };

    codex_days_cost(&parse_result.days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_codex_pricing() {
        // Test GPT-4o pricing: $2.50/1M input, $10/1M output
        let cost = CodexPricing::cost_usd("gpt-4o", 1_000_000, 0, 1_000_000);
        assert!((cost - 12.50).abs() < 0.01);
    }

    #[test]
    fn test_codex_pricing_uses_gpt55_standard_short_context_rates() {
        let cost = CodexPricing::cost_usd("gpt-5.5", 1_000_000, 400_000, 1_000_000);

        // GPT-5.5 standard short-context pricing:
        // 600k non-cached input at $5/M, 400k cached input at $0.50/M,
        // and 1M output at $30/M.
        assert!((cost - 33.20).abs() < 0.01);
    }

    #[test]
    fn test_unknown_model_falls_back_to_sonnet() {
        // Unknown/retired Claude IDs fall back to Sonnet 4.6 base pricing
        // ($3/1M input, $15/1M output). 100k tokens stay under the 200k tier.
        let cost =
            ClaudePricing::cost_usd_with_cache_ttl("claude-3-5-sonnet", 100_000, 0, 0, 0, 100_000);
        // 100k * $3/M + 100k * $15/M = 0.30 + 1.50 = 1.80
        assert!((cost - 1.80).abs() < 0.001);
    }

    #[test]
    fn test_claude_fable_5_pricing() {
        let cost = ClaudePricing::cost_usd_with_cache_ttl("claude-fable-5", 100, 10, 0, 20, 5);
        let expected = (100.0 / 1_000_000.0) * 10.00
            + (10.0 / 1_000_000.0) * 12.50
            + (20.0 / 1_000_000.0) * 1.00
            + (5.0 / 1_000_000.0) * 50.00;
        assert!((cost - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn test_claude_one_hour_cache_write_pricing() {
        let cost = ClaudePricing::cost_usd_with_cache_ttl("claude-fable-5", 100, 30, 20, 20, 5);
        let expected = (100.0 / 1_000_000.0) * 10.00
            + (10.0 / 1_000_000.0) * 12.50
            + (20.0 / 1_000_000.0) * 20.00
            + (20.0 / 1_000_000.0) * 1.00
            + (5.0 / 1_000_000.0) * 50.00;
        assert!((cost - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn test_claude_sonnet_46_honors_200k_tier() {
        // Delegating to the canonical table means the scanner now honors the
        // 200k long-context tier: 200k @ $3/M + 40k @ $6/M = 0.60 + 0.24 = 0.84
        // (the scanner's old inline table applied a flat $3/M = 0.72).
        let cost = ClaudePricing::cost_usd_with_cache_ttl("claude-sonnet-4-6", 240_000, 0, 0, 0, 0);
        assert!((cost - 0.84).abs() < 0.001);
    }

    #[test]
    fn test_current_gen_opus_uses_5_25_pricing() {
        // Opus 4.5/4.6/4.7/4.8 bill at $5/1M input + $25/1M output = $30 total.
        // Delegation regression guard: opus-4-8 in particular must resolve
        // through the canonical table (it was missing there before this fix).
        for model in [
            "claude-opus-4-5",
            "claude-opus-4-6",
            "claude-opus-4-7",
            "claude-opus-4-8",
        ] {
            let cost = ClaudePricing::cost_usd_with_cache_ttl(model, 1_000_000, 0, 0, 0, 1_000_000);
            assert!(
                (cost - 30.00).abs() < 0.001,
                "{model} should bill $30 ($5 in + $25 out), got {cost}"
            );
        }
    }

    #[test]
    fn test_legacy_opus_keeps_legacy_pricing() {
        // Legacy Opus 4.0 / 4.1 remain at $15/1M input + $75/1M output = $90 in
        // the canonical table. (Retired IDs absent from the table — e.g. Opus 3
        // `claude-3-opus-...` — fall back to Sonnet instead; they are outside
        // any realistic 30-day scan window.)
        for model in ["claude-opus-4-20250514", "claude-opus-4-1"] {
            let cost = ClaudePricing::cost_usd_with_cache_ttl(model, 1_000_000, 0, 0, 0, 1_000_000);
            assert!(
                (cost - 90.00).abs() < 0.001,
                "{model} should bill $90 ($15 in + $75 out), got {cost}"
            );
        }
    }

    #[test]
    fn test_haiku_45_uses_current_pricing() {
        // Haiku 4.5 bills at $1/1M input + $5/1M output = $6 via the canonical
        // table (previously the scanner under-priced it at the Haiku 3 rate).
        let cost = ClaudePricing::cost_usd_with_cache_ttl(
            "claude-haiku-4-5",
            1_000_000,
            0,
            0,
            0,
            1_000_000,
        );
        assert!(
            (cost - 6.00).abs() < 0.001,
            "haiku-4-5 should bill $6 ($1 in + $5 out), got {cost}"
        );
    }

    #[test]
    fn test_codex_speed_bucket() {
        assert_eq!(codex_speed_bucket("gpt-5.5-fast"), "fast");
        assert_eq!(codex_speed_bucket("gpt-5.3-codex-spark"), "fast");
        assert_eq!(codex_speed_bucket("gpt-5-codex"), "standard");
    }

    #[test]
    fn parses_current_codex_payload_token_count_events() {
        let path = std::env::temp_dir().join(format!(
            "codexbar-current-codex-token-count-{}.jsonl",
            std::process::id()
        ));
        // Use a recent timestamp so the event stays inside the scanner's
        // 30-day window no matter when the test runs. A hardcoded date
        // silently ages out of the window and makes this test fail with 0
        // sessions once it is more than 30 days in the past.
        let recent = (Utc::now() - Duration::hours(1))
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":125,"cached_input_tokens":30,"output_tokens":15}}}}}}}}"#,
            ts = recent
        )
        .unwrap();
        drop(file);

        let scanner = CostScanner::new(30);
        let mut summary = CostSummary::default();
        scanner.parse_codex_file(&path, &mut summary, None);

        assert_eq!(summary.sessions_count, 1);
        assert_eq!(summary.input_tokens, 125);
        assert_eq!(summary.cached_tokens, 30);
        assert_eq!(summary.output_tokens, 15);
        assert_eq!(
            summary
                .by_model_tokens
                .get("gpt-5")
                .map(ModelTokenCounts::total),
            Some(140)
        );
        assert!(scan_codex_file_cost(&path) > 0.0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn derives_claude_dedup_key_from_message_and_request_ids() {
        let cases = [
            (
                r#"{"requestId":"req_1","message":{"id":"msg_1"}}"#,
                Some("msg_1:req_1"),
            ),
            (r#"{"message":{"id":"msg_1"}}"#, Some("message:msg_1")),
            (
                r#"{"request_id":"req_1","message":{}}"#,
                Some("request:req_1"),
            ),
            (r#"{"message":{}}"#, None),
        ];
        for (json, expected) in cases {
            let event: serde_json::Value = serde_json::from_str(json).unwrap();
            let message = event.get("message").unwrap();
            assert_eq!(
                claude_usage_dedup_key(&event, message).as_deref(),
                expected,
                "unexpected dedup key for {json}"
            );
        }
    }

    #[test]
    fn counts_claude_usage_once_across_duplicate_records() {
        // The same API response can be replayed into several transcript files
        // (session resume, observer sessions); it must only be counted once.
        let event: serde_json::Value = serde_json::from_str(
            r#"{"type":"assistant","timestamp":"2026-01-15T10:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":20}}}"#,
        )
        .unwrap();

        let record = claude_usage_record_from_event(&event).expect("usage record");
        assert_eq!(record.model, "claude-sonnet-4-6");
        assert_eq!(record.input, 100);
        assert_eq!(record.output, 50);
        assert_eq!(record.cache_create, 10);
        assert_eq!(record.cache_read, 20);
        assert!(record.cost > 0.0);

        let cutoff = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut seen = HashSet::new();
        assert!(should_count_claude_record(&record, &cutoff, &mut seen));
        assert!(!should_count_claude_record(&record, &cutoff, &mut seen));
    }

    #[test]
    fn rejects_claude_records_before_cutoff() {
        let event: serde_json::Value = serde_json::from_str(
            r#"{"type":"assistant","timestamp":"2025-12-01T10:00:00Z","requestId":"req_old","message":{"id":"msg_old","model":"claude-sonnet-4-6","usage":{"input_tokens":1,"output_tokens":1}}}"#,
        )
        .unwrap();
        let record = claude_usage_record_from_event(&event).expect("usage record");
        let cutoff = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut seen = HashSet::new();
        assert!(!should_count_claude_record(&record, &cutoff, &mut seen));
    }

    #[test]
    fn ignores_claude_events_without_countable_usage() {
        // Non-assistant events carry no billable usage.
        let event: serde_json::Value =
            serde_json::from_str(r#"{"type":"user","message":{"usage":{"input_tokens":5}}}"#)
                .unwrap();
        assert!(claude_usage_record_from_event(&event).is_none());

        // Zero-token usage blocks (e.g. synthetic messages) are not sessions.
        let event: serde_json::Value = serde_json::from_str(
            r#"{"type":"assistant","message":{"id":"msg_zero","model":"claude-sonnet-4-6","usage":{"input_tokens":0,"output_tokens":0}}}"#,
        )
        .unwrap();
        assert!(claude_usage_record_from_event(&event).is_none());
    }
}
