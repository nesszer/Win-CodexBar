//! Setup shared by the cost scanner tests.

use super::*;

/// A temp root holding the `sessions` and `cache` dirs most Codex scans use.
pub(super) fn codex_scan_dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let sessions = root.path().join("sessions");
    let cache_root = root.path().join("cache");
    (root, sessions, cache_root)
}

/// The app-driven scanner over one sessions dir.
pub(super) fn app_scanner(
    days: u32,
    cache_root: impl Into<PathBuf>,
    sessions: &Path,
) -> CostScanner {
    CostScanner::new(days)
        .with_options(CostScanOptions::app_driven())
        .with_cache_root(cache_root)
        .with_sessions_dirs(vec![sessions.to_path_buf()])
}

/// The `YYYY/MM/DD` partition for `day` under a sessions dir.
pub(super) fn partition_dir(sessions: &Path, day: NaiveDate) -> PathBuf {
    sessions
        .join(day.format("%Y").to_string())
        .join(day.format("%m").to_string())
        .join(day.format("%d").to_string())
}

pub(super) fn cached_file<'a>(cache: &'a CostUsageCache, path: &Path) -> &'a CostUsageFileUsage {
    cache
        .files
        .get(&path.to_string_lossy().to_string())
        .unwrap()
}

/// An event time for a fresh Codex session fixture: an hour ago, kept on today's local date.
///
/// The scanner files each event under its local date, and the session fixtures live in today's
/// date folder. A plain `now - 1h` lands on yesterday in the first hour after local midnight
/// (00:00-01:00Z on the UTC CI runner), so tests that read today's bucket or rely on the day
/// folder scan order failed in that hour.
pub(super) fn recent_codex_fixture_time() -> DateTime<Utc> {
    recent_fixture_time_at(Local::now())
}

/// `now - 1h`, or the start of `now`'s local day when that hour reaches back into yesterday.
pub(super) fn recent_fixture_time_at<Tz: TimeZone>(now: DateTime<Tz>) -> DateTime<Utc> {
    let hour_ago = now.clone() - Duration::hours(1);
    if hour_ago.date_naive() == now.date_naive() {
        return hour_ago.with_timezone(&Utc);
    }
    now.timezone()
        .from_local_datetime(&now.date_naive().and_time(NaiveTime::MIN))
        .earliest()
        .unwrap_or(now)
        .with_timezone(&Utc)
}

pub(super) fn write_codex_session_fixture(
    sessions_root: &Path,
    name: &str,
    input_tokens: u64,
) -> PathBuf {
    let event_time = recent_codex_fixture_time();
    let today = event_time.with_timezone(&Local).date_naive();
    let day_dir = partition_dir(sessions_root, today);
    std::fs::create_dir_all(&day_dir).unwrap();
    let path = day_dir.join(name);
    let ts = event_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let body = format!(
        r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{input_tokens},"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
    );
    std::fs::write(&path, body).unwrap();
    path
}

pub(super) fn write_codex_session_fixture_with_inputs(
    sessions_root: &Path,
    name: &str,
    input_tokens: &[u64],
) -> PathBuf {
    let base = recent_codex_fixture_time();
    let today = base.with_timezone(&Local).date_naive();
    let day_dir = partition_dir(sessions_root, today);
    std::fs::create_dir_all(&day_dir).unwrap();
    let mut body = String::new();
    for (index, input) in input_tokens.iter().enumerate() {
        let timestamp = (base
            + Duration::seconds(i64::try_from(index).expect("fixture index fits i64")))
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
        body.push_str(&format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5","total_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":5}}}}}}}}
"#
        ));
    }
    let path = day_dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

pub(super) fn cached_usage_with_packed(
    day: &str,
    model: &str,
    packed: Vec<i64>,
) -> CostUsageFileUsage {
    CostUsageFileUsage {
        parsed_bytes: Some(1),
        ..test_file_usage(
            1,
            HashMap::from([(
                day.to_string(),
                HashMap::from([(model.to_string(), packed)]),
            )]),
        )
    }
}

pub(super) fn write_codex_fork_session_fixture(
    sessions_root: &Path,
    name: &str,
    session_id: &str,
    parent_id: Option<&str>,
    fork_timestamp: DateTime<Utc>,
    token_start: DateTime<Utc>,
    totals: &[i64],
) -> PathBuf {
    let today = Local::now().date_naive();
    let day_dir = partition_dir(sessions_root, today);
    std::fs::create_dir_all(&day_dir).unwrap();

    let mut body = format!(
        "{{\"type\":\"session_meta\",\"timestamp\":\"{}\",\"payload\":{{\"session_id\":\"{}\"",
        fork_timestamp.to_rfc3339(),
        session_id
    );
    if let Some(parent_id) = parent_id {
        body.push_str(&format!(",\"forked_from_id\":\"{parent_id}\""));
    }
    body.push_str("}}\n");

    for (index, total) in totals.iter().enumerate() {
        let timestamp = (token_start
            + Duration::seconds(i64::try_from(index).expect("fixture index fits i64")))
        .to_rfc3339();
        let line = serde_json::json!({
            "timestamp": timestamp,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "model": "gpt-5",
                    "total_token_usage": {
                        "input_tokens": total,
                        "cached_input_tokens": 0,
                        "output_tokens": 5
                    }
                }
            }
        });
        body.push_str(&line.to_string());
        body.push('\n');
    }

    let path = day_dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

pub(super) fn cached_input_total(usage: &CostUsageFileUsage) -> i64 {
    usage
        .days
        .values()
        .flat_map(|models| models.values())
        .map(|tokens| tokens.first().copied().unwrap_or_default())
        .sum()
}

/// Stream the de-duplicated, in-window usage records from one transcript
/// file into `on_record`, returning how many it consumed.
pub(super) fn for_each_claude_usage_record<F>(
    path: &Path,
    cutoff: &DateTime<Utc>,
    seen: &mut HashSet<ClaudeUsageDedupKey>,
    cancel: Option<&AtomicBool>,
    on_record: F,
) -> usize
where
    F: FnMut(&ClaudeUsageRecord),
{
    let mut pricing = ClaudeScanPricingResolver::default();
    let mut incomplete = ClaudeIncompleteTracker::default();
    scan_claude_file_with_pricing(
        path,
        cutoff,
        seen,
        cancel,
        &mut pricing,
        &mut incomplete,
        on_record,
    )
    .counted
}

pub(super) fn claude_usage_record_from_event(event: &ClaudeEvent) -> Option<ClaudeUsageRecord> {
    let mut pricing = ClaudeScanPricingResolver::default();
    claude_usage_record_from_event_with_pricing(event, &mut pricing)
}
