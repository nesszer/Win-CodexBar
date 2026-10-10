use super::*;
use crate::providers::test_support::{mock_response_expect, mock_status};
use mockito::{Matcher, Server, ServerGuard};

const COSTS_PATH: &str = "/v1/organization/costs";
const COMPLETIONS_PATH: &str = "/v1/organization/usage/completions";
const GRANTS_PATH: &str = "/v1/dashboard/billing/credit_grants";
const EMPTY_PAGE: &str = r#"{"object":"page","data":[],"has_more":false,"next_page":null}"#;
/// 2023-11-17T00:00:00Z, the fixture clock used by upstream `OpenAIAPIUsageFetcherTests`.
const NOW: i64 = 1_700_179_200;

fn fixed_now(offset_seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(NOW + offset_seconds, 0).single().unwrap()
}

/// A provider pointed at the mock server, with retry delays removed so tests stay fast.
fn provider(server: &ServerGuard) -> OpenAIApiProvider {
    let mut provider = OpenAIApiProvider::new();
    provider.client = Client::builder()
        .no_proxy()
        .build()
        .expect("the test client should build");
    provider.endpoints = Endpoints {
        credit_grants: format!("{}{GRANTS_PATH}", server.url()),
        costs: format!("{}{COSTS_PATH}", server.url()),
        completions: format!("{}{COMPLETIONS_PATH}", server.url()),
    };
    provider.retry = RetryPolicy {
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    };
    provider
}

fn admin_credential() -> ApiCredential {
    ApiCredential {
        key: "sk-test".to_string(),
        is_admin: true,
    }
}

async fn mock_page(
    server: &mut ServerGuard,
    path: &str,
    query: Matcher,
    body: &str,
) -> mockito::Mock {
    server
        .mock("GET", path)
        .match_query(query)
        .with_status(200)
        .with_body(body)
        .create_async()
        .await
}

fn extra_window<'a>(result: &'a ProviderFetchResult, id: &str) -> &'a str {
    result
        .usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == id)
        .and_then(|window| window.window.reset_description.as_deref())
        .unwrap_or_else(|| panic!("missing extra window {id}"))
}

fn cost_bucket(amount: serde_json::Value) -> CostBucket {
    CostBucket {
        start_time: 1_797_638_400,
        end_time: 1_797_724_800,
        results: vec![CostResult {
            amount: Some(CostAmount { value: amount }),
            line_item: Some("API".to_string()),
        }],
    }
}

#[test]
fn openai_api_credit_snapshot_formats_available_balance() {
    let result = result_from_grants(&CreditGrantsResponse {
        total_granted: 100.0,
        total_used: 25.0,
        total_available: 75.0,
        grants: None,
    });
    assert_eq!(result.usage.primary.used_percent, 25.0);
    assert_eq!(result.cost.unwrap().remaining(), Some(75.0));
}

#[test]
fn openai_admin_usage_accepts_numeric_string_cost_amounts() {
    let costs = [cost_bucket(serde_json::json!("12.50"))];
    let completions = [CompletionsUsageBucket {
        start_time: 1_797_638_400,
        end_time: 1_797_724_800,
        results: vec![CompletionsUsageResult {
            model: Some("gpt-5.1".to_string()),
            input_tokens: Some(100),
            input_cached_tokens: Some(25),
            output_tokens: Some(50),
            input_audio_tokens: None,
            output_audio_tokens: None,
            num_model_requests: Some(7),
        }],
    }];
    let now = Utc.timestamp_opt(1_797_724_800, 0).single().unwrap();
    let result = result_from_admin_usage(&costs, &completions, now, Some("proj_demo")).unwrap();
    assert_eq!(result.cost.as_ref().unwrap().used, 12.5);
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("Project: proj_demo")
    );
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Admin API: proj_demo")
    );
    assert!(result.usage.secondary.is_none());
    let requests = result
        .usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == "requests")
        .unwrap();
    assert!(requests.window.is_informational);
    assert_eq!(
        requests.window.reset_description.as_deref(),
        Some("7 requests")
    );
    // Cached input is a subset of input: 100 + 50, not 100 + 25 + 50.
    assert_eq!(extra_window(&result, "tokens"), "150 tokens");
    assert_eq!(extra_window(&result, "model-0"), "150 tokens");
}

