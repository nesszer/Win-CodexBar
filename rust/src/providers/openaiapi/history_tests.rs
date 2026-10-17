use super::*;
use crate::core::{OpenAiApiDailyUsage, OpenAiApiModelUsage};

/// 2023-11-17T00:00:00Z, the fixture clock used by upstream `OpenAIAPIUsageFetcherTests`.
const NOW: i64 = 1_700_179_200;
const DAY: i64 = 86_400;

fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(NOW, 0).single().unwrap()
}

/// The costs page from upstream `parses admin costs and completions usage into daily summaries`.
const UPSTREAM_COSTS: &str = r#"{
  "object": "page",
  "data": [
    {"object": "bucket", "start_time": 1700000000, "end_time": 1700086400, "results": [
      {"object": "organization.costs.result", "amount": {"value": 12.50, "currency": "usd"}, "line_item": "Text tokens"},
      {"object": "organization.costs.result", "amount": {"value": "2.25", "currency": "usd"}, "line_item": "Web search tool calls"}
    ]},
    {"object": "bucket", "start_time": 1700086400, "end_time": 1700172800, "results": [
      {"object": "organization.costs.result", "amount": {"value": 4.00, "currency": "usd"}, "line_item": "Text tokens"}
    ]}
  ],
  "has_more": false,
  "next_page": null
}"#;

/// The completions page from the same upstream test.
const UPSTREAM_COMPLETIONS: &str = r#"{
  "object": "page",
  "data": [
    {"object": "bucket", "start_time": 1700000000, "end_time": 1700086400, "results": [
      {"object": "organization.usage.completions.result", "input_tokens": 1000, "input_cached_tokens": 250, "output_tokens": 500, "num_model_requests": 7, "model": "gpt-5.2"},
      {"object": "organization.usage.completions.result", "input_tokens": 300, "output_tokens": 200, "num_model_requests": 3, "model": "gpt-5.2-codex"}
    ]},
    {"object": "bucket", "start_time": 1700086400, "end_time": 1700172800, "results": [
      {"object": "organization.usage.completions.result", "input_tokens": 200, "output_tokens": 100, "num_model_requests": 2, "model": "gpt-5.2"}
    ]}
  ],
  "has_more": false,
  "next_page": null
}"#;

fn upstream_costs() -> Vec<CostBucket> {
    serde_json::from_str::<Page<CostBucket>>(UPSTREAM_COSTS)
        .unwrap()
        .data
}

fn upstream_completions() -> Vec<CompletionsUsageBucket> {
    serde_json::from_str::<Page<CompletionsUsageBucket>>(UPSTREAM_COMPLETIONS)
        .unwrap()
        .data
}

fn completion(
    model: Option<&str>,
    input: i64,
    output: i64,
    requests: i64,
) -> CompletionsUsageResult {
    CompletionsUsageResult {
        model: model.map(str::to_string),
        input_tokens: Some(input),
        input_cached_tokens: None,
        output_tokens: Some(output),
        input_audio_tokens: None,
        output_audio_tokens: None,
        num_model_requests: Some(requests),
    }
}

fn completions_bucket(start: i64, results: Vec<CompletionsUsageResult>) -> CompletionsUsageBucket {
    CompletionsUsageBucket {
        start_time: start,
        end_time: start + DAY,
        results,
    }
}

fn cost_bucket_at(start: i64, amount: f64, line_item: Option<&str>) -> CostBucket {
    CostBucket {
        start_time: start,
        end_time: start + DAY,
        results: vec![CostResult {
            amount: Some(CostAmount {
                value: serde_json::json!(amount),
            }),
            line_item: line_item.map(str::to_string),
        }],
    }
}

fn daily(costs: &[CostBucket], completions: &[CompletionsUsageBucket]) -> Vec<OpenAiApiDailyUsage> {
    history::daily_usage(costs, completions, now(), HISTORY_DAYS).unwrap()
}

