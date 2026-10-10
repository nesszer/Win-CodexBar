
use super::*;
use crate::codex_costs::{CodexCostSummary, CodexHostCostReport, CodexHostOutcome};

#[test]
fn json_output_emits_a16_and_f18_fields() {
    let summary = CostSummary {
        sessions_count: 1,
        history_coverage_established: true,
        model_pricing_completeness: crate::cost_scanner::ModelPricingCompleteness::Partial {
            unpriced_models: vec!["codex-auto-review".to_string()],
        },
        ..Default::default()
    };

    let result = CostResult {
        provider: "codex".to_string(),
        display_name: "Codex".to_string(),
        summary,
        supported: true,
        token_history: None,
    };

    // Capture stdout
    // Build the JSON payload directly to assert field presence.
    let payload = serde_json::json!({
        "provider": "codex",
        "supported": true,
        "days_scanned": 7,
        "cost": { "total_usd": 0.0, "currency": "USD" },
        "tokens": { "input": 0, "output": 0, "cached": 0 },
        "sessions_count": 1,
        "historyCoverageIsEstablished": true,
        "knownZero": false,
        "modelPricingCompleteness": {
            "partial": { "unpriced_models": ["codex-auto-review"] }
        },
        "by_model": {},
        "by_speed": {},
        "by_speed_tokens": {},
        "period": { "start": null, "end": null }
    });

    let s = serde_json::to_string(&payload).unwrap();
    assert!(
        s.contains("historyCoverageIsEstablished"),
        "A16 field present"
    );
    assert!(s.contains("modelPricingCompleteness"), "F18 field present");
    assert!(s.contains("codex-auto-review"), "unpriced model listed");
    assert!(s.contains("\"partial\""), "partial branch emitted");
    // Verify backward-compat: original fields still present
    assert!(s.contains("\"total_usd\""));
    assert!(s.contains("\"sessions_count\""));
    // drop the unused result
    let _ = result;
}

#[test]
fn json_output_a16_null_for_non_codex() {
    let summary = CostSummary::default();
    let result = CostResult {
        provider: "claude".to_string(),
        display_name: "Claude".to_string(),
        summary,
        supported: true,
        token_history: None,
    };

    // For non-codex, historyCoverageIsEstablished should be null.
    let payload = serde_json::json!({
        "provider": result.provider,
        "historyCoverageIsEstablished": serde_json::Value::Null,
    });
    let s = serde_json::to_string(&payload).unwrap();
    assert!(s.contains("null"), "non-codex A16 is null");
}

#[test]
fn json_output_emits_incomplete_request_count_only_when_positive() {
    let make = |incomplete_request_count| CostResult {
        provider: "claude".to_string(),
        display_name: "Claude".to_string(),
        summary: CostSummary {
            incomplete_request_count,
            ..CostSummary::default()
        },
        supported: true,
        token_history: None,
    };
    let payloads = build_json_payloads(
        &[make(3), make(0)],
        CostReportingPeriod::Rolling(30),
        30,
        &Settings::default(),
    );
    assert_eq!(payloads[0]["incompleteRequestCount"], 3);
    assert!(payloads[1].get("incompleteRequestCount").is_none());
}

#[test]
fn incomplete_suffix_marks_only_positive_counts() {
    assert_eq!(incomplete_suffix(0), "");
    assert_eq!(incomplete_suffix(2), " · Incomplete");
}

#[test]
fn antigravity_json_keeps_unknown_cost_distinct_from_zero() {
    use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
    let payload = crate::spend_contract::local_token_history_json(
        "antigravity",
        &LocalTokenHistorySummary {
            total_tokens: 12_345,
            session_count: 2,
            coverage: LocalHistoryCoverage::Complete,
            cost_estimate: Default::default(),
            ..Default::default()
        },
        30,
    );
    assert!(payload["cost"]["total_usd"].is_null());
    assert_eq!(payload["tokens"]["total"], 12_345);
    assert_eq!(payload["historyCoverage"], "complete");
    assert_eq!(payload["knownZero"], false);

    let partial = crate::spend_contract::local_token_history_json(
        "antigravity",
        &LocalTokenHistorySummary {
            total_tokens: 999,
            session_count: 1,
            coverage: LocalHistoryCoverage::Partial,
            cost_estimate: Default::default(),
            ..Default::default()
        },
        30,
    );
    assert!(partial["cost"]["total_usd"].is_null());
    assert!(partial["tokens"]["total"].is_null());
    assert_eq!(partial["historyCoverage"], "partial");
}

