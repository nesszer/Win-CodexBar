use super::*;
use crate::core::CodexUsageRecord;
use chrono::DateTime;
use std::path::Path;

/// Cost of one Codex file over the last 30 days, through the record path.
pub(crate) fn scan_codex_file_cost(path: &Path) -> f64 {
    let today = chrono::Local::now().date_naive();
    let range = CostUsageDayRange::new(codex_period_start(today, 30), today);
    scan_codex_file_cost_for_range(path, &range)
}

fn scan_codex_file_cost_for_range(path: &Path, range: &CostUsageDayRange) -> f64 {
    let parse_result = match JsonlScanner::parse_codex_file(path, range, 0, None, None) {
        Ok(result) => result,
        Err(_) => return 0.0,
    };

    codex_records_cost(
        &parse_result
            .records
            .iter()
            .map(|(record, _)| record.clone())
            .collect::<Vec<_>>(),
        range,
    )
}

fn codex_records_cost(records: &[CodexUsageRecord], range: &CostUsageDayRange) -> f64 {
    let mut total_cost = 0.0;

    for record in records.iter().filter(|record| {
        CostUsageDayRange::is_in_range(&record.day_key, &range.since_key, &range.until_key)
    }) {
        if CostUsagePricing::is_codex_unattributed_model(&record.model)
            || !CostUsagePricing::counts_toward_codex_subscription(&record.model)
        {
            continue;
        }
        let tokens = CodexTokenCounts::from_values(record.input, record.cached, record.output);
        if !tokens.is_empty() {
            total_cost += codex_cost_usd_for_day(
                &record.model,
                tokens.input,
                tokens.cached,
                tokens.output,
                CostUsageDayRange::parse_day_key(&record.day_key),
            );
        }
    }

    total_cost
}

fn codex_cost_usd(model: &str, input: u64, cached: u64, output: u64) -> f64 {
    codex_cost_usd_for_day(model, input, cached, output, None)
}

fn codex_cost_usd_for_day(
    model: &str,
    input: u64,
    cached: u64,
    output: u64,
    pricing_day: Option<NaiveDate>,
) -> f64 {
    if CostUsagePricing::is_codex_unattributed_model(model) {
        return 0.0;
    }
    CostUsagePricing::codex_day_aggregate_cost_usd(model, input, cached, output, pricing_day)
        .unwrap_or_else(|| codex_cost_usd_fallback(model, input, cached, output))
}

#[test]
fn test_codex_pricing() {
    // Test GPT-4o pricing: $2.50/1M input, $10/1M output
    let cost = codex_cost_usd("gpt-4o", 1_000_000, 0, 1_000_000);
    assert!((cost - 12.50).abs() < 0.01);
}

#[test]
fn token_breakdown_addition_saturates_without_wrapping() {
    let counts = ModelTokenCounts {
        input_tokens: u64::MAX,
        output_tokens: 1,
        cached_tokens: u64::MAX,
        reasoning_tokens: Some(7),
    };
    assert_eq!(counts.total(), u64::MAX);

    let mut merged = ModelTokenCounts {
        input_tokens: u64::MAX,
        output_tokens: u64::MAX,
        cached_tokens: u64::MAX,
        reasoning_tokens: Some(7),
    };
    add_tokens(
        &mut merged,
        CodexTokenCounts {
            input: 1,
            cached: 1,
            output: 1,
            reasoning: Some(1),
        },
    );
    assert_eq!(merged.input_tokens, u64::MAX);
    assert_eq!(merged.output_tokens, u64::MAX);
    assert_eq!(merged.cached_tokens, u64::MAX);
}