#[test]
fn openai_admin_tokens_sum_text_and_audio_without_cached() {
    // Upstream `tokens = input + input_audio + output + output_audio`.
    let completions = [CompletionsUsageBucket {
        start_time: 1_700_000_000,
        end_time: 1_700_086_400,
        results: vec![
            CompletionsUsageResult {
                model: Some("gpt-5.2".to_string()),
                input_tokens: Some(1000),
                input_cached_tokens: Some(250),
                output_tokens: Some(500),
                input_audio_tokens: Some(40),
                output_audio_tokens: Some(10),
                num_model_requests: Some(7),
            },
            CompletionsUsageResult {
                model: Some("gpt-5.2-codex".to_string()),
                input_tokens: Some(300),
                input_cached_tokens: None,
                output_tokens: Some(200),
                input_audio_tokens: None,
                output_audio_tokens: None,
                num_model_requests: Some(3),
            },
        ],
    }];
    let result = result_from_admin_usage(&[], &completions, fixed_now(0), None).unwrap();
    assert_eq!(extra_window(&result, "tokens"), "2050 tokens");
    assert_eq!(extra_window(&result, "requests"), "10 requests");
    assert_eq!(extra_window(&result, "model-0"), "1550 tokens");
    assert_eq!(extra_window(&result, "model-1"), "500 tokens");
}

#[test]
fn openai_admin_usage_sums_costs_and_ranks_line_items() {
    // Cases from upstream `parses admin costs and completions usage into daily summaries`.
    let costs = [
        CostBucket {
            start_time: 1_700_000_000,
            end_time: 1_700_086_400,
            results: vec![
                CostResult {
                    amount: Some(CostAmount {
                        value: serde_json::json!(12.50),
                    }),
                    line_item: Some("Text tokens".to_string()),
                },
                CostResult {
                    amount: Some(CostAmount {
                        value: serde_json::json!("2.25"),
                    }),
                    line_item: Some("Web search tool calls".to_string()),
                },
            ],
        },
        CostBucket {
            start_time: 1_700_086_400,
            end_time: 1_700_172_800,
            results: vec![CostResult {
                amount: Some(CostAmount {
                    value: serde_json::json!(4.00),
                }),
                line_item: Some("Text tokens".to_string()),
            }],
        },
    ];
    let result = result_from_admin_usage(&costs, &[], fixed_now(0), None).unwrap();
    assert_eq!(result.cost.as_ref().unwrap().used, 18.75);
    assert_eq!(result.cost.as_ref().unwrap().period, "Last 30 days");
    assert_eq!(extra_window(&result, "line-item-0"), "$16.50");
    assert_eq!(extra_window(&result, "line-item-1"), "$2.25");
}

#[test]
fn openai_admin_usage_rejects_nonfinite_cost_amounts() {
    assert_eq!(json::lenient_finite_f64(&serde_json::json!("NaN")), None);
    assert_eq!(
        json::lenient_finite_f64(&serde_json::json!("Infinity")),
        None
    );
    for value in ["NaN", "Infinity", "-Infinity", "1e309", "-1e309"] {
        let costs = [cost_bucket(serde_json::json!(value))];
        let error = result_from_admin_usage(&costs, &[], fixed_now(0), None)
            .err()
            .unwrap_or_else(|| panic!("{value} should be rejected"));
        assert!(matches!(error, ProviderError::Parse(_)), "{value}: {error}");
    }
}

#[test]
fn openai_admin_usage_treats_blank_amounts_as_zero() {
    let costs = [
        cost_bucket(serde_json::Value::Null),
        cost_bucket(serde_json::json!("  ")),
        CostBucket {
            start_time: 1,
            end_time: 2,
            results: vec![CostResult {
                amount: None,
                line_item: None,
            }],
        },
    ];
    let result = result_from_admin_usage(&costs, &[], fixed_now(0), None).unwrap();
    assert_eq!(result.cost.unwrap().used, 0.0);
}

