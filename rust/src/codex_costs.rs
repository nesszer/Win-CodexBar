//! Codex local-log cost aggregation helpers.

#[cfg(test)]
use chrono::Local;
use chrono::{Duration, NaiveDate};
use std::collections::HashMap;
use std::path::Path;

use crate::core::{CostUsageDayRange, CostUsagePricing, JsonlScanner};
use crate::cost_scanner::{CostSummary, ModelTokenCounts};

pub(crate) type CodexDays = HashMap<String, HashMap<String, Vec<i32>>>;

pub(crate) fn codex_period_start(today: NaiveDate, days: u32) -> NaiveDate {
    today - Duration::days(days.saturating_sub(1) as i64)
}

pub(crate) fn codex_scan_dates(range: &CostUsageDayRange) -> Vec<NaiveDate> {
    let Some(mut date) = CostUsageDayRange::parse_day_key(&range.scan_since_key) else {
        return Vec::new();
    };
    let Some(until) = CostUsageDayRange::parse_day_key(&range.scan_until_key) else {
        return Vec::new();
    };
    let mut dates = Vec::new();
    while date <= until {
        dates.push(date);
        date += Duration::days(1);
    }
    dates
}

pub(crate) fn add_codex_days_to_summary(
    summary: &mut CostSummary,
    days: &CodexDays,
    range: &CostUsageDayRange,
) -> (f64, bool) {
    let mut total_cost = 0.0;
    let mut has_tokens = false;

    for models in codex_days_in_range(days, range) {
        for (model, packed) in models {
            let tokens = CodexTokenCounts::from_packed(packed);
            if tokens.is_empty() {
                continue;
            }

            let cost = codex_cost_usd(model, tokens.input, tokens.cached, tokens.output);
            total_cost += cost;
            has_tokens = true;

            summary.input_tokens += tokens.input;
            summary.cached_tokens += tokens.cached;
            summary.output_tokens += tokens.output;
            *summary.by_model.entry(model.clone()).or_insert(0.0) += cost;

            let speed_bucket = codex_speed_bucket(model);
            *summary
                .by_speed
                .entry(speed_bucket.to_string())
                .or_insert(0.0) += cost;

            add_tokens(
                summary.by_model_tokens.entry(model.clone()).or_default(),
                tokens,
            );
            add_tokens(
                summary
                    .by_speed_tokens
                    .entry(speed_bucket.to_string())
                    .or_default(),
                tokens,
            );
        }
    }

    (total_cost, has_tokens)
}

pub(crate) fn scan_codex_file_cost_for_range(path: &Path, range: &CostUsageDayRange) -> f64 {
    let parse_result = match JsonlScanner::parse_codex_file(path, range, 0, None, None) {
        Ok(result) => result,
        Err(_) => return 0.0,
    };

    codex_days_cost(&parse_result.days, range)
}

#[cfg(test)]
pub(crate) fn scan_codex_file_cost(path: &Path) -> f64 {
    let today = Local::now().date_naive();
    let range = CostUsageDayRange::new(codex_period_start(today, 30), today);
    scan_codex_file_cost_for_range(path, &range)
}

#[derive(Clone, Copy)]
struct CodexTokenCounts {
    input: u64,
    cached: u64,
    output: u64,
}

impl CodexTokenCounts {
    fn from_packed(packed: &[i32]) -> Self {
        let input = packed.first().copied().unwrap_or(0).max(0) as u64;
        Self {
            input,
            cached: (packed.get(1).copied().unwrap_or(0).max(0) as u64).min(input),
            output: packed.get(2).copied().unwrap_or(0).max(0) as u64,
        }
    }

    fn is_empty(self) -> bool {
        self.input == 0 && self.cached == 0 && self.output == 0
    }
}

fn add_tokens(summary: &mut ModelTokenCounts, tokens: CodexTokenCounts) {
    summary.input_tokens += tokens.input;
    summary.output_tokens += tokens.output;
    summary.cached_tokens += tokens.cached;
}

fn codex_days_cost(days: &CodexDays, range: &CostUsageDayRange) -> f64 {
    let mut total_cost = 0.0;

    for models in codex_days_in_range(days, range) {
        for (model, packed) in models {
            let tokens = CodexTokenCounts::from_packed(packed);
            if tokens.is_empty() {
                continue;
            }

            total_cost += codex_cost_usd(model, tokens.input, tokens.cached, tokens.output);
        }
    }

    total_cost
}

