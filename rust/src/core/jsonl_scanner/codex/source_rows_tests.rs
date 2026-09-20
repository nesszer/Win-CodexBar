use super::*;
use std::io::{Seek, SeekFrom, Write};

#[test]
fn source_row_recovery_preserves_unanimous_pricing_only() {
    let cached = vec![
        CodexSourceUsageRow {
            day_key: "2026-09-19".to_string(),
            model: "gpt-5.5".to_string(),
            input: 200_000,
            cached: 0,
            output: 0,
            reasoning: None,
            source_end_offset: 100,
            pricing: CodexSourcePricingEvidence {
                pricing_model: Some("gpt-5.5".to_string()),
                pricing_mode: Some("priority".to_string()),
            },
        },
        CodexSourceUsageRow {
            day_key: "2026-09-19".to_string(),
            model: "gpt-5.5".to_string(),
            input: 200_000,
            cached: 0,
            output: 0,
            reasoning: None,
            source_end_offset: 200,
            pricing: CodexSourcePricingEvidence {
                pricing_model: Some("gpt-5.5".to_string()),
                pricing_mode: Some("standard".to_string()),
            },
        },
    ];
    let source = vec![
        cached[0].clone(),
        CodexSourceUsageRow {
            day_key: "2026-09-19".to_string(),
            model: "gpt-5.4".to_string(),
            input: 50_000,
            cached: 0,
            output: 0,
            reasoning: None,
            source_end_offset: 300,
            pricing: CodexSourcePricingEvidence {
                pricing_model: Some("gpt-5.4".to_string()),
                pricing_mode: Some("standard".to_string()),
            },
        },
    ];

    let recovered = recover_rows(&cached, &source, 200);
    assert_eq!(recovered.len(), 2);
    assert_eq!(recovered[0].pricing, CodexSourcePricingEvidence::default());
    assert_eq!(recovered[1].pricing, CodexSourcePricingEvidence::default());
}

#[test]
fn source_row_recovery_does_not_price_an_appended_duplicate() {
    let historical = CodexSourceUsageRow {
        day_key: "2026-09-19".to_string(),
        model: "gpt-5.5".to_string(),
        input: 200_000,
        cached: 0,
        output: 0,
        reasoning: None,
        source_end_offset: 100,
        pricing: CodexSourcePricingEvidence {
            pricing_model: Some("gpt-5.5".to_string()),
            pricing_mode: Some("priority".to_string()),
        },
    };
    let mut appended = historical.clone();
    appended.source_end_offset = 200;
    appended.pricing = CodexSourcePricingEvidence {
        pricing_model: Some("gpt-5.5".to_string()),
        pricing_mode: Some("standard".to_string()),
    };

    let recovered = recover_rows(
        std::slice::from_ref(&historical),
        &[historical.clone(), appended],
        100,
    );
    assert_eq!(
        recovered[0].pricing.pricing_mode.as_deref(),
        Some("priority")
    );
    assert_eq!(recovered[1].pricing, CodexSourcePricingEvidence::default());
}

#[test]
fn source_rows_from_records_use_source_model_as_initial_evidence() {
    let records = vec![(
        CodexUsageRecord {
            day_key: "2026-09-19".to_string(),
            model: "gpt-5.5-priority".to_string(),
            input: 12,
            cached: 20,
            output: -4,
            reasoning: Some(-2),
        },
        42,
    )];

    let rows = rows_from_records(&records);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cached, 12);
    assert_eq!(rows[0].output, 0);
    assert_eq!(rows[0].reasoning, Some(0));
    assert_eq!(rows[0].source_end_offset, 42);
    assert_eq!(
        rows[0].pricing.pricing_model.as_deref(),
        Some("gpt-5.5-priority")
    );
    assert_eq!(rows[0].pricing.pricing_mode.as_deref(), Some("priority"));
}

#[test]
fn source_row_cache_rejects_changed_prefix_and_accepts_append() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    std::fs::write(&path, b"stable-prefix\nrow\n").unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    let cached = row_cache(&path, &metadata, Vec::new()).expect("source cache");

    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"append\n")
        .unwrap();
    let grown = std::fs::metadata(&path).unwrap();
    assert!(row_cache_matches(&path, &grown, &cached));

    std::fs::write(&path, b"changed-prefix\nrow\napp").unwrap();
    let changed = std::fs::metadata(&path).unwrap();
    assert!(!row_cache_matches(&path, &changed, &cached));
}

#[test]
fn source_row_cache_hashes_the_entire_cached_prefix() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large-session.jsonl");
    let mut contents = vec![b'a'; 70 * 1024];
    contents.push(b'\n');
    std::fs::write(&path, contents).unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    let cached = row_cache(&path, &metadata, Vec::new()).expect("source cache");

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.seek(SeekFrom::Start(68 * 1024)).unwrap();
    file.write_all(b"b").unwrap();
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(b"append\n").unwrap();
    let changed = std::fs::metadata(&path).unwrap();
    assert!(!row_cache_matches(&path, &changed, &cached));
}

#[test]
fn pricing_mode_suffix_bijection_round_trips() {
    // Evidence derives the mode from the suffix; days_from_codex_source_rows
    // rebuilds the model from that mode, so the pair must round-trip.
    for (model, base) in [
        ("gpt-5.5", "gpt-5.5"),
        ("gpt-5.5-priority", "gpt-5.5"),
        ("gpt-5.6-sol", "gpt-5.6-sol"),
    ] {
        let evidence = pricing_evidence(model);
        assert_eq!(evidence.pricing_model.as_deref(), Some(model));
        let expected_mode = if model.ends_with("-priority") {
            "priority"
        } else {
            "standard"
        };
        assert_eq!(evidence.pricing_mode.as_deref(), Some(expected_mode));
        assert_eq!(model_of_pricing_mode(model), base);
    }
}