#[test]
fn known_reasoning_is_exposed_without_changing_cost() {
    let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(target, target);
    let make_record = |reasoning| CodexUsageRecord {
        day_key: "2026-05-31".to_string(),
        timestamp: None,
        model: "gpt-5.6-sol".to_string(),
        input: 100,
        cached: 0,
        output: 20,
        reasoning,
        turn_id: None,
    };

    let mut known_summary = CostSummary::default();
    let (known_cost, known_has_tokens) =
        add_codex_records_to_summary(&mut known_summary, &[(make_record(Some(7)), 0)], &range);
    let mut unknown_summary = CostSummary::default();
    let (unknown_cost, unknown_has_tokens) =
        add_codex_records_to_summary(&mut unknown_summary, &[(make_record(None), 0)], &range);

    assert!(known_has_tokens && unknown_has_tokens);
    assert_eq!(known_summary.output_tokens, 20);
    assert_eq!(known_summary.reasoning_tokens, Some(7));
    assert_eq!(
        known_summary.by_model_tokens["gpt-5.6-sol"].reasoning_tokens,
        Some(7)
    );
    assert_eq!(known_cost, unknown_cost);
}

#[test]
fn reasoning_unknown_is_sticky_for_summary_and_model() {
    let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(target, target);
    let make_record = |reasoning| CodexUsageRecord {
        day_key: "2026-05-31".to_string(),
        timestamp: None,
        model: "gpt-5.6-sol".to_string(),
        input: 1,
        cached: 0,
        output: 20,
        reasoning,
        turn_id: None,
    };
    let records = vec![
        (make_record(Some(7)), 0),
        (make_record(None), 0),
        (make_record(Some(3)), 0),
    ];
    let mut summary = CostSummary::default();

    add_codex_records_to_summary(&mut summary, &records, &range);

    assert_eq!(summary.reasoning_tokens, None);
    assert_eq!(
        summary.by_model_tokens["gpt-5.6-sol"].reasoning_tokens,
        None
    );
}

#[test]
fn packed_reasoning_slot_distinguishes_known_from_unknown() {
    let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let mut known = CostSummary::default();
    add_codex_packed_tokens_to_summary(&mut known, "gpt-5.6-sol", &[100, 0, 20, 7], Some(target));
    assert_eq!(known.reasoning_tokens, Some(7));

    let mut unknown = CostSummary::default();
    add_codex_packed_tokens_to_summary(&mut unknown, "gpt-5.6-sol", &[100, 0, 20], Some(target));
    assert_eq!(unknown.reasoning_tokens, None);
}

#[test]
fn test_codex_pricing_uses_gpt55_standard_short_context_rates() {
    let cost = codex_cost_usd("gpt-5.5", 200_000, 80_000, 100_000);

    // GPT-5.5 standard short-context pricing:
    // 120k non-cached input at $5/M, 80k cached input at $0.50/M,
    // and 100k output at $30/M.
    assert!((cost - 3.64).abs() < 1e-9);
}

#[test]
fn test_codex_pricing_bills_whole_gpt55_request_at_long_context_rates() {
    let cost = codex_cost_usd("gpt-5.5", 1_000_000, 400_000, 1_000_000);

    // Above 272K input the whole request bills at $10/M input,
    // $1/M cached input and $45/M output.
    assert!((cost - 51.40).abs() < 1e-9);
}

#[test]
fn codex_summary_prices_gpt56_usage_records_individually() {
    let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(target, target);
    let records = vec![
        (
            CodexUsageRecord {
                day_key: "2026-05-31".to_string(),
                timestamp: None,
                model: "gpt-5.6-sol".to_string(),
                input: 200_000,
                cached: 0,
                output: 0,
                reasoning: None,
                turn_id: None,
            },
            0,
        ),
        (
            CodexUsageRecord {
                day_key: "2026-05-31".to_string(),
                timestamp: None,
                model: "gpt-5.6-sol".to_string(),
                input: 200_000,
                cached: 0,
                output: 0,
                reasoning: None,
                turn_id: None,
            },
            0,
        ),
        (
            CodexUsageRecord {
                day_key: "2026-05-30".to_string(),
                timestamp: None,
                model: "gpt-5.6-sol".to_string(),
                input: 200_000,
                cached: 0,
                output: 0,
                reasoning: None,
                turn_id: None,
            },
            0,
        ),
    ];
    let mut summary = CostSummary::default();

    let (cost, has_tokens) = add_codex_records_to_summary(&mut summary, &records, &range);

    assert!(has_tokens);
    assert_eq!(summary.input_tokens, 400_000);
    assert!((cost - 2.0).abs() < f64::EPSILON);
}

