use super::codex_routed_pricing;
use super::*;

impl CostUsagePricing {
    /// Get the display label for a Codex model (e.g. "Research Preview")
    fn codex_display_label(model: &str) -> Option<&'static str> {
        let key = Self::normalize_codex_model(model);
        CODEX_PRICING
            .get(key.as_str())
            .and_then(|p| p.display_label)
    }

    /// Format model name for display (e.g., "claude-3.5-sonnet" → "Sonnet 3.5")
    fn format_model_name(model: &str) -> String {
        let lower = model.to_lowercase();

        // GPT models: format as "GPT-{version}[ Mini| Nano]"
        if lower.contains("gpt-") {
            let version = regex_lite::Regex::new(r"gpt-(\d+(?:\.\d+)?)")
                .ok()
                .and_then(|re| re.captures(&lower))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

            let suffix = if lower.contains("nano") {
                " Nano"
            } else if lower.contains("mini") {
                " Mini"
            } else {
                ""
            };

            return match version {
                Some(v) => format!("GPT-{}{}", v, suffix),
                None => model.to_string(),
            };
        }

        // Claude models: extract version and family
        let version = regex_lite::Regex::new(r"(\d+(?:\.\d+)?)")
            .ok()
            .and_then(|re| re.find(&lower))
            .map(|m| m.as_str().to_string());

        let family = if lower.contains("opus") {
            "Opus"
        } else if lower.contains("sonnet") {
            "Sonnet"
        } else if lower.contains("haiku") {
            "Haiku"
        } else {
            return model.to_string();
        };

        match version {
            Some(v) => format!("{} {}", family, v),
            None => family.to_string(),
        }
    }

    fn codex_cost_usd_with_pricing_snapshot(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
        pricing_snapshot: Option<&models_dev_pricing::ModelsDevPricingSnapshot>,
    ) -> Option<f64> {
        Self::codex_cost_usd_with_cache_write_and_pricing_snapshot(
            model,
            input_tokens,
            cached_input_tokens,
            0,
            output_tokens,
            pricing_snapshot,
        )
    }
}

#[test]
fn test_normalize_codex_model() {
    assert_eq!(CostUsagePricing::normalize_codex_model("gpt-5"), "gpt-5");
    assert_eq!(
        CostUsagePricing::normalize_codex_model("openai/gpt-5"),
        "gpt-5"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("gpt-5-codex"),
        "gpt-5"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model(""),
        CostUsagePricing::CODEX_UNATTRIBUTED_MODEL
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("unknown"),
        CostUsagePricing::CODEX_UNATTRIBUTED_MODEL
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("gpt-reserve"),
        "gpt-5.6-luna"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model(" GPT-RESERVE "),
        "gpt-5.6-luna"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("openai/gpt-reserve"),
        "gpt-5.6-luna"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("OPENAI/GPT-RESERVE"),
        "gpt-5.6-luna"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("gpt-reserve-preview"),
        "gpt-reserve-preview"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("my-gpt-reserve"),
        "my-gpt-reserve"
    );
}

#[test]
fn unattributed_codex_usage_stays_unpriced() {
    assert!(
        CostUsagePricing::codex_cost_usd(CostUsagePricing::CODEX_UNATTRIBUTED_MODEL, 1_000, 0, 500)
            .is_none()
    );
    assert!(CostUsagePricing::is_codex_unattributed_model("unknown"));
    assert!(CostUsagePricing::is_codex_unattributed_model("  "));
}

#[test]
fn test_normalize_claude_model() {
    assert_eq!(
        CostUsagePricing::normalize_claude_model("claude-sonnet-4-5"),
        "claude-sonnet-4-5"
    );
    assert_eq!(
        CostUsagePricing::normalize_claude_model("anthropic.claude-sonnet-4-5"),
        "claude-sonnet-4-5"
    );
}

#[test]
fn test_codex_cost() {
    let cost = CostUsagePricing::codex_cost_usd("gpt-5", 1000, 0, 500).unwrap();
    assert!((cost - 0.00625).abs() < 1e-10);
}

#[test]
fn test_claude_cost() {
    assert!(
        CostUsagePricing::claude_cost_usd("claude-haiku-4-5-20251001", 1000, 0, 0, 500).is_some()
    );
}

#[test]
fn test_opus_4_8_cost() {
    let cost = CostUsagePricing::claude_cost_usd("claude-opus-4-8", 1_000, 0, 0, 500).unwrap();
    assert!((cost - 0.0175).abs() < 1e-10);
}

#[test]
fn test_fable_5_cost() {
    let cost = CostUsagePricing::claude_cost_usd("claude-fable-5", 1_000, 0, 0, 500).unwrap();
    assert!((cost - 0.035).abs() < 1e-10);
}

#[test]
fn test_claude_input_cost_per_token() {
    assert_eq!(
        CostUsagePricing::claude_input_cost_per_token("claude-opus-4-8"),
        Some(5e-6)
    );
    assert_eq!(
        CostUsagePricing::claude_input_cost_per_token("claude-fable-5"),
        Some(1e-5)
    );
    assert_eq!(
        CostUsagePricing::claude_input_cost_per_token("totally-unknown-model"),
        None
    );
}

