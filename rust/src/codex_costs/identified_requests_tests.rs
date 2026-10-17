use super::*;
use serde_json::json;

// 2023-11-14T22:13:20Z; its UTC day starts at 1_699_920_000_000.
const END: i64 = 1_700_000_000_000;
const DAY_START: i64 = 1_699_920_000_000;

fn record(id_seed: &str, timestamp_unix_ms: i64, input: i64, output: i64) -> CodexRequestRecord {
    CodexRequestRecord {
        id: digest(&["codex", "thread", id_seed]),
        timestamp_unix_ms,
        input_tokens: input,
        cache_read_tokens: 0,
        output_tokens: output,
        reasoning_tokens: Some(0),
    }
}

fn ledger(records: Vec<CodexRequestRecord>) -> CodexRequestLedger {
    let mut ledger = CodexRequestLedger::empty(DAY_START, END);
    ledger.records = records;
    ledger
}

#[test]
fn digest_is_length_framed_sha256_hex() {
    let id = digest(&["codex", "thread", "response"]);
    assert!(is_digest(&id));
    // Framing keeps component boundaries distinct.
    assert_ne!(id, digest(&["codex", "threadr", "esponse"]));
    assert_eq!(id, digest(&["codex", "thread", "response"]));
}

#[test]
fn window_starts_at_utc_midnight_days_before_end() {
    assert_eq!(identified_window_start(1, END), Some(DAY_START));
    assert_eq!(
        identified_window_start(30, END),
        Some(DAY_START - 29 * DAY_MS)
    );
    assert_eq!(identified_window_start(0, END), None);
    assert_eq!(identified_window_start(366, END), None);
    assert_eq!(identified_window_start(2, 0), None);
    assert_eq!(
        identified_window_start(1, MAX_IDENTIFIED_END_UNIX_MS + 1),
        None
    );
}

#[test]
fn merge_counts_identical_copies_once_and_excludes_conflicts() {
    let local = ledger(vec![
        record("a", END - 10, 100, 10),
        record("b", END - 10, 200, 20),
    ]);
    let remote = ledger(vec![
        record("a", END - 5, 100, 10),
        // Contradictory copy of `b`: excluded throughout the merge.
        record("b", END - 5, 999, 20),
        record("c", END - 5, 50, 5),
    ]);
    let subtotal = CodexRequestLedger::merge(&[local, remote]).unwrap();
    assert_eq!(subtotal.request_count, 2);
    assert_eq!(subtotal.total_tokens, 110 + 55);
    assert_eq!(subtotal.conflict_count, 1);
    assert!(subtotal.partial);

    let value = serde_json::to_value(&subtotal).unwrap();
    assert_eq!(value["kind"], "identified-request-subtotal");
    assert!(value["costUSD"].is_null());
    assert!(value.as_object().unwrap().contains_key("costUSD"));
}

#[test]
fn merge_keeps_source_conflicts_after_a_clean_later_copy() {
    let mut local = ledger(vec![]);
    let conflicted = record("x", END - 1, 10, 1);
    local.conflicting_ids = vec![conflicted.id.clone()];
    let remote = ledger(vec![conflicted]);
    let subtotal = CodexRequestLedger::merge(&[local, remote]).unwrap();
    assert_eq!(subtotal.request_count, 0);
    assert_eq!(subtotal.conflict_count, 1);
    assert!(subtotal.partial);
}

#[test]
fn clean_merge_is_complete() {
    let subtotal = CodexRequestLedger::merge(&[ledger(vec![record("a", END, 1, 2)])]).unwrap();
    assert!(!subtotal.partial);
    assert_eq!(subtotal.total_tokens, 3);
}

