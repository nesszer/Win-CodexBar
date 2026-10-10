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
    /// `None` falls back to the standard cache-write rate, then to this
    /// tier's input rate.
    pub cache_write_input_cost_per_token: Option<f64>,
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
    /// Cost per cache-write input token in USD; `None` bills cache writes at
    /// the input rate.
    pub cache_write_input_cost_per_token: Option<f64>,
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

impl CodexPricing {
    /// Standard per-token rates; cache writes bill at the input rate and there
    /// is no long-context tier.
    const fn new(input: f64, output: f64, cache_read: f64) -> Self {
        Self {
            input_cost_per_token: input,
            output_cost_per_token: output,
            cache_read_input_cost_per_token: cache_read,
            cache_write_input_cost_per_token: None,
            display_label: None,
            long_context: None,
        }
    }

    const fn with_cache_write(mut self, cache_write: f64) -> Self {
        self.cache_write_input_cost_per_token = Some(cache_write);
        self
    }

    const fn with_label(mut self, label: &'static str) -> Self {
        self.display_label = Some(label);
        self
    }

    const fn with_long_context(
        mut self,
        input: f64,
        output: f64,
        cache_read: f64,
        cache_write: Option<f64>,
    ) -> Self {
        self.long_context = Some(CodexLongContextRates {
            input_cost_per_token: input,
            output_cost_per_token: output,
            cache_read_input_cost_per_token: cache_read,
            cache_write_input_cost_per_token: cache_write,
        });
        self
    }
}

impl ClaudePricing {
    /// Untiered per-token rates.
    const fn new(input: f64, output: f64, cache_creation: f64, cache_read: f64) -> Self {
        Self {
            input_cost_per_token: input,
            output_cost_per_token: output,
            cache_creation_input_cost_per_token: cache_creation,
            cache_read_input_cost_per_token: cache_read,
            threshold_tokens: None,
            input_cost_per_token_above_threshold: None,
            output_cost_per_token_above_threshold: None,
            cache_creation_input_cost_per_token_above_threshold: None,
            cache_read_input_cost_per_token_above_threshold: None,
        }
    }

    /// Rates for requests above 200k tokens, in the same order as `new`.
    const fn with_200k_tier(
        mut self,
        input: f64,
        output: f64,
        cache_creation: f64,
        cache_read: f64,
    ) -> Self {
        self.threshold_tokens = Some(200_000);
        self.input_cost_per_token_above_threshold = Some(input);
        self.output_cost_per_token_above_threshold = Some(output);
        self.cache_creation_input_cost_per_token_above_threshold = Some(cache_creation);
        self.cache_read_input_cost_per_token_above_threshold = Some(cache_read);
        self
    }
}

const GPT_5: CodexPricing = CodexPricing::new(1.25e-6, 1e-5, 1.25e-7);
const GPT_5_MINI: CodexPricing = CodexPricing::new(2.5e-7, 2e-6, 2.5e-8);
const GPT_5_2: CodexPricing = CodexPricing::new(1.75e-6, 1.4e-5, 1.75e-7);
// GPT-5.4 pricing (updated to match upstream 0.22). Like upstream, the
// whole request bills 2x input / 1.5x output above 272K input tokens.
const GPT_5_4: CodexPricing =
    CodexPricing::new(2.5e-6, 1.5e-5, 2.5e-7).with_long_context(5e-6, 2.25e-5, 5e-7, None);
// GPT-5.4 Mini and Nano pricing (updated to match upstream 0.22)
const GPT_5_4_MINI: CodexPricing = CodexPricing::new(7.5e-7, 4.5e-6, 7.5e-8);
const GPT_5_4_NANO: CodexPricing = CodexPricing::new(2e-7, 1.25e-6, 2e-8);
const GPT_5_PRO_TIER: CodexPricing = CodexPricing::new(3e-5, 1.8e-4, 3e-5);
const GPT_CYBER: CodexPricing = CodexPricing::new(1.25e-5, 7.5e-5, 1.25e-6);

