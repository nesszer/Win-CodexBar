//! `custom-pricing.json` overrides for local spend (upstream `CostUsageCustomPricing`).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::cost_scanner::ModelTokenCounts;

#[derive(Debug, Clone, Default)]
pub(super) struct CustomPricing {
    pub(super) entries: HashMap<String, CustomRates>,
}

/// One `custom-pricing.json` entry, in USD per million tokens. `Some(0.0)` is
/// free; `None` is unknown and is never filled from another source.
#[derive(Debug, Clone, Default)]
pub(super) struct CustomRates {
    pub(super) input: Option<f64>,
    pub(super) output: Option<f64>,
    pub(super) cache_read: Option<f64>,
    pub(super) cache_write: Option<f64>,
}

impl CustomPricing {
    pub(super) fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|path| path.join("CodexBar").join("custom-pricing.json"))
    }

    pub(super) fn load() -> Self {
        Self::default_path()
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| Self::parse(&bytes))
            .unwrap_or_default()
    }

    /// Upstream `CostUsageCustomPricing.parse`: keys are trimmed and
    /// lowercased, and every entry is read on its own, so one malformed entry
    /// never discards the others. An entry without a single usable rate is
    /// dropped. A document that is not a JSON object is empty.
    pub(super) fn parse(bytes: &[u8]) -> Self {
        let Ok(serde_json::Value::Object(object)) = serde_json::from_slice(bytes) else {
            return Self::default();
        };
        Self {
            entries: object
                .into_iter()
                .filter_map(|(key, value)| {
                    let key = key.trim().to_ascii_lowercase();
                    if key.is_empty() {
                        return None;
                    }
                    CustomRates::from_value(&value).map(|rates| (key, rates))
                })
                .collect(),
        }
    }

    pub(super) fn rates(&self, provider_id: &str, model: &str) -> Option<&CustomRates> {
        let model_key = model.trim().to_ascii_lowercase();
        let provider_key = format!("{}/{}", provider_id.trim().to_ascii_lowercase(), model_key);
        self.entries
            .get(&provider_key)
            .or_else(|| self.entries.get(&model_key))
    }

    /// Upstream `CostUsageCustomPricing.rates(providerID:model:)` for imported
    /// ledgers: the bare model key first, then `provider/model`. An empty model
    /// has no override.
    pub(super) fn overlay_rates(&self, provider_id: &str, model: &str) -> Option<&CustomRates> {
        let model_key = model.trim().to_ascii_lowercase();
        if model_key.is_empty() {
            return None;
        }
        self.entries.get(&model_key).or_else(|| {
            let provider_key = format!("{}/{}", provider_id.trim(), model.trim());
            self.entries.get(&provider_key.to_ascii_lowercase())
        })
    }
}

impl CustomRates {
    /// Upstream `rates(from:)`: a rate that is missing, not a number,
    /// negative or non-finite is unknown, and the camelCase spelling wins over
    /// the snake_case one. `None` when the entry has no usable rate at all.
    pub(super) fn from_value(value: &serde_json::Value) -> Option<Self> {
        let object = value.as_object()?;
        let rate = |key: &str| {
            object
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .filter(|rate| rate.is_finite() && *rate >= 0.0)
        };
        let rates = Self {
            input: rate("input"),
            output: rate("output"),
            cache_read: rate("cacheRead").or_else(|| rate("cache_read")),
            cache_write: rate("cacheWrite")
                .or_else(|| rate("cache_write"))
                .or_else(|| rate("cacheCreation"))
                .or_else(|| rate("cache_creation")),
        };
        [
            rates.input,
            rates.output,
            rates.cache_read,
            rates.cache_write,
        ]
        .iter()
        .any(Option::is_some)
        .then_some(rates)
    }

    pub(super) fn cost(&self, counts: &ModelTokenCounts) -> Option<f64> {
        self.cost_parts(
            counts.input_tokens,
            counts.output_tokens,
            counts.cached_tokens,
            0,
        )
    }

    pub(super) fn cost_parts(
        &self,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> Option<f64> {
        let cached = cache_read.min(input);
        self.lane_cost(input.saturating_sub(cached), output, cached, cache_write)
    }

    /// Upstream `CostUsageCustomPricing.costUSD(rates:...)`: every token lane is
    /// billed on its own, and a lane with tokens but no rate leaves the cost
    /// unknown. A missing rate is never filled from another source.
    pub(super) fn lane_cost(
        &self,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
    ) -> Option<f64> {
        let mut total = 0.0;
        if input > 0 {
            total += input as f64 * self.input? / 1_000_000.0;
        }
        if output > 0 {
            total += output as f64 * self.output? / 1_000_000.0;
        }
        if cache_read > 0 {
            total += cache_read as f64 * self.cache_read? / 1_000_000.0;
        }
        if cache_write > 0 {
            total += cache_write as f64 * self.cache_write? / 1_000_000.0;
        }
        total.is_finite().then_some(total)
    }
}
