use crate::core::{ClaudePricingResolution, CostUsagePricing, ModelsDevPricingSnapshot};
use std::collections::HashMap;

pub(super) const FALLBACK_CLAUDE_MODEL: &str = "claude-sonnet-4-6";

/// Per-scan Claude pricing memo.
///
/// Claude logs commonly repeat the same model across many files and records. Keep model
/// normalization and positive/negative models.dev resolution scan-local while retaining the
/// canonical pricing arithmetic and provider-routing rules.
#[derive(Default)]
pub(super) struct ClaudeScanPricingResolver {
    snapshot: Option<ModelsDevPricingSnapshot>,
    pub(super) normalized_models: HashMap<String, String>,
    pub(super) resolutions: HashMap<String, Option<ClaudePricingResolution>>,
    #[cfg(test)]
    pub(super) normalization_cache_misses: usize,
    #[cfg(test)]
    pub(super) resolution_cache_misses: usize,
}

impl ClaudeScanPricingResolver {
    pub(super) const MEMO_ENTRY_LIMIT: usize = 1024;

    #[cfg(test)]
    pub(super) fn with_snapshot(snapshot: ModelsDevPricingSnapshot) -> Self {
        Self {
            snapshot: Some(snapshot),
            ..Self::default()
        }
    }

    pub(super) fn normalize(&mut self, model: &str) -> String {
        if let Some(normalized) = self.normalized_models.get(model) {
            return normalized.clone();
        }
        #[cfg(test)]
        {
            self.normalization_cache_misses += 1;
        }
        let normalized = CostUsagePricing::normalize_claude_model(model);
        if self.normalized_models.len() < Self::MEMO_ENTRY_LIMIT {
            self.normalized_models
                .insert(model.to_string(), normalized.clone());
        }
        normalized
    }

    fn resolve(&mut self, model: &str) -> Option<ClaudePricingResolution> {
        if let Some(resolution) = self.resolutions.get(model) {
            return *resolution;
        }
        #[cfg(test)]
        {
            self.resolution_cache_misses += 1;
        }

        let normalized = self.normalize(model);
        let needs_catalog = self.snapshot.is_none();
        let mut resolution =
            CostUsagePricing::resolve_claude_pricing(model, &normalized, self.snapshot.as_ref());
        if resolution.is_none() && needs_catalog {
            let snapshot = self
                .snapshot
                .get_or_insert_with(crate::core::pricing_snapshot);
            resolution =
                CostUsagePricing::resolve_claude_pricing(model, &normalized, Some(snapshot));
        }
        if self.resolutions.len() < Self::MEMO_ENTRY_LIMIT {
            self.resolutions.insert(model.to_string(), resolution);
        }
        resolution
    }

    pub(super) fn is_known(&mut self, model: &str) -> bool {
        self.resolve(model).is_some()
    }

    pub(super) fn cost_usd_with_cache_ttl(
        &mut self,
        model: &str,
        input: u64,
        cache_create: u64,
        cache_create_1h: u64,
        cache_read: u64,
        output: u64,
    ) -> f64 {
        let cache_create_1h = cache_create_1h.min(cache_create);
        let cache_create_5m = cache_create.saturating_sub(cache_create_1h);

        let resolved = self.resolve(model);
        let billable = resolved.or_else(|| self.resolve(FALLBACK_CLAUDE_MODEL));
        let base = billable
            .map(|pricing| {
                CostUsagePricing::claude_cost_usd_u64_from_resolution(
                    pricing,
                    input,
                    cache_read,
                    cache_create_5m,
                    output,
                )
            })
            .unwrap_or(0.0);
        let input_rate = billable
            .map(CostUsagePricing::claude_input_cost_per_token_from_resolution)
            .unwrap_or(0.0);

        base + (cache_create_1h as f64) * input_rate * 2.0
    }
}