/// Codex model pricing table
static CODEX_PRICING: LazyLock<HashMap<&'static str, CodexPricing>> = LazyLock::new(|| {
    HashMap::from([
        ("gpt-5", GPT_5),
        ("gpt-5-codex", GPT_5),
        ("gpt-5-mini", GPT_5_MINI),
        ("gpt-5-nano", CodexPricing::new(5e-8, 4e-7, 5e-9)),
        ("gpt-5-pro", CodexPricing::new(1.5e-5, 1.2e-4, 1.5e-5)),
        ("gpt-5.1", GPT_5),
        ("gpt-5.1-codex", GPT_5),
        ("gpt-5.1-codex-max", GPT_5),
        ("gpt-5.1-codex-mini", GPT_5_MINI),
        ("gpt-5.2", GPT_5_2),
        ("gpt-5.2-codex", GPT_5_2),
        ("gpt-5.2-pro", CodexPricing::new(2.1e-5, 1.68e-4, 2.1e-5)),
        ("gpt-5.3-codex", GPT_5_2),
        (
            "gpt-5.3-codex-spark",
            CodexPricing::new(0.0, 0.0, 0.0).with_label("Research Preview"),
        ),
        ("gpt-5.4", GPT_5_4),
        ("gpt-5.4-codex", GPT_5_4),
        ("gpt-5.4-mini", GPT_5_4_MINI),
        ("gpt-5.4-mini-codex", GPT_5_4_MINI),
        ("gpt-5.4-nano", GPT_5_4_NANO),
        ("gpt-5.4-nano-codex", GPT_5_4_NANO),
        ("gpt-5.4-pro", GPT_5_PRO_TIER),
        (
            "gpt-5.5",
            CodexPricing::new(5e-6, 3e-5, 5e-7).with_long_context(1e-5, 4.5e-5, 1e-6, None),
        ),
        ("gpt-5.5-pro", GPT_5_PRO_TIER),
        // GPT-5.6 Sol/Terra/Luna (OpenAI pricing page and model cards), in
        // upstream `gpt56Pricing` order (input, cache read, cache write, output).
        // Above 272K input tokens the whole request bills 2x input / 1.5x output;
        // cache writes bill at 1.25x uncached input. Sol was repriced from $5/$30
        // to $4/$20 on 2026-08-21. Dated usage before a model's repricing keeps
        // the rates in `codex_pricing::codex_historical_pricing`.
        (
            "gpt-5.6-sol",
            codex_pricing::gpt56_pricing((4e-6, 4e-7, 5e-6, 2e-5), (8e-6, 8e-7, 1e-5, 3e-5)),
        ),
        (
            "gpt-5.6-terra",
            codex_pricing::gpt56_pricing((2e-6, 2e-7, 2.5e-6, 1.2e-5), (4e-6, 4e-7, 5e-6, 1.8e-5)),
        ),
        (
            "gpt-5.6-luna",
            codex_pricing::gpt56_pricing((2e-7, 2e-8, 2.5e-7, 1.2e-6), (4e-7, 4e-8, 5e-7, 1.8e-6)),
        ),
        // Daybreak Cyber models (OpenAI pricing page). No long-context tier is
        // published, and gpt-5.5-cyber lists no cache-write rate, so its writes
        // bill at the input rate.
        ("gpt-5.6-cyber", GPT_CYBER.with_cache_write(1.5625e-5)),
        ("gpt-5.5-cyber", GPT_CYBER),
        // GPT-6 Astra pricing (OpenAI model card and pricing table).
        // Long-context rates apply to the whole request above 272K input tokens;
        // cache writes bill at 1.25x uncached input in both tiers.
        (
            "gpt-6-astra",
            CodexPricing::new(1e-5, 5e-5, 1e-6)
                .with_cache_write(1.25e-5)
                .with_long_context(2e-5, 7.5e-5, 2e-6, Some(2.5e-5)),
        ),
    ])
});

const CLAUDE_HAIKU_4_5: ClaudePricing = ClaudePricing::new(1e-6, 5e-6, 1.25e-6, 1e-7);
// Opus 4.5 through 4.8 share one price.
const CLAUDE_OPUS_4_5: ClaudePricing = ClaudePricing::new(5e-6, 2.5e-5, 6.25e-6, 5e-7);
// Sonnet 4 through 4.6 share one price, with tiered pricing at 200k tokens.
const CLAUDE_SONNET_4: ClaudePricing =
    ClaudePricing::new(3e-6, 1.5e-5, 3.75e-6, 3e-7).with_200k_tier(6e-6, 2.25e-5, 7.5e-6, 6e-7);
const CLAUDE_OPUS_4: ClaudePricing = ClaudePricing::new(1.5e-5, 7.5e-5, 1.875e-5, 1.5e-6);

/// Claude model pricing table
static CLAUDE_PRICING: LazyLock<HashMap<&'static str, ClaudePricing>> = LazyLock::new(|| {
    HashMap::from([
        (
            "claude-fable-5",
            ClaudePricing::new(1e-5, 5e-5, 1.25e-5, 1e-6),
        ),
        ("claude-haiku-4-5", CLAUDE_HAIKU_4_5),
        ("claude-haiku-4-5-20251001", CLAUDE_HAIKU_4_5),
        ("claude-opus-4-6", CLAUDE_OPUS_4_5),
        ("claude-opus-4-6-20260205", CLAUDE_OPUS_4_5),
        ("claude-opus-4-7", CLAUDE_OPUS_4_5),
        ("claude-opus-4-8", CLAUDE_OPUS_4_5),
        ("claude-opus-4-5", CLAUDE_OPUS_4_5),
        ("claude-opus-4-5-20251101", CLAUDE_OPUS_4_5),
        ("claude-sonnet-4-5", CLAUDE_SONNET_4),
        ("claude-sonnet-4-5-20250929", CLAUDE_SONNET_4),
        ("claude-sonnet-4-6", CLAUDE_SONNET_4),
        ("claude-opus-4-20250514", CLAUDE_OPUS_4),
        ("claude-opus-4-1", CLAUDE_OPUS_4),
        ("claude-sonnet-4-20250514", CLAUDE_SONNET_4),
    ])
});

