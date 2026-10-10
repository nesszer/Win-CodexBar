use chrono::NaiveDate;

use super::super::{codex_routed_pricing, models_dev_pricing};
use super::{CODEX_PRICING, CodexPricing, CostUsagePricing};

pub(super) const CODEX_LONG_CONTEXT_THRESHOLD: u64 = 272_000;

/// GPT-5.6 rates per token in upstream `gpt56Pricing` order:
/// (input, cache read, cache write, output).
pub(super) type Gpt56Rates = (f64, f64, f64, f64);

/// Upstream `gpt56Pricing`: standard rates plus the whole-request rates above
/// the 272K-token long-context threshold, each with its own cache-write rate.
pub(super) const fn gpt56_pricing(standard: Gpt56Rates, long_context: Gpt56Rates) -> CodexPricing {
    let (input, cache_read, cache_write, output) = standard;
    let (long_input, long_cache_read, long_cache_write, long_output) = long_context;
    CodexPricing::new(input, output, cache_read)
        .with_cache_write(cache_write)
        .with_long_context(
            long_input,
            long_output,
            long_cache_read,
            Some(long_cache_write),
        )
}

/// Upstream `codexHistoricalPricing`: the rates a model billed at before its
/// repricing. GPT-5.6 Terra and Luna were cut on 2026-07-30 (Unix 1785369600)
/// and Sol on 2026-08-21 (Unix 1787270400). Windows keys usage by calendar
/// day, so the cutoff compares days rather than event instants.
pub(super) fn codex_historical_pricing(key: &str, pricing_date: NaiveDate) -> Option<CodexPricing> {
    let ((year, month, day), standard, long_context) = match key {
        "gpt-5.6-sol" => (
            (2026, 8, 21),
            (5e-6, 5e-7, 6.25e-6, 3e-5),
            (1e-5, 1e-6, 1.25e-5, 4.5e-5),
        ),
        "gpt-5.6-terra" => (
            (2026, 7, 30),
            (2.5e-6, 2.5e-7, 3.125e-6, 1.5e-5),
            (5e-6, 5e-7, 6.25e-6, 2.25e-5),
        ),
        "gpt-5.6-luna" => (
            (2026, 7, 30),
            (1e-6, 1e-7, 1.25e-6, 6e-6),
            (2e-6, 2e-7, 2.5e-6, 9e-6),
        ),
        _ => return None,
    };
    let cutoff = NaiveDate::from_ymd_opt(year, month, day)?;
    (pricing_date < cutoff).then(|| gpt56_pricing(standard, long_context))
}