#[test]
fn validate_rejects_invalid_ledgers() {
    assert!(
        ledger(vec![record("a", END, 1, 1)])
            .validate(DAY_START, END)
            .is_ok()
    );
    // Wrong window.
    assert!(ledger(vec![]).validate(DAY_START + 1, END).is_err());
    // Out-of-window record.
    assert!(
        ledger(vec![record("a", END + 1, 1, 1)])
            .validate(DAY_START, END)
            .is_err()
    );
    // Cached above input, reasoning above output, negative tokens.
    let mut bad = record("a", END, 1, 1);
    bad.cache_read_tokens = 2;
    assert!(ledger(vec![bad]).validate(DAY_START, END).is_err());
    let mut bad = record("a", END, 1, 1);
    bad.reasoning_tokens = Some(2);
    assert!(ledger(vec![bad]).validate(DAY_START, END).is_err());
    let mut bad = record("a", END, -1, 1);
    bad.cache_read_tokens = -1;
    assert!(ledger(vec![bad]).validate(DAY_START, END).is_err());
    // Non-digest identities.
    let mut bad = record("a", END, 1, 1);
    bad.id = "raw-session-id".to_string();
    assert!(ledger(vec![bad]).validate(DAY_START, END).is_err());
    // Aggregate overflow.
    let huge = vec![
        record("a", END, i64::MAX / 2, 0),
        record("b", END, i64::MAX / 2, 0),
        record("c", END, i64::MAX / 2, 0),
    ];
    assert!(ledger(huge).validate(DAY_START, END).is_err());
}

#[test]
fn ledger_json_uses_the_versioned_wire_names() {
    let value = serde_json::to_value(ledger(vec![record("a", END, 3, 2)])).unwrap();
    for key in [
        "schemaVersion",
        "provider",
        "startUnixMs",
        "endUnixMs",
        "records",
        "conflictingIDs",
        "excludedCount",
        "partial",
    ] {
        assert!(value.get(key).is_some(), "missing {key}");
    }
    let row = &value["records"][0];
    for key in [
        "id",
        "timestampUnixMs",
        "inputTokens",
        "cacheReadTokens",
        "outputTokens",
        "reasoningTokens",
    ] {
        assert!(row.get(key).is_some(), "missing {key}");
    }
    let round_trip = decode_remote_ledger(&value.to_string(), DAY_START, END).unwrap();
    assert_eq!(round_trip.records.len(), 1);
}

#[test]
fn remote_decoder_fails_closed() {
    assert_eq!(
        decode_remote_ledger("not json", DAY_START, END),
        Err(IdentifiedUsageError::InvalidReport)
    );
    let other_window = serde_json::to_string(&ledger(vec![])).unwrap();
    assert!(decode_remote_ledger(&other_window, DAY_START - DAY_MS, END).is_err());
    let oversized = " ".repeat(MAX_IDENTIFIED_OUTPUT_BYTES + 1);
    assert!(decode_remote_ledger(&oversized, DAY_START, END).is_err());
}

fn line(value: serde_json::Value) -> String {
    format!("{value}\n")
}

fn usage(input: i64, cached: i64, output: i64) -> serde_json::Value {
    json!({"input_tokens": input, "cached_input_tokens": cached, "output_tokens": output, "reasoning_output_tokens": 0})
}

fn usage_record(
    thread: &str,
    response: &str,
    timestamp: &str,
    request: serde_json::Value,
    total: serde_json::Value,
) -> String {
    line(json!({
        "type": "token_usage_record",
        "timestamp": timestamp,
        "payload": {"thread_id": thread, "response_id": response, "usage": request, "thread_token_usage": total}
    }))
}

fn session_file(root: &Path, name: &str, session_id: &str, body: &str) {
    let dir = root.join("2023").join("11").join("14");
    fs::create_dir_all(&dir).unwrap();
    let meta = line(json!({"type": "session_meta", "payload": {"id": session_id}}));
    fs::write(dir.join(name), format!("{meta}{body}")).unwrap();
}

