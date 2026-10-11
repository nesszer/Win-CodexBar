use super::super::CustomRates;
use super::cache::{load_entries_with_cache, read_cache};
use super::*;
use std::fs;

#[test]
fn aggregate_deduplicates_requests_and_applies_history_window() {
    let now = DateTime::parse_from_rfc3339("2026-08-19T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let make = |request_id: &str, timestamp: &str, input: u64| OpenCodexEntry {
        request_id: request_id.into(),
        timestamp: DateTime::parse_from_rfc3339(timestamp)
            .unwrap()
            .with_timezone(&Utc),
        provider: "openai".into(),
        model: "gpt-5".into(),
        usage_status: "reported".into(),
        conversation_id: Some(request_id.into()),
        input_tokens: Some(input),
        output_tokens: Some(1),
        cache_read_tokens: Some(0),
        cache_creation_tokens: None,
        reasoning_tokens: None,
        total_tokens: Some(input + 1),
    };
    let source = aggregate(
        vec![
            make("same", "2026-08-18T10:00:00Z", 10),
            make("same", "2026-08-18T11:00:00Z", 20),
            make("old", "2026-08-01T10:00:00Z", 30),
        ],
        now,
        7,
        &CustomPricing::default(),
    )
    .expect("source");
    assert_eq!(source.request_count, 1);
    assert_eq!(source.conversation_count, 1);
    assert_eq!(source.token_mix.input_tokens, Some(20));
    assert_eq!(source.coverage.priced, 1);
    assert!(source.known_cost_usd.is_some());
    assert_eq!(source.provenance, CostProvenance::VendorMetered);
}

