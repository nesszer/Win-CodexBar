#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn record(id: &str, micros: u64, kind: &str, usage: &str, model: &str) -> String {
        format!(
            r#"{{"schema_version":1,"id":"{id}","recorded_at":{micros},"record_type":"event","payload_type":"runtime.session","payload_schema_version":1,"payload":{{"event":{{"kind":"{kind}","model":"{model}","usage":{usage}}}}}}}"#
        )
    }

    #[test]
    fn parses_token_turns_without_double_counting_cache_or_reasoning() {
        let value = record(
            "e1",
            1_788_177_600_000_000,
            "model_completed",
            r#"{"input_tokens":10,"output_tokens":2,"cached_tokens":9,"reasoning_tokens":1}"#,
            "muse-1",
        );
        assert_eq!(
            parse_line(value.as_bytes()).unwrap().unwrap().total_tokens,
            12
        );
    }

    #[test]
    fn ignores_telemetry_and_rejects_unknown_token_shapes() {
        let telemetry = record(
            "e1",
            1_788_177_600_000_000,
            "resource_usage_sampled",
            r#"{"cpu_self_ms":1}"#,
            "unknown",
        );
        assert_eq!(parse_line(telemetry.as_bytes()).unwrap(), None);
        let unknown = record(
            "e2",
            1_788_177_600_000_000,
            "future_inference",
            r#"{"input_tokens":1,"output_tokens":1}"#,
            "muse-1",
        );
        assert_eq!(parse_line(unknown.as_bytes()), Err(()));

        let schema_drift = telemetry.replace("\"schema_version\":1", "\"schema_version\":2");
        assert_eq!(parse_line(schema_drift.as_bytes()), Err(()));
    }

    #[test]
    fn scans_deduplicated_events_and_reuses_cache() {
        let root = tempdir().unwrap();
        let session = root.path().join("2026/08/31/a");
        fs::create_dir_all(&session).unwrap();
        let first = record(
            "first",
            1_788_177_600_000_000,
            "model_completed",
            r#"{"input_tokens":10,"output_tokens":2}"#,
            "muse-1",
        );
        let second = record(
            "second",
            1_788_177_601_000_000,
            "model_completed",
            r#"{"input_tokens":20,"output_tokens":4}"#,
            "muse-1",
        );
        fs::write(
            session.join("session.jsonl"),
            format!("{first}\n{second}\n{first}\n"),
        )
        .unwrap();
        let duplicate_session = root.path().join("2026/08/31/b");
        fs::create_dir_all(&duplicate_session).unwrap();
        fs::write(
            duplicate_session.join("session.jsonl"),
            format!("{first}\n"),
        )
        .unwrap();
        let cache = tempdir().unwrap();
        let cold = scan_in(root.path(), cache.path(), "2026-08-31", "2026-08-31", None);
        let warm = scan_in(root.path(), cache.path(), "2026-08-31", "2026-08-31", None);
        assert_eq!(cold.coverage, LocalHistoryCoverage::Complete);
        assert_eq!(cold.total_tokens, Some(36));
        assert_eq!(cold.session_count, 1);
        assert_eq!(warm.total_tokens, cold.total_tokens);
    }

    #[test]
    fn aggregate_overflow_downgrades_coverage_without_publishing_total() {
        let root = tempdir().unwrap();
        let first_session = root.path().join("2026/08/30/first");
        let second_session = root.path().join("2026/08/31/second");
        fs::create_dir_all(&first_session).unwrap();
        fs::create_dir_all(&second_session).unwrap();
        let value = u64::MAX;
        fs::write(
            first_session.join("session.jsonl"),
            record(
                "first",
                1_777_000_000_000_000,
                "model_completed",
                &format!(r#"{{"input_tokens":{value},"output_tokens":0}}"#),
                "muse-1",
            ),
        )
        .unwrap();
        fs::write(
            second_session.join("session.jsonl"),
            record(
                "second",
                1_777_086_400_000_000,
                "model_completed",
                &format!(r#"{{"input_tokens":{value},"output_tokens":0}}"#),
                "muse-1",
            ),
        )
        .unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-04-01",
            "2026-12-31",
            None,
        );
        assert_eq!(report.total_tokens, None);
        assert_eq!(report.coverage, LocalHistoryCoverage::Partial);
    }

    #[test]
    fn complete_scan_with_only_ignored_records_is_a_known_zero() {
        let root = tempdir().unwrap();
        let session = root.path().join("2026/08/31/empty");
        fs::create_dir_all(&session).unwrap();
        fs::write(
            session.join("session.jsonl"),
            record(
                "telemetry",
                1_788_177_600_000_000,
                "resource_usage_sampled",
                r#"{"cpu_self_ms":1}"#,
                "unknown",
            ),
        )
        .unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert!(report.is_available());
        assert!(report.is_complete());
        assert_eq!(report.total_tokens, Some(0));
        assert_eq!(report.coverage, LocalHistoryCoverage::Complete);
    }

    #[test]
    fn discovery_errors_downgrade_coverage() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("2026"), b"not a directory").unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert!(!report.is_complete());
        assert_eq!(report.coverage, LocalHistoryCoverage::Partial);
    }

    #[test]
    fn old_malformed_history_does_not_downgrade_current_window() {
        let root = tempdir().unwrap();
        let old_session = root.path().join("2020/01/01/old");
        fs::create_dir_all(&old_session).unwrap();
        fs::write(old_session.join("session.jsonl"), b"not-json\n").unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert_eq!(report.coverage, LocalHistoryCoverage::Complete);
        assert_eq!(report.total_tokens, Some(0));
    }

    #[test]
    fn continuing_session_in_old_directory_contributes_current_event() {
        let root = tempdir().unwrap();
        let old_session = root.path().join("2020/01/01/old");
        fs::create_dir_all(&old_session).unwrap();
        fs::write(
            old_session.join("session.jsonl"),
            record(
                "current",
                1_788_177_600_000_000,
                "model_completed",
                r#"{"input_tokens":10,"output_tokens":2}"#,
                "muse-1",
            ),
        )
        .unwrap();

        let report = scan_in(
            root.path(),
            tempdir().unwrap().path(),
            "2026-08-31",
            "2026-08-31",
            None,
        );
        assert_eq!(report.coverage, LocalHistoryCoverage::Complete);
        assert_eq!(report.total_tokens, Some(12));
        assert_eq!(report.session_count, 1);
    }

    #[test]
    fn oversized_line_is_discarded_without_consuming_the_next_record() {
        let mut input = vec![b'x'; MAX_LINE_BYTES + 1];
        input.push(b'\n');
        input.extend_from_slice(b"{}\n");
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        assert!(
            read_bounded_line(&mut reader, MAX_LINE_BYTES)
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            read_bounded_line(&mut reader, MAX_LINE_BYTES)
                .unwrap()
                .unwrap(),
            b"{}\n"
        );
    }

    #[test]
    fn content_digest_changes_when_same_size_content_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::write(&path, b"aaaaaaaa").unwrap();
        let stamp = file_stamp(&path).unwrap();
        let mut state = ScanState {
            started: SystemTime::now(),
            files: 0,
            bytes: 0,
            retained_event_bytes: 0,
            cancelled: None,
        };
        let before = digest_bytes(&path, &stamp, &mut state).unwrap();
        fs::write(&path, b"bbbbbbbb").unwrap();
        let after = digest_bytes(&path, &stamp, &mut state);
        assert_ne!(after.as_deref(), Some(before.as_str()));
    }
}