fn codex_days_in_range<'a>(
    days: &'a CodexDays,
    range: &'a CostUsageDayRange,
) -> impl Iterator<Item = &'a HashMap<String, Vec<i32>>> + 'a {
    days.iter()
        .filter(move |(day_key, _)| {
            CostUsageDayRange::is_in_range(day_key, &range.since_key, &range.until_key)
        })
        .map(|(_, models)| models)
}

fn codex_speed_bucket(model: &str) -> &'static str {
    let normalized = model.to_ascii_lowercase();
    if normalized.contains("fast")
        || normalized.contains("priority")
        || normalized.contains("spark")
        || normalized.contains("smoke")
    {
        "fast"
    } else {
        "standard"
    }
}

fn codex_cost_usd(model: &str, input: u64, cached: u64, output: u64) -> f64 {
    if let Some(cost) = CostUsagePricing::codex_cost_usd(model, input, cached, output) {
        return cost;
    }

    let (input_price, cached_price, output_price) = match model.to_lowercase().as_str() {
        m if m.contains("gpt-4o-mini") => (0.15, 0.075, 0.60),
        m if m.contains("gpt-4o") => (2.50, 1.25, 10.00),
        m if m.contains("gpt-4-turbo") => (10.00, 5.00, 30.00),
        m if m.contains("gpt-4") => (30.00, 15.00, 60.00),
        m if m.contains("o1-mini") => (3.00, 1.50, 12.00),
        m if m.contains("o1") => (15.00, 7.50, 60.00),
        _ => (2.50, 1.25, 10.00),
    };

    let cached = cached.min(input);
    let non_cached = input.saturating_sub(cached);
    let input_cost = (non_cached as f64 / 1_000_000.0) * input_price;
    let cached_cost = (cached as f64 / 1_000_000.0) * cached_price;
    let output_cost = (output as f64 / 1_000_000.0) * output_price;

    input_cost + cached_cost + output_cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codex_pricing() {
        // Test GPT-4o pricing: $2.50/1M input, $10/1M output
        let cost = codex_cost_usd("gpt-4o", 1_000_000, 0, 1_000_000);
        assert!((cost - 12.50).abs() < 0.01);
    }

    #[test]
    fn test_codex_pricing_uses_gpt55_standard_short_context_rates() {
        let cost = codex_cost_usd("gpt-5.5", 1_000_000, 400_000, 1_000_000);

        // GPT-5.5 standard short-context pricing:
        // 600k non-cached input at $5/M, 400k cached input at $0.50/M,
        // and 1M output at $30/M.
        assert!((cost - 33.20).abs() < 0.01);
    }

    #[test]
    fn test_codex_speed_bucket() {
        assert_eq!(codex_speed_bucket("gpt-5.5-fast"), "fast");
        assert_eq!(codex_speed_bucket("gpt-5.3-codex-spark"), "fast");
        assert_eq!(codex_speed_bucket("gpt-5-codex"), "standard");
    }

    #[test]
    fn codex_summary_filters_expanded_scan_days_to_requested_range() {
        let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let range = CostUsageDayRange::new(target, target);
        let mut days = CodexDays::new();
        for (day, input) in [
            ("2026-05-30", 1_000),
            ("2026-05-31", 2_000),
            ("2026-06-01", 4_000),
        ] {
            days.entry(day.to_string())
                .or_default()
                .insert("gpt-5".to_string(), vec![input, 0, 0]);
        }

        let mut summary = CostSummary::default();
        let (cost, has_tokens) = add_codex_days_to_summary(&mut summary, &days, &range);

        let expected = codex_cost_usd("gpt-5", 2_000, 0, 0);
        assert!(has_tokens);
        assert_eq!(summary.input_tokens, 2_000);
        assert_eq!(summary.output_tokens, 0);
        assert!((cost - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn codex_daily_cost_filters_adjacent_scan_padding_days() {
        let target = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
        let range = CostUsageDayRange::new(target, target);
        let mut days = CodexDays::new();
        days.entry("2026-05-30".to_string())
            .or_default()
            .insert("gpt-5".to_string(), vec![1_000, 0, 0]);
        days.entry("2026-05-31".to_string())
            .or_default()
            .insert("gpt-5".to_string(), vec![2_000, 0, 0]);

        let cost = codex_days_cost(&days, &range);
        let expected = codex_cost_usd("gpt-5", 2_000, 0, 0);

        assert!((cost - expected).abs() < f64::EPSILON);
    }
}