#[test]
fn cached_day_fold_preserves_gpt56_historical_pricing() {
    let before = NaiveDate::from_ymd_opt(2026, 7, 29).unwrap();
    let after = NaiveDate::from_ymd_opt(2026, 7, 30).unwrap();
    let range = CostUsageDayRange::new(before, after);
    let days = std::collections::HashMap::from([
        (
            "2026-07-29".to_string(),
            std::collections::HashMap::from([("gpt-5.6-terra".to_string(), vec![100, 10, 5])]),
        ),
        (
            "2026-07-30".to_string(),
            std::collections::HashMap::from([("gpt-5.6-terra".to_string(), vec![100, 10, 5])]),
        ),
    ]);
    let mut summary = CostSummary::default();
    let (cost, has_tokens) = add_codex_days_map_to_summary(&mut summary, &days, &range);

    let before_expected = 90.0 * 2.5e-6 + 10.0 * 2.5e-7 + 5.0 * 1.5e-5;
    let after_expected = 90.0 * 2e-6 + 10.0 * 2e-7 + 5.0 * 1.2e-5;
    assert!(has_tokens);
    assert!((cost - (before_expected + after_expected)).abs() < 1e-12);
}

#[test]
fn routed_models_do_not_count_toward_native_codex_summary() {
    let target = NaiveDate::from_ymd_opt(2026, 8, 19).unwrap();
    let range = CostUsageDayRange::new(target, target);
    let records = vec![
        (
            CodexUsageRecord {
                day_key: "2026-08-19".to_string(),
                timestamp: None,
                model: "gpt-5.6-sol".to_string(),
                input: 100,
                cached: 0,
                output: 5,
                reasoning: None,
                turn_id: None,
            },
            0,
        ),
        (
            CodexUsageRecord {
                day_key: "2026-08-19".to_string(),
                timestamp: None,
                model: "deepseek/deepseek-chat".to_string(),
                input: 1_000_000,
                cached: 0,
                output: 1_000_000,
                reasoning: None,
                turn_id: None,
            },
            0,
        ),
    ];
    let mut summary = CostSummary::default();
    let (cost, has_tokens) = add_codex_records_to_summary(&mut summary, &records, &range);
    assert!(has_tokens);
    assert_eq!(summary.input_tokens, 100);
    assert_eq!(summary.output_tokens, 5);
    assert!(!summary.by_model.contains_key("deepseek/deepseek-chat"));
    assert!(
        cost < 0.01,
        "routed DeepSeek cost leaked into native Codex: {cost}"
    );
}

#[test]
fn routed_models_are_not_persisted_in_codex_day_token_cache() {
    let records = vec![(
        CodexUsageRecord {
            day_key: "2026-08-19".to_string(),
            timestamp: None,
            model: "opencode/gpt-5".to_string(),
            input: 10,
            cached: 0,
            output: 1,
            reasoning: None,
            turn_id: None,
        },
        0,
    )];
    let mut days = std::collections::HashMap::new();
    merge_codex_records_into_days(&mut days, &records);
    assert!(days.is_empty());
}

#[test]
fn model_less_codex_usage_is_visible_but_unpriced() {
    let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(target, target);
    let records = vec![(
        CodexUsageRecord {
            day_key: "2026-05-31".to_string(),
            timestamp: None,
            model: CostUsagePricing::CODEX_UNATTRIBUTED_MODEL.to_string(),
            input: 55_000_000,
            cached: 0,
            output: 0,
            reasoning: None,
            turn_id: None,
        },
        0,
    )];
    let mut summary = CostSummary::default();

    let (cost, has_tokens) = add_codex_records_to_summary(&mut summary, &records, &range);

    assert!(has_tokens);
    assert_eq!(cost, 0.0);
    assert_eq!(summary.input_tokens, 55_000_000);
    assert_eq!(
        summary
            .by_model
            .get(CostUsagePricing::CODEX_UNATTRIBUTED_MODEL)
            .copied(),
        Some(0.0)
    );
    assert!(summary.unknown_models.is_empty());
}

