use super::*;
use crate::core::{CodexSourceUsageRow, CodexUsageRecord, CostUsagePricing};

pub(super) fn rebuild_cache_days(cache: &mut CostUsageCache) {
    cache.days.clear();
    for usage in cache.files.values() {
        for (day, models) in &usage.days {
            let day_entry = cache.days.entry(day.clone()).or_default();
            for (model, packed) in models {
                let dest = day_entry
                    .entry(model.clone())
                    .or_insert_with(|| vec![0, 0, 0]);
                if dest.len() < 3 {
                    dest.resize(3, 0);
                }

                let had_core_tokens = dest[0] != 0 || dest[1] != 0 || dest[2] != 0;
                let source_input = packed.first().copied().unwrap_or(0);
                let source_cached = packed.get(1).copied().unwrap_or(0);
                let source_output = packed.get(2).copied().unwrap_or(0);
                let source_has_tokens =
                    source_input != 0 || source_cached != 0 || source_output != 0;
                let source_reasoning = packed
                    .get(3)
                    .copied()
                    .map(|reasoning| reasoning.max(0).min(source_output.max(0)));

                dest[0] = dest[0].saturating_add(source_input);
                dest[1] = dest[1].saturating_add(source_cached);
                dest[2] = dest[2].saturating_add(source_output);

                if !source_has_tokens {
                    continue;
                }

                if !had_core_tokens {
                    match source_reasoning {
                        Some(reasoning) => {
                            if dest.len() >= 4 {
                                dest[3] = reasoning.min(dest[2].max(0));
                            } else {
                                dest.push(reasoning.min(dest[2].max(0)));
                            }
                        }
                        None => dest.truncate(3),
                    }
                    continue;
                }

                match (dest.get(3).copied(), source_reasoning) {
                    (Some(previous), Some(reasoning)) => {
                        let merged = previous.saturating_add(reasoning).min(dest[2].max(0));
                        dest[3] = merged;
                    }
                    _ => dest.truncate(3),
                }
            }
        }
    }
}

pub(super) fn days_from_codex_source_rows(
    rows: &[CodexSourceUsageRow],
) -> HashMap<String, HashMap<String, Vec<i64>>> {
    let mut days: HashMap<String, HashMap<String, Vec<i64>>> = HashMap::new();
    for row in rows {
        let model = match row.pricing.pricing_model.as_deref() {
            Some(model) if !model.is_empty() => {
                if row.pricing.pricing_mode.as_deref() == Some("priority")
                    && !model.ends_with("-priority")
                {
                    format!("{model}-priority")
                } else {
                    model.to_string()
                }
            }
            _ => CostUsagePricing::CODEX_UNATTRIBUTED_MODEL.to_string(),
        };
        let record = CodexUsageRecord {
            day_key: row.day_key.clone(),
            model,
            input: row.input,
            cached: row.cached,
            output: row.output,
            reasoning: row.reasoning,
        };
        let packed = days
            .entry(record.day_key.clone())
            .or_default()
            .entry(record.model.clone())
            .or_default();
        JsonlScanner::merge_codex_record_into_packed(packed, &record);
    }
    days
}
