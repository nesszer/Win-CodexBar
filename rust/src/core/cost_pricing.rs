//! Cost usage pricing — model-specific token pricing for Codex (OpenAI) and Claude (Anthropic).

use super::codex_routed_pricing;
use super::models_dev_pricing;
use chrono::NaiveDate;
use std::collections::HashMap;
use std::sync::LazyLock;
#[path = "cost_pricing/claude.rs"]
mod claude_pricing;
#[path = "cost_pricing/codex.rs"]
mod codex_pricing;
pub(crate) use claude_pricing::ClaudePricingResolution;
/// Whole-request Codex rates for input above the model context threshold.
#[derive(Debug, Clone, Copy)]
pub struct CodexLongContextRates {
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub cache_read_input_cost_per_token: f64,
}
/// Codex (OpenAI) model pricing
#[derive(Debug, Clone, Copy)]
pub struct CodexPricing {
    /// Cost per input token in USD
    pub input_cost_per_token: f64,
    /// Cost per output token in USD
    pub output_cost_per_token: f64,
    /// Cost per cached input token in USD
    pub cache_read_input_cost_per_token: f64,
    /// Optional display label override (e.g. "Research Preview")
    pub display_label: Option<&'static str>,
    /// Whole-request rates above the Codex long-context threshold.
    pub long_context: Option<CodexLongContextRates>,
}
/// Claude (Anthropic) model pricing with optional tiered pricing
#[derive(Debug, Clone, Copy)]
pub struct ClaudePricing {
    /// Cost per input token in USD
    pub input_cost_per_token: f64,
    /// Cost per output token in USD
    pub output_cost_per_token: f64,
    /// Cost per cache creation input token in USD
    pub cache_creation_input_cost_per_token: f64,
    /// Cost per cache read input token in USD
    pub cache_read_input_cost_per_token: f64,
    /// Token threshold for tiered pricing (None = no tiering)
    pub threshold_tokens: Option<i32>,
    /// Cost per input token above threshold
    pub input_cost_per_token_above_threshold: Option<f64>,
    /// Cost per output token above threshold
    pub output_cost_per_token_above_threshold: Option<f64>,
    /// Cost per cache creation input token above threshold
    pub cache_creation_input_cost_per_token_above_threshold: Option<f64>,
    /// Cost per cache read input token above threshold
    pub cache_read_input_cost_per_token_above_threshold: Option<f64>,
}