#[test]
fn test_format_model_name() {
    assert_eq!(
        CostUsagePricing::format_model_name("claude-3.5-sonnet"),
        "Sonnet 3.5"
    );
    assert_eq!(
        CostUsagePricing::format_model_name("claude-opus-4"),
        "Opus 4"
    );
    assert_eq!(CostUsagePricing::format_model_name("gpt-5"), "GPT-5");
}

#[test]
fn test_gpt54_mini_cost() {
    let cost = CostUsagePricing::codex_cost_usd("gpt-5.4-mini", 1000, 0, 500).unwrap();
    assert!((cost - 0.003).abs() < 1e-10);
}

#[test]
fn test_gpt54_nano_cost() {
    let cost = CostUsagePricing::codex_cost_usd("gpt-5.4-nano", 1000, 0, 500).unwrap();
    assert!((cost - 0.000825).abs() < 1e-10);
}

#[test]
fn test_normalize_gpt54_codex() {
    assert_eq!(
        CostUsagePricing::normalize_codex_model("gpt-5.4-mini-codex"),
        "gpt-5.4-mini"
    );
}

#[test]
fn test_gpt55_pricing() {
    assert_eq!(
        CostUsagePricing::normalize_codex_model("openai/gpt-5.5-2026-04-23"),
        "gpt-5.5"
    );
    assert_eq!(
        CostUsagePricing::normalize_codex_model("gpt-5.5-pro-2026-04-23"),
        "gpt-5.5-pro"
    );
    let cost = CostUsagePricing::codex_cost_usd("gpt-5.5", 1000, 500, 500).unwrap();
    assert!((cost - 0.01775).abs() < 1e-10);
}

#[test]
fn test_format_gpt54_mini() {
    assert_eq!(
        CostUsagePricing::format_model_name("gpt-5.4-mini"),
        "GPT-5.4 Mini"
    );
}

#[test]
fn test_opus_4_7_cost() {
    assert!(CostUsagePricing::claude_cost_usd("claude-opus-4-7", 1000, 0, 0, 500).is_some());
}

#[test]
fn test_sonnet_4_6_cost() {
    assert!(CostUsagePricing::claude_cost_usd("claude-sonnet-4-6", 1000, 0, 0, 500).is_some());
}

#[test]
fn test_gpt5_pro_cost() {
    let cost = CostUsagePricing::codex_cost_usd("gpt-5-pro", 1000, 0, 500).unwrap();
    assert!((cost - 0.075).abs() < 1e-10);
}

#[test]
fn test_gpt56_standard_pricing() {
    for (model, expected) in [
        ("gpt-5.6-sol", 0.02256),
        ("gpt-5.6-terra", 0.01328),
        ("gpt-5.6-luna", 0.001328),
    ] {
        let cost = CostUsagePricing::codex_cost_usd(model, 1_000, 400, 1_000);
        assert!((cost.unwrap() - expected).abs() < 1e-10, "{model}");
    }
}

#[test]
fn gpt_reserve_alias_uses_luna_pricing() {
    let luna =
        CostUsagePricing::codex_cost_usd("gpt-5.6-luna", 1_000, 400, 1_000).expect("Luna pricing");
    for model in [
        "gpt-reserve",
        " GPT-RESERVE ",
        "openai/gpt-reserve",
        "OPENAI/GPT-RESERVE",
    ] {
        let reserve =
            CostUsagePricing::codex_cost_usd(model, 1_000, 400, 1_000).expect("reserve pricing");
        assert!((reserve - luna).abs() < 1e-10, "{model}");
    }
}

#[test]
fn test_gpt56_long_context_pricing() {
    for (model, expected) in [
        ("gpt-5.6-sol", 30.2176008),
        ("gpt-5.6-terra", 18.1088004),
        ("gpt-5.6-luna", 1.81088004),
    ] {
        let cost = CostUsagePricing::codex_cost_usd(model, 272_001, 272_001, 1_000_000);
        assert!((cost.unwrap() - expected).abs() < 1e-10, "{model}");
    }
}

#[test]
fn test_gpt56_context_threshold_is_exclusive() {
    for (model, expected) in [
        ("gpt-5.6-sol", 0.1088),
        ("gpt-5.6-terra", 0.0544),
        ("gpt-5.6-luna", 0.00544),
    ] {
        let cost = CostUsagePricing::codex_cost_usd(model, 272_000, 272_000, 0);
        assert!((cost.unwrap() - expected).abs() < 1e-10, "{model}");
    }
}

#[test]
fn test_normalize_gpt56_aliases() {
    for model in [
        "gpt-5.6",
        "openai/gpt-5.6",
        "gpt-5.6-codex",
        "gpt-5.6-2099-01-01",
        "openai/gpt-5.6-codex-2099-01-01",
    ] {
        assert_eq!(
            CostUsagePricing::normalize_codex_model(model),
            "gpt-5.6-sol",
            "{model}"
        );
    }
}

#[test]
fn test_codex_display_label() {
    assert_eq!(
        CostUsagePricing::codex_display_label("gpt-5.3-codex-spark"),
        Some("Research Preview")
    );
    assert_eq!(CostUsagePricing::codex_display_label("gpt-5.4"), None);
}