#[test]
fn openai_admin_query_scopes_project_ids_and_page_when_configured() {
    let range = UsageRange {
        start: 1,
        end: 2,
        limit: 31,
    };
    let query = AdminQuery {
        group_by: "model",
        project_id: Some("  proj_123  "),
        label: "completions",
    };
    let params = admin_query(&range, &query, Some("cursor_2"));
    assert!(params.contains(&("project_ids", "proj_123".to_string())));
    assert!(params.contains(&("page", "cursor_2".to_string())));
    assert!(params.contains(&("bucket_width", "1d".to_string())));
    assert!(params.contains(&("group_by", "model".to_string())));

    let unscoped = admin_query(
        &range,
        &AdminQuery {
            project_id: None,
            ..query
        },
        None,
    );
    assert!(unscoped.iter().all(|(name, _)| *name != "project_ids"));
    assert!(unscoped.iter().all(|(name, _)| *name != "page"));
}

#[test]
fn openai_admin_time_range_does_not_end_in_the_future() {
    let now = Utc.timestamp_opt(1_783_981_862, 0).single().unwrap();
    let ranges = usage_ranges(now, HISTORY_DAYS);

    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].end, now.timestamp());
    assert_eq!(
        ranges[0].start,
        1_783_981_862 - 1_783_981_862 % 86_400 - 29 * 86_400
    );
    assert_eq!(ranges[0].limit, 30);
    assert!(ranges[0].start < ranges[0].end);
}

#[test]
fn openai_usage_ranges_page_long_history_within_endpoint_bucket_limit() {
    // Upstream `admin usage fetch pages long history within endpoint bucket limit`: 90 days
    // is three ranges of 31, 31 and 28 buckets, contiguous and aligned to UTC day starts.
    let ranges = usage_ranges(fixed_now(0), 90);
    let limits: Vec<u32> = ranges.iter().map(|range| range.limit).collect();
    assert_eq!(limits, [31, 31, 28]);
    assert!(limits.iter().all(|limit| *limit <= 31));
    assert_eq!(ranges[0].start, NOW - 89 * 86_400);
    assert!(ranges.iter().all(|range| range.start % 86_400 == 0));
    assert_eq!(ranges[1].start, ranges[0].start + 31 * 86_400);
    assert_eq!(ranges[2].start, ranges[1].start + 31 * 86_400);
    assert_eq!(ranges[0].end, ranges[1].start);
    assert_eq!(ranges[2].end, NOW);

    assert_eq!(usage_ranges(fixed_now(3_600), 1).len(), 1);
    assert_eq!(usage_ranges(fixed_now(3_600), 1)[0].start, NOW);
    assert_eq!(usage_ranges(fixed_now(3_600), 31).len(), 1);
    assert_eq!(usage_ranges(fixed_now(3_600), 32).len(), 2);
}

#[test]
fn openai_response_error_detail_prefers_api_message() {
    assert_eq!(
        response_error_detail(r#"{"error":{"message":"end_time must not be in the future"}}"#),
        "end_time must not be in the future"
    );
    assert_eq!(
        response_error_detail("plain text failure"),
        "plain text failure"
    );
    assert_eq!(response_error_detail("  "), "");
}

#[test]
fn openai_retry_policy_honors_retry_after_up_to_ten_seconds() {
    let policy = RetryPolicy::DEFAULT;
    let header = |value: &'static str| HeaderValue::from_static(value);

    assert_eq!(policy.delay(None), Duration::from_secs(1));
    assert_eq!(policy.delay(Some(&header("3"))), Duration::from_secs(3));
    assert_eq!(policy.delay(Some(&header(" 2 "))), Duration::from_secs(2));
    assert_eq!(policy.delay(Some(&header("0"))), Duration::ZERO);
    assert_eq!(policy.delay(Some(&header("600"))), Duration::from_secs(10));
    for invalid in ["soon", "-5", "inf", "NaN", ""] {
        assert_eq!(
            policy.delay(Some(&header(invalid))),
            Duration::from_secs(1),
            "{invalid:?}"
        );
    }
}

#[test]
fn openai_retry_policy_retries_only_transient_statuses() {
    for status in [408, 429, 500, 502, 503, 504] {
        assert!(RetryPolicy::retries_status(
            reqwest::StatusCode::from_u16(status).unwrap()
        ));
    }
    for status in [200, 400, 401, 403, 404, 422] {
        assert!(!RetryPolicy::retries_status(
            reqwest::StatusCode::from_u16(status).unwrap()
        ));
    }
}

#[test]
fn openai_balance_fallback_is_blocked_only_for_project_scoped_admin_keys() {
    assert!(allows_legacy_balance_fallback(None, true));
    assert!(allows_legacy_balance_fallback(None, false));
    assert!(allows_legacy_balance_fallback(Some("proj_abc"), false));
    assert!(!allows_legacy_balance_fallback(Some("proj_abc"), true));
}

#[test]
fn openai_credential_kind_follows_the_key_source() {
    let credential = resolve_api_key(
        Some("  sk-admin-configured  "),
        "codexbar-openaiapi-test-unused",
        &[("CODEXBAR_TEST_OPENAI_UNSET", false)],
    )
    .unwrap();
    assert_eq!(credential.key, "sk-admin-configured");
    assert!(credential.is_admin);
}

#[tokio::test]
async fn openai_admin_usage_filters_costs_and_completions_by_project() {
    let mut server = Server::new_async().await;
    let query = |group_by: &str| {
        Matcher::AllOf(vec![
            Matcher::UrlEncoded("project_ids".into(), "proj_abc".into()),
            Matcher::UrlEncoded("group_by".into(), group_by.into()),
            Matcher::UrlEncoded("bucket_width".into(), "1d".into()),
            Matcher::UrlEncoded("limit".into(), "30".into()),
        ])
    };
    let costs = mock_page(&mut server, COSTS_PATH, query("line_item"), EMPTY_PAGE)
        .await
        .expect(1);
    let completions = mock_page(&mut server, COMPLETIONS_PATH, query("model"), EMPTY_PAGE)
        .await
        .expect(1);

    let result = provider(&server)
        .fetch_admin_usage("sk-test", Some(" proj_abc "), fixed_now(3_600))
        .await
        .unwrap();

    costs.assert_async().await;
    completions.assert_async().await;
    assert_eq!(result.source_label, "admin-api");
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("Project: proj_abc")
    );
}

