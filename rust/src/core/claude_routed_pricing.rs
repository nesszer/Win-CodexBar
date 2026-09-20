//! Claude routed-model pricing via models.dev (upstream 0.53).

use super::models_dev_pricing;

pub fn models_dev_target(model: &str, normalized: String) -> Option<(&'static str, String)> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((route, raw_model)) = trimmed.split_once('/') {
        if raw_model.trim().is_empty() {
            return None;
        }
        let provider = match route.to_ascii_lowercase().as_str() {
            "anthropic" => "anthropic",
            "openai" => "openai",
            "google" => "google",
            "moonshot" => "moonshot",
            "kimi-for-coding" => "kimi-for-coding",
            "minimax" => "minimax",
            "deepseek" => "deepseek",
            _ => return None,
        };
        return Some((provider, raw_model.trim().to_string()));
    }

    let lower = normalized.to_ascii_lowercase();
    let provider = if lower.starts_with("claude-") {
        "anthropic"
    } else if lower.starts_with("gpt-")
        || lower.starts_with("chatgpt-")
        || lower.starts_with("text-embedding-")
        || ["o1", "o3", "o4"]
            .iter()
            .any(|prefix| lower == *prefix || lower.starts_with(&format!("{prefix}-")))
    {
        "openai"
    } else if ["gemini-", "gemma-", "deep-research-", "veo-", "lyria-"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        "google"
    } else if lower == "kimi-for-coding"
        || lower == "k3"
        || lower == "k3[1m]"
        || lower.starts_with("k3-")
    {
        "kimi-for-coding"
    } else if lower.starts_with("kimi-") || lower.starts_with("moonshot-") {
        "moonshot"
    } else if lower.starts_with("minimax-") {
        "minimax"
    } else if lower.starts_with("deepseek-") {
        "deepseek"
    } else {
        "anthropic"
    };

    Some((provider, normalized))
}

fn models_dev_targets(model: &str, normalized: String) -> Vec<(&'static str, String)> {
    let Some(primary) = models_dev_target(model, normalized) else {
        return Vec::new();
    };
    let mut targets = vec![primary.clone()];
    if primary.0 == "kimi-for-coding" && primary.1.eq_ignore_ascii_case("k3[1m]") {
        targets.push(("kimi-for-coding", "k3".to_string()));
    }
    targets
}

/// Resolve a routed Claude model against one invocation-owned models.dev snapshot.
///
/// Keeping this separate from the arithmetic lets a local scan memoize both positive and
/// negative model resolution without changing the provider-routing rules.
pub fn resolve_with_snapshot(
    model: &str,
    normalized: &str,
    snapshot: &models_dev_pricing::ModelsDevPricingSnapshot,
) -> Option<(models_dev_pricing::DynamicModelPricing, Option<u64>)> {
    models_dev_targets(model, normalized.to_string())
        .into_iter()
        .find_map(|(provider, lookup_model)| {
            let pricing = snapshot.lookup(provider, &lookup_model)?;
            let threshold = effective_threshold(provider, &lookup_model, pricing.threshold_tokens);
            Some((pricing, threshold))
        })
}

pub fn cost_usd(
    model: &str,
    normalized: String,
    input: i32,
    cache_read: i32,
    cache_write: i32,
    output: i32,
) -> Option<f64> {
    let (provider, lookup_model, pricing) = models_dev_targets(model, normalized)
        .into_iter()
        .find_map(|(provider, lookup_model)| {
            let pricing = models_dev_pricing::lookup(provider, &lookup_model)?;
            Some((provider, lookup_model, pricing))
        })?;
    let threshold = effective_threshold(provider, &lookup_model, pricing.threshold_tokens);
    Some(if threshold == pricing.threshold_tokens {
        cost_usd_from_pricing(pricing, input, cache_read, cache_write, output)
    } else {
        cost_usd_from_pricing_with_threshold(
            pricing,
            threshold,
            input,
            cache_read,
            cache_write,
            output,
        )
    })
}

/// Calculate routed cost after the models.dev resolution has already been memoized.
pub fn cost_usd_from_pricing(
    pricing: models_dev_pricing::DynamicModelPricing,
    input: i32,
    cache_read: i32,
    cache_write: i32,
    output: i32,
) -> f64 {
    cost_usd_from_pricing_with_threshold(
        pricing,
        pricing.threshold_tokens,
        input,
        cache_read,
        cache_write,
        output,
    )
}