#[test]
fn test_codex_fast_multiplier() {
    assert_eq!(
        CostUsagePricing::codex_api_fast_multiplier("gpt-5.6-sol"),
        Some(2.0)
    );
    assert_eq!(
        CostUsagePricing::codex_api_fast_multiplier("gpt-5.6-terra"),
        Some(2.0)
    );
    assert_eq!(
        CostUsagePricing::codex_api_fast_multiplier("gpt-5.6-luna"),
        Some(2.0)
    );
    assert_eq!(
        CostUsagePricing::codex_api_fast_multiplier("gpt-5.4"),
        Some(2.0)
    );
    assert_eq!(
        CostUsagePricing::codex_api_fast_multiplier("gpt-5.5"),
        Some(2.5)
    );
    assert_eq!(
        CostUsagePricing::codex_api_fast_multiplier("gpt-5.6-sol-fast"),
        Some(2.0)
    );
    assert_eq!(CostUsagePricing::codex_api_fast_multiplier("unknown"), None);
}

#[test]
fn test_codex_fast_cost_is_double_standard() {
    let standard = CostUsagePricing::codex_cost_usd("gpt-5.6-sol", 1000, 0, 500).unwrap();
    let fast = CostUsagePricing::codex_fast_cost_usd("gpt-5.6-sol", 1000, 0, 500).unwrap();
    assert!((fast - standard * 2.0).abs() < 1e-10);
}

#[test]
fn test_codex_fast_cost_none_above_long_context_threshold() {
    // Input above 272_000 → None (Fast not offered)
    assert_eq!(
        CostUsagePricing::codex_fast_cost_usd("gpt-5.6-sol", 272_001, 0, 100),
        None
    );
}

#[test]
fn test_codex_fast_cost_usd_suffixed_models_resolve_to_base() {
    // gpt-5.5-fast → base gpt-5.5 → 2.5x multiplier
    let base = CostUsagePricing::codex_cost_usd("gpt-5.5", 1000, 0, 500).unwrap();
    let fast = CostUsagePricing::codex_fast_cost_usd("gpt-5.5-fast", 1000, 0, 500).unwrap();
    assert!(
        (fast - base * 2.5).abs() < 1e-10,
        "gpt-5.5-fast should be gpt-5.5 × 2.5"
    );

    // gpt-5.6-sol-priority → base gpt-5.6-sol → 2.0x multiplier
    let sol_base = CostUsagePricing::codex_cost_usd("gpt-5.6-sol", 1000, 400, 1000).unwrap();
    let sol_fast =
        CostUsagePricing::codex_fast_cost_usd("gpt-5.6-sol-priority", 1000, 400, 1000).unwrap();
    assert!(
        (sol_fast - sol_base * 2.0).abs() < 1e-10,
        "gpt-5.6-sol-priority should be gpt-5.6-sol × 2.0"
    );
}

#[test]
fn test_codex_fast_cost_usd_base_model_unsuffixed() {
    // Unsuffixed base models still resolve to themselves.
    assert_eq!(
        CostUsagePricing::codex_fast_base_model("gpt-5.6-terra"),
        "gpt-5.6-terra"
    );
    assert_eq!(
        CostUsagePricing::codex_fast_base_model("gpt-5.5"),
        "gpt-5.5"
    );
    // Unknown models return normalized original.
    assert_eq!(
        CostUsagePricing::codex_fast_base_model("my-custom-model"),
        "my-custom-model"
    );
}

// ── Upstream 0.50.1 #2946: provider-qualified routed model pricing ──────────

#[test]
fn codex_routed_provider_detects_known_routes() {
    assert_eq!(
        codex_routed_pricing::codex_routed_provider("deepseek/deepseek-chat"),
        Some("deepseek")
    );
    assert_eq!(
        codex_routed_pricing::codex_routed_provider("kimi/kimi-k2"),
        Some("kimi")
    );
    assert_eq!(
        codex_routed_pricing::codex_routed_provider("opencode/gpt-5"),
        Some("opencode")
    );
    // Case-insensitive prefix.
    assert_eq!(
        codex_routed_pricing::codex_routed_provider("DeepSeek/deepseek-chat"),
        Some("deepseek")
    );
}

#[test]
fn codex_routed_provider_returns_none_for_unknown_and_unrouted() {
    assert!(codex_routed_pricing::codex_routed_provider("acme/model-x").is_none());
    assert!(codex_routed_pricing::codex_routed_provider("gpt-5").is_none());
    assert!(codex_routed_pricing::codex_routed_provider("deepseek-chat").is_none());
    assert!(codex_routed_pricing::codex_routed_provider("openai/gpt-5").is_none());
}

#[test]
fn native_codex_nous_prefix_stays_unpriced() {
    // Upstream `codexModelsDevProviderIDs` has no `nous`: only OpenCodex
    // ledger rows reach the Nous catalog.
    let snapshot = crate::core::ModelsDevPricingSnapshot::from_catalog_json_for_tests(
        r#"{"nous":{"models":{"z-ai/glm-5":{"id":"z-ai/glm-5","cost":{"input":2,"output":8}}}}}"#,
    )
    .expect("catalog");
    assert!(codex_routed_pricing::codex_routed_provider("nous/z-ai/glm-5").is_none());
    assert!(
        CostUsagePricing::codex_cost_usd_with_pricing_snapshot(
            "nous/z-ai/glm-5",
            1_000,
            0,
            500,
            Some(&snapshot)
        )
        .is_none()
    );
}