#[tokio::test]
async fn openai_admin_usage_returns_per_day_history_from_the_wire_pages() {
    let mut server = Server::new_async().await;
    let day = 1_700_000_000;
    let costs_page = format!(
        r#"{{"object":"page","has_more":false,"next_page":null,"data":[
            {{"object":"bucket","start_time":{day},"end_time":{end},"results":[
                {{"object":"organization.costs.result","amount":{{"value":"2.50","currency":"usd"}},"line_item":"Text tokens"}}]}}]}}"#,
        end = day + 86_400
    );
    let completions_page = format!(
        r#"{{"object":"page","has_more":false,"next_page":null,"data":[
            {{"object":"bucket","start_time":{day},"end_time":{end},"results":[
                {{"object":"organization.usage.completions.result","input_tokens":100,"input_cached_tokens":40,"output_tokens":50,"input_audio_tokens":null,"num_model_requests":4,"model":"gpt-5.2"}}]}}]}}"#,
        end = day + 86_400
    );
    let _costs = mock_page(&mut server, COSTS_PATH, Matcher::Any, &costs_page).await;
    let _completions = mock_page(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Any,
        &completions_page,
    )
    .await;

    let result = provider(&server)
        .fetch_admin_usage("sk-test", Some("proj_abc"), fixed_now(3_600))
        .await
        .unwrap();

    let history = result
        .open_ai_api_usage
        .expect("Admin path returns history");
    assert_eq!(history.history_days, 30);
    assert_eq!(history.project_id.as_deref(), Some("proj_abc"));
    assert_eq!(history.daily.len(), 1);
    let bucket = &history.daily[0];
    assert_eq!((bucket.start_time, bucket.end_time), (day, day + 86_400));
    assert_eq!(bucket.cost_usd, 2.5);
    assert_eq!(bucket.requests, 4);
    assert_eq!(bucket.cached_input_tokens, 40);
    assert_eq!(bucket.total_tokens, 150);
    assert_eq!(bucket.line_items[0].name, "Text tokens");
    assert_eq!(bucket.models[0].name, "gpt-5.2");
}

#[tokio::test]
async fn openai_admin_usage_pages_each_range_of_a_long_history() {
    let mut server = Server::new_async().await;
    let costs = mock_page(&mut server, COSTS_PATH, Matcher::Any, EMPTY_PAGE)
        .await
        .expect(3);
    let completions = mock_page(&mut server, COMPLETIONS_PATH, Matcher::Any, EMPTY_PAGE)
        .await
        .expect(3);

    let provider = provider(&server);
    let ranges = usage_ranges(fixed_now(0), 90);
    let buckets: Vec<CostBucket> = provider
        .fetch_pages(
            &provider.endpoints.costs,
            AdminQuery {
                group_by: "line_item",
                project_id: None,
                label: "costs",
            },
            &ranges,
            "sk-test",
        )
        .await
        .unwrap();
    assert!(buckets.is_empty());
    let _: Vec<CompletionsUsageBucket> = provider
        .fetch_pages(
            &provider.endpoints.completions,
            AdminQuery {
                group_by: "model",
                project_id: None,
                label: "completions",
            },
            &ranges,
            "sk-test",
        )
        .await
        .unwrap();

    costs.assert_async().await;
    completions.assert_async().await;
}