/// Calculate routed cost while optionally replacing the catalog's context
/// boundary. OpenAI GPT rows recorded by Claude Code use the bundled Codex
/// boundary, but retain the catalog's per-token rates.
pub fn cost_usd_from_pricing_with_threshold(
    pricing: models_dev_pricing::DynamicModelPricing,
    threshold_tokens: Option<u64>,
    input: i32,
    cache_read: i32,
    cache_write: i32,
    output: i32,
) -> f64 {
    let input = input.max(0);
    let cache_read = cache_read.max(0);
    let cache_write = cache_write.max(0);
    let output = output.max(0);
    let use_tier = threshold_tokens.is_some_and(|threshold| {
        (input as u64) + (cache_read as u64) + (cache_write as u64) > threshold
    });
    let pick = |base: f64, above: Option<f64>| {
        if use_tier {
            above.unwrap_or(base)
        } else {
            base
        }
    };
    let input_rate = pick(
        pricing.input_cost_per_token,
        pricing.input_cost_per_token_above_threshold,
    );
    let cache_read_rate = if use_tier {
        pricing
            .cache_read_input_cost_per_token_above_threshold
            .or(pricing.cache_read_input_cost_per_token)
            .unwrap_or(input_rate)
    } else {
        pricing
            .cache_read_input_cost_per_token
            .unwrap_or(input_rate)
    };
    let cache_write_rate = if use_tier {
        pricing
            .cache_write_input_cost_per_token_above_threshold
            .or(pricing.cache_write_input_cost_per_token)
            .unwrap_or(input_rate)
    } else {
        pricing
            .cache_write_input_cost_per_token
            .unwrap_or(input_rate)
    };
    let output_rate = pick(
        pricing.output_cost_per_token,
        pricing.output_cost_per_token_above_threshold,
    );

    (input as f64) * input_rate
        + (cache_read as f64) * cache_read_rate
        + (cache_write as f64) * cache_write_rate
        + (output as f64) * output_rate
}

fn effective_threshold(provider: &str, model: &str, catalog_threshold: Option<u64>) -> Option<u64> {
    (provider == "openai")
        .then(|| super::cost_pricing::bundled_codex_long_context_threshold(model))
        .flatten()
        .or(catalog_threshold)
}

pub fn input_cost_per_token(model: &str, normalized: String) -> Option<f64> {
    models_dev_targets(model, normalized)
        .into_iter()
        .find_map(|(provider, lookup_model)| models_dev_pricing::lookup(provider, &lookup_model))
        .map(|pricing| pricing.input_cost_per_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_context_alias_falls_back_only_inside_kimi_vendor() {
        assert_eq!(
            models_dev_targets("k3[1m]", "k3[1m]".to_string()),
            vec![
                ("kimi-for-coding", "k3[1m]".to_string()),
                ("kimi-for-coding", "k3".to_string()),
            ]
        );
        assert_eq!(
            models_dev_targets(
                "kimi-for-coding/k3[1m]",
                "kimi-for-coding/k3[1m]".to_string()
            ),
            vec![
                ("kimi-for-coding", "k3[1m]".to_string()),
                ("kimi-for-coding", "k3".to_string()),
            ]
        );
        assert_eq!(
            models_dev_targets("moonshot/k3[1m]", "moonshot/k3[1m]".to_string()),
            vec![("moonshot", "k3[1m]".to_string())]
        );
    }

    #[test]
    fn gpt_proxy_uses_bundled_boundary_but_keeps_catalog_rates() {
        let snapshot = models_dev_pricing::ModelsDevPricingSnapshot::from_catalog_json_for_tests(
            r#"{
                "openai": {"models": {"gpt-5.6-sol": {"id": "gpt-5.6-sol", "cost": {
                    "input": 2, "output": 4, "cache_read": 0.25, "cache_write": 3,
                    "context_over_200k": {"input": 7, "output": 11, "cache_read": 0.5, "cache_write": 9}
                }}}},
                "anthropic": {"models": {"threshold-fixture": {"id": "threshold-fixture", "cost": {
                    "input": 2, "output": 4, "cache_read": 0.25, "cache_write": 3,
                    "context_over_200k": {"input": 7, "output": 11, "cache_read": 0.5, "cache_write": 9}
                }}}}
            }"#,
        )
        .expect("pricing fixture");

        let (pricing, threshold) = resolve_with_snapshot("gpt-5.6-sol", "gpt-5.6-sol", &snapshot)
            .expect("OpenAI proxy pricing");
        assert_eq!(threshold, Some(272_000));
        assert_eq!(pricing.input_cost_per_token, 2e-6);

        let short =
            cost_usd_from_pricing_with_threshold(pricing, threshold, 262_000, 10_000, 0, 13);
        let long = cost_usd_from_pricing_with_threshold(pricing, threshold, 262_001, 10_000, 0, 13);
        assert!((short - 0.526552).abs() < 1e-12);
        assert!((long - 1.83915).abs() < 1e-12);

        let (_, anthropic_threshold) = resolve_with_snapshot(
            "anthropic/threshold-fixture",
            "anthropic/threshold-fixture",
            &snapshot,
        )
        .expect("Anthropic pricing");
        assert_eq!(anthropic_threshold, Some(200_000));
    }
}