#[test]
fn models_dev_rates_follow_upstream_codex_cost_semantics() {
    let snapshot = crate::core::ModelsDevPricingSnapshot::from_catalog_json_for_tests(
        r#"{"deepseek":{"models":{
            "full":{"id":"full","cost":{"input":2,"output":8,"cache_read":0.5,"cache_write":3,
                "context_over_200k":{"input":4,"output":16,"cache_read":1,"cache_write":6}}},
            "bare":{"id":"bare","cost":{"input":2,"output":8,
                "context_over_200k":{"input":4,"output":16}}}}}}"#,
    )
    .expect("catalog");
    let full = snapshot.lookup_exact("deepseek", "full").expect("full");
    let bare = snapshot.lookup_exact("deepseek", "bare").expect("bare");
    let close = |actual: f64, expected: f64| {
        assert!((actual - expected).abs() < 1e-12, "{actual} vs {expected}");
    };

    // Short context: each cache lane uses its own catalog rate.
    close(
        CostUsagePricing::models_dev_cost_usd(&full, 1_000, 200, 300, 100),
        500.0 * 2e-6 + 200.0 * 0.5e-6 + 300.0 * 3e-6 + 100.0 * 8e-6,
    );
    // Inclusive input above 200k selects the long-context rate of every lane.
    close(
        CostUsagePricing::models_dev_cost_usd(&full, 200_001, 100_000, 50_000, 1_000),
        50_001.0 * 4e-6 + 100_000.0 * 1e-6 + 50_000.0 * 6e-6 + 1_000.0 * 16e-6,
    );
    // A lane without a catalog rate falls back to the tier's input rate.
    close(
        CostUsagePricing::models_dev_cost_usd(&bare, 1_000, 200, 300, 100),
        1_000.0 * 2e-6 + 100.0 * 8e-6,
    );
    close(
        CostUsagePricing::models_dev_cost_usd(&bare, 200_001, 100_000, 50_000, 1_000),
        200_001.0 * 4e-6 + 1_000.0 * 16e-6,
    );
    // Routed Codex rows share these rates.
    close(
        CostUsagePricing::codex_cost_usd_with_pricing_snapshot(
            "deepseek/bare",
            200_001,
            100_000,
            1_000,
            Some(&snapshot),
        )
        .expect("routed"),
        200_001.0 * 4e-6 + 1_000.0 * 16e-6,
    );
}

#[test]
fn codex_routed_model_with_unknown_prefix_stays_unpriced() {
    // An unknown provider/ prefix must NOT fall back to the OpenAI catalog
    // (upstream 0.50.1 #2946: unknown prefixes are left unpriced, not guessed).
    assert!(CostUsagePricing::codex_cost_usd("acme/secret-model", 1_000, 0, 500).is_none());
}

#[test]
fn codex_routed_model_strips_prefix_for_lookup() {
    // A known route prefix produces a clean model id for models.dev lookup.
    // A nonexistent sub-model returns None (cleanly unpriced) rather than
    // falling back to the OpenAI catalog.
    assert!(
        CostUsagePricing::codex_cost_usd("deepseek/nonexistent-model-xyz", 1_000, 0, 500).is_none()
    );
    assert!(
        CostUsagePricing::codex_cost_usd("kimi/nonexistent-model-xyz", 1_000, 0, 500).is_none()
    );
}

// ── Upstream 0.53: Claude first-party models.dev routing ───────────────

#[test]
fn claude_bare_models_route_to_first_party_vendors() {
    assert_eq!(
        CostUsagePricing::claude_models_dev_target("gpt-5")
            .unwrap()
            .0,
        "openai"
    );
    assert_eq!(
        CostUsagePricing::claude_models_dev_target("gemini-2.5-pro")
            .unwrap()
            .0,
        "google"
    );
    assert_eq!(
        CostUsagePricing::claude_models_dev_target("deepseek-chat")
            .unwrap()
            .0,
        "deepseek"
    );
    assert_eq!(
        CostUsagePricing::claude_models_dev_target("claude-sonnet-4-6")
            .unwrap()
            .0,
        "anthropic"
    );
}

#[test]
fn claude_explicit_unknown_vendor_fails_closed() {
    assert!(CostUsagePricing::claude_models_dev_target("acme/secret-model").is_none());
    assert_eq!(
        CostUsagePricing::claude_models_dev_target("openai/gpt-5").unwrap(),
        ("openai", "gpt-5".to_string())
    );
}