const COSTS_PAGE_1: &str = r#"{"object":"page","data":[{"object":"bucket","start_time":1700000000,"end_time":1700086400,"results":[{"object":"organization.costs.result","amount":{"value":1.25,"currency":"usd"},"line_item":"Text tokens"}]}],"has_more":true,"next_page":"costs_page_2"}"#;
const COSTS_PAGE_2: &str = r#"{"object":"page","data":[{"object":"bucket","start_time":1700000000,"end_time":1700086400,"results":[{"object":"organization.costs.result","amount":{"value":2.75,"currency":"usd"},"line_item":"Web search tool calls"}]}],"has_more":false,"next_page":null}"#;
const COMPLETIONS_PAGE_1: &str = r#"{"object":"page","data":[{"object":"bucket","start_time":1700000000,"end_time":1700086400,"results":[{"object":"organization.usage.completions.result","input_tokens":10,"output_tokens":5,"num_model_requests":1,"model":"gpt-5.2"}]}],"has_more":true,"next_page":"completions_page_2"}"#;
const COMPLETIONS_PAGE_2: &str = r#"{"object":"page","data":[{"object":"bucket","start_time":1700000000,"end_time":1700086400,"results":[{"object":"organization.usage.completions.result","input_tokens":20,"output_tokens":10,"num_model_requests":2,"model":"gpt-5.2"}]}],"has_more":false,"next_page":null}"#;

#[tokio::test]
async fn openai_admin_usage_follows_costs_and_completions_pagination_cursors() {
    let mut server = Server::new_async().await;
    let page = |name: &str| Matcher::UrlEncoded("page".into(), name.into());
    let mocks = vec![
        server
            .mock("GET", COSTS_PATH)
            .match_query(Matcher::AllOf(vec![Matcher::Regex(
                "project_ids=proj_abc$".into(),
            )]))
            .with_body(COSTS_PAGE_1)
            .expect(1)
            .create_async()
            .await,
        server
            .mock("GET", COSTS_PATH)
            .match_query(Matcher::AllOf(vec![
                page("costs_page_2"),
                Matcher::UrlEncoded("project_ids".into(), "proj_abc".into()),
            ]))
            .with_body(COSTS_PAGE_2)
            .expect(1)
            .create_async()
            .await,
        server
            .mock("GET", COMPLETIONS_PATH)
            .match_query(Matcher::AllOf(vec![Matcher::Regex(
                "project_ids=proj_abc$".into(),
            )]))
            .with_body(COMPLETIONS_PAGE_1)
            .expect(1)
            .create_async()
            .await,
        server
            .mock("GET", COMPLETIONS_PATH)
            .match_query(Matcher::AllOf(vec![
                page("completions_page_2"),
                Matcher::UrlEncoded("project_ids".into(), "proj_abc".into()),
            ]))
            .with_body(COMPLETIONS_PAGE_2)
            .expect(1)
            .create_async()
            .await,
    ];

    let result = provider(&server)
        .fetch_admin_usage("sk-test", Some("proj_abc"), fixed_now(3_600))
        .await
        .unwrap();

    for mock in &mocks {
        mock.assert_async().await;
    }
    assert_eq!(result.cost.as_ref().unwrap().used, 4.0);
    assert_eq!(extra_window(&result, "requests"), "3 requests");
    assert_eq!(extra_window(&result, "tokens"), "45 tokens");
}

async fn fetch_with_page_body(body: &str) -> ProviderError {
    let mut server = Server::new_async().await;
    let _costs = mock_page(&mut server, COSTS_PATH, Matcher::Any, body).await;
    let _completions = mock_page(&mut server, COMPLETIONS_PATH, Matcher::Any, EMPTY_PAGE).await;
    provider(&server)
        .fetch_admin_usage("sk-test", None, fixed_now(3_600))
        .await
        .expect_err("the page should be rejected")
}