#[test]
fn local_scan_identifies_owned_requests_and_reports_exclusions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sessions");
    let ts = "2023-11-14T12:00:00.000Z";
    let mut body = String::new();
    body += &line(json!({"type": "turn_context", "payload": {"model": "gpt-5", "turn_id": "t1"}}));
    body += &usage_record("s1", "r1", ts, usage(100, 40, 10), usage(100, 40, 10));
    // Mirrored legacy counter for r1: not excluded.
    body += &line(json!({
        "type": "event_msg", "timestamp": ts,
        "payload": {"type": "token_count", "info": {"last_token_usage": usage(100, 40, 10), "total_token_usage": usage(100, 40, 10)}}
    }));
    // Verbatim replay of r1 counts once.
    body += &usage_record("s1", "r1", ts, usage(100, 40, 10), usage(100, 40, 10));
    body += &usage_record("s1", "r2", ts, usage(50, 0, 5), usage(150, 40, 15));
    // Copied parent record: not owned, ignored.
    body += &usage_record("parent", "r9", ts, usage(7, 0, 7), usage(7, 0, 7));
    // Outside the window: ignored.
    body += &usage_record(
        "s1",
        "r3",
        "2023-11-13T12:00:00Z",
        usage(9, 0, 9),
        usage(159, 40, 24),
    );
    // Unmatched legacy usage: excluded.
    body += &line(json!({
        "type": "event_msg", "timestamp": ts,
        "payload": {"type": "token_count", "info": {"last_token_usage": usage(30, 0, 3), "total_token_usage": usage(500, 40, 50)}}
    }));
    session_file(&root, "rollout-2023-11-14T12-00-00-s1.jsonl", "s1", &body);

    let ledger = load_ledger_from_roots(&[root], DAY_START, END).unwrap();
    assert_eq!(ledger.records.len(), 2);
    assert_eq!(ledger.excluded_count, 1);
    assert!(ledger.conflicting_ids.is_empty());
    assert!(ledger.partial);
    let first = ledger
        .records
        .iter()
        .find(|row| row.id == digest(&["codex", "s1", "r1"]))
        .unwrap();
    assert_eq!(first.input_tokens, 100);
    assert_eq!(first.cache_read_tokens, 40);
    assert_eq!(first.output_tokens, 10);
    assert_eq!(first.timestamp_unix_ms, 1_699_963_200_000);

    let serialized = serde_json::to_string(&ledger).unwrap();
    assert!(!serialized.contains("s1"));
    assert!(!serialized.contains("gpt-5"));
}

#[test]
fn files_last_written_before_the_window_are_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sessions");
    let ts = "2023-11-14T12:00:00Z";
    let body = usage_record("s1", "r1", ts, usage(10, 0, 1), usage(10, 0, 1));
    let name = "rollout-2023-11-14T12-00-00-s1.jsonl";
    session_file(&root, name, "s1", &body);
    let path = root.join("2023").join("11").join("14").join(name);
    let set_mtime = |unix_ms: i64| {
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(unix_ms as u64);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    };

    set_mtime(DAY_START - 2 * 24 * 60 * 60 * 1000);
    let ledger = load_ledger_from_roots(std::slice::from_ref(&root), DAY_START, END).unwrap();
    assert!(ledger.records.is_empty());

    // Inside the clock-skew margin the file is still scanned.
    set_mtime(DAY_START - 60 * 60 * 1000);
    let ledger = load_ledger_from_roots(&[root], DAY_START, END).unwrap();
    assert_eq!(ledger.records.len(), 1);
}

#[test]
fn copied_parent_legacy_counters_are_not_excluded_from_a_fork() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sessions");
    let ts = "2023-11-14T12:00:00Z";
    let mut body = String::new();
    body += &line(json!({"type": "turn_context", "payload": {"model": "gpt-5"}}));
    body += &usage_record("parent", "r1", ts, usage(70, 0, 7), usage(70, 0, 7));
    body += &line(json!({
        "type": "event_msg", "timestamp": ts,
        "payload": {"type": "token_count", "info": {"last_token_usage": usage(70, 0, 7), "total_token_usage": usage(70, 0, 7)}}
    }));
    body += &usage_record("child", "r2", ts, usage(5, 0, 1), usage(75, 0, 8));
    session_file(
        &root,
        "rollout-2023-11-14T12-00-00-child.jsonl",
        "child",
        &body,
    );

    let ledger = load_ledger_from_roots(&[root], DAY_START, END).unwrap();
    assert_eq!(ledger.records.len(), 1);
    assert_eq!(ledger.excluded_count, 0);
    assert!(!ledger.partial);
}