#[test]
fn aggregate_preserves_list_and_mixed_provenance() {
    let now = DateTime::parse_from_rfc3339("2026-08-19T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut estimated = entry("openai", "gpt-5");
    estimated.request_id = "estimated".to_string();
    estimated.usage_status = "estimated".to_string();
    let list_only = aggregate(vec![estimated.clone()], now, 30, &CustomPricing::default())
        .expect("list-price source");
    assert_eq!(list_only.provenance, CostProvenance::ListPriceEstimate);

    let mut reported = entry("openai", "gpt-5");
    reported.request_id = "reported".to_string();
    let mixed = aggregate(
        vec![reported, estimated],
        now,
        30,
        &CustomPricing::default(),
    )
    .expect("mixed source");
    assert_eq!(mixed.provenance, CostProvenance::Mixed);
}

#[test]
fn aggregate_preserves_zero_cost_authoritative_provenance() {
    let now = DateTime::parse_from_rfc3339("2026-08-19T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let custom = CustomPricing {
        entries: std::collections::HashMap::from([(
            "openai/gpt-5".to_string(),
            CustomRates {
                input: Some(0.0),
                output: Some(0.0),
                cache_read: Some(0.0),
                cache_write: Some(0.0),
            },
        )]),
    };

    let reported = aggregate(vec![entry("openai", "gpt-5")], now, 30, &custom)
        .expect("zero-cost vendor source");
    assert_eq!(reported.known_cost_usd, Some(0.0));
    assert_eq!(reported.provenance, CostProvenance::VendorMetered);

    let mut estimated_entry = entry("openai", "gpt-5");
    estimated_entry.usage_status = "estimated".to_string();
    let estimated =
        aggregate(vec![estimated_entry], now, 30, &custom).expect("zero-cost list source");
    assert_eq!(estimated.known_cost_usd, Some(0.0));
    assert_eq!(estimated.provenance, CostProvenance::ListPriceEstimate);
}

// Regression (PR #611 review): cache_read is part of input for imports, and
// an authoritative `totalTokens` must not be re-derived. The window total
// must use the same per-entry totals as the model and daily rows.
#[test]
fn aggregate_window_total_matches_model_and_daily_totals() {
    let now = DateTime::parse_from_rfc3339("2026-08-19T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let authoritative = entry("openai", "gpt-5");
    let mut derived = entry("openai", "gpt-5.6-sol");
    derived.request_id = "derived".to_string();
    derived.input_tokens = Some(50);
    derived.output_tokens = Some(3);
    derived.cache_read_tokens = Some(8);
    derived.cache_creation_tokens = Some(2);
    derived.total_tokens = None;

    let source = aggregate(
        vec![authoritative, derived],
        now,
        30,
        &CustomPricing::default(),
    )
    .expect("source");

    // 105 (authoritative) + 50 + 3 + 2 (cache_read is inside input).
    assert_eq!(source.token_total, Some(160));
    let model_total: u64 = source.models.iter().map(|row| row.total_tokens).sum();
    let daily_total: u64 = source
        .daily
        .iter()
        .filter_map(|point| point.total_tokens)
        .sum();
    assert_eq!(model_total, 160);
    assert_eq!(daily_total, 160);
}

fn entry(provider: &str, model: &str) -> OpenCodexEntry {
    OpenCodexEntry {
        request_id: format!("{provider}:{model}"),
        timestamp: DateTime::parse_from_rfc3339("2026-07-29T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        provider: provider.to_string(),
        model: model.to_string(),
        usage_status: "reported".to_string(),
        conversation_id: None,
        input_tokens: Some(100),
        output_tokens: Some(5),
        cache_read_tokens: Some(10),
        cache_creation_tokens: None,
        reasoning_tokens: None,
        total_tokens: Some(105),
    }
}

#[test]
fn routes_opencodex_entries_into_subscription_rows() {
    assert_eq!(
        route_entry(&entry("openai", "gpt-5.6-sol")),
        RouteTarget::Subscription("codex")
    );
    assert_eq!(
        route_entry(&entry("opencode-go", "gpt-5.6-sol")),
        RouteTarget::Subscription("opencodego")
    );
    assert_eq!(
        route_entry(&entry("kimi-coding", "k2p5")),
        RouteTarget::Subscription("kimi")
    );
    assert_eq!(
        route_entry(&entry("deepseek", "deepseek-chat")),
        RouteTarget::Subscription("deepseek")
    );
    assert_eq!(
        route_entry(&entry("opencode-free", "free-model")),
        RouteTarget::TokenOnly
    );
}

#[test]
fn recorded_provider_wins_over_mismatched_model_namespace() {
    assert_eq!(
        route_entry(&entry("opencode-go", "openai/gpt-5.6-sol")),
        RouteTarget::Subscription("opencodego")
    );
    assert_eq!(
        route_entry(&entry("deepseek", "openai/gpt-5.6-sol")),
        RouteTarget::Subscription("deepseek")
    );
}

/// The models.dev identities that price a row, as (provider, model).
fn targets_of(provider: &str, model: &str) -> Vec<(String, String)> {
    models_dev_pricing_targets(&pricing_provider(&entry(provider, model)), model)
        .into_iter()
        .map(|target| (target.provider_id, target.model_id))
        .collect()
}

fn pair(provider: &str, model: &str) -> (String, String) {
    (provider.to_string(), model.to_string())
}

#[test]
fn legacy_openai_transport_still_uses_explicit_route() {
    assert_eq!(
        route_entry(&entry("openai", "opencode-go/deepseek-v4-flash")),
        RouteTarget::Subscription("opencodego")
    );
    assert_eq!(
        targets_of("openai", "opencode-go/gpt-5"),
        vec![pair("opencode-go", "gpt-5")]
    );
}

#[test]
fn pricing_targets_follow_the_recorded_provider() {
    assert_eq!(
        targets_of("opencode-go", "gpt-5"),
        vec![pair("opencode-go", "gpt-5")]
    );
    assert_eq!(
        targets_of("kimi-coding", "k2p5"),
        vec![pair("kimi-coding", "k2p5"), pair("kimi-for-coding", "k2p5")]
    );
    assert_eq!(
        targets_of("deepseek", "deepseek-chat"),
        vec![pair("deepseek", "deepseek-chat")]
    );
    // Another vendor's namespace is part of the model id on the recorded
    // provider's catalog, never a route to that vendor's own rates.
    assert_eq!(
        targets_of("opencode-go", "openai/gpt-5"),
        vec![pair("opencode-go", "openai/gpt-5")]
    );
}

#[test]
fn unknown_provider_or_namespace_fails_closed_for_routing_and_pricing() {
    let snapshot = ModelsDevPricingSnapshot::from_catalog_json_for_tests(
        r#"{"openai":{"models":{"gpt-5":{"id":"gpt-5","cost":{"input":2,"output":8,"cache_read":0.2}}}}}"#,
    )
    .expect("catalog");
    let none = CustomPricing::default();
    let proxy = entry("private-proxy", "openai/gpt-5");
    assert_eq!(route_entry(&proxy), RouteTarget::Unknown);
    assert_eq!(entry_cost(&proxy, &none, &snapshot), None);
    let malformed = entry("openai", "/gpt-5");
    assert_eq!(route_entry(&malformed), RouteTarget::Subscription("codex"));
    assert!(targets_of("openai", "/gpt-5").is_empty());
    assert_eq!(entry_cost(&malformed, &none, &snapshot), None);
}

#[test]
fn opencodex_uses_request_day_for_historical_gpt56_pricing() {
    let entry = entry("openai", "gpt-5.6-terra");
    let empty = ModelsDevPricingSnapshot::from_catalog_json_for_tests("{}").expect("catalog");
    let cost = entry_cost(&entry, &CustomPricing::default(), &empty).unwrap();
    let expected = 90.0 * 2.5e-6 + 10.0 * 2.5e-7 + 5.0 * 1.5e-5;
    assert!((cost - expected).abs() < 1e-12);
}

#[test]
fn opencodex_historical_gpt56_bills_cache_writes_at_their_own_rate() {
    let mut entry = entry("openai", "gpt-5.6-terra");
    entry.cache_creation_tokens = Some(20);
    let empty = ModelsDevPricingSnapshot::from_catalog_json_for_tests("{}").expect("catalog");
    let cost = entry_cost(&entry, &CustomPricing::default(), &empty).unwrap();
    // Input includes cache reads and writes; each lane has its own rate.
    let expected = 70.0 * 2.5e-6 + 10.0 * 2.5e-7 + 20.0 * 3.125e-6 + 5.0 * 1.5e-5;
    assert!((cost - expected).abs() < 1e-12);
}

#[test]
fn parser_keeps_reported_token_classes() {
    let value = serde_json::json!({
        "requestId": "r1", "timestamp": "2026-08-18T10:00:00Z", "provider": "openai",
        "model": "gpt-test", "usageStatus": "reported", "conversationId": "c1",
        "usage": {"inputTokens": 10, "outputTokens": 4, "cachedInputTokens": 3, "reasoningOutputTokens": 2}
    });
    let entry = parse_line(&value.to_string()).expect("entry");
    assert_eq!(entry.model, "gpt-test");
    assert_eq!(entry.input_tokens, Some(10));
    assert_eq!(entry.output_tokens, Some(4));
    assert_eq!(entry.cache_read_tokens, Some(3));
    assert_eq!(entry.reasoning_tokens, Some(2));
}

#[test]
fn parser_normalizes_defaults_and_rejects_malformed_lines() {
    let minimal = serde_json::json!({
        "requestId": "  r1  ", "model": "gpt-test", "timestamp": "2026-08-18T10:00:00Z",
        "usageStatus": "  REPORTED ", "usage": {"cacheCreationInputTokens": 7}
    });
    let entry = parse_line(&minimal.to_string()).expect("entry");
    assert_eq!(entry.request_id, "r1", "ids are trimmed");
    assert_eq!(
        entry.provider, "openai",
        "missing provider defaults to openai"
    );
    assert_eq!(
        entry.usage_status, "reported",
        "status is lowercased and trimmed"
    );
    assert_eq!(entry.conversation_id, None);
    assert_eq!(entry.cache_creation_tokens, Some(7));

    for malformed in [
        "{}",
        r#"{"requestId": "", "model": "m", "timestamp": "2026-08-18T10:00:00Z"}"#,
        r#"{"requestId": "r1", "model": "   ", "timestamp": "2026-08-18T10:00:00Z"}"#,
        r#"{"requestId": "r1", "model": "m"}"#,
        "not json at all",
    ] {
        assert!(parse_line(malformed).is_none(), "rejected: {malformed}");
    }
}

#[test]
fn incremental_cache_appends_only_newline_terminated_tail() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("usage.jsonl");
    let cache = dir.path().join("cache.sqlite");
    let row = |id: &str, input: u64| {
        format!(
            r#"{{"requestId":"{id}","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z","usageStatus":"reported","usage":{{"inputTokens":{input}}}}}"#
        )
    };

    fs::write(&log, format!("{}\n{}\n", row("a", 1), row("b", 2))).unwrap();
    let first = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    let first_cursor = read_cache(&cache).unwrap().cursor;

    let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
    use std::io::Write as _;
    writeln!(file, "{}", row("c", 3)).unwrap();
    drop(file);

    let second = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(
        second
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    let second_cursor = read_cache(&cache).unwrap().cursor;
    assert!(second_cursor.parsed_offset > first_cursor.parsed_offset);
}

#[test]
fn incomplete_trailing_opencodex_record_waits_for_newline() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("usage.jsonl");
    let cache = dir.path().join("cache.sqlite");
    let complete = r#"{"requestId":"a","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z"}"#;
    let pending = r#"{"requestId":"b","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z"}"#;
    let split = pending.len() / 2;
    fs::write(&log, format!("{complete}\n{}", &pending[..split])).unwrap();

    let first = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );
    let cursor = read_cache(&cache).unwrap().cursor;
    assert_eq!(
        cursor.parsed_offset,
        u64::try_from(complete.len() + 1).unwrap()
    );

    let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
    use std::io::Write as _;
    writeln!(file, "{}", &pending[split..]).unwrap();
    drop(file);
    let second = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(
        second
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn complete_trailing_opencodex_record_waits_for_newline() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("usage.jsonl");
    let cache = dir.path().join("cache.sqlite");
    let first = r#"{"requestId":"a","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z"}"#;
    let trailing = r#"{"requestId":"b","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z"}"#;
    fs::write(&log, format!("{first}\n{trailing}")).unwrap();

    let before_newline = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(
        before_newline
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );

    let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
    use std::io::Write as _;
    writeln!(file).unwrap();
    drop(file);

    let after_newline = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(
        after_newline
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn later_request_id_replaces_cached_entry_without_full_cache_loss() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("usage.jsonl");
    let cache = dir.path().join("cache.sqlite");
    let row = |id: &str, input: u64| {
        format!(
            r#"{{"requestId":"{id}","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z","usageStatus":"reported","usage":{{"inputTokens":{input}}}}}"#
        )
    };
    fs::write(&log, format!("{}\n{}\n", row("dup", 1), row("keep", 2))).unwrap();
    let _ = load_entries_with_cache(&log, &cache).unwrap();
    let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
    use std::io::Write as _;
    writeln!(file, "{}", row("dup", 9)).unwrap();
    drop(file);

    let entries = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.request_id == "dup")
            .unwrap()
            .input_tokens,
        Some(9)
    );
    assert!(entries.iter().any(|entry| entry.request_id == "keep"));
}

#[test]
fn truncation_invalidates_opencodex_cursor_and_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("usage.jsonl");
    let cache = dir.path().join("cache.sqlite");
    let old = r#"{"requestId":"old","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z"}"#;
    let replacement = r#"{"requestId":"new","model":"gpt-5","timestamp":"2026-08-18T10:00:00Z"}"#;
    fs::write(&log, format!("{old}\n{old}\n")).unwrap();
    let _ = load_entries_with_cache(&log, &cache).unwrap();
    fs::write(&log, format!("{replacement}\n")).unwrap();

    let rebuilt = load_entries_with_cache(&log, &cache).unwrap();
    assert_eq!(
        rebuilt
            .iter()
            .map(|entry| entry.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["new"]
    );
}

#[test]
fn timestamps_parse_rfc3339_epoch_seconds_and_millis() {
    let expected = Utc.with_ymd_and_hms(2026, 8, 18, 10, 0, 0).unwrap();
    let rfc3339 = serde_json::json!("2026-08-18T10:00:00Z");
    assert_eq!(parse_timestamp(&rfc3339), Some(expected));
    let epoch_seconds = serde_json::json!(1_787_047_200i64);
    assert_eq!(parse_timestamp(&epoch_seconds), Some(expected));
    let epoch_millis = serde_json::json!(1_787_047_200_000f64);
    assert_eq!(parse_timestamp(&epoch_millis), Some(expected));
    let numeric_string = serde_json::json!("1787047200.0");
    assert_eq!(parse_timestamp(&numeric_string), Some(expected));
    for invalid in [
        serde_json::json!("not a date"),
        serde_json::json!(0),
        serde_json::json!(-5.0),
        serde_json::Value::Null,
        serde_json::json!(true),
    ] {
        assert!(parse_timestamp(&invalid).is_none(), "rejected: {invalid}");
    }
}

#[test]
fn nonnegative_u64_accepts_json_numbers_and_bounded_floats() {
    assert_eq!(nonnegative_u64(Some(&serde_json::json!(42))), Some(42));
    assert_eq!(nonnegative_u64(Some(&serde_json::json!(12.0))), Some(12));
    // Fractional floats are accepted via `as u64` truncation.
    assert_eq!(nonnegative_u64(Some(&serde_json::json!(1.5))), Some(1));
    // `u64::MAX as f64` rounds up to 2^64; f64 spacing there is 4096, so
    // +2048.0 rounds back into range. First out-of-range step is +4096.0.
    assert_eq!(
        nonnegative_u64(Some(&serde_json::json!(u64::MAX as f64 + 4096.0))),
        None
    );
    assert_eq!(nonnegative_u64(None), None);
}

#[test]
fn aggregate_counts_one_activity_conversation_per_kept_row() {
    use super::super::tests::activity::{cells, counted};
    let now = DateTime::parse_from_rfc3339("2026-08-19T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let at = |timestamp: &str| {
        DateTime::parse_from_rfc3339(timestamp)
            .unwrap()
            .with_timezone(&Utc)
    };
    let row = |request_id: &str, timestamp: DateTime<Utc>| OpenCodexEntry {
        request_id: request_id.into(),
        timestamp,
        ..entry("openai", "gpt-5")
    };
    let first = at("2026-08-18T10:00:00Z");
    let second = at("2026-08-18T10:10:00Z");
    let other_day = at("2026-08-17T03:00:00Z");
    let source = aggregate(
        vec![
            row("a", first),
            row("b", second),
            row("c", other_day),
            row("old", at("2026-08-01T10:00:00Z")),
        ],
        now,
        7,
        &CustomPricing::default(),
    )
    .expect("source");
    assert_eq!(
        cells(&source.hourly_activity),
        counted(&[first, second, other_day])
    );
}
