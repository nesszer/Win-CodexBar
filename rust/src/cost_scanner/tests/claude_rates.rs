//! Claude pricing as the scanner applies it.

use super::*;

/// Scanner Claude pricing without the per-scan memo, as an oracle.
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
        #[allow(
            clippy::cast_possible_truncation,
            reason = "clamped to i32::MAX before casting"
        )]
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
fn records_unknown_claude_model_while_using_fallback_cost() {
    let event: ClaudeEvent = serde_json::from_str(
        r#"{"type":"assistant","timestamp":"2026-01-15T10:00:00Z","requestId":"req_unknown","message":{"id":"msg_unknown","model":"claude-retired-unknown","usage":{"input_tokens":100000,"output_tokens":100000}}}"#,
    )
    .unwrap();
    let record = claude_usage_record_from_event(&event).expect("usage record");
    let mut summary = CostSummary::default();

    add_claude_record_to_summary(&mut summary, &record);

    assert!(summary.total_cost_usd > 0.0);
    assert!(summary.unknown_models.contains("claude-retired-unknown"));
}

#[test]
fn claude_scan_pricing_resolver_reuses_positive_and_negative_resolution() {
    let unknown = format!("claude-scan-unknown-{}", std::process::id());
    let mut resolver = ClaudeScanPricingResolver::default();

    assert!(resolver.is_known("claude-sonnet-4-6"));
    assert!(!resolver.is_known(&unknown));
    assert!(resolver.is_known("claude-sonnet-4-6"));
    assert!(!resolver.is_known(&unknown));
    assert_eq!(resolver.normalization_cache_misses, 2);
    assert_eq!(resolver.resolution_cache_misses, 2);
    assert_eq!(resolver.resolutions.len(), 2);

    let mut cost_resolver = ClaudeScanPricingResolver::default();
    let resolved_unknown = cost_resolver.cost_usd_with_cache_ttl(&unknown, 100, 20, 10, 30, 40);
    let fallback =
        ClaudePricing::cost_usd_with_cache_ttl(FALLBACK_CLAUDE_MODEL, 100, 20, 10, 30, 40);
    assert!((resolved_unknown - fallback).abs() < f64::EPSILON);
}

#[test]
fn claude_scan_pricing_resolver_preserves_tiered_and_cache_ttl_pricing() {
    let mut resolver = ClaudeScanPricingResolver::default();
    let cases = [
        ("claude-sonnet-4-6", 240_000, 0, 0, 0, 0),
        ("claude-fable-5", 100, 30, 20, 20, 5),
    ];

    for (model, input, cache_create, cache_create_1h, cache_read, output) in cases {
        let actual = resolver.cost_usd_with_cache_ttl(
            model,
            input,
            cache_create,
            cache_create_1h,
            cache_read,
            output,
        );
        let expected = ClaudePricing::cost_usd_with_cache_ttl(
            model,
            input,
            cache_create,
            cache_create_1h,
            cache_read,
            output,
        );
        assert!((actual - expected).abs() < f64::EPSILON, "{model}");
    }
}

#[test]
fn claude_scan_resolver_applies_gpt_proxy_long_context_boundary() {
    let snapshot = crate::core::ModelsDevPricingSnapshot::from_catalog_json_for_tests(
        r#"{
            "openai": {"models": {"gpt-5.6-sol": {"id": "gpt-5.6-sol", "cost": {
                "input": 2, "output": 4, "cache_read": 0.25, "cache_write": 3,
                "context_over_200k": {"input": 7, "output": 11, "cache_read": 0.5, "cache_write": 9}
            }}}}
        }"#,
    )
    .expect("pricing fixture");
    let mut resolver = ClaudeScanPricingResolver::with_snapshot(snapshot);

    let short = resolver.cost_usd_with_cache_ttl("gpt-5.6-sol", 262_000, 0, 0, 10_000, 13);
    let long = resolver.cost_usd_with_cache_ttl("gpt-5.6-sol", 262_001, 0, 0, 10_000, 13);

    assert!((short - 0.526552).abs() < 1e-12);
    assert!((long - 1.83915).abs() < 1e-12);
}