#[test]
fn gpt56_historical_terra_luna_rates_change_at_2026_07_30() {
    use chrono::NaiveDate;

    let before = NaiveDate::from_ymd_opt(2026, 7, 29).unwrap();
    let after = NaiveDate::from_ymd_opt(2026, 7, 30).unwrap();

    let terra_before =
        CostUsagePricing::codex_cost_usd_at_date("gpt-5.6-terra", 100, 10, 5, before).unwrap();
    let terra_after =
        CostUsagePricing::codex_cost_usd_at_date("gpt-5.6-terra", 100, 10, 5, after).unwrap();
    let luna_before =
        CostUsagePricing::codex_cost_usd_at_date("gpt-5.6-luna", 100, 10, 5, before).unwrap();
    let luna_after =
        CostUsagePricing::codex_cost_usd_at_date("gpt-5.6-luna", 100, 10, 5, after).unwrap();

    let terra_before_expected = 90.0 * 2.5e-6 + 10.0 * 2.5e-7 + 5.0 * 1.5e-5;
    let terra_after_expected = 90.0 * 2e-6 + 10.0 * 2e-7 + 5.0 * 1.2e-5;
    let luna_before_expected = 90.0 * 1e-6 + 10.0 * 1e-7 + 5.0 * 6e-6;
    let luna_after_expected = 90.0 * 2e-7 + 10.0 * 2e-8 + 5.0 * 1.2e-6;

    assert!((terra_before - terra_before_expected).abs() < 1e-12);
    assert!((terra_after - terra_after_expected).abs() < 1e-12);
    assert!((luna_before - luna_before_expected).abs() < 1e-12);
    assert!((luna_after - luna_after_expected).abs() < 1e-12);
    assert!(terra_before > terra_after);
    assert!(luna_before > luna_after);
}

#[test]
fn gpt56_historical_sol_rates_change_at_2026_08_21() {
    use chrono::NaiveDate;

    // Sol keeps its own cutoff: still historical on Terra/Luna's cut day.
    let historical = 90.0 * 5e-6 + 10.0 * 5e-7 + 5.0 * 3e-5;
    let current = 90.0 * 4e-6 + 10.0 * 4e-7 + 5.0 * 2e-5;
    for (date, expected) in [
        ((2026, 7, 30), historical),
        ((2026, 8, 20), historical),
        ((2026, 8, 21), current),
    ] {
        let day = NaiveDate::from_ymd_opt(date.0, date.1, date.2).unwrap();
        let cost = CostUsagePricing::codex_cost_usd_at_date("gpt-5.6-sol", 100, 10, 5, day);
        assert!((cost.unwrap() - expected).abs() < 1e-12, "{day}");
    }
    let undated = CostUsagePricing::codex_cost_usd("gpt-5.6-sol", 100, 10, 5).unwrap();
    assert!((undated - current).abs() < 1e-12);
}

#[test]
fn gpt56_historical_long_context_uses_pre_cut_rates() {
    use chrono::NaiveDate;

    let before = NaiveDate::from_ymd_opt(2026, 7, 29).unwrap();
    let terra =
        CostUsagePricing::codex_cost_usd_at_date("gpt-5.6-terra", 300_000, 30_000, 1_000, before)
            .unwrap();
    let expected = 270_000.0 * 5e-6 + 30_000.0 * 5e-7 + 1_000.0 * 2.25e-5;
    assert!((terra - expected).abs() < 1e-10);
}

#[test]
fn gpt6_astra_aliases_use_standard_rates_and_preserve_cached_semantics() {
    for model in [
        "gpt-6-astra",
        "openai/gpt-6-astra",
        "gpt-6-astra-2099-01-01",
    ] {
        let cost = CostUsagePricing::codex_cost_usd(model, 1000, 300, 100).unwrap();
        // This API receives cache-read tokens only. The remaining 700 input
        // tokens are standard input; explicit cache-write tokens use the
        // 1.25x Astra rate in the adjacent cache-write regression.
        let expected = 700.0 * 10e-6 + 300.0 * 1e-6 + 100.0 * 50e-6;
        assert!(
            (cost - expected).abs() < 1e-12,
            "{model}: expected {expected}, got {cost}"
        );
    }
}

#[test]
fn gpt6_astra_short_context_prices_cache_writes_at_125_percent() {
    let cost =
        CostUsagePricing::codex_cost_usd_with_cache_write("gpt-6-astra", 1_000, 200, 300, 100)
            .unwrap();
    let expected = 500.0 * 1e-5 + 200.0 * 1e-6 + 300.0 * 1.25e-5 + 100.0 * 5e-5;
    assert!((cost - expected).abs() < 1e-12);
}

#[test]
fn gpt6_astra_long_context_prices_cache_writes_at_125_percent() {
    let cost = CostUsagePricing::codex_cost_usd_with_cache_write(
        "gpt-6-astra",
        272_001,
        100_000,
        50_000,
        1_000,
    )
    .unwrap();
    let expected = 122_001.0 * 2e-5 + 100_000.0 * 2e-6 + 50_000.0 * 2.5e-5 + 1_000.0 * 7.5e-5;
    assert!((cost - expected).abs() < 1e-12);
}

#[test]
fn codex_cache_writes_preserve_non_astra_pricing() {
    let without_cache_write = CostUsagePricing::codex_cost_usd("gpt-5", 1_000, 200, 100).unwrap();
    let with_cache_write =
        CostUsagePricing::codex_cost_usd_with_cache_write("gpt-5", 1_000, 200, 300, 100).unwrap();
    assert!((with_cache_write - without_cache_write).abs() < 1e-12);
}

#[test]
fn gpt6_astra_switches_the_whole_request_at_long_context_boundary() {
    let standard = CostUsagePricing::codex_cost_usd("gpt-6-astra", 272_000, 100_000, 1000).unwrap();
    let long = CostUsagePricing::codex_cost_usd("gpt-6-astra", 272_001, 100_000, 1000).unwrap();
    let expected_standard = 172_000.0 * 10e-6 + 100_000.0 * 1e-6 + 1000.0 * 50e-6;
    let expected_long = 172_001.0 * 20e-6 + 100_000.0 * 2e-6 + 1000.0 * 75e-6;
    assert!((standard - expected_standard).abs() < 1e-12);
    assert!((long - expected_long).abs() < 1e-12);

    let fast = CostUsagePricing::codex_fast_cost_usd("openai/gpt-6-astra", 272_001, 100_000, 1000)
        .unwrap();
    assert!((fast - expected_long * 2.0).abs() < 1e-12);
}

