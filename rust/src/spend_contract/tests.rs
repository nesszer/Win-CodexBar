use super::*;

pub(super) mod activity;

#[test]
fn local_history_total_requires_complete_scan_and_pricing() {
    let priced = LocalCostEstimate {
        known_subtotal_usd: Some(1.25),
        coverage: CostCoverageCounts {
            estimated: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let partial_history = LocalTokenHistorySummary {
        total_tokens: 100,
        session_count: 1,
        coverage: LocalHistoryCoverage::Partial,
        cost_estimate: priced.clone(),
        ..Default::default()
    };
    assert_eq!(partial_history.total_usd(), None);
    assert_eq!(partial_history.cost_estimate.known_subtotal_usd, Some(1.25));

    let mixed_pricing = LocalTokenHistorySummary {
        total_tokens: 100,
        session_count: 1,
        coverage: LocalHistoryCoverage::Complete,
        cost_estimate: LocalCostEstimate {
            known_subtotal_usd: Some(1.25),
            coverage: CostCoverageCounts {
                estimated: 1,
                unpriced: 1,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(mixed_pricing.total_usd(), None);
    assert_eq!(mixed_pricing.cost_estimate.known_subtotal_usd, Some(1.25));

    let complete = LocalTokenHistorySummary {
        total_tokens: 100,
        session_count: 1,
        coverage: LocalHistoryCoverage::Complete,
        cost_estimate: priced,
        ..Default::default()
    };
    assert_eq!(complete.total_usd(), Some(1.25));
}

#[test]
fn complete_empty_local_history_has_a_known_zero_total() {
    let history = LocalTokenHistorySummary {
        coverage: LocalHistoryCoverage::Complete,
        ..Default::default()
    };

    assert_eq!(history.total_usd(), Some(0.0));
}

#[test]
fn coverage_ratio_counts_estimated_as_covered() {
    let coverage = CostCoverageCounts {
        priced: 1,
        unpriced: 1,
        unmetered: 0,
        estimated: 2,
    };
    assert_eq!(coverage.coverage_ratio(), Some(0.75));
}

#[test]
fn coverage_overflow_is_unknown_instead_of_a_fake_ratio() {
    let coverage = CostCoverageCounts {
        priced: u32::MAX,
        unpriced: 1,
        unmetered: 0,
        estimated: 0,
    };
    assert_eq!(coverage.total(), u32::MAX);
    assert_eq!(coverage.checked_total(), None);
    assert_eq!(coverage.coverage_ratio(), None);

    let (merged, exact) = merge_coverage(
        CostCoverageCounts {
            priced: u32::MAX,
            ..CostCoverageCounts::default()
        },
        &CostCoverageCounts {
            priced: 1,
            ..CostCoverageCounts::default()
        },
    );
    assert!(!exact);
    assert_eq!(merged.priced, u32::MAX);
}

#[test]
fn explicit_zero_custom_rate_is_known_free_but_missing_rate_is_unknown() {
    let counts = ModelTokenCounts {
        input_tokens: 1_000_000,
        output_tokens: 0,
        cached_tokens: 0,
        reasoning_tokens: None,
    };
    let free = CustomRates {
        input: Some(0.0),
        ..CustomRates::default()
    };
    let missing = CustomRates::default();
    assert_eq!(free.cost(&counts), Some(0.0));
    assert_eq!(missing.cost(&counts), None);
}

#[test]
fn custom_pricing_reads_each_entry_on_its_own_like_upstream() {
    let custom = CustomPricing::parse(
        br#"{
            " GPT-5 ": {"input": 1.25, "output": 10, "cacheRead": 0.125, "cache_read": 9},
            "negative-input": {"input": -1, "output": 2},
            "string-rate": {"input": "3", "output": 4},
            "snake": {"cache_write": 0, "cacheCreation": 7},
            "empty": {},
            "only-unusable": {"input": -1, "output": "free"},
            "not-an-object": 5,
            "  ": {"input": 1}
        }"#,
    );
    let rates = |key: &str| {
        let rates = custom.entries.get(key).expect(key);
        (
            rates.input,
            rates.output,
            rates.cache_read,
            rates.cache_write,
        )
    };
    assert_eq!(rates("gpt-5"), (Some(1.25), Some(10.0), Some(0.125), None));
    assert_eq!(rates("negative-input"), (None, Some(2.0), None, None));
    assert_eq!(rates("string-rate"), (None, Some(4.0), None, None));
    assert_eq!(rates("snake"), (None, None, None, Some(0.0)));
    assert_eq!(
        custom.entries.len(),
        4,
        "entries without a usable rate are dropped"
    );
    assert!(CustomPricing::parse(b"[1, 2]").entries.is_empty());
    assert!(CustomPricing::parse(b"not json").entries.is_empty());
}

#[test]
fn local_spend_contract_exposes_reasoning_tokens_and_preserves_unknown() {
    let known_summary = CostSummary {
        reasoning_tokens: Some(7),
        ..CostSummary::default()
    };
    let known = build_local_spend_contract_from_summary(
        "unknown-provider",
        30,
        false,
        false,
        false,
        known_summary,
    );
    assert_eq!(known.token_mix.reasoning_tokens, Some(7));
    let known_json = serde_json::to_value(&known).expect("known spend contract serializes");
    assert_eq!(known_json["tokenMix"]["reasoningTokens"], 7);

    let unknown = build_local_spend_contract_from_summary(
        "unknown-provider",
        30,
        false,
        false,
        false,
        CostSummary::default(),
    );
    assert_eq!(unknown.token_mix.reasoning_tokens, None);
    let unknown_json = serde_json::to_value(&unknown).expect("unknown spend contract serializes");
    assert!(unknown_json["tokenMix"]["reasoningTokens"].is_null());
}

#[test]
fn sum_optional_cost_propagates_unknown_and_rejects_non_finite() {
    assert_eq!(sum_optional_cost(Some(1.5), Some(2.25)), Some(3.75));
    assert_eq!(sum_optional_cost(Some(1.5), None), Some(1.5));
    assert_eq!(sum_optional_cost(None, Some(2.0)), Some(2.0));
    assert_eq!(sum_optional_cost(None, None), None);
    assert_eq!(sum_optional_cost(Some(f64::INFINITY), Some(1.0)), None);
    assert_eq!(sum_optional_cost(Some(-1.0), None), None);
    assert_eq!(sum_optional_cost(Some(1.0), Some(f64::NAN)), None);
}

#[test]
fn add_optional_token_counts_guard_against_overflow() {
    assert_eq!(add_optional(Some(2), Some(3)), Some(5));
    assert_eq!(add_optional(Some(2), None), Some(2));
    assert_eq!(add_optional(None, None), None);
    assert_eq!(add_optional(Some(u64::MAX), Some(1)), None);
}

#[test]
fn merge_token_mix_preserves_optional_reasoning_and_unknown_classes() {
    let merged = merge_token_mix(
        SpendTokenMix {
            input_tokens: None,
            reasoning_tokens: Some(2),
            ..SpendTokenMix::default()
        },
        &SpendTokenMix {
            input_tokens: Some(5),
            reasoning_tokens: Some(3),
            ..SpendTokenMix::default()
        },
    );
    assert_eq!(merged.input_tokens, Some(5));
    assert_eq!(merged.reasoning_tokens, Some(5));
    assert_eq!(merged.output_tokens, None);
}

#[test]
fn merge_token_mix_keeps_overflow_unknown_across_later_sources() {
    let overflowed = merge_token_mix(
        SpendTokenMix {
            input_tokens: Some(u64::MAX),
            ..SpendTokenMix::default()
        },
        &SpendTokenMix {
            input_tokens: Some(1),
            ..SpendTokenMix::default()
        },
    );
    assert_eq!(overflowed.input_tokens, None);

    let merged = merge_token_mix(
        overflowed,
        &SpendTokenMix {
            input_tokens: Some(2),
            ..SpendTokenMix::default()
        },
    );
    assert_eq!(merged.input_tokens, None);
}

#[test]
fn merge_models_combines_duplicate_models_and_sorts_priced_first() {
    let make_row = |model: &str, cost: Option<f64>, input: u64, custom: bool| SpendModelRow {
        model: model.to_string(),
        cost_usd: cost,
        input_tokens: input,
        output_tokens: 0,
        cache_read_tokens: 0,
        total_tokens: input,
        custom_pricing: custom,
    };
    let merged = merge_models(
        vec![
            make_row("beta", Some(1.0), 10, false),
            make_row("alpha", None, 5, false),
            make_row("zzz", Some(1.0), 1, false),
            make_row("aaa", Some(1.0), 1, false),
        ],
        &[make_row("beta", Some(2.0), 7, true)],
    );
    let names: Vec<&str> = merged.iter().map(|row| row.model.as_str()).collect();
    assert_eq!(
        names,
        ["beta", "aaa", "zzz", "alpha"],
        "cost desc, then name asc for ties, unknown cost last"
    );
    let beta = &merged[0];
    assert_eq!(beta.cost_usd, Some(3.0));
    assert_eq!(beta.input_tokens, 17);
    assert_eq!(beta.total_tokens, 17);
    assert!(beta.custom_pricing, "custom pricing flags are OR-ed");
}

#[test]
fn merge_daily_sums_matching_days_and_keeps_iso_day_ordering() {
    let make_point = |day: &str, cost: Option<f64>, tokens: Option<u64>| SpendDailyPoint {
        day: day.to_string(),
        cost_usd: cost,
        total_tokens: tokens,
    };
    let merged = merge_daily(
        vec![
            make_point("2026-08-02", Some(1.0), Some(10)),
            make_point("2026-08-01", None, None),
        ],
        &[
            make_point("2026-08-02", Some(2.5), Some(15)),
            make_point("2026-08-03", Some(4.0), None),
        ],
    );
    let days: Vec<&str> = merged.iter().map(|point| point.day.as_str()).collect();
    assert_eq!(days, ["2026-08-01", "2026-08-02", "2026-08-03"]);
    assert_eq!(merged[1].cost_usd, Some(3.5));
    assert_eq!(merged[1].total_tokens, Some(25));
    assert_eq!(merged[0].cost_usd, None, "unknown stays unknown");
    assert_eq!(merged[0].total_tokens, None);
    assert_eq!(merged[2].total_tokens, None);
}

#[test]
fn resolve_spend_preserves_merged_report_details() {
    let native_models = vec![SpendModelRow {
        model: "gpt-5".to_string(),
        cost_usd: Some(1.0),
        input_tokens: 10,
        output_tokens: 2,
        cache_read_tokens: 0,
        total_tokens: 12,
        custom_pricing: false,
    }];
    let imported = ImportedSpendSource {
        source_id: "fixture".to_string(),
        display_name: "Fixture".to_string(),
        request_count: 2,
        conversation_count: 1,
        known_cost_usd: Some(2.0),
        provenance: CostProvenance::VendorMetered,
        token_mix: SpendTokenMix {
            input_tokens: Some(5),
            cache_read_tokens: Some(4),
            reasoning_tokens: Some(1),
            ..SpendTokenMix::default()
        },
        token_total: Some(5),
        coverage: CostCoverageCounts {
            priced: 2,
            unpriced: 1,
            unmetered: 0,
            estimated: 1,
        },
        models: vec![SpendModelRow {
            model: "gpt-5".to_string(),
            cost_usd: Some(2.0),
            input_tokens: 5,
            output_tokens: 0,
            cache_read_tokens: 4,
            total_tokens: 9,
            custom_pricing: true,
        }],
        daily: vec![
            SpendDailyPoint {
                day: "2026-08-01".to_string(),
                cost_usd: Some(2.0),
                total_tokens: Some(9),
            },
            SpendDailyPoint {
                day: "2026-08-02".to_string(),
                cost_usd: None,
                total_tokens: None,
            },
        ],
        hourly_activity: vec![SpendActivityCell {
            weekday: 1,
            hour: 2,
            conversations: 4,
        }],
    };

    let resolved = resolve_spend(
        Some(1.0),
        CostProvenance::ListPriceEstimate,
        true,
        CostCoverageCounts {
            priced: 1,
            unpriced: 0,
            unmetered: 1,
            estimated: 0,
        },
        SpendTokenMix {
            input_tokens: Some(10),
            output_tokens: Some(2),
            ..SpendTokenMix::default()
        },
        native_models,
        vec![SpendDailyPoint {
            day: "2026-08-01".to_string(),
            cost_usd: Some(1.0),
            total_tokens: Some(12),
        }],
        vec![SpendActivityCell {
            weekday: 1,
            hour: 2,
            conversations: 3,
        }],
        Some(&imported),
        false,
    );

    assert_eq!(resolved.known_cost_usd, Some(3.0));
    assert_eq!(resolved.provenance, CostProvenance::Mixed);
    assert_eq!(resolved.token_mix.input_tokens, Some(15));
    assert_eq!(resolved.token_mix.output_tokens, Some(2));
    assert_eq!(resolved.token_mix.cache_read_tokens, Some(4));
    assert_eq!(resolved.token_mix.reasoning_tokens, Some(1));
    assert_eq!(resolved.price_coverage.priced, 3);
    assert_eq!(resolved.price_coverage.unpriced, 1);
    assert_eq!(resolved.price_coverage.unmetered, 1);
    assert_eq!(resolved.price_coverage.estimated, 1);
    assert!(resolved.price_coverage_exact);

    let model = &resolved.models[0];
    assert_eq!(model.cost_usd, Some(3.0));
    assert_eq!(model.input_tokens, 15);
    assert_eq!(model.cache_read_tokens, 4);
    assert!(model.custom_pricing);
    assert_eq!(resolved.daily[0].cost_usd, Some(3.0));
    assert_eq!(resolved.daily[0].total_tokens, Some(21));
    assert_eq!(resolved.daily[1].cost_usd, None);
    assert_eq!(resolved.daily[1].total_tokens, None);
    assert_eq!(resolved.hourly_activity[0].conversations, 7);
}

#[test]
fn cost_provenance_for_window_matches_upstream_truth_table() {
    let cases = [
        (
            CostProvenance::ListPriceEstimate,
            false,
            false,
            CostProvenance::Unknown,
        ),
        (
            CostProvenance::ListPriceEstimate,
            true,
            false,
            CostProvenance::ListPriceEstimate,
        ),
        (
            CostProvenance::VendorMetered,
            false,
            false,
            CostProvenance::Unknown,
        ),
        (
            CostProvenance::VendorMetered,
            true,
            false,
            CostProvenance::VendorMetered,
        ),
        (
            CostProvenance::VendorMetered,
            false,
            true,
            CostProvenance::VendorMetered,
        ),
        (CostProvenance::Mixed, false, false, CostProvenance::Unknown),
        (
            CostProvenance::Mixed,
            true,
            false,
            CostProvenance::ListPriceEstimate,
        ),
        (
            CostProvenance::Mixed,
            false,
            true,
            CostProvenance::VendorMetered,
        ),
        (CostProvenance::Mixed, true, true, CostProvenance::Mixed),
        (CostProvenance::Unknown, true, true, CostProvenance::Unknown),
    ];

    for (snapshot, has_window_costs, includes_metered, expected) in cases {
        assert_eq!(
            CostProvenance::for_window(snapshot, has_window_costs, includes_metered),
            expected,
            "snapshot={snapshot:?}, has_window_costs={has_window_costs}, includes_metered={includes_metered}"
        );
    }
}

#[test]
fn zero_cost_authoritative_sources_use_presence_not_positive_value() {
    assert_eq!(
        CostProvenance::for_window(CostProvenance::ListPriceEstimate, true, false),
        CostProvenance::ListPriceEstimate
    );
    assert_eq!(
        CostProvenance::for_window(CostProvenance::VendorMetered, true, false),
        CostProvenance::VendorMetered
    );

    let resolved = resolve_spend(
        Some(0.0),
        CostProvenance::ListPriceEstimate,
        true,
        CostCoverageCounts::default(),
        SpendTokenMix::default(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        false,
    );
    assert_eq!(resolved.known_cost_usd, Some(0.0));
    assert_eq!(resolved.provenance, CostProvenance::ListPriceEstimate);
}

#[test]
fn provenance_merge_keeps_unknown_conservative_and_mixes_vendor_with_list() {
    assert_eq!(
        merge_provenance(
            CostProvenance::ListPriceEstimate,
            true,
            CostProvenance::VendorMetered,
            true,
        ),
        CostProvenance::Mixed
    );
    assert_eq!(
        merge_provenance(
            CostProvenance::ListPriceEstimate,
            true,
            CostProvenance::ListPriceEstimate,
            true,
        ),
        CostProvenance::ListPriceEstimate
    );
    assert_eq!(
        merge_provenance(
            CostProvenance::VendorMetered,
            true,
            CostProvenance::VendorMetered,
            true,
        ),
        CostProvenance::VendorMetered
    );
    assert_eq!(
        merge_provenance(
            CostProvenance::Unknown,
            true,
            CostProvenance::ListPriceEstimate,
            true,
        ),
        CostProvenance::Unknown
    );
    assert_eq!(
        merge_provenance(
            CostProvenance::Unknown,
            false,
            CostProvenance::ListPriceEstimate,
            true,
        ),
        CostProvenance::ListPriceEstimate
    );
}

#[test]
fn spend_contract_serializes_provenance_for_tauri_and_cli() {
    let contract = SpendContract {
        provider_id: "codex".to_string(),
        history_days: 30,
        reporting_period: "rolling:30".to_string(),
        known_cost_usd: Some(0.0),
        known_zero: false,
        provenance: CostProvenance::VendorMetered,
        price_coverage: CostCoverageCounts::default(),
        price_coverage_ratio: None,
        history_coverage_established: true,
        token_mix: SpendTokenMix::default(),
        token_total: None,
        conversation_count: 0,
        models: Vec::new(),
        projects: Vec::new(),
        conversations: Vec::new(),
        daily: Vec::new(),
        hourly_activity: Vec::new(),
        project_source_status: None,
        custom_pricing_active: false,
        imports: Vec::new(),
    };

    let json = serde_json::to_value(contract).expect("spend contract serializes");
    assert_eq!(json["provenance"], "vendorMetered");
}

#[test]
fn known_subtotal_sums_known_costs_only_and_needs_known_zero_for_empty() {
    let make_row = |cost: Option<f64>| SpendModelRow {
        model: String::new(),
        cost_usd: cost,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        total_tokens: 0,
        custom_pricing: false,
    };
    let mut summary = CostSummary::default();
    assert_eq!(known_subtotal(&[], &summary), None);
    summary.known_zero = true;
    assert_eq!(known_subtotal(&[], &summary), Some(0.0));
    summary.known_zero = false;
    let mixed = [make_row(Some(1.5)), make_row(None), make_row(Some(2.25))];
    assert_eq!(known_subtotal(&mixed, &summary), Some(3.75));
    let all_unknown = [make_row(None)];
    assert_eq!(known_subtotal(&all_unknown, &summary), None);
}

#[test]
fn coverage_for_models_counts_priced_rows_as_estimated() {
    let make_row = |cost: Option<f64>| SpendModelRow {
        model: String::new(),
        cost_usd: cost,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        total_tokens: 0,
        custom_pricing: false,
    };
    let coverage = coverage_for_models(&[make_row(Some(0.0)), make_row(None), make_row(Some(3.0))]);
    assert_eq!(coverage.estimated, 2);
    assert_eq!(coverage.unpriced, 1);
    assert_eq!(coverage.total(), 3);
}

#[test]
fn lower_bound_history_publishes_floors_and_never_an_exact_total() {
    let priced = LocalCostEstimate {
        known_subtotal_usd: Some(0.5),
        coverage: CostCoverageCounts {
            estimated: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let scanned = LocalTokenHistorySummary {
        total_tokens: 900,
        session_count: 2,
        coverage: LocalHistoryCoverage::Partial,
        cost_estimate: priced,
        ..Default::default()
    };
    let floor = scanned.with_lower_bound_if_partial();
    assert!(floor.lower_bound);
    assert_eq!(floor.published_tokens(), Some(900));
    assert_eq!(floor.total_usd(), None);

    let payload = local_token_history_json("antigravity", &floor, 30);
    assert_eq!(payload["tokens"]["total"], 900);
    assert_eq!(payload["sessions_count"], 2);
    assert_eq!(payload["tokensAreLowerBound"], true);
    assert_eq!(payload["costIsLowerBound"], true);
    assert!(payload["cost"]["total_usd"].is_null());
    assert_eq!(payload["cost"]["known_subtotal_usd"], 0.5);
}

#[test]
fn withheld_history_is_partial_with_no_published_numbers() {
    let withheld = LocalTokenHistorySummary::withheld();
    assert_eq!(withheld.coverage, LocalHistoryCoverage::Partial);
    assert!(!withheld.lower_bound);
    assert_eq!(withheld.published_tokens(), None);
    assert!(!withheld.clone().with_lower_bound_if_partial().lower_bound);

    let payload = local_token_history_json("antigravity", &withheld, 30);
    assert!(payload["tokens"]["total"].is_null());
    assert!(payload["sessions_count"].is_null());
    assert_eq!(payload["tokensAreLowerBound"], false);
    assert_eq!(payload["costIsLowerBound"], false);
    assert_eq!(payload["historyCoverage"], "partial");
}

#[test]
fn complete_history_is_never_marked_as_a_lower_bound() {
    let complete = LocalTokenHistorySummary {
        total_tokens: 10,
        session_count: 1,
        coverage: LocalHistoryCoverage::Complete,
        ..Default::default()
    }
    .with_lower_bound_if_partial();
    assert!(!complete.lower_bound);
    assert_eq!(complete.published_tokens(), Some(10));
}

#[test]
fn unpriced_model_names_are_recorded_but_not_serialized() {
    let mut estimate = LocalCostEstimate::default();
    estimate.record_list_price(Some("  mystery-model "), None);
    estimate.record_list_price(None, None);
    estimate.record_list_price(Some("known"), Some(1.0));
    assert_eq!(estimate.coverage.unpriced, 2);
    assert_eq!(estimate.coverage.estimated, 1);
    assert_eq!(
        estimate.unpriced_models.iter().collect::<Vec<_>>(),
        vec!["mystery-model"]
    );
    let json = serde_json::to_value(&estimate).unwrap();
    assert!(json.get("unpricedModels").is_none());
}

#[test]
fn period_contract_reports_its_period_and_sidecar_window() {
    let now = Utc::now();
    let cases = [
        (CostReportingPeriod::Rolling(7), "rolling:7", 7),
        (
            CostReportingPeriod::MonthToDate,
            "month-to-date",
            CostReportingPeriod::MonthToDate.sidecar_days(now),
        ),
        (CostReportingPeriod::AllAvailable, "all", MAX_ROLLING_DAYS),
    ];
    for (period, raw, sidecar_days) in cases {
        let contract = build_contract_from_period_summary(
            "unknown-provider",
            period,
            false,
            false,
            false,
            CostSummary::default(),
        );
        assert_eq!(contract.reporting_period, raw);
        assert_eq!(contract.history_days, sidecar_days);
        let json = serde_json::to_value(&contract).expect("contract serializes");
        assert_eq!(json["reportingPeriod"], raw);
    }
}

#[test]
fn legacy_summary_builder_keeps_a_rolling_period() {
    let contract = build_local_spend_contract_from_summary(
        "unknown-provider",
        90,
        false,
        false,
        false,
        CostSummary::default(),
    );
    assert_eq!(contract.reporting_period, "rolling:90");
    assert_eq!(contract.history_days, 90);
}

fn summary_with_model(model: &str, input: u64, output: u64, cached: u64) -> CostSummary {
    CostSummary {
        input_tokens: input,
        output_tokens: output,
        cached_tokens: cached,
        by_model: HashMap::from([(model.to_string(), 1.0)]),
        by_model_tokens: HashMap::from([(
            model.to_string(),
            ModelTokenCounts {
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                reasoning_tokens: None,
            },
        )]),
        ..CostSummary::default()
    }
}

fn imported_with_total(token_total: Option<u64>) -> ImportedSpendSource {
    ImportedSpendSource {
        source_id: "opencodex".to_string(),
        display_name: "OpenCodex".to_string(),
        request_count: 1,
        conversation_count: 1,
        known_cost_usd: None,
        provenance: CostProvenance::Unknown,
        // The OpenCodex fixture row: cache_read is inside input, total is 105.
        token_mix: SpendTokenMix {
            input_tokens: Some(100),
            output_tokens: Some(5),
            cache_read_tokens: Some(10),
            ..SpendTokenMix::default()
        },
        token_total,
        coverage: CostCoverageCounts::default(),
        models: Vec::new(),
        daily: Vec::new(),
        hourly_activity: Vec::new(),
    }
}

// Regression (PR #611 review): the native side follows the provider's cache
// rule, so the window total agrees with the native model totals.
#[test]
fn native_window_total_matches_model_totals_for_each_provider() {
    for (provider, expected) in [("codex", 1_050), ("claude", 1_950)] {
        let summary = summary_with_model("m", 1_000, 50, 900);
        let rows = model_rows(provider, &summary, &CustomPricing::default());
        let model_total: u64 = rows.iter().map(|row| row.total_tokens).sum();
        let window_total =
            resolve_token_total(summary.total_tokens_for_provider(provider), None, false);
        assert_eq!(model_total, expected, "{provider} model total");
        assert_eq!(window_total, Some(expected), "{provider} window total");
    }
}

// Regression (PR #611 review): the imported side uses the importer's resolved
// total (105), not a total re-derived from its token mix (115).
#[test]
fn imported_window_total_uses_resolved_import_total() {
    let imported = imported_with_total(Some(105));
    assert_eq!(
        resolve_token_total(1_050, Some(&imported), false),
        Some(1_155)
    );
    assert_eq!(
        resolve_token_total(1_050, Some(&imported), true),
        Some(105),
        "replace_native drops the native side entirely"
    );
    assert_eq!(resolve_token_total(1_050, None, false), Some(1_050));
    assert_eq!(
        resolve_token_total(1_050, Some(&imported_with_total(None)), true),
        None,
        "an import without token data stays unknown"
    );
}
