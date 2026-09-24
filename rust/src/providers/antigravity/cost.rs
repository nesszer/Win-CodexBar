use crate::core::CostUsagePricing;

pub(super) fn estimate_cost_usd(
    model: Option<&str>,
    input: u64,
    cache_read: u64,
    cache_write: u64,
    output: u64,
) -> Option<f64> {
    let model = model.map(str::trim).filter(|value| !value.is_empty())?;
    let input = i32::try_from(input).ok()?;
    let cache_read = i32::try_from(cache_read).ok()?;
    let cache_write = i32::try_from(cache_write).ok()?;
    let output = i32::try_from(output).ok()?;
    let resolve = |candidate: &str| {
        CostUsagePricing::claude_cost_usd(candidate, input, cache_read, cache_write, output)
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
    };
    resolve(model).or_else(|| {
        ["-tiered", "-low", "-thinking"]
            .iter()
            .find_map(|suffix| model.strip_suffix(suffix))
            .filter(|base| !base.is_empty())
            .and_then(resolve)
    })
}

#[cfg(test)]
mod tests {
    use super::estimate_cost_usd;

    #[test]
    fn prices_known_models_and_provider_local_routing_variants() {
        let direct = estimate_cost_usd(Some("claude-sonnet-4-6"), 1_000, 200, 100, 500)
            .expect("known public price");
        let routed = estimate_cost_usd(Some("claude-sonnet-4-6-thinking"), 1_000, 200, 100, 500)
            .expect("routing suffix uses the base public price");
        assert!(direct > 0.0);
        assert_eq!(direct, routed);
    }

    #[test]
    fn unknown_or_oversized_pricing_inputs_fail_closed() {
        assert_eq!(estimate_cost_usd(Some("unknown"), 1, 2, 3, 4), None);
        assert_eq!(
            estimate_cost_usd(Some("claude-sonnet-4-6"), i32::MAX as u64 + 1, 0, 0, 0),
            None
        );
    }
}