#[test]
fn gpt6_astra_unknown_models_fail_closed() {
    for model in ["gpt-6", "other-provider/gpt-6-astra"] {
        assert!(CostUsagePricing::codex_cost_usd(model, 1000, 0, 100).is_none());
    }
}

// Upstream 0.65.0 #3820: Priority rows and day aggregates.
// Upstream 0.70.0 #4094 `CodexAliasedModelPricingTests`.
#[test]
fn normalize_codex_model_maps_daybreak_aliases_and_cyber_ids() {
    for (raw, expected) in [
        ("gpt-daybreak-blue-latest", "gpt-5.6-sol"),
        ("openai/gpt-daybreak-blue-latest", "gpt-5.6-sol"),
        ("gpt-daybreak-red-latest", "gpt-5.6-cyber"),
        ("gpt-5.6-cyber", "gpt-5.6-cyber"),
        ("gpt-5.5-cyber", "gpt-5.5-cyber"),
    ] {
        assert_eq!(
            CostUsagePricing::normalize_codex_model(raw),
            expected,
            "{raw}"
        );
    }
}

#[test]
fn codex_cost_prices_daybreak_aliases_and_cyber_bundled_fallback() {
    let cost = |model: &str, writes: u64| {
        CostUsagePricing::codex_cost_usd_with_cache_write(model, 100, 10, writes, 5).unwrap()
    };
    // Cyber rates per token: $12.50 input, $1.25 cached input, $75 output per 1M.
    let cyber = 90.0 * 1.25e-5 + 10.0 * 1.25e-6 + 5.0 * 7.5e-5;
    assert!((cost("gpt-5.6-cyber", 0) - cyber).abs() < 1e-12);
    assert!((cost("gpt-5.5-cyber", 0) - cyber).abs() < 1e-12);
    let expected_write = 70.0 * 1.25e-5 + 10.0 * 1.25e-6 + 20.0 * 1.5625e-5 + 5.0 * 7.5e-5;
    assert!((cost("gpt-5.6-cyber", 20) - expected_write).abs() < 1e-12);
    // gpt-5.5-cyber lists no cache-write rate, so its writes bill as input.
    assert!((cost("gpt-5.5-cyber", 20) - cyber).abs() < 1e-12);
    assert!((cost("gpt-daybreak-blue-latest", 0) - cost("gpt-5.6-sol", 0)).abs() < 1e-12);
    assert!((cost("gpt-daybreak-red-latest", 0) - cyber).abs() < 1e-12);
}

// Upstream 0.70.0 #4094 `CodexSolHistoricalPricingTests`. Windows prices by
// usage day, so 2026-08-20 stands in for upstream's `cutoff - 1s`.
#[test]
fn sol_keeps_historical_rates_before_its_august_repricing() {
    use chrono::NaiveDate;
    use models_dev_pricing::ModelsDevPricingSnapshot;

    let catalog = ModelsDevPricingSnapshot::from_catalog_json_for_tests(
        r#"{"openai":{"id":"openai","models":{"gpt-5.6-sol":{
          "id":"gpt-5.6-sol","cost":{"input":4,"cache_read":0.4,"cache_write":5,"output":20}
        }}}}"#,
    )
    .expect("catalog fixture");
    let empty = ModelsDevPricingSnapshot::from_catalog_json_for_tests("{}").expect("empty");
    let last_historical_day = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
    let cutoff = NaiveDate::from_ymd_opt(2026, 8, 21).unwrap();
    for model in ["gpt-5.6-sol", "gpt-5.6"] {
        for input in [100_u64, 272_001] {
            let long = input > 272_000;
            for (day, historical) in [(last_historical_day, true), (cutoff, false)] {
                // Per 1M tokens; cache reads bill at 0.1x and writes at 1.25x input.
                let (input_rate, output_rate) = match (historical, long) {
                    (true, false) => (5.0, 30.0),
                    (true, true) => (10.0, 45.0),
                    (false, false) => (4.0, 20.0),
                    (false, true) => (8.0, 30.0),
                };
                let expected = ((input - 30) as f64 * input_rate
                    + input_rate
                    + 25.0 * input_rate
                    + 5.0 * output_rate)
                    / 1_000_000.0;
                for snapshot in [Some(&catalog), Some(&empty)] {
                    let standard =
                        CostUsagePricing::codex_cost_usd_at_date_with_cache_write_and_pricing_snapshot(
                            model, input, 10, 20, 5, day, snapshot,
                        )
                        .unwrap();
                    assert!((standard - expected).abs() < 1e-12, "{model} {input} {day}");
                }
                // Windows' Fast lane has no cache-write input; price it without writes.
                let expected_without_writes =
                    ((input - 10) as f64 * input_rate + input_rate + 5.0 * output_rate)
                        / 1_000_000.0;
                let fast = CostUsagePricing::codex_fast_cost_usd_at_date(model, input, 10, 5, day);
                if long {
                    assert!(fast.is_none(), "{model} {input} {day}");
                } else {
                    let fast = fast.unwrap();
                    assert!((fast - expected_without_writes * 2.0).abs() < 1e-12);
                }
            }
        }
    }
    assert!(
        CostUsagePricing::codex_cost_usd_at_date_with_cache_write_and_pricing_snapshot(
            "fixture-unknown-model",
            100,
            0,
            0,
            5,
            cutoff,
            Some(&catalog),
        )
        .is_none()
    );
}