#[test]
fn upstream_fixture_buckets_into_two_days() {
    let daily = daily(&upstream_costs(), &upstream_completions());
    assert_eq!(daily.len(), 2);

    let first = &daily[0];
    assert_eq!(
        (first.start_time, first.end_time),
        (1_700_000_000, 1_700_086_400)
    );
    assert_eq!(first.cost_usd, 14.75);
    assert_eq!(first.requests, 10);
    assert_eq!(first.input_tokens, 1300);
    assert_eq!(first.cached_input_tokens, 250);
    assert_eq!(first.output_tokens, 700);
    assert_eq!(first.total_tokens, 2000);
    let items: Vec<_> = first
        .line_items
        .iter()
        .map(|item| (item.name.as_str(), item.cost_usd))
        .collect();
    assert_eq!(
        items,
        [("Text tokens", 12.5), ("Web search tool calls", 2.25)]
    );
    let models: Vec<_> = first
        .models
        .iter()
        .map(|model| (model.name.as_str(), model.total_tokens))
        .collect();
    assert_eq!(models, [("gpt-5.2", 1500), ("gpt-5.2-codex", 500)]);

    let second = &daily[1];
    assert_eq!(second.cost_usd, 4.0);
    assert_eq!(second.requests, 2);
    assert_eq!(second.total_tokens, 300);

    // Upstream `last30Days` and `topModels`.
    assert_eq!(daily.iter().map(|day| day.cost_usd).sum::<f64>(), 18.75);
    assert_eq!(daily.iter().map(|day| day.requests).sum::<u64>(), 12);
    assert_eq!(daily.iter().map(|day| day.total_tokens).sum::<u64>(), 2300);
}

#[test]
fn upstream_fixture_result_carries_history_and_summary() {
    let result = result_from_admin_usage(
        &upstream_costs(),
        &upstream_completions(),
        now(),
        Some("proj_abc"),
    )
    .unwrap();
    let history = result.open_ai_api_usage.as_ref().unwrap();
    assert_eq!(history.history_days, 30);
    assert_eq!(history.project_id.as_deref(), Some("proj_abc"));
    assert_eq!(history.daily.len(), 2);
    assert_eq!(result.cost.as_ref().unwrap().used, 18.75);
    assert_eq!(result.cost.as_ref().unwrap().period, "Last 30 days");
}

#[test]
fn audio_tokens_are_already_included_in_input_and_output_and_not_added_again() {
    // The Admin API's `input_tokens`/`output_tokens` already include the audio portions
    // reported in the breakdown fields, so audio must not be counted a second time.
    let completions = [completions_bucket(
        NOW - DAY,
        vec![CompletionsUsageResult {
            model: Some("gpt-audio".to_string()),
            input_tokens: Some(1000),
            input_cached_tokens: Some(400),
            output_tokens: Some(500),
            input_audio_tokens: Some(40),
            output_audio_tokens: Some(10),
            num_model_requests: Some(2),
        }],
    )];
    let day = &daily(&[], &completions)[0];
    assert_eq!(day.input_tokens, 1000);
    assert_eq!(day.cached_input_tokens, 400);
    assert_eq!(day.output_tokens, 500);
    assert_eq!(day.total_tokens, 1500);
    assert_eq!(day.models[0].input_tokens, 1000);
    assert_eq!(day.models[0].output_tokens, 500);
    assert_eq!(day.models[0].total_tokens, 1500);
    assert!(day.line_items.is_empty());
}

#[test]
fn audio_breakdown_fields_are_validated_but_not_added_to_totals() {
    let completions = [completions_bucket(
        NOW - DAY,
        vec![CompletionsUsageResult {
            model: Some("gpt-audio".to_string()),
            input_tokens: Some(200),
            input_cached_tokens: None,
            output_tokens: Some(100),
            input_audio_tokens: Some(-1),
            output_audio_tokens: None,
            num_model_requests: Some(1),
        }],
    )];
    assert!(history::daily_usage(&[], &completions, now(), HISTORY_DAYS).is_err());
}

#[test]
fn costs_and_completions_for_one_day_share_a_bucket_with_the_first_end_time() {
    let costs = [CostBucket {
        start_time: NOW - DAY,
        end_time: NOW,
        results: cost_bucket_at(NOW - DAY, 1.0, Some("Text tokens")).results,
    }];
    let completions = [CompletionsUsageBucket {
        start_time: NOW - DAY,
        end_time: NOW + 1,
        results: vec![completion(Some("gpt-5.2"), 10, 5, 1)],
    }];
    let daily = daily(&costs, &completions);
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].end_time, NOW);
    assert_eq!(daily[0].cost_usd, 1.0);
    assert_eq!(daily[0].total_tokens, 15);
}

#[test]
fn ties_sort_by_name_and_blank_names_fall_back() {
    let costs = [CostBucket {
        start_time: NOW - DAY,
        end_time: NOW,
        results: ["b", "a", "  ", "c"]
            .into_iter()
            .map(|name| CostResult {
                amount: Some(CostAmount {
                    value: serde_json::json!(1.0),
                }),
                line_item: Some(name.to_string()),
            })
            .collect(),
    }];
    let completions = [completions_bucket(
        NOW - DAY,
        vec![
            completion(Some("zeta"), 5, 5, 1),
            completion(Some(" alpha "), 5, 5, 1),
            completion(None, 1, 0, 1),
            completion(Some(""), 1, 0, 1),
        ],
    )];
    let day = &daily(&costs, &completions)[0];
    let items: Vec<_> = day
        .line_items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(items, ["API", "a", "b", "c"]);
    let models: Vec<_> = day.models.iter().map(|model| model.name.as_str()).collect();
    assert_eq!(models, ["alpha", "zeta", "Responses and Chat Completions"]);
    // Both blank-named results merge into the default model.
    assert_eq!(day.models[2].requests, 2);
}