#[test]
fn records_unknown_codex_model_while_using_fallback_cost() {
    let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let range = CostUsageDayRange::new(target, target);
    let records = vec![(
        CodexUsageRecord {
            day_key: "2026-05-31".to_string(),
            timestamp: None,
            model: "gpt-mystery".to_string(),
            input: 1_000_000,
            cached: 0,
            output: 1_000_000,
            reasoning: None,
            turn_id: None,
        },
        0,
    )];
    let mut summary = CostSummary::default();

    let (cost, has_tokens) = add_codex_records_to_summary(&mut summary, &records, &range);

    assert!(has_tokens);
    assert!(cost > 0.0);
    assert!(summary.unknown_models.contains("gpt-mystery"));
}

#[test]
fn test_codex_speed_bucket() {
    assert_eq!(codex_speed_bucket("gpt-5.5-fast"), "fast");
    assert_eq!(codex_speed_bucket("gpt-5.3-codex-spark"), "fast");
    assert_eq!(codex_speed_bucket("gpt-5-codex"), "standard");
}

#[test]
fn summary_wire_is_versioned_and_path_free() {
    let complete = CostSummary {
        total_cost_usd: 0.25,
        input_tokens: 100,
        output_tokens: 50,
        cached_tokens: 20,
        sessions_count: 1,
        by_model: std::collections::HashMap::from([("fixture-model".to_string(), 0.25)]),
        by_model_tokens: std::collections::HashMap::from([(
            "fixture-model".to_string(),
            ModelTokenCounts {
                input_tokens: 100,
                output_tokens: 50,
                cached_tokens: 20,
                reasoning_tokens: None,
            },
        )]),
        history_coverage_established: true,
        ..CostSummary::default()
    };
    let summary = CodexCostSummary::from_summaries_at(
        &complete,
        &complete,
        30,
        DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        "UTC",
    );

    summary.validate(30).unwrap();
    let wire = serde_json::to_string(&[summary]).unwrap();
    assert!(wire.contains("schemaVersion"));
    assert!(wire.contains("costUSD"));
    assert!(wire.contains("bucketTimeZone"));
    assert!(!wire.contains("fixture-model"));
    assert!(!wire.contains("sessions"));
    assert!(!wire.contains("project"));
}

#[test]
fn incomplete_scan_keeps_remote_totals_unknown() {
    let partial = CostSummary {
        total_cost_usd: 9.0,
        input_tokens: 1_000,
        output_tokens: 200,
        history_coverage_established: false,
        ..CostSummary::default()
    };
    let summary = CodexCostSummary::from_summaries_at(
        &partial,
        &partial,
        30,
        DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        "UTC",
    );

    assert_eq!(summary.history.total_tokens, None);
    assert_eq!(summary.history.cost_usd, None);
    assert_eq!(summary.today.total_tokens, None);
    assert_eq!(summary.today.cost_usd, None);
    summary.validate(30).unwrap();
}

#[test]
fn remote_summary_decoder_rejects_wrong_version_and_oversized_output() {
    let source = CostSummary {
        history_coverage_established: true,
        known_zero: true,
        ..CostSummary::default()
    };
    let mut summary = CodexCostSummary::from_summaries_at(
        &source,
        &source,
        30,
        DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        "UTC",
    );
    summary.schema_version = 2;
    let wire = serde_json::to_string(&[summary]).unwrap();
    assert!(decode_remote_codex_summary(&wire, 30).is_err());
    assert!(decode_remote_codex_summary(&"x".repeat(MAX_REMOTE_CODEX_COST_BYTES + 1), 30).is_err());
}

#[test]
fn remote_summary_decoder_rejects_numeric_totals_with_incomplete_coverage() {
    let source = CostSummary {
        history_coverage_established: true,
        known_zero: true,
        ..Default::default()
    };
    let mut summary = CodexCostSummary::from_summaries_at(
        &source,
        &source,
        30,
        DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        "UTC",
    );
    summary.history_coverage_is_established = false;
    summary.history.total_tokens = Some(42);
    summary.history.cost_usd = Some(1.25);

    let wire = serde_json::to_string(&[summary]).unwrap();
    assert!(decode_remote_codex_summary(&wire, 30).is_err());
}