#[test]
fn gpt56_bundled_rates_price_cache_writes_at_125_percent() {
    let cost = |model: &str, input: u64, output: u64| {
        CostUsagePricing::codex_cost_usd_with_cache_write(model, input, 10, 20, output).unwrap()
    };
    let sol = 70.0 * 4e-6 + 10.0 * 4e-7 + 20.0 * 5e-6 + 5.0 * 2e-5;
    assert!((cost("gpt-5.6-sol", 100, 5) - sol).abs() < 1e-12);
    // Long-context (>272K) rates apply to the entire request. Total input
    // contains 10 cached, 20 cache-write, and 271,971 ordinary input tokens.
    for (model, expected) in [
        (
            "gpt-5.6-sol",
            271_971.0 * 8e-6 + 10.0 * 8e-7 + 20.0 * 1e-5 + 10.0 * 3e-5,
        ),
        (
            "gpt-5.6-terra",
            271_971.0 * 4e-6 + 10.0 * 4e-7 + 20.0 * 5e-6 + 10.0 * 1.8e-5,
        ),
        (
            "gpt-5.6-luna",
            271_971.0 * 4e-7 + 10.0 * 4e-8 + 20.0 * 5e-7 + 10.0 * 1.8e-6,
        ),
    ] {
        assert!(
            (cost(model, 272_001, 10) - expected).abs() < 1e-12,
            "{model}"
        );
    }
}

#[test]
fn gpt54_and_gpt55_bill_the_whole_request_at_long_context_rates_above_272k() {
    let cost = |model: &str, input: u64| {
        CostUsagePricing::codex_cost_usd(model, input, 1_000, 100).unwrap()
    };
    for (model, standard, long) in [
        ("gpt-5.4", (2.5e-6, 2.5e-7, 1.5e-5), (5e-6, 5e-7, 2.25e-5)),
        (
            "gpt-5.4-codex",
            (2.5e-6, 2.5e-7, 1.5e-5),
            (5e-6, 5e-7, 2.25e-5),
        ),
        ("gpt-5.5", (5e-6, 5e-7, 3e-5), (1e-5, 1e-6, 4.5e-5)),
        (
            "openai/gpt-5.5-2026-04-23",
            (5e-6, 5e-7, 3e-5),
            (1e-5, 1e-6, 4.5e-5),
        ),
    ] {
        let price = |(input_rate, cached_rate, output_rate): (f64, f64, f64), input: u64| {
            (input - 1_000) as f64 * input_rate + 1_000.0 * cached_rate + 100.0 * output_rate
        };
        // The 272K boundary itself still bills standard rates.
        assert!(
            (cost(model, 272_000) - price(standard, 272_000)).abs() < 1e-12,
            "{model}"
        );
        assert!(
            (cost(model, 272_001) - price(long, 272_001)).abs() < 1e-12,
            "{model}"
        );
    }
    // Fast mode keeps its 272K cutoff for these models.
    assert!(CostUsagePricing::codex_fast_cost_usd("gpt-5.5-priority", 272_001, 0, 1).is_none());
    assert!(CostUsagePricing::codex_fast_cost_usd("gpt-5.4-fast", 272_000, 0, 1).is_some());
}