/// Upstream `codexCostUSD(pricing:)` for one bundled entry. `input_tokens` is
/// the inclusive prompt size and selects the long-context tier. A long-context
/// cache write without its own rate falls back to the standard cache-write
/// rate, then to the tier's input rate.
pub(super) fn codex_cost_from_pricing(
    pricing: &CodexPricing,
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_input_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let long_context = pricing
        .long_context
        .filter(|_| input_tokens > CODEX_LONG_CONTEXT_THRESHOLD);
    let (input_rate, cache_read_rate, cache_write_rate, output_rate) = match long_context {
        Some(long) => (
            long.input_cost_per_token,
            long.cache_read_input_cost_per_token,
            long.cache_write_input_cost_per_token
                .or(pricing.cache_write_input_cost_per_token)
                .unwrap_or(long.input_cost_per_token),
            long.output_cost_per_token,
        ),
        None => (
            pricing.input_cost_per_token,
            pricing.cache_read_input_cost_per_token,
            pricing
                .cache_write_input_cost_per_token
                .unwrap_or(pricing.input_cost_per_token),
            pricing.output_cost_per_token,
        ),
    };
    codex_cost_from_rates_with_cache_write(
        input_tokens,
        cached_input_tokens,
        cache_write_input_tokens,
        output_tokens,
        input_rate,
        cache_read_rate,
        cache_write_rate,
        output_rate,
    )
}

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
pub(super) fn codex_cost_from_rates_with_cache_write(
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
    /// Whether one request of `input_tokens` can run in the Fast lane of
    /// `model`. Older models offer no Fast lane above the long-context
    /// threshold, so upstream charges such a Priority request the Standard
    /// cost; Astra publishes long-context Fast rates.
    pub fn codex_fast_lane_covers(model: &str, input_tokens: u64) -> bool {
        Self::codex_api_fast_multiplier(model).is_some()
            && (input_tokens <= CODEX_LONG_CONTEXT_THRESHOLD
                || codex_fast_allows_long_context(model))
    }

    /// Fast cost in USD of a day aggregate under a Fast key (`-priority` or
    /// `-fast`), or `None` when `model` names no Fast lane.
    ///
    /// Upstream prices every request on its own. A day aggregate sums
    /// requests that each ran in the Fast lane, so the summed input must
    /// neither refuse the surcharge nor switch to long-context rates: older
    /// models price at the base model's short-context rates times the
    /// multiplier. Astra's Fast lane has long-context rates, so its
    /// aggregate keeps the whole-aggregate rule that Standard aggregates use.
    pub fn codex_fast_aggregate_cost_usd(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
        pricing_date: Option<NaiveDate>,
    ) -> Option<f64> {
        let base = Self::codex_fast_base_model(model);
        if base == Self::normalize_codex_model(model) {
            return None;
        }
        if codex_fast_allows_long_context(model) {
            return pricing_date
                .and_then(|date| {
                    Self::codex_fast_cost_usd_at_date(
                        model,
                        input_tokens,
                        cached_input_tokens,
                        output_tokens,
                        date,
                    )
                })
                .or_else(|| {
                    Self::codex_fast_cost_usd(
                        model,
                        input_tokens,
                        cached_input_tokens,
                        output_tokens,
                    )
                });
        }
        let multiplier = Self::codex_api_fast_multiplier(model)?;
        let (input_rate, cache_read_rate, output_rate) =
            Self::codex_short_context_rates(&base, pricing_date)?;
        Some(
            codex_cost_from_rates(
                input_tokens,
                cached_input_tokens,
                output_tokens,
                input_rate,
                cache_read_rate,
                output_rate,
            ) * multiplier,
        )
    }

    /// Known cost in USD of one Codex day aggregate: a Fast key prices
    /// through its base model's Fast lane
    /// ([`Self::codex_fast_aggregate_cost_usd`]), any other model at the
    /// rates in effect on `pricing_date`. `None` means no rate is known.
    pub fn codex_day_aggregate_cost_usd(
        model: &str,
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
        pricing_date: Option<NaiveDate>,
    ) -> Option<f64> {
        let (input, cached, output) = (input_tokens, cached_input_tokens, output_tokens);
        Self::codex_fast_aggregate_cost_usd(model, input, cached, output, pricing_date)
            .or_else(|| {
                pricing_date.and_then(|date| {
                    Self::codex_cost_usd_at_date(model, input, cached, output, date)
                })
            })
            .or_else(|| Self::codex_cost_usd(model, input, cached, output))
    }

    /// Short-context `(input, cache read, output)` rates of `model` on
    /// `pricing_date`. Short-context pricing is linear per token, so
    /// one-token probes read the exact dated rates back.
    fn codex_short_context_rates(
        model: &str,
        pricing_date: Option<NaiveDate>,
    ) -> Option<(f64, f64, f64)> {
        let cost = |input, cached, output| {
            pricing_date
                .and_then(|date| Self::codex_cost_usd_at_date(model, input, cached, output, date))
                .or_else(|| Self::codex_cost_usd(model, input, cached, output))
        };
        Some((cost(1, 0, 0)?, cost(1, 1, 0)?, cost(0, 0, 1)?))
    }

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

    pub(super) fn codex_cost_usd_with_cache_write_and_pricing_snapshot(
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
            return Some(codex_cost_from_pricing(
                pricing,
                input_tokens,
                cached_input_tokens,
                cache_write_input_tokens,
                output_tokens,
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
        Some(Self::models_dev_cost_usd(
            &pricing,
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
        ))
    }

    /// Upstream `codexCostUSD(pricing:)` for one models.dev entry.
    /// `input_tokens` is the inclusive prompt size (cache reads and writes are
    /// subsets of it) and also selects the long-context tier. A cache lane
    /// without its own rate falls back to the tier's input rate.
    pub(crate) fn models_dev_cost_usd(
        pricing: &models_dev_pricing::DynamicModelPricing,
        input_tokens: u64,
        cached_input_tokens: u64,
        cache_write_input_tokens: u64,
        output_tokens: u64,
    ) -> f64 {
        let long = pricing
            .threshold_tokens
            .is_some_and(|threshold| input_tokens > threshold);
        let above = |rate: Option<f64>| rate.filter(|_| long);
        let input_rate = above(pricing.input_cost_per_token_above_threshold)
            .unwrap_or(pricing.input_cost_per_token);
        let output_rate = above(pricing.output_cost_per_token_above_threshold)
            .unwrap_or(pricing.output_cost_per_token);
        let cache_read_rate = above(pricing.cache_read_input_cost_per_token_above_threshold)
            .or(pricing.cache_read_input_cost_per_token)
            .unwrap_or(input_rate);
        let cache_write_rate = above(pricing.cache_write_input_cost_per_token_above_threshold)
            .or(pricing.cache_write_input_cost_per_token)
            .unwrap_or(input_rate);
        codex_cost_from_rates_with_cache_write(
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
            input_rate,
            cache_read_rate,
            cache_write_rate,
            output_rate,
        )
    }
}
