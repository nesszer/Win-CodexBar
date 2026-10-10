//! Provider-specific cache rules across model, day and window totals.

use super::*;

// Regression (PR #611 review): Codex `input_tokens` already contains cached
// input, Claude cache buckets are separate. Every total must follow the
// provider's rule, and the model, daily and window totals must agree.
#[test]
fn provider_totals_add_cache_only_when_it_is_separate_from_input() {
    let counts = ModelTokenCounts {
        input_tokens: 100,
        output_tokens: 5,
        cached_tokens: 90,
        reasoning_tokens: None,
    };
    assert_eq!(counts.total_for_provider("codex"), 105);
    assert_eq!(counts.total_for_provider("claude"), 195);

    let summary = CostSummary {
        input_tokens: 100,
        output_tokens: 5,
        cached_tokens: 90,
        ..CostSummary::default()
    };
    assert_eq!(summary.total_tokens_for_provider("codex"), 105);
    assert_eq!(summary.total_tokens_for_provider("claude"), 195);
}

#[test]
fn codex_day_total_excludes_cached_input_and_matches_model_totals() {
    let day_key = "2026-08-18".to_string();
    let day = CostUsageDayRange::parse_day_key(&day_key).expect("day");
    let range = CostUsageDayRange::new(day, day);
    let mut one_day = HashMap::new();
    one_day.insert(
        day_key,
        HashMap::from([("gpt-5".to_string(), vec![1_000, 900, 50])]),
    );
    let mut scratch = CostSummary::default();
    add_codex_days_map_to_summary(&mut scratch, &one_day, &range);

    assert_eq!(scratch.input_tokens, 1_000);
    assert_eq!(scratch.cached_tokens, 900);
    // The value `get_daily_token_history("codex")` stores for the day.
    let day_total = scratch.total_tokens_for_provider("codex");
    assert_eq!(day_total, 1_050, "cached input is already inside input");
    let model_total: u64 = scratch
        .by_model_tokens
        .values()
        .map(|counts| counts.total_for_provider("codex"))
        .sum();
    assert_eq!(model_total, day_total);
}

#[test]
fn claude_day_total_includes_cache_and_matches_summary_and_model_totals() {
    use chrono::TimeZone as _;
    let timestamp = Local
        .with_ymd_and_hms(2026, 8, 18, 12, 0, 0)
        .single()
        .expect("local time")
        .with_timezone(&Utc);
    let record = ClaudeUsageRecord {
        model: "claude-sonnet-4-5".to_string(),
        pricing_known: true,
        timestamp: Some(timestamp),
        dedup_key: None,
        input: 10,
        output: 20,
        cache_create: 300,
        cache_read: 4_000,
        cost: Some(0.0),
    };
    let mut summary = CostSummary::default();
    assert!(add_claude_record_to_summary(&mut summary, &record));
    let mut daily = HashMap::from([("2026-08-18".to_string(), 0u64)]);
    assert!(add_claude_record_to_daily_tokens(&mut daily, &record));

    let window_total = summary.total_tokens_for_provider("claude");
    assert_eq!(window_total, 4_330);
    assert_eq!(daily["2026-08-18"], window_total);
    let model_total: u64 = summary
        .by_model_tokens
        .values()
        .map(|counts| counts.total_for_provider("claude"))
        .sum();
    assert_eq!(model_total, window_total);
}