fn sorted_keys<P>(table: &HashMap<&'static str, P>) -> Vec<&'static str> {
    let mut keys: Vec<_> = table.keys().copied().collect();
    keys.sort_unstable();
    keys
}

fn codex_pricing_rows() -> String {
    sorted_keys(&CODEX_PRICING)
        .into_iter()
        .map(|key| {
            let p = CODEX_PRICING[key];
            let long = p.long_context.map(|l| {
                (
                    l.input_cost_per_token,
                    l.output_cost_per_token,
                    l.cache_read_input_cost_per_token,
                    l.cache_write_input_cost_per_token,
                )
            });
            format!(
                "{key} {:?} {:?} {:?} {:?} {:?} {:?}\n",
                p.input_cost_per_token,
                p.output_cost_per_token,
                p.cache_read_input_cost_per_token,
                p.cache_write_input_cost_per_token,
                p.display_label,
                long,
            )
        })
        .collect()
}

fn claude_pricing_rows() -> String {
    sorted_keys(&CLAUDE_PRICING)
        .into_iter()
        .map(|key| {
            let p = CLAUDE_PRICING[key];
            format!(
                "{key} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}\n",
                p.input_cost_per_token,
                p.output_cost_per_token,
                p.cache_creation_input_cost_per_token,
                p.cache_read_input_cost_per_token,
                p.threshold_tokens,
                p.input_cost_per_token_above_threshold,
                p.output_cost_per_token_above_threshold,
                p.cache_creation_input_cost_per_token_above_threshold,
                p.cache_read_input_cost_per_token_above_threshold,
            )
        })
        .collect()
}

#[test]
fn bundled_pricing_tables_keep_every_model_and_rate() {
    assert_eq!(
        codex_pricing_rows(),
        "\
gpt-5 1.25e-6 1e-5 1.25e-7 None None None
gpt-5-codex 1.25e-6 1e-5 1.25e-7 None None None
gpt-5-mini 2.5e-7 2e-6 2.5e-8 None None None
gpt-5-nano 5e-8 4e-7 5e-9 None None None
gpt-5-pro 1.5e-5 0.00012 1.5e-5 None None None
gpt-5.1 1.25e-6 1e-5 1.25e-7 None None None
gpt-5.1-codex 1.25e-6 1e-5 1.25e-7 None None None
gpt-5.1-codex-max 1.25e-6 1e-5 1.25e-7 None None None
gpt-5.1-codex-mini 2.5e-7 2e-6 2.5e-8 None None None
gpt-5.2 1.75e-6 1.4e-5 1.75e-7 None None None
gpt-5.2-codex 1.75e-6 1.4e-5 1.75e-7 None None None
gpt-5.2-pro 2.1e-5 0.000168 2.1e-5 None None None
gpt-5.3-codex 1.75e-6 1.4e-5 1.75e-7 None None None
gpt-5.3-codex-spark 0.0 0.0 0.0 None Some(\"Research Preview\") None
gpt-5.4 2.5e-6 1.5e-5 2.5e-7 None None Some((5e-6, 2.25e-5, 5e-7, None))
gpt-5.4-codex 2.5e-6 1.5e-5 2.5e-7 None None Some((5e-6, 2.25e-5, 5e-7, None))
gpt-5.4-mini 7.5e-7 4.5e-6 7.5e-8 None None None
gpt-5.4-mini-codex 7.5e-7 4.5e-6 7.5e-8 None None None
gpt-5.4-nano 2e-7 1.25e-6 2e-8 None None None
gpt-5.4-nano-codex 2e-7 1.25e-6 2e-8 None None None
gpt-5.4-pro 3e-5 0.00018 3e-5 None None None
gpt-5.5 5e-6 3e-5 5e-7 None None Some((1e-5, 4.5e-5, 1e-6, None))
gpt-5.5-cyber 1.25e-5 7.5e-5 1.25e-6 None None None
gpt-5.5-pro 3e-5 0.00018 3e-5 None None None
gpt-5.6-cyber 1.25e-5 7.5e-5 1.25e-6 Some(1.5625e-5) None None
gpt-5.6-luna 2e-7 1.2e-6 2e-8 Some(2.5e-7) None Some((4e-7, 1.8e-6, 4e-8, Some(5e-7)))
gpt-5.6-sol 4e-6 2e-5 4e-7 Some(5e-6) None Some((8e-6, 3e-5, 8e-7, Some(1e-5)))
gpt-5.6-terra 2e-6 1.2e-5 2e-7 Some(2.5e-6) None Some((4e-6, 1.8e-5, 4e-7, Some(5e-6)))
gpt-6-astra 1e-5 5e-5 1e-6 Some(1.25e-5) None Some((2e-5, 7.5e-5, 2e-6, Some(2.5e-5)))
"
    );
    assert_eq!(
        claude_pricing_rows(),
        "\
claude-fable-5 1e-5 5e-5 1.25e-5 1e-6 None None None None None
claude-haiku-4-5 1e-6 5e-6 1.25e-6 1e-7 None None None None None
claude-haiku-4-5-20251001 1e-6 5e-6 1.25e-6 1e-7 None None None None None
claude-opus-4-1 1.5e-5 7.5e-5 1.875e-5 1.5e-6 None None None None None
claude-opus-4-20250514 1.5e-5 7.5e-5 1.875e-5 1.5e-6 None None None None None
claude-opus-4-5 5e-6 2.5e-5 6.25e-6 5e-7 None None None None None
claude-opus-4-5-20251101 5e-6 2.5e-5 6.25e-6 5e-7 None None None None None
claude-opus-4-6 5e-6 2.5e-5 6.25e-6 5e-7 None None None None None
claude-opus-4-6-20260205 5e-6 2.5e-5 6.25e-6 5e-7 None None None None None
claude-opus-4-7 5e-6 2.5e-5 6.25e-6 5e-7 None None None None None
claude-opus-4-8 5e-6 2.5e-5 6.25e-6 5e-7 None None None None None
claude-sonnet-4-20250514 3e-6 1.5e-5 3.75e-6 3e-7 Some(200000) Some(6e-6) Some(2.25e-5) Some(7.5e-6) Some(6e-7)
claude-sonnet-4-5 3e-6 1.5e-5 3.75e-6 3e-7 Some(200000) Some(6e-6) Some(2.25e-5) Some(7.5e-6) Some(6e-7)
claude-sonnet-4-5-20250929 3e-6 1.5e-5 3.75e-6 3e-7 Some(200000) Some(6e-6) Some(2.25e-5) Some(7.5e-6) Some(6e-7)
claude-sonnet-4-6 3e-6 1.5e-5 3.75e-6 3e-7 Some(200000) Some(6e-6) Some(2.25e-5) Some(7.5e-6) Some(6e-7)
"
    );
}