#[test]
fn claude_scan_pricing_resolver_bounds_normalization_memo() {
    let mut resolver = ClaudeScanPricingResolver::default();
    for index in 0..(ClaudeScanPricingResolver::MEMO_ENTRY_LIMIT + 8) {
        let model = format!("claude-memo-{index}");
        assert_eq!(resolver.normalize(&model), model);
    }
    assert_eq!(
        resolver.normalized_models.len(),
        ClaudeScanPricingResolver::MEMO_ENTRY_LIMIT
    );

    let misses = resolver.normalization_cache_misses;
    assert_eq!(resolver.normalize("claude-memo-0"), "claude-memo-0");
    assert_eq!(resolver.normalization_cache_misses, misses);
    assert_eq!(resolver.normalize("claude-memo-1024"), "claude-memo-1024");
    assert_eq!(resolver.normalization_cache_misses, misses + 1);
}

#[test]
fn claude_fable_5_prices_five_minute_and_one_hour_cache_writes() {
    // (input, cache write, 1h share of it, cache read, output, expected USD).
    for (input, cache_create, cache_create_1h, cache_read, output, expected) in [
        (
            100,
            10,
            0,
            20,
            5,
            (100.0 / 1_000_000.0) * 10.00
                + (10.0 / 1_000_000.0) * 12.50
                + (20.0 / 1_000_000.0) * 1.00
                + (5.0 / 1_000_000.0) * 50.00,
        ),
        (
            100,
            30,
            20,
            20,
            5,
            (100.0 / 1_000_000.0) * 10.00
                + (10.0 / 1_000_000.0) * 12.50
                + (20.0 / 1_000_000.0) * 20.00
                + (20.0 / 1_000_000.0) * 1.00
                + (5.0 / 1_000_000.0) * 50.00,
        ),
    ] {
        let cost = ClaudePricing::cost_usd_with_cache_ttl(
            "claude-fable-5",
            input,
            cache_create,
            cache_create_1h,
            cache_read,
            output,
        );
        assert!((cost - expected).abs() < f64::EPSILON, "{cache_create_1h}");
    }
}

#[test]
fn claude_scan_pricing_follows_the_canonical_table() {
    // Sonnet 4.6 honors the 200k tier: 200k @ $3/M + 40k @ $6/M = 0.84 (the
    // scanner's old inline table applied a flat $3/M = 0.72). Opus 4.5-4.8
    // bill $5 in + $25 out; opus-4-8 must resolve through the canonical table.
    // Legacy Opus 4.0/4.1 stay at $15 + $75 (retired IDs absent from the
    // table, e.g. `claude-3-opus-...`, fall back to Sonnet). Haiku 4.5 bills
    // $1 + $5, not the Haiku 3 rate.
    for (model, input, output, expected) in [
        ("claude-sonnet-4-6", 240_000, 0, 0.84),
        ("claude-opus-4-5", 1_000_000, 1_000_000, 30.00),
        ("claude-opus-4-6", 1_000_000, 1_000_000, 30.00),
        ("claude-opus-4-7", 1_000_000, 1_000_000, 30.00),
        ("claude-opus-4-8", 1_000_000, 1_000_000, 30.00),
        ("claude-opus-4-20250514", 1_000_000, 1_000_000, 90.00),
        ("claude-opus-4-1", 1_000_000, 1_000_000, 90.00),
        ("claude-haiku-4-5", 1_000_000, 1_000_000, 6.00),
    ] {
        let cost = ClaudePricing::cost_usd_with_cache_ttl(model, input, 0, 0, 0, output);
        assert!(
            (cost - expected).abs() < 0.001,
            "{model} should bill ${expected}, got {cost}"
        );
    }
}
