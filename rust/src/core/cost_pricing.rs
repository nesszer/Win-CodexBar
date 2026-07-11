//! Cost Usage Pricing
//!
//! Model-specific token pricing for Codex (OpenAI) and Claude (Anthropic) models.
//! Supports tiered pricing for models with token thresholds.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::LazyLock;

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
        },
    );
    m.insert(
        "gpt-5-codex",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5-mini",
        CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: 2.5e-8,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5-nano",
        CodexPricing {
            input_cost_per_token: 5e-8,
            output_cost_per_token: 4e-7,
            cache_read_input_cost_per_token: 5e-9,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5-pro",
        CodexPricing {
            input_cost_per_token: 1.5e-5,
            output_cost_per_token: 1.2e-4,
            cache_read_input_cost_per_token: 1.5e-5,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.1",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.1-codex",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.1-codex-max",
        CodexPricing {
            input_cost_per_token: 1.25e-6,
            output_cost_per_token: 1e-5,
            cache_read_input_cost_per_token: 1.25e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.1-codex-mini",
        CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: 2.5e-8,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.2",
        CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: 1.75e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.2-codex",
        CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: 1.75e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.2-pro",
        CodexPricing {
            input_cost_per_token: 2.1e-5,
            output_cost_per_token: 1.68e-4,
            cache_read_input_cost_per_token: 2.1e-5,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.3-codex",
        CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: 1.75e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.3-codex-spark",
        CodexPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            cache_read_input_cost_per_token: 0.0,
            display_label: Some("Research Preview"),
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
        },
    );
    m.insert(
        "gpt-5.4-codex",
        CodexPricing {
            input_cost_per_token: 2.5e-6,
            output_cost_per_token: 1.5e-5,
            cache_read_input_cost_per_token: 2.5e-7,
            display_label: None,
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
        },
    );
    m.insert(
        "gpt-5.4-mini-codex",
        CodexPricing {
            input_cost_per_token: 7.5e-7,
            output_cost_per_token: 4.5e-6,
            cache_read_input_cost_per_token: 7.5e-8,
            display_label: None,
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
        },
    );
    m.insert(
        "gpt-5.4-nano-codex",
        CodexPricing {
            input_cost_per_token: 2e-7,
            output_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: 2e-8,
            display_label: None,
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
        },
    );
    m.insert(
        "gpt-5.5",
        CodexPricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 3e-5,
            cache_read_input_cost_per_token: 5e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.5-pro",
        CodexPricing {
            input_cost_per_token: 3e-5,
            output_cost_per_token: 1.8e-4,
            cache_read_input_cost_per_token: 3e-5,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.6-sol",
        CodexPricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 3e-5,
            cache_read_input_cost_per_token: 5e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.6-terra",
        CodexPricing {
            input_cost_per_token: 2.5e-6,
            output_cost_per_token: 1.5e-5,
            cache_read_input_cost_per_token: 2.5e-7,
            display_label: None,
        },
    );
    m.insert(
        "gpt-5.6-luna",
        CodexPricing {
            input_cost_per_token: 1e-6,
            output_cost_per_token: 6e-6,
            cache_read_input_cost_per_token: 1e-7,
            display_label: None,
        },
    );

    m
});

const CODEX_LONG_CONTEXT_THRESHOLD: u64 = 272_000;

/// Codex rates that apply to the entire request above the model's context threshold.
static CODEX_LONG_CONTEXT_PRICING: LazyLock<HashMap<&'static str, CodexPricing>> =
    LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert(
            "gpt-5.6-sol",
            CodexPricing {
                input_cost_per_token: 1e-5,
                output_cost_per_token: 4.5e-5,
                cache_read_input_cost_per_token: 1e-6,
                display_label: None,
            },
        );
        m.insert(
            "gpt-5.6-terra",
            CodexPricing {
                input_cost_per_token: 5e-6,
                output_cost_per_token: 2.25e-5,
                cache_read_input_cost_per_token: 5e-7,
                display_label: None,
            },
        );
        m.insert(
            "gpt-5.6-luna",
            CodexPricing {
                input_cost_per_token: 2e-6,
                output_cost_per_token: 9e-6,
                cache_read_input_cost_per_token: 2e-7,
                display_label: None,
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
    /// Normalize a Codex model name for pricing lookup
    pub fn normalize_codex_model(raw: &str) -> String {
        let mut trimmed = raw.trim().to_string();

        // Remove "openai/" prefix
        if let Some(rest) = trimmed.strip_prefix("openai/") {
            trimmed = rest.to_string();
        }

        if trimmed == "gpt-5.6" {
            return "gpt-5.6-sol".to_string();
        }

        // Check if base model (without -codex suffix) exists in pricing
        if let Some(idx) = trimmed.find("-codex") {
            let base = &trimmed[..idx];
            if CODEX_PRICING.contains_key(base) {
                return base.to_string();
            }
            if base == "gpt-5.6" {
                return "gpt-5.6-sol".to_string();
            }
        }

        let date_pattern = regex_lite::Regex::new(r"-\d{4}-\d{2}-\d{2}$").unwrap();
        if let Some(mat) = date_pattern.find(&trimmed) {
            let base = &trimmed[..mat.start()];
            if CODEX_PRICING.contains_key(base) {
                return base.to_string();
            }
            if base == "gpt-5.6" {
                return "gpt-5.6-sol".to_string();
            }
        }

        trimmed
    }

    /// Get the display label for a Codex model (e.g. "Research Preview")
    pub fn codex_display_label(model: &str) -> Option<&'static str> {
        let key = Self::normalize_codex_model(model);
        CODEX_PRICING
            .get(key.as_str())
            .and_then(|p| p.display_label)
    }

    /// Normalize a Claude model name for pricing lookup
    pub fn normalize_claude_model(raw: &str) -> String {
        let mut trimmed = raw.trim().to_string();

        // Remove "anthropic." prefix
        if let Some(rest) = trimmed.strip_prefix("anthropic.") {
            trimmed = rest.to_string();
        }

        // Handle nested model names like "anthropic.claude-sonnet-4.claude-sonnet-4-20250514"
        if trimmed.contains("claude-")
            && let Some(last_dot) = trimmed.rfind('.')
        {
            let tail = &trimmed[last_dot + 1..];
            if tail.starts_with("claude-") {
                trimmed = tail.to_string();
            }
        }

        // Remove version suffix like "-v1:0"
        let version_pattern = regex_lite::Regex::new(r"-v\d+:\d+$").unwrap();
        trimmed = version_pattern.replace(&trimmed, "").to_string();

        // Try without date suffix if base exists in pricing
        let date_pattern = regex_lite::Regex::new(r"-\d{8}$").unwrap();
        if let Some(mat) = date_pattern.find(&trimmed) {
            let base = &trimmed[..mat.start()];
            if CLAUDE_PRICING.contains_key(base) {
                return base.to_string();
            }
        }

        trimmed
    }

    /// Calculate cost for Codex usage in USD
    pub fn codex_cost_usd(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
    ) -> Option<f64> {
        let key = Self::normalize_codex_model(model);
        let pricing = if input_tokens > CODEX_LONG_CONTEXT_THRESHOLD {
            CODEX_LONG_CONTEXT_PRICING
                .get(key.as_str())
                .or_else(|| CODEX_PRICING.get(key.as_str()))?
        } else {
            CODEX_PRICING.get(key.as_str())?
        };

        let cached = cached_input_tokens.min(input_tokens);
        let non_cached = input_tokens.saturating_sub(cached);

        let cost = (non_cached as f64) * pricing.input_cost_per_token
            + (cached as f64) * pricing.cache_read_input_cost_per_token
            + (output_tokens as f64) * pricing.output_cost_per_token;

        Some(cost)
    }

    /// Calculate cost for Claude usage in USD
    pub fn claude_cost_usd(
        model: &str,
        input_tokens: i32,
        cache_read_input_tokens: i32,
        cache_creation_input_tokens: i32,
        output_tokens: i32,
    ) -> Option<f64> {
        let key = Self::normalize_claude_model(model);
        let pricing = CLAUDE_PRICING.get(key.as_str())?;

        /// Calculate tiered cost
        fn tiered(tokens: i32, base: f64, above: Option<f64>, threshold: Option<i32>) -> f64 {
            let tokens = tokens.max(0);
            match (threshold, above) {
                (Some(thresh), Some(above_rate)) => {
                    let below = tokens.min(thresh);
                    let over = (tokens - thresh).max(0);
                    (below as f64) * base + (over as f64) * above_rate
                }
                _ => (tokens as f64) * base,
            }
        }

        let cost = tiered(
            input_tokens,
            pricing.input_cost_per_token,
            pricing.input_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        ) + tiered(
            cache_read_input_tokens,
            pricing.cache_read_input_cost_per_token,
            pricing.cache_read_input_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        ) + tiered(
            cache_creation_input_tokens,
            pricing.cache_creation_input_cost_per_token,
            pricing.cache_creation_input_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        ) + tiered(
            output_tokens,
            pricing.output_cost_per_token,
            pricing.output_cost_per_token_above_threshold,
            pricing.threshold_tokens,
        );

        Some(cost)
    }

    /// Base per-token input rate for a Claude model. Exposed for callers that
    /// need a rate the standard cost function doesn't model — e.g. the usage
    /// scanner's one-hour cache-write premium, billed at 2x the input rate.
    pub fn claude_input_cost_per_token(model: &str) -> Option<f64> {
        let key = Self::normalize_claude_model(model);
        CLAUDE_PRICING
            .get(key.as_str())
            .map(|p| p.input_cost_per_token)
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
mod tests {
    use super::*;

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
        let cost = CostUsagePricing::codex_cost_usd("gpt-5", 1000, 0, 500);
        assert!(cost.is_some());
        let cost = cost.unwrap();
        // 1000 * 1.25e-6 + 500 * 1e-5 = 0.00125 + 0.005 = 0.00625
        assert!((cost - 0.00625).abs() < 1e-10);
    }

    #[test]
    fn test_claude_cost() {
        let cost = CostUsagePricing::claude_cost_usd("claude-haiku-4-5-20251001", 1000, 0, 0, 500);
        assert!(cost.is_some());
    }

    #[test]
    fn test_opus_4_8_cost() {
        // Opus 4.8 bills at $5/1M input + $25/1M output (same tier as 4.5/4.6/4.7).
        let cost = CostUsagePricing::claude_cost_usd("claude-opus-4-8", 1_000, 0, 0, 500).unwrap();
        // 1000 * 5e-6 + 500 * 2.5e-5 = 0.005 + 0.0125 = 0.0175
        assert!((cost - 0.0175).abs() < 1e-10);
    }

    #[test]
    fn test_fable_5_cost() {
        // Fable 5 bills at $10/1M input + $50/1M output.
        let cost = CostUsagePricing::claude_cost_usd("claude-fable-5", 1_000, 0, 0, 500).unwrap();
        // 1000 * 1e-5 + 500 * 5e-5 = 0.01 + 0.025 = 0.035
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
        let cost = CostUsagePricing::codex_cost_usd("gpt-5.4-mini", 1000, 0, 500);
        assert!(cost.is_some());
        // 1000 * 7.5e-7 + 500 * 4.5e-6 = 0.00075 + 0.00225 = 0.003
        assert!((cost.unwrap() - 0.003).abs() < 1e-10);
    }

    #[test]
    fn test_gpt54_nano_cost() {
        let cost = CostUsagePricing::codex_cost_usd("gpt-5.4-nano", 1000, 0, 500);
        assert!(cost.is_some());
        // 1000 * 2e-7 + 500 * 1.25e-6 = 0.0002 + 0.000625 = 0.000825
        assert!((cost.unwrap() - 0.000825).abs() < 1e-10);
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

        let cost = CostUsagePricing::codex_cost_usd("gpt-5.5", 1000, 500, 500);
        assert!(cost.is_some());
        assert!((cost.unwrap() - 0.01775).abs() < 1e-10);
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
        let cost = CostUsagePricing::claude_cost_usd("claude-opus-4-7", 1000, 0, 0, 500);
        assert!(cost.is_some());
    }

    #[test]
    fn test_sonnet_4_6_cost() {
        let cost = CostUsagePricing::claude_cost_usd("claude-sonnet-4-6", 1000, 0, 0, 500);
        assert!(cost.is_some());
    }

    #[test]
    fn test_gpt5_pro_cost() {
        let cost = CostUsagePricing::codex_cost_usd("gpt-5-pro", 1000, 0, 500);
        assert!(cost.is_some());
        // 1000 * 1.5e-5 + 500 * 1.2e-4 = 0.015 + 0.06 = 0.075
        assert!((cost.unwrap() - 0.075).abs() < 1e-10);
    }

    #[test]
    fn test_gpt56_standard_pricing() {
        for (model, expected) in [
            ("gpt-5.6-sol", 0.0332),
            ("gpt-5.6-terra", 0.0166),
            ("gpt-5.6-luna", 0.00664),
        ] {
            let cost = CostUsagePricing::codex_cost_usd(model, 1_000, 400, 1_000);
            assert!((cost.unwrap() - expected).abs() < 1e-10, "{model}");
        }
    }

    #[test]
    fn test_gpt56_long_context_pricing() {
        for (model, expected) in [
            ("gpt-5.6-sol", 45.272001),
            ("gpt-5.6-terra", 22.6360005),
            ("gpt-5.6-luna", 9.0544002),
        ] {
            let cost = CostUsagePricing::codex_cost_usd(model, 272_001, 272_001, 1_000_000);
            assert!((cost.unwrap() - expected).abs() < 1e-10, "{model}");
        }
    }

    #[test]
    fn test_gpt56_context_threshold_is_exclusive() {
        for (model, expected) in [
            ("gpt-5.6-sol", 0.136),
            ("gpt-5.6-terra", 0.068),
            ("gpt-5.6-luna", 0.0272),
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
}