#[test]
fn days_are_sorted_future_days_dropped_and_the_window_trimmed() {
    let costs: Vec<_> = (0..4)
        .map(|offset| cost_bucket_at(NOW - offset * DAY, 1.0, None))
        .chain([cost_bucket_at(NOW + 1, 9.0, None)])
        .collect();
    let all = history::daily_usage(&costs, &[], now(), 30).unwrap();
    let starts: Vec<_> = all.iter().map(|day| day.start_time).collect();
    assert_eq!(starts, [NOW - 3 * DAY, NOW - 2 * DAY, NOW - DAY, NOW]);

    let recent = history::daily_usage(&costs, &[], now(), 2).unwrap();
    let starts: Vec<_> = recent.iter().map(|day| day.start_time).collect();
    assert_eq!(starts, [NOW - DAY, NOW]);
}

#[test]
fn invalid_token_counts_are_parse_failures() {
    for bad in [-1, 9_007_199_254_740_992] {
        let completions = [completions_bucket(
            NOW - DAY,
            vec![completion(Some("gpt-5.2"), bad, 0, 1)],
        )];
        let error = history::daily_usage(&[], &completions, now(), 30).unwrap_err();
        assert!(matches!(error, ProviderError::Parse(_)), "{bad}: {error}");
    }
    let mut result = completion(Some("gpt-5.2"), 1, 1, 1);
    result.num_model_requests = Some(-3);
    let completions = [completions_bucket(NOW - DAY, vec![result])];
    assert!(history::daily_usage(&[], &completions, now(), 30).is_err());
}

#[test]
fn history_is_dropped_but_the_summary_kept_for_a_bucket_that_does_not_end_after_it_starts() {
    let costs = [CostBucket {
        start_time: NOW - DAY,
        end_time: NOW - DAY,
        results: cost_bucket_at(NOW - DAY, 3.0, None).results,
    }];
    let result = result_from_admin_usage(&costs, &[], now(), None).unwrap();
    assert!(result.open_ai_api_usage.is_none());
    assert_eq!(result.cost.unwrap().used, 3.0);
}

#[test]
fn history_is_dropped_when_aggregated_counts_exceed_javascript_integer_precision() {
    let max_safe_count = 9_007_199_254_740_991;
    let costs = [cost_bucket_at(NOW - DAY, 3.0, None)];
    let completions = [completions_bucket(
        NOW - DAY,
        vec![
            completion(Some("gpt-5.2"), max_safe_count, 0, 0),
            completion(Some("gpt-5.2"), max_safe_count, 0, 0),
        ],
    )];

    let result = result_from_admin_usage(&costs, &completions, now(), None).unwrap();

    assert!(result.open_ai_api_usage.is_none());
    assert_eq!(result.cost.unwrap().used, 3.0);
}

#[test]
fn history_is_dropped_beyond_ten_thousand_breakdown_rows() {
    let models = |count: usize| -> Vec<OpenAiApiModelUsage> {
        (0..count)
            .map(|index| OpenAiApiModelUsage {
                name: format!("model-{index}"),
                requests: 1,
                input_tokens: 1,
                cached_input_tokens: 0,
                output_tokens: 1,
                total_tokens: 2,
            })
            .collect()
    };
    let day = |start: i64, count: usize| OpenAiApiDailyUsage {
        start_time: start,
        end_time: start + DAY,
        cost_usd: 0.0,
        requests: 0,
        input_tokens: 0,
        cached_input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        line_items: Vec::new(),
        models: models(count),
    };
    assert!(history::usage_history(vec![day(0, 5_000), day(DAY, 5_000)], 30, None).is_some());
    assert!(history::usage_history(vec![day(0, 5_000), day(DAY, 5_001)], 30, None).is_none());
}

#[test]
fn an_empty_window_still_yields_an_empty_history() {
    let result = result_from_admin_usage(&[], &[], now(), None).unwrap();
    let history = result.open_ai_api_usage.unwrap();
    assert!(history.daily.is_empty());
    assert_eq!(history.project_id, None);
}
