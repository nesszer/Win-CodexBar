use super::*;

#[test]
fn session_meta_paginated_v2_subagents_have_independent_usage_identity() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("subagent.jsonl");
    for source in [
        serde_json::json!({"thread_source": "subagent"}),
        serde_json::json!({"source": {"subagent": {"thread_spawn": {
            "parent_thread_id": "parent-id", "depth": 1
        }}}}),
    ] {
        let mut payload = serde_json::json!({
            "id": "child-id",
            "session_id": "parent-id",
            "forked_from_id": "parent-id",
            "parent_thread_id": "parent-id",
            "history_mode": "paginated",
            "subagent_history_start_ordinal": 42,
            "multi_agent_version": "v2"
        });
        payload
            .as_object_mut()
            .unwrap()
            .extend(source.as_object().unwrap().clone());
        let row = serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-09-17T10:00:00Z",
            "payload": payload
        });
        std::fs::write(&path, format!("{row}\n")).unwrap();

        let metadata = JsonlScanner::read_codex_session_metadata(&path).unwrap();
        assert_eq!(metadata.session_id.as_deref(), Some("child-id"));
        assert_eq!(metadata.forked_from_id, None, "source: {source}");
        assert_eq!(
            metadata.fork_timestamp.as_deref(),
            Some("2026-09-17T10:00:00Z")
        );
    }
}

#[test]
fn session_meta_creator_account_flags_only_non_blank_ids() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("creator.jsonl");
    for (payload, expected) in [
        (
            serde_json::json!({"id": "s", "creator_account_id": "acct-123"}),
            true,
        ),
        (
            serde_json::json!({"id": "s", "creator_account_id": "  "}),
            false,
        ),
        (
            serde_json::json!({"id": "s", "creator_account_id": null}),
            false,
        ),
        (serde_json::json!({"id": "s"}), false),
    ] {
        let row = serde_json::json!({"type": "session_meta", "payload": payload});
        std::fs::write(&path, format!("{row}\n")).unwrap();
        let metadata = JsonlScanner::read_codex_session_metadata(&path).unwrap();
        assert_eq!(metadata.has_creator_account, expected, "payload: {payload}");
    }
}

#[test]
fn source_rows_key_auto_review_requests_only_for_chatgpt_rollouts() {
    let root = tempfile::tempdir().unwrap();
    let after = "2026-10-06T08:00:00Z";
    for (creator, expected_model) in [
        (Some("acct-123"), "codex-auto-review-included"),
        (None, "codex-auto-review"),
    ] {
        let mut payload = serde_json::json!({"id": "auto-review-session"});
        if let Some(creator) = creator {
            payload["creator_account_id"] = serde_json::json!(creator);
        }
        let lines = [
            serde_json::json!({"type": "session_meta", "timestamp": after, "payload": payload}),
            serde_json::json!({
                "type": "turn_context", "timestamp": after,
                "payload": {"model": "codex-auto-review"}
            }),
            serde_json::json!({
                "type": "event_msg", "timestamp": after,
                "payload": {"type": "token_count", "info": {"last_token_usage": {
                    "input_tokens": 100, "cached_input_tokens": 20, "output_tokens": 10
                }}}
            }),
        ];
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        let path = root
            .path()
            .join(format!("rollout-creator-{}.jsonl", creator.is_some()));
        std::fs::write(&path, body).unwrap();
        let range = CostUsageDayRange::new(
            chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
        );
        let rows = read_source_rows(&path, &range).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, expected_model, "creator: {creator:?}");
        assert_eq!(rows[0].input, 100);
    }
}

#[test]
fn session_meta_incomplete_v2_markers_preserve_inherited_fork_accounting() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("fork.jsonl");
    for markers in [
        serde_json::json!({"history_mode": "paginated", "multi_agent_version": "v2"}),
        serde_json::json!({
            "history_mode": "paginated", "multi_agent_version": "v2", "thread_source": "cli"
        }),
        serde_json::json!({"history_mode": "paginated", "thread_source": "subagent"}),
        serde_json::json!({"multi_agent_version": "v2", "thread_source": "subagent"}),
        serde_json::json!({
            "history_mode": "paginated", "multi_agent_version": "v1", "thread_source": "subagent"
        }),
        serde_json::json!({
            "history_mode": "full", "multi_agent_version": "v2", "thread_source": "subagent"
        }),
        serde_json::json!({
            "history_mode": "paginated", "multi_agent_version": "v2",
            "source": {"subagent": {"thread_spawn": null}}
        }),
    ] {
        let mut payload = serde_json::json!({
            "id": "child-id",
            "session_id": "parent-id",
            "forked_from_id": "parent-id"
        });
        payload
            .as_object_mut()
            .unwrap()
            .extend(markers.as_object().unwrap().clone());
        let row = serde_json::json!({"type": "session_meta", "payload": payload});
        std::fs::write(&path, format!("{row}\n")).unwrap();

        let metadata = JsonlScanner::read_codex_session_metadata(&path).unwrap();
        assert_eq!(metadata.session_id.as_deref(), Some("child-id"));
        assert_eq!(
            metadata.forked_from_id.as_deref(),
            Some("parent-id"),
            "incomplete markers must not opt into independent counters: {markers}"
        );
    }
}