/// Cost usage pricing utilities
pub struct CostUsagePricing;

pub(crate) fn bundled_codex_long_context_threshold(model: &str) -> Option<u64> {
    claude_pricing::bundled_codex_long_context_threshold(model)
}

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

        // Remove the provider-qualified OpenAI prefix.
        if let Some((prefix, rest)) = trimmed.split_once('/')
            && prefix.eq_ignore_ascii_case("openai")
        {
            trimmed = rest.to_string();
        }

        // OpenAI's Daybreak aliases currently point to Sol (blue) and Cyber
        // (red). https://developers.openai.com/api/docs/pricing
        match trimmed.as_str() {
            "gpt-daybreak-blue-latest" => return "gpt-5.6-sol".to_string(),
            "gpt-daybreak-red-latest" => return "gpt-5.6-cyber".to_string(),
            _ => {}
        }

        // Codex uses this alias for the Luna reserve quota bucket (#714).
        if trimmed.eq_ignore_ascii_case("gpt-reserve") {
            return "gpt-5.6-luna".to_string();
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
    pub fn codex_fast_cost_usd(model: &str, input: u64, cached: u64, output: u64) -> Option<f64> {
        let multiplier = Self::codex_api_fast_multiplier(model)?;
        // Older models do not offer Fast for long-context requests. Astra
        // publishes a Fast rate for the same whole-request long-context tier.
        if input > codex_pricing::CODEX_LONG_CONTEXT_THRESHOLD
            && !codex_pricing::codex_fast_allows_long_context(model)
        {
            return None;
        }
        let base = Self::codex_fast_base_model(model);
        let base_cost = Self::codex_cost_usd(&base, input, cached, output)?;
        Some(base_cost * multiplier)
    }

    /// Calculate Codex cost using the rates in effect on a historical usage day.
    /// GPT-5.6 Terra/Luna were cut on 2026-07-30 and Sol on 2026-08-21.
    pub fn codex_cost_usd_at_date(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
        pricing_date: NaiveDate,
    ) -> Option<f64> {
        Self::codex_cost_usd_at_date_with_cache_write_and_pricing_snapshot(
            model,
            input_tokens,
            cached_input_tokens,
            0,
            output_tokens,
            pricing_date,
            None,
        )
    }

    /// Codex cost on a historical usage day when the prompt also wrote cache
    /// tokens. `input_tokens` is the inclusive prompt size: cache reads and
    /// writes are subsets of it. A model's pre-repricing rates (upstream
    /// `codexHistoricalPricing`) win over today's bundled and catalog rates.
    pub fn codex_cost_usd_at_date_with_cache_write_and_pricing_snapshot(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        cache_write_input_tokens: u64,
        output_tokens: u64,
        pricing_date: NaiveDate,
        pricing_snapshot: Option<&models_dev_pricing::ModelsDevPricingSnapshot>,
    ) -> Option<f64> {
        let key = Self::normalize_codex_model(model);
        if let Some(pricing) = codex_pricing::codex_historical_pricing(&key, pricing_date) {
            return Some(codex_pricing::codex_cost_from_pricing(
                &pricing,
                input_tokens,
                cached_input_tokens,
                cache_write_input_tokens,
                output_tokens,
            ));
        }
        Self::codex_cost_usd_with_cache_write_and_pricing_snapshot(
            model,
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
            pricing_snapshot,
        )
    }

    /// True when the bundled Codex table prices `model`. Windows resolves the
    /// bundled rates before any models.dev entry, so such a model never needs
    /// a catalog refresh.
    pub fn has_bundled_codex_pricing(model: &str) -> bool {
        CODEX_PRICING.contains_key(Self::normalize_codex_model(model).as_str())
    }

    pub fn codex_fast_cost_usd_at_date(
        model: &str,
        input: u64,
        cached: u64,
        output: u64,
        pricing_date: NaiveDate,
    ) -> Option<f64> {
        let multiplier = Self::codex_api_fast_multiplier(model)?;
        if input > codex_pricing::CODEX_LONG_CONTEXT_THRESHOLD
            && !codex_pricing::codex_fast_allows_long_context(model)
        {
            return None;
        }
        let base = Self::codex_fast_base_model(model);
        let base_cost = Self::codex_cost_usd_at_date(&base, input, cached, output, pricing_date)?;
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