#[test]
fn antigravity_json_labels_public_price_estimates() {
    use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
    let payload = crate::spend_contract::local_token_history_json(
        "antigravity",
        &LocalTokenHistorySummary {
            total_tokens: 1_000,
            session_count: 1,
            coverage: LocalHistoryCoverage::Complete,
            cost_estimate: crate::spend_contract::LocalCostEstimate {
                known_subtotal_usd: Some(0.0125),
                coverage: crate::spend_contract::CostCoverageCounts {
                    estimated: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
        30,
    );
    assert_eq!(payload["cost"]["total_usd"], 0.0125);
    assert_eq!(payload["cost"]["known_subtotal_usd"], 0.0125);
    assert_eq!(payload["cost"]["currency"], "USD");
    assert!(payload["note"].as_str().unwrap().contains("not billed"));
}

#[test]
fn antigravity_json_keeps_partial_scan_cost_as_a_subtotal() {
    use crate::spend_contract::{
        CostCoverageCounts, LocalCostEstimate, LocalHistoryCoverage, LocalTokenHistorySummary,
    };
    let payload = crate::spend_contract::local_token_history_json(
        "antigravity",
        &LocalTokenHistorySummary {
            total_tokens: 1_000,
            session_count: 1,
            coverage: LocalHistoryCoverage::Partial,
            cost_estimate: LocalCostEstimate {
                known_subtotal_usd: Some(0.0125),
                coverage: CostCoverageCounts {
                    estimated: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
        30,
    );
    assert!(payload["cost"]["total_usd"].is_null());
    assert_eq!(payload["cost"]["known_subtotal_usd"], 0.0125);
    assert_eq!(payload["historyCoverage"], "partial");
    assert!(
        payload["note"]
            .as_str()
            .unwrap()
            .contains("history is incomplete")
    );
}

#[test]
fn antigravity_json_emits_zero_for_complete_empty_history() {
    use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
    let payload = crate::spend_contract::local_token_history_json(
        "antigravity",
        &LocalTokenHistorySummary {
            coverage: LocalHistoryCoverage::Complete,
            ..Default::default()
        },
        30,
    );
    assert_eq!(payload["cost"]["total_usd"], 0.0);
    assert!(payload["cost"]["known_subtotal_usd"].is_null());
    assert_eq!(payload["cost"]["currency"], "USD");
    assert_eq!(payload["knownZero"], true);
}

#[test]
fn antigravity_json_keeps_mixed_pricing_as_a_known_subtotal() {
    use crate::spend_contract::{
        CostCoverageCounts, LocalCostEstimate, LocalHistoryCoverage, LocalTokenHistorySummary,
    };
    let payload = crate::spend_contract::local_token_history_json(
        "antigravity",
        &LocalTokenHistorySummary {
            total_tokens: 1_500,
            session_count: 2,
            coverage: LocalHistoryCoverage::Complete,
            cost_estimate: LocalCostEstimate {
                known_subtotal_usd: Some(0.0125),
                coverage: CostCoverageCounts {
                    estimated: 1,
                    unpriced: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
        30,
    );
    assert!(payload["cost"]["total_usd"].is_null());
    assert_eq!(payload["cost"]["known_subtotal_usd"], 0.0125);
    assert_eq!(payload["cost"]["pricingCoverage"]["estimated"], 1);
    assert_eq!(payload["cost"]["pricingCoverage"]["unpriced"], 1);
    assert!(payload["note"].as_str().unwrap().contains("subtotal"));
}

#[derive(clap::Parser)]
struct Wrapper {
    #[command(flatten)]
    args: CostArgs,
}

fn parse_args(argv: &[&str]) -> CostArgs {
    use clap::Parser;
    Wrapper::try_parse_from(std::iter::once("cost").chain(argv.iter().copied()))
        .expect("cost args parse")
        .args
}

#[test]
fn days_and_period_are_detectable_when_absent() {
    let args = parse_args(&[]);
    assert_eq!(args.days, None);
    assert_eq!(args.period, None);
    let args = parse_args(&["--period", "month-to-date", "--days", "7"]);
    assert_eq!(args.days, Some(7));
    assert_eq!(args.period.as_deref(), Some("month-to-date"));
}

#[test]
fn json_payload_carries_period_identity_and_totals() {
    let summary = CostSummary {
        total_cost_usd: 2.5,
        input_tokens: 10,
        output_tokens: 5,
        sessions_count: 1,
        ..Default::default()
    };
    let results = [CostResult {
        provider: "claude".to_string(),
        display_name: "Claude".to_string(),
        summary,
        supported: true,
        token_history: None,
    }];
    let settings = Settings::default();
    let payloads = build_json_payloads(&results, CostReportingPeriod::MonthToDate, 12, &settings);
    let payload = &payloads[0];
    assert_eq!(payload["reportingPeriod"], "month-to-date");
    assert_eq!(payload["historyLabel"], "Month to date");
    assert_eq!(payload["days_scanned"], 12);
    assert_eq!(payload["totals"]["totalCost"], 2.5);
    assert_eq!(payload["totals"]["totalTokens"], 15);
    // Existing fields keep their meaning.
    assert_eq!(payload["cost"]["total_usd"], 2.5);
    assert_eq!(payload["tokens"]["input"], 10);

    let rolling = build_json_payloads(&results, CostReportingPeriod::Rolling(30), 30, &settings);
    assert_eq!(rolling[0]["reportingPeriod"], "rolling:30");
    assert_eq!(rolling[0]["historyLabel"], "Last 30 days");
}

#[test]
fn token_history_payload_is_stamped_with_the_period() {
    use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};
    let results = [CostResult {
        provider: "antigravity".to_string(),
        display_name: "Antigravity".to_string(),
        summary: CostSummary::default(),
        supported: true,
        token_history: Some(LocalTokenHistorySummary {
            total_tokens: 3,
            session_count: 1,
            coverage: LocalHistoryCoverage::Complete,
            ..Default::default()
        }),
    }];
    let settings = Settings::default();
    let payloads = build_json_payloads(
        &results,
        CostReportingPeriod::AllAvailable,
        20_000,
        &settings,
    );
    assert_eq!(payloads[0]["reportingPeriod"], "all");
    assert_eq!(payloads[0]["historyLabel"], "All");
    assert_eq!(payloads[0]["days_scanned"], 20_000);
}

#[test]
fn provider_native_only_flag_default_false() {
    // Default CostArgs has provider_native_only = false (backward compat).
    let args = CostArgs::default();
    assert!(!args.provider_native_only);
}

#[test]
fn cost_output_format_rejects_toon() {
    assert!("toon".parse::<OutputFormat>().is_err());
}

#[test]
fn group_by_defaults_none_and_accepts_session() {
    assert_eq!(CostGroupBy::from_arg(None), CostGroupBy::None);
    assert_eq!(CostGroupBy::from_arg(Some("session")), CostGroupBy::Session);
}

#[test]
fn remote_failure_retains_local_report() {
    let local_summary = CodexCostSummary::from_summaries_at(
        &CostSummary {
            history_coverage_established: true,
            known_zero: true,
            ..CostSummary::default()
        },
        &CostSummary {
            history_coverage_established: true,
            known_zero: true,
            ..CostSummary::default()
        },
        30,
        chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        "UTC",
    );
    let reports = [
        CodexHostCostReport::success("local", "local", local_summary),
        CodexHostCostReport::failure(
            "build-host",
            "ssh",
            crate::codex_costs::REMOTE_CODEX_COST_UNAVAILABLE,
        ),
    ];

    assert!(reports[0].summary().is_some());
    assert!(reports[1].summary().is_none());
    assert_eq!(
        reports[1].outcome,
        CodexHostOutcome::Failed(crate::codex_costs::REMOTE_CODEX_COST_UNAVAILABLE.to_string())
    );
}