/// Codex model pricing table
static CODEX_PRICING: LazyLock<HashMap<&'static str, CodexPricing>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // GPT-5 pricing
    m.insert(
        "gpt-5",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5-codex",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5-mini",
        CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: 2.5e-8,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5-nano",
        CodexPricing {
            input_cost_per_token: 5e-8,
            output_cost_per_token: 4e-7,
            cache_read_input_cost_per_token: 5e-9,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5-pro",
        CodexPricing {
            input_cost_per_token: 1.5e-5,
            output_cost_per_token: 1.2e-4,
            cache_read_input_cost_per_token: 1.5e-5,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.1",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.1-codex",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.1-codex-max",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.1-codex-mini",
        CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: 2.5e-8,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.2",
        CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: 1.75e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.2-codex",
        CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: 1.75e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.2-pro",
        CodexPricing {
            input_cost_per_token: 2.1e-5,
            output_cost_per_token: 1.68e-4,
            cache_read_input_cost_per_token: 2.1e-5,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.3-codex",
        CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: 1.75e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.3-codex-spark",
        CodexPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            cache_read_input_cost_per_token: 0.0,
            display_label: Some("Research Preview"),
            long_context: None,
        },
    );

    // GPT-5.4 pricing (updated to match upstream 0.22)
    m.insert(
        "gpt-5.4",
        CodexPricing {
            input_cost_per_token: 2.5e-6,
            output_cost_per_token: 1.5e-5,
            cache_read_input_cost_per_token: 2.5e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.4-codex",
        CodexPricing {
            input_cost_per_token: 2.5e-6,
            output_cost_per_token: 1.5e-5,
            cache_read_input_cost_per_token: 2.5e-7,
            display_label: None,
            long_context: None,
        },
    );

    // GPT-5.4 Mini pricing (updated to match upstream 0.22)
    m.insert(
        "gpt-5.4-mini",
        CodexPricing {
            input_cost_per_token: 7.5e-7,
            output_cost_per_token: 4.5e-6,
            cache_read_input_cost_per_token: 7.5e-8,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.4-mini-codex",
        CodexPricing {
            input_cost_per_token: 7.5e-7,
            output_cost_per_token: 4.5e-6,
            cache_read_input_cost_per_token: 7.5e-8,
            display_label: None,
            long_context: None,
        },
    );

    // GPT-5.4 Nano pricing (updated to match upstream 0.22)
    m.insert(
        "gpt-5.4-nano",
        CodexPricing {
            input_cost_per_token: 2e-7,
            output_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: 2e-8,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.4-nano-codex",
        CodexPricing {
            input_cost_per_token: 2e-7,
            output_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: 2e-8,
            display_label: None,
            long_context: None,
        },
    );

    // GPT-5.4 Pro
    m.insert(
        "gpt-5.4-pro",
        CodexPricing {
            input_cost_per_token: 3e-5,
            output_cost_per_token: 1.8e-4,
            cache_read_input_cost_per_token: 3e-5,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.5",
        CodexPricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 3e-5,
            cache_read_input_cost_per_token: 5e-7,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.5-pro",
        CodexPricing {
            input_cost_per_token: 3e-5,
            output_cost_per_token: 1.8e-4,
            cache_read_input_cost_per_token: 3e-5,
            display_label: None,
            long_context: None,
        },
    );
    m.insert(
        "gpt-5.6-sol",
        CodexPricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 3e-5,
            cache_read_input_cost_per_token: 5e-7,
            display_label: None,
            long_context: Some(CodexLongContextRates {
                input_cost_per_token: 1e-5,
                output_cost_per_token: 4.5e-5,
                cache_read_input_cost_per_token: 1e-6,
            }),
        },
    );
    m.insert(
        "gpt-5.6-terra",
        CodexPricing {
            input_cost_per_token: 2e-6,
            output_cost_per_token: 1.2e-5,
            cache_read_input_cost_per_token: 2e-7,
            display_label: None,
            long_context: Some(CodexLongContextRates {
                input_cost_per_token: 4e-6,
                output_cost_per_token: 1.8e-5,
                cache_read_input_cost_per_token: 4e-7,
            }),
        },
    );
    m.insert(
        "gpt-5.6-luna",
        CodexPricing {
            input_cost_per_token: 2e-7,
            output_cost_per_token: 1.2e-6,
            cache_read_input_cost_per_token: 2e-8,
            display_label: None,
            long_context: Some(CodexLongContextRates {
                input_cost_per_token: 4e-7,
                output_cost_per_token: 1.8e-6,
                cache_read_input_cost_per_token: 4e-8,
            }),
        },
    );
    // GPT-6 Astra pricing (OpenAI model card and pricing table).
    // Long-context rates apply to the whole request above 272K input tokens.
    m.insert(
        "gpt-6-astra",
        CodexPricing {
            input_cost_per_token: 1e-5,
            output_cost_per_token: 5e-5,
            cache_read_input_cost_per_token: 1e-6,
            display_label: None,
            long_context: Some(CodexLongContextRates {
                input_cost_per_token: 2e-5,
                output_cost_per_token: 7.5e-5,
                cache_read_input_cost_per_token: 2e-6,
            }),
        },
    );

    m
});

/// Claude model pricing table
static CLAUDE_PRICING: LazyLock<HashMap<&'static str, ClaudePricing>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // Fable 5
    m.insert(
        "claude-fable-5",
        ClaudePricing {
            input_cost_per_token: 1e-5,
            output_cost_per_token: 5e-5,
            cache_creation_input_cost_per_token: 1.25e-5,
            cache_read_input_cost_per_token: 1e-6,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );

    // Haiku 4.5
    m.insert(
        "claude-haiku-4-5",
        ClaudePricing {
            input_cost_per_token: 1e-6,
            output_cost_per_token: 5e-6,
            cache_creation_input_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: 1e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );
    m.insert(
        "claude-haiku-4-5-20251001",
        ClaudePricing {
            input_cost_per_token: 1e-6,
            output_cost_per_token: 5e-6,
            cache_creation_input_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: 1e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );

    // Opus 4.6
    m.insert(
        "claude-opus-4-6",
        ClaudePricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 2.5e-5,
            cache_creation_input_cost_per_token: 6.25e-6,
            cache_read_input_cost_per_token: 5e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );
    m.insert(
        "claude-opus-4-6-20260205",
        ClaudePricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 2.5e-5,
            cache_creation_input_cost_per_token: 6.25e-6,
            cache_read_input_cost_per_token: 5e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );

    // Opus 4.7 (same pricing as Opus 4.6)
    m.insert(
        "claude-opus-4-7",
        ClaudePricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 2.5e-5,
            cache_creation_input_cost_per_token: 6.25e-6,
            cache_read_input_cost_per_token: 5e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );

    // Opus 4.8 (same pricing as Opus 4.5/4.6/4.7)
    m.insert(
        "claude-opus-4-8",
        ClaudePricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 2.5e-5,
            cache_creation_input_cost_per_token: 6.25e-6,
            cache_read_input_cost_per_token: 5e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );

    // Opus 4.5
    m.insert(
        "claude-opus-4-5",
        ClaudePricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 2.5e-5,
            cache_creation_input_cost_per_token: 6.25e-6,
            cache_read_input_cost_per_token: 5e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );
    m.insert(
        "claude-opus-4-5-20251101",
        ClaudePricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 2.5e-5,
            cache_creation_input_cost_per_token: 6.25e-6,
            cache_read_input_cost_per_token: 5e-7,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );

    // Sonnet 4.5 (with tiered pricing at 200k tokens)
    m.insert(
        "claude-sonnet-4-5",
        ClaudePricing {
            input_cost_per_token: 3e-6,
            output_cost_per_token: 1.5e-5,
            cache_creation_input_cost_per_token: 3.75e-6,
            cache_read_input_cost_per_token: 3e-7,
            threshold_tokens: Some(200_000),
            input_cost_per_token_above_threshold: Some(6e-6),
            output_cost_per_token_above_threshold: Some(2.25e-5),
            cache_creation_input_cost_per_token_above_threshold: Some(7.5e-6),
            cache_read_input_cost_per_token_above_threshold: Some(6e-7),
        },
    );
    m.insert(
        "claude-sonnet-4-5-20250929",
        ClaudePricing {
            input_cost_per_token: 3e-6,
            output_cost_per_token: 1.5e-5,
            cache_creation_input_cost_per_token: 3.75e-6,
            cache_read_input_cost_per_token: 3e-7,
            threshold_tokens: Some(200_000),
            input_cost_per_token_above_threshold: Some(6e-6),
            output_cost_per_token_above_threshold: Some(2.25e-5),
            cache_creation_input_cost_per_token_above_threshold: Some(7.5e-6),
            cache_read_input_cost_per_token_above_threshold: Some(6e-7),
        },
    );

    // Sonnet 4.6 (same pricing as Sonnet 4.5, with 200k tier)
    m.insert(
        "claude-sonnet-4-6",
        ClaudePricing {
            input_cost_per_token: 3e-6,
            output_cost_per_token: 1.5e-5,
            cache_creation_input_cost_per_token: 3.75e-6,
            cache_read_input_cost_per_token: 3e-7,
            threshold_tokens: Some(200_000),
            input_cost_per_token_above_threshold: Some(6e-6),
            output_cost_per_token_above_threshold: Some(2.25e-5),
            cache_creation_input_cost_per_token_above_threshold: Some(7.5e-6),
            cache_read_input_cost_per_token_above_threshold: Some(6e-7),
        },
    );

    // Opus 4
    m.insert(
        "claude-opus-4-20250514",
        ClaudePricing {
            input_cost_per_token: 1.5e-5,
            output_cost_per_token: 7.5e-5,
            cache_creation_input_cost_per_token: 1.875e-5,
            cache_read_input_cost_per_token: 1.5e-6,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );
    m.insert(
        "claude-opus-4-1",
        ClaudePricing {
            input_cost_per_token: 1.5e-5,
            output_cost_per_token: 7.5e-5,
            cache_creation_input_cost_per_token: 1.875e-5,
            cache_read_input_cost_per_token: 1.5e-6,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        },
    );

    // Sonnet 4
    m.insert(
        "claude-sonnet-4-20250514",
        ClaudePricing {
            input_cost_per_token: 3e-6,
            output_cost_per_token: 1.5e-5,
            cache_creation_input_cost_per_token: 3.75e-6,
            cache_read_input_cost_per_token: 3e-7,
            threshold_tokens: Some(200_000),
            input_cost_per_token_above_threshold: Some(6e-6),
            output_cost_per_token_above_threshold: Some(2.25e-5),
            cache_creation_input_cost_per_token_above_threshold: Some(7.5e-6),
            cache_read_input_cost_per_token_above_threshold: Some(6e-7),
        },
    );

    m
});

/// Cost usage pricing utilities
pub struct CostUsagePricing;

impl CostUsagePricing {
    /// Sentinel model key for model-less Codex token events.
    ///
    /// Usage remains visible under this key but is never priced as a real model
    /// (including catalog collisions with a generic "unknown" entry).
    pub const CODEX_UNATTRIBUTED_MODEL: &'static str = "unknown";
    /// True when `model` is the unattributed / model-less sentinel.
    pub fn is_codex_unattributed_model(model: &str) -> bool {
        Self::normalize_codex_model(model) == Self::CODEX_UNATTRIBUTED_MODEL
    }

    /// Normalize a Codex model name for pricing lookup
    pub fn normalize_codex_model(raw: &str) -> String {
        let mut trimmed = raw.trim().to_string();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("unknown")
            || trimmed.eq_ignore_ascii_case("unpriced")
        {
            return Self::CODEX_UNATTRIBUTED_MODEL.to_string();
        }

        // Remove "openai/" prefix
        if let Some(rest) = trimmed.strip_prefix("openai/") {
            trimmed = rest.to_string();
        }

        // Check if base model (without -codex suffix) exists in pricing
        if let Some(idx) = trimmed.find("-codex") {
            let base = &trimmed[..idx];
            if CODEX_PRICING.contains_key(base) || base == "gpt-5.6" {
                trimmed = base.to_string();
            }
        }

        let date_pattern = regex_lite::Regex::new(r"-\d{4}-\d{2}-\d{2}$").unwrap();
        if let Some(mat) = date_pattern.find(&trimmed) {
            let base = &trimmed[..mat.start()];
            if CODEX_PRICING.contains_key(base) || base == "gpt-5.6" {
                trimmed = base.to_string();
            }
        }

        if trimmed == "gpt-5.6" {
            return "gpt-5.6-sol".to_string();
        }

        trimmed
    }

    /// Detect a provider-qualified route prefix on a Codex model name.
    /// Delegates to [`codex_routed_pricing::codex_routed_provider`].
    pub fn codex_routed_provider(model: &str) -> Option<&'static str> {
        codex_routed_pricing::codex_routed_provider(model)
    }

    /// Whether a Codex model belongs to the native OpenAI subscription rather
    /// than a provider-qualified routed subscription.
    pub fn counts_toward_codex_subscription(model: &str) -> bool {
        codex_routed_pricing::counts_toward_codex_subscription(model)
    }

    /// Get the display label for a Codex model (e.g. "Research Preview")
    pub fn codex_display_label(model: &str) -> Option<&'static str> {
        let key = Self::normalize_codex_model(model);
        CODEX_PRICING
            .get(key.as_str())
            .and_then(|p| p.display_label)
    }

    /// Strip Fast/priority suffix to find the base model for pricing lookup.
    ///
    /// Fast-tier models ("gpt-5.5-fast", "gpt-5.6-sol-priority") price as the
    /// standard base × multiplier. Both `codex_api_fast_multiplier` and
    /// `codex_fast_cost_usd` must use this helper so the original suffix does
    /// not leak into the base lookup (audit C4).
    pub fn codex_fast_base_model(model: &str) -> String {
        let key = Self::normalize_codex_model(model);
        key.strip_suffix("-fast")
            .or_else(|| key.strip_suffix("-priority"))
            .map(Self::normalize_codex_model)
            .unwrap_or(key)
    }

    /// Fast-tier multiplier per model (upstream 0.48.0 C4). Fast USD = Standard
    /// cost × multiplier. Returns `None` for models without a Fast lane.
    ///
    /// Multipliers: gpt-5.4, gpt-5.4-mini, gpt-5.6-sol, gpt-5.6-terra,
    /// gpt-5.6-luna → 2.0; gpt-5.5 → 2.5; else nil.
    pub fn codex_api_fast_multiplier(model: &str) -> Option<f64> {
        let base = Self::codex_fast_base_model(model);
        match base.as_str() {
            "gpt-5.4" | "gpt-5.4-mini" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna"
            | "gpt-6-astra" => Some(2.0),
            "gpt-5.5" => Some(2.5),
            _ => None,
        }
    }

    /// Fast-tier cost in USD for a model (upstream 0.48.0 C4).
    ///
    /// Computes the standard cost for the BASE model (stripping fast/priority
    /// suffixes), then applies the Fast multiplier. Returns `None` when the
    /// model has no Fast lane or when a model without Astra's published
    /// long-context Fast rates exceeds the 272 000 threshold.
    pub fn codex_fast_cost_usd(model: &str, input: i32, cached: i32, output: i32) -> Option<f64> {
        let multiplier = Self::codex_api_fast_multiplier(model)?;
        // Older models do not offer Fast for long-context requests. Astra
        // publishes a Fast rate for the same whole-request long-context tier.
        if (input.max(0) as u64) > codex_pricing::CODEX_LONG_CONTEXT_THRESHOLD
            && !codex_pricing::codex_fast_allows_long_context(model)
        {
            return None;
        }
        let base = Self::codex_fast_base_model(model);
        let base_cost = Self::codex_cost_usd(
            &base,
            input.max(0) as u64,
            cached.max(0) as u64,
            output.max(0) as u64,
        )?;
        Some(base_cost * multiplier)
    }

    /// Calculate Codex cost using the rates in effect on a historical usage day.
    /// GPT-5.6 Terra/Luna were cut on 2026-07-30; Sol was unchanged.
    pub fn codex_cost_usd_at_date(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
        pricing_date: NaiveDate,
    ) -> Option<f64> {
        Self::codex_cost_usd_at_date_with_pricing_snapshot(
            model,
            input_tokens,
            cached_input_tokens,
            output_tokens,
            pricing_date,
            None,
        )
    }

    pub fn codex_cost_usd_at_date_with_pricing_snapshot(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
        pricing_date: NaiveDate,
        pricing_snapshot: Option<&models_dev_pricing::ModelsDevPricingSnapshot>,
    ) -> Option<f64> {
        let key = Self::normalize_codex_model(model);
        let cutoff = NaiveDate::from_ymd_opt(2026, 7, 30).expect("valid pricing cutoff");
        if pricing_date < cutoff {
            let long = input_tokens > codex_pricing::CODEX_LONG_CONTEXT_THRESHOLD;
            let rates = match (key.as_str(), long) {
                ("gpt-5.6-terra", false) => Some((2.5e-6, 2.5e-7, 1.5e-5)),
                ("gpt-5.6-terra", true) => Some((5e-6, 5e-7, 2.25e-5)),
                ("gpt-5.6-luna", false) => Some((1e-6, 1e-7, 6e-6)),
                ("gpt-5.6-luna", true) => Some((2e-6, 2e-7, 9e-6)),
                _ => None,
            };
            if let Some((input_rate, cache_rate, output_rate)) = rates {
                return Some(codex_pricing::codex_cost_from_rates(
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                    input_rate,
                    cache_rate,
                    output_rate,
                ));
            }
        }
        Self::codex_cost_usd_with_pricing_snapshot(
            model,
            input_tokens,
            cached_input_tokens,
            output_tokens,
            pricing_snapshot,
        )
    }

    pub fn codex_fast_cost_usd_at_date(
        model: &str,
        input: i32,
        cached: i32,
        output: i32,
        pricing_date: NaiveDate,
    ) -> Option<f64> {
        let multiplier = Self::codex_api_fast_multiplier(model)?;
        if (input.max(0) as u64) > codex_pricing::CODEX_LONG_CONTEXT_THRESHOLD
            && !codex_pricing::codex_fast_allows_long_context(model)
        {
            return None;
        }
        let base = Self::codex_fast_base_model(model);
        let base_cost = Self::codex_cost_usd_at_date(
            &base,
            input.max(0) as u64,
            cached.max(0) as u64,
            output.max(0) as u64,
            pricing_date,
        )?;
        Some(base_cost * multiplier)
    }

    /// Calculate cost for Codex usage in USD
    pub fn codex_cost_usd(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
    ) -> Option<f64> {
        Self::codex_cost_usd_with_cache_write(
            model,
            input_tokens,
            cached_input_tokens,
            0,
            output_tokens,
        )
    }

    /// Format model name for display (e.g., "claude-3.5-sonnet" → "Sonnet 3.5")
    pub fn format_model_name(model: &str) -> String {
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
}

#[cfg(test)]
#[path = "cost_pricing_tests.rs"]
mod tests;