#[test]
fn local_scan_marks_contradictions_rejections_and_foreign_routes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sessions");
    let ts = "2023-11-14T12:00:00Z";
    let mut body = String::new();
    body += &usage_record("s2", "r1", ts, usage(10, 0, 1), usage(10, 0, 1));
    body += &usage_record("s2", "r1", ts, usage(20, 0, 1), usage(20, 0, 1));
    // Rejected owned record with an identity: conflict + excluded.
    body += &line(json!({
        "type": "token_usage_record", "timestamp": ts,
        "payload": {"thread_id": "s2", "response_id": "r2", "usage": {"input_tokens": -1}}
    }));
    // Foreign-provider route is not Codex subscription usage.
    body += &line(json!({
        "type": "token_usage_record", "timestamp": ts,
        "payload": {"thread_id": "s2", "response_id": "r3", "model": "deepseek/deepseek-chat",
                    "usage": usage(5, 0, 5), "thread_token_usage": usage(25, 0, 7)}
    }));
    session_file(&root, "rollout-s2.jsonl", "s2", &body);

    let ledger = load_ledger_from_roots(&[root], DAY_START, END).unwrap();
    assert!(
        ledger
            .records
            .iter()
            .all(|row| row.id != digest(&["codex", "s2", "r3"]))
    );
    assert_eq!(ledger.conflicting_ids.len(), 2);
    assert_eq!(ledger.excluded_count, 1);
    assert!(ledger.partial);
    let subtotal = CodexRequestLedger::merge(&[ledger]).unwrap();
    assert_eq!(subtotal.request_count, 0);
}

#[test]
fn malformed_lines_mark_the_ledger_partial() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sessions");
    let body = format!(
        "{{not json\n{}",
        usage_record(
            "s3",
            "r1",
            "2023-11-14T12:00:00Z",
            usage(1, 0, 1),
            usage(1, 0, 1)
        )
    );
    session_file(&root, "rollout-s3.jsonl", "s3", &body);
    let ledger = load_ledger_from_roots(&[root], DAY_START, END).unwrap();
    assert_eq!(ledger.records.len(), 1);
    assert_eq!(ledger.excluded_count, 0);
    assert!(ledger.partial);
}

#[test]
fn readable_empty_history_is_available_and_missing_history_is_not() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sessions");
    fs::create_dir_all(&root).unwrap();
    let ledger = load_ledger_from_roots(
        &[root, temp.path().join("archived_sessions")],
        DAY_START,
        END,
    )
    .unwrap();
    assert!(ledger.records.is_empty());
    assert!(!ledger.partial);

    assert_eq!(
        load_ledger_from_roots(&[temp.path().join("missing")], DAY_START, END),
        Err(IdentifiedUsageError::Unavailable)
    );
}

#[tokio::test]
async fn remote_failure_retains_the_local_subtotal_and_marks_partial() {
    let result = collect_identified_usage(
        Some("build-host"),
        DAY_START,
        END,
        || Ok(ledger(vec![record("a", END, 4, 1)])),
        |_| async { Err(IdentifiedUsageError::Unavailable) },
    )
    .await;
    assert!(!result.all_sources_available);
    assert!(result.partial);
    assert_eq!(result.reports.len(), 2);
    assert_eq!(result.reports[1].host, "build-host");
    assert_eq!(
        result.reports[1].error.as_deref(),
        Some(REMOTE_IDENTIFIED_UNAVAILABLE)
    );
    let subtotal = result.subtotal.as_ref().unwrap();
    assert_eq!(subtotal.total_tokens, 5);
    assert!(!subtotal.partial);

    let json: serde_json::Value =
        serde_json::from_str(&identified_usage_json(true, &result, false).unwrap()).unwrap();
    assert_eq!(json["allSourcesAvailable"], false);
    assert!(json["reports"][1].get("ledger").is_none());
    assert!(json["subtotal"]["costUSD"].is_null());
}