#[tokio::test]
async fn openai_admin_usage_rejects_repeated_pagination_cursor() {
    let error = fetch_with_page_body(
        r#"{"object":"page","data":[],"has_more":true,"next_page":"same_page"}"#,
    )
    .await;
    assert!(
        matches!(&error, ProviderError::Parse(message) if message.contains("cursor repeated")),
        "{error}"
    );
}

#[tokio::test]
async fn openai_admin_usage_rejects_missing_pagination_cursor() {
    for body in [
        r#"{"object":"page","data":[],"has_more":true,"next_page":null}"#,
        r#"{"object":"page","data":[],"has_more":true,"next_page":"  "}"#,
        r#"{"object":"page","data":[],"has_more":true}"#,
    ] {
        let error = fetch_with_page_body(body).await;
        assert!(
            matches!(&error, ProviderError::Parse(message) if message.contains("cursor missing")),
            "{body}: {error}"
        );
    }
}

#[tokio::test]
async fn openai_admin_usage_rejects_malformed_pages() {
    for body in [
        r#"{"object":"page","has_more":false,"next_page":null}"#,
        r#"{"object":"page","data":[],"next_page":null}"#,
        r#"{"object":"page","data":[],"has_more":false,"next_page":7}"#,
    ] {
        let error = fetch_with_page_body(body).await;
        assert!(matches!(error, ProviderError::Parse(_)), "{body}: {error}");
    }
}

#[tokio::test]
async fn openai_admin_usage_rejects_pagination_beyond_one_hundred_pages() {
    let mut server = Server::new_async().await;
    let costs = server
        .mock("GET", COSTS_PATH)
        .match_query(Matcher::Any)
        .with_body_from_request(|request| {
            let path = request.path_and_query();
            let current = path
                .split("page=p")
                .nth(1)
                .and_then(|rest| rest.split('&').next())
                .and_then(|number| number.parse::<u32>().ok())
                .unwrap_or(0);
            format!(
                r#"{{"data":[],"has_more":true,"next_page":"p{}"}}"#,
                current + 1
            )
            .into_bytes()
        })
        .expect(MAX_PAGES_PER_RANGE)
        .create_async()
        .await;

    let error = provider(&server)
        .fetch_admin_usage("sk-test", None, fixed_now(3_600))
        .await
        .expect_err("the endless cursor chain should be rejected");

    costs.assert_async().await;
    assert!(
        matches!(&error, ProviderError::Parse(message) if message.contains("exceeded 100 pages")),
        "{error}"
    );
}

#[tokio::test]
async fn openai_admin_usage_retries_transient_completions_failure_once() {
    let mut server = Server::new_async().await;
    let costs = mock_page(&mut server, COSTS_PATH, Matcher::Any, EMPTY_PAGE)
        .await
        .expect(1);
    let failure = server
        .mock("GET", COMPLETIONS_PATH)
        .match_query(Matcher::Any)
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let success = mock_page(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Any,
        r#"{"object":"page","data":[{"start_time":1700000000,"end_time":1700086400,"results":[{"input_tokens":10,"output_tokens":5,"num_model_requests":1,"model":"gpt-5.2"}]}],"has_more":false,"next_page":null}"#,
    )
    .await
    .expect(1);

    let result = provider(&server)
        .fetch_admin_usage("sk-test", None, fixed_now(3_600))
        .await
        .unwrap();

    costs.assert_async().await;
    failure.assert_async().await;
    success.assert_async().await;
    assert_eq!(extra_window(&result, "tokens"), "15 tokens");
    assert_eq!(extra_window(&result, "requests"), "1 requests");
}

