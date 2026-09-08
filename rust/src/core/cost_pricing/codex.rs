use super::super::{codex_routed_pricing, models_dev_pricing};
use super::{CODEX_PRICING, CostUsagePricing};

pub(super) const CODEX_LONG_CONTEXT_THRESHOLD: u64 = 272_000;
const CODEX_ASTRA_CACHE_WRITE_RATE: f64 = 1.25e-5;
const CODEX_ASTRA_LONG_CACHE_WRITE_RATE: f64 = 2.5e-5;

pub(super) fn codex_cost_from_rates(
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    input_rate: f64,
    cache_read_rate: f64,
    output_rate: f64,
) -> f64 {
    let cached = cached_input_tokens.min(input_tokens);
    let non_cached = input_tokens.saturating_sub(cached);
    (non_cached as f64) * input_rate
        + (cached as f64) * cache_read_rate
        + (output_tokens as f64) * output_rate
}

#[allow(
    clippy::too_many_arguments,
    reason = "Arguments mirror independent token classes and their corresponding pricing rates."
)]
fn codex_cost_from_rates_with_cache_write(
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    output_tokens: u64,
    input_rate: f64,
    cache_read_rate: f64,
    cache_write_rate: f64,
    output_rate: f64,
) -> f64 {
    if cache_write_input_tokens == 0 {
        return codex_cost_from_rates(
            input_tokens,
            cached_input_tokens,
            output_tokens,
            input_rate,
            cache_read_rate,
            output_rate,
        );
    }

    let cached = cached_input_tokens.min(input_tokens);
    let remaining_input = input_tokens.saturating_sub(cached);
    let cache_write = cache_write_input_tokens.min(remaining_input);
    let non_cached = remaining_input.saturating_sub(cache_write);
    (non_cached as f64) * input_rate
        + (cached as f64) * cache_read_rate
        + (cache_write as f64) * cache_write_rate
        + (output_tokens as f64) * output_rate
}

pub(super) fn codex_fast_allows_long_context(model: &str) -> bool {
    CostUsagePricing::codex_fast_base_model(model) == "gpt-6-astra"
}

impl CostUsagePricing {
    /// Calculate Codex cost in USD when input includes cache-write tokens.
    pub fn codex_cost_usd_with_cache_write(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        cache_write_input_tokens: u64,
        output_tokens: u64,
    ) -> Option<f64> {
        Self::codex_cost_usd_with_cache_write_and_pricing_snapshot(
            model,
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
            None,
        )
    }

    pub fn codex_cost_usd_with_pricing_snapshot(
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

    fn codex_cost_usd_with_cache_write_and_pricing_snapshot(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        cache_write_input_tokens: u64,
        output_tokens: u64,
        pricing_snapshot: Option<&models_dev_pricing::ModelsDevPricingSnapshot>,
    ) -> Option<f64> {
        let key = Self::normalize_codex_model(model);
        // Model-less / deliberately unattributed usage stays unpriced even if a
        // pricing catalog later contains a colliding generic entry.
        if key == Self::CODEX_UNATTRIBUTED_MODEL {
            return None;
        }
        if let Some(pricing) = CODEX_PRICING.get(key.as_str()) {
            let long = input_tokens > CODEX_LONG_CONTEXT_THRESHOLD;
            let (input_rate, cache_read_rate, output_rate) = if long {
                if let Some(long_context) = pricing.long_context {
                    (
                        long_context.input_cost_per_token,
                        long_context.cache_read_input_cost_per_token,
                        long_context.output_cost_per_token,
                    )
                } else {
                    (
                        pricing.input_cost_per_token,
                        pricing.cache_read_input_cost_per_token,
                        pricing.output_cost_per_token,
                    )
                }
            } else {
                (
                    pricing.input_cost_per_token,
                    pricing.cache_read_input_cost_per_token,
                    pricing.output_cost_per_token,
                )
            };
            let cache_write_rate = if key == "gpt-6-astra" {
                if long {
                    CODEX_ASTRA_LONG_CACHE_WRITE_RATE
                } else {
                    CODEX_ASTRA_CACHE_WRITE_RATE
                }
            } else {
                input_rate
            };
            return Some(codex_cost_from_rates_with_cache_write(
                input_tokens,
                cached_input_tokens,
                cache_write_input_tokens,
                output_tokens,
                input_rate,
                cache_read_rate,
                cache_write_rate,
                output_rate,
            ));
        }

        // Upstream 0.50.1 #2946: provider-qualified routed models are priced
        // against the matching models.dev provider, not OpenAI. Unknown
        // `provider/` prefixes are left unpriced (not guessed as OpenAI).
        let (provider_id, lookup_model) = match codex_routed_pricing::codex_routed_provider(model) {
            Some(routed) => (routed, codex_routed_pricing::strip_route_prefix(model)),
            None if model.trim().contains('/') && !model.trim().starts_with("openai/") => {
                // Unknown route prefix — do not guess. Leave unpriced.
                return None;
            }
            None => ("openai", model),
        };
        let pricing = match pricing_snapshot {
            Some(snapshot) => snapshot.lookup(provider_id, lookup_model),
            None => models_dev_pricing::lookup(provider_id, lookup_model),
        }?;
        let use_tier = pricing
            .threshold_tokens
            .is_some_and(|threshold| input_tokens > threshold);
        let input_rate = if use_tier {
            pricing
                .input_cost_per_token_above_threshold
                .unwrap_or(pricing.input_cost_per_token)
        } else {
            pricing.input_cost_per_token
        };
        let cache_read_rate = if use_tier {
            pricing
                .cache_read_input_cost_per_token_above_threshold
                .or(pricing.cache_read_input_cost_per_token)
                .unwrap_or(pricing.input_cost_per_token)
        } else {
            pricing
                .cache_read_input_cost_per_token
                .unwrap_or(pricing.input_cost_per_token)
        };
        let output_rate = if use_tier {
            pricing
                .output_cost_per_token_above_threshold
                .unwrap_or(pricing.output_cost_per_token)
        } else {
            pricing.output_cost_per_token
        };
        Some(codex_cost_from_rates_with_cache_write(
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
            input_rate,
            cache_read_rate,
            input_rate,
            output_rate,
        ))
    }
}