#[tokio::test]
async fn remote_ledger_deduplicates_with_local() {
    let result = collect_identified_usage(
        Some("build-host"),
        DAY_START,
        END,
        || Ok(ledger(vec![record("a", END, 4, 1)])),
        |_| async { Ok(ledger(vec![record("a", END, 4, 1), record("b", END, 2, 2)])) },
    )
    .await;
    assert!(result.all_sources_available);
    assert!(!result.partial);
    let subtotal = result.subtotal.unwrap();
    assert_eq!(subtotal.request_count, 2);
    assert_eq!(subtotal.total_tokens, 9);
}

#[tokio::test]
async fn overflowing_remote_union_is_rejected_while_local_remains() {
    let result = collect_identified_usage(
        Some("build-host"),
        DAY_START,
        END,
        || Ok(ledger(vec![record("a", END, i64::MAX - 10, 0)])),
        |_| async { Ok(ledger(vec![record("b", END, 100, 0)])) },
    )
    .await;
    assert!(!result.all_sources_available);
    assert_eq!(result.subtotal.unwrap().total_tokens, i64::MAX - 10);
}

#[tokio::test]
async fn local_only_success_prints_the_bare_ledger() {
    let result = collect_identified_usage(
        None,
        DAY_START,
        END,
        || Ok(ledger(vec![])),
        |_| async { Err(IdentifiedUsageError::Unavailable) },
    )
    .await;
    let json: serde_json::Value =
        serde_json::from_str(&identified_usage_json(false, &result, false).unwrap()).unwrap();
    assert_eq!(json["schemaVersion"], 1);
    assert!(json.get("reports").is_none());

    let failed = collect_identified_usage(
        None,
        DAY_START,
        END,
        || Err(IdentifiedUsageError::Unavailable),
        |_| async { Err(IdentifiedUsageError::Unavailable) },
    )
    .await;
    assert!(failed.partial);
    assert!(failed.subtotal.is_none());
    let json: serde_json::Value =
        serde_json::from_str(&identified_usage_json(false, &failed, false).unwrap()).unwrap();
    assert_eq!(json["reports"][0]["error"], LOCAL_IDENTIFIED_UNAVAILABLE);
    assert!(json.get("subtotal").is_none());
}

fn valid_args() -> IdentifiedUsageArgs {
    IdentifiedUsageArgs {
        identified_only: true,
        provider_is_codex_only: true,
        json: true,
        days: None,
        end_unix_ms: Some(END),
        remote: None,
        period_set: false,
        group_by_set: false,
        summary_only: false,
        pretty: false,
    }
}

#[test]
fn identified_args_require_explicit_codex_json_without_other_modes() {
    assert_eq!(
        identified_usage_window(&valid_args(), 0),
        Ok((30, DAY_START - 29 * DAY_MS, END))
    );
    let mut now = valid_args();
    now.end_unix_ms = None;
    assert_eq!(identified_usage_window(&now, END).unwrap().2, END);

    let rejected: [fn(&mut IdentifiedUsageArgs); 8] = [
        |a| a.identified_only = false,
        |a| a.provider_is_codex_only = false,
        |a| a.json = false,
        |a| a.period_set = true,
        |a| a.group_by_set = true,
        |a| a.summary_only = true,
        |a| a.days = Some(366),
        |a| a.end_unix_ms = Some(MAX_IDENTIFIED_END_UNIX_MS + 1),
    ];
    for mutate in rejected {
        let mut args = valid_args();
        mutate(&mut args);
        assert_eq!(
            identified_usage_window(&args, END),
            Err(IDENTIFIED_USAGE_ARGS_ERROR)
        );
    }
}