#[tokio::test]
async fn openai_admin_usage_retries_once_then_reports_the_status() {
    let mut server = Server::new_async().await;
    let costs = server
        .mock("GET", COSTS_PATH)
        .match_query(Matcher::Any)
        .with_status(429)
        .with_header("Retry-After", "600")
        .with_body(r#"{"error":{"message":"slow down"}}"#)
        .expect(2)
        .create_async()
        .await;

    let error = provider(&server)
        .fetch_admin_usage("sk-test", None, fixed_now(3_600))
        .await
        .expect_err("a persistent 429 should fail");

    costs.assert_async().await;
    assert!(
        matches!(&error, ProviderError::Other(message)
            if message.contains("429") && message.contains("slow down")),
        "{error}"
    );
}

#[tokio::test]
async fn openai_admin_usage_never_retries_auth_failures() {
    for status in [401, 403] {
        let mut server = Server::new_async().await;
        let costs = server
            .mock("GET", COSTS_PATH)
            .match_query(Matcher::Any)
            .with_status(status)
            .expect(1)
            .create_async()
            .await;

        let error = provider(&server)
            .fetch_admin_usage("sk-test", None, fixed_now(3_600))
            .await
            .expect_err("auth failures should surface");

        costs.assert_async().await;
        assert!(matches!(error, ProviderError::AuthRequired), "{status}");
    }
}

const GRANTS_BODY: &str = r#"{"total_granted":100.0,"total_used":25.0,"total_available":75.0}"#;

async fn mock_admin_status(server: &mut ServerGuard, status: usize) -> mockito::Mock {
    server
        .mock("GET", COSTS_PATH)
        .match_query(Matcher::Any)
        .with_status(status)
        .create_async()
        .await
}

#[tokio::test]
async fn openai_unscoped_key_falls_back_to_balance_on_admin_auth_failure() {
    let mut server = Server::new_async().await;
    let _costs = mock_admin_status(&mut server, 403).await;
    let grants = mock_response_expect(&mut server, "GET", GRANTS_PATH, 200, GRANTS_BODY, 1).await;

    let result = provider(&server)
        .fetch_admin_or_balance(&admin_credential(), None, fixed_now(3_600))
        .await
        .unwrap();

    grants.assert_async().await;
    assert_eq!(result.source_label, "billing-api");
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("API balance: $75.00")
    );
    // The balance endpoint has no per-day data, so there is no chart history.
    assert!(result.open_ai_api_usage.is_none());
}

#[tokio::test]
async fn openai_unscoped_key_falls_back_to_balance_during_admin_outage() {
    let mut server = Server::new_async().await;
    let costs = mock_admin_status(&mut server, 500).await.expect(2);
    let grants = mock_response_expect(&mut server, "GET", GRANTS_PATH, 200, GRANTS_BODY, 1).await;

    let result = provider(&server)
        .fetch_admin_or_balance(&admin_credential(), None, fixed_now(3_600))
        .await
        .unwrap();

    costs.assert_async().await;
    grants.assert_async().await;
    assert_eq!(result.source_label, "billing-api");
}

#[tokio::test]
async fn openai_project_scoped_admin_key_never_uses_the_unfiltered_balance() {
    let mut server = Server::new_async().await;
    let _costs = mock_admin_status(&mut server, 403).await;
    let grants = mock_response_expect(&mut server, "GET", GRANTS_PATH, 200, GRANTS_BODY, 0).await;

    let error = provider(&server)
        .fetch_admin_or_balance(&admin_credential(), Some("proj_abc"), fixed_now(3_600))
        .await
        .expect_err("a project-scoped admin failure should not fall back");

    grants.assert_async().await;
    assert!(matches!(error, ProviderError::AuthRequired));
}

#[tokio::test]
async fn openai_project_scoped_plain_key_keeps_the_balance_fallback() {
    let mut server = Server::new_async().await;
    let _costs = mock_admin_status(&mut server, 403).await;
    let grants = mock_response_expect(&mut server, "GET", GRANTS_PATH, 200, GRANTS_BODY, 1).await;
    let credential = ApiCredential {
        key: "sk-plain".to_string(),
        is_admin: false,
    };

    let result = provider(&server)
        .fetch_admin_or_balance(&credential, Some("proj_abc"), fixed_now(3_600))
        .await
        .unwrap();

    grants.assert_async().await;
    assert_eq!(result.source_label, "billing-api");
}

#[tokio::test]
async fn openai_balance_failure_keeps_the_admin_outage_error() {
    let mut server = Server::new_async().await;
    let _costs = mock_admin_status(&mut server, 500).await;
    let _grants = mock_status(&mut server, "GET", GRANTS_PATH, 404).await;

    let error = provider(&server)
        .fetch_admin_or_balance(&admin_credential(), None, fixed_now(3_600))
        .await
        .expect_err("both endpoints failed");

    assert!(
        matches!(&error, ProviderError::Other(message) if message.contains("costs returned status 500")),
        "{error}"
    );
}
