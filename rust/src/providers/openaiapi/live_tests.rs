//! Fixtures ported from upstream 0.75.0 `OpenAILiveUsageTests`, `OpenAIThroughputTests`
//! and `OpenAIOrgRateLimitsTests` (#4495).

use super::live::{
    LiveMinute, LiveModelFilter, LiveModelUsage, LiveUsageError, OpenAiLiveUsage, build,
};
use super::org_limits::{
    OrgLimitResolver, OrgRateLimit, OrgRateLimits, OrgRateLimitsError, throughput_limit_utilization,
};
use super::*;
use crate::settings::ModelRateLimit;
use mockito::{Matcher, Server, ServerGuard};

const COSTS_PATH: &str = "/v1/organization/costs";
const COMPLETIONS_PATH: &str = "/v1/organization/usage/completions";
const PROJECTS_PATH: &str = "/v1/organization/projects";
const EMPTY_PAGE: &str = r#"{"object":"page","data":[],"has_more":false,"next_page":null}"#;
/// Upstream fixture clock: 25 seconds into the minute starting at 120000.
const NOW: i64 = 120_025;
/// A realistic refresh clock so the daily ranges stay positive: 25 seconds into `BASE`.
const BASE: i64 = 1_700_179_200;
const REFRESH_NOW: i64 = BASE + 25;
const REFRESH_START: i64 = BASE - 59 * 60;

fn at(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).single().unwrap()
}

fn provider(server: &ServerGuard) -> OpenAIApiProvider {
    let mut provider = OpenAIApiProvider::new();
    provider.client = Client::builder()
        .no_proxy()
        .build()
        .expect("the test client should build");
    provider.endpoints = Endpoints {
        credit_grants: format!("{}/v1/dashboard/billing/credit_grants", server.url()),
        costs: format!("{}{COSTS_PATH}", server.url()),
        completions: format!("{}{COMPLETIONS_PATH}", server.url()),
        projects: format!("{}{PROJECTS_PATH}", server.url()),
    };
    provider.retry = RetryPolicy {
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    };
    provider
}

fn admin() -> ApiCredential {
    ApiCredential {
        key: "test-admin-key".to_string(),
        is_admin: true,
    }
}

fn page(buckets: &str, more: bool, cursor: Option<&str>) -> String {
    let next = cursor.map_or_else(|| "null".to_string(), |cursor| format!("\"{cursor}\""));
    format!(r#"{{"data":[{buckets}],"has_more":{more},"next_page":{next},"unknown":true}}"#)
}

/// The exact first-page query `live_query` sends (parameter order included).
fn live_query_string(start: i64, end: i64, limit: u32, project: Option<&str>) -> String {
    let mut query = format!(
        "start_time={start}&end_time={end}&bucket_width=1m&group_by=model&group_by=batch&limit={limit}"
    );
    if let Some(project) = project {
        query.push_str("&project_ids=");
        query.push_str(&project.replace('&', "%26").replace(' ', "+"));
    }
    query
}

async fn mock(
    server: &mut ServerGuard,
    path: &str,
    query: Matcher,
    status: usize,
    body: &str,
) -> mockito::Mock {
    server
        .mock("GET", path)
        .match_query(query)
        .match_header("authorization", "Bearer test-admin-key")
        .with_status(status)
        .with_body(body)
        .create_async()
        .await
}

fn model(name: &str, input: u64, cached: u64, output: u64, requests: u64) -> LiveModelUsage {
    LiveModelUsage {
        name: name.to_string(),
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        requests,
        batch_tokens: 0,
        batch_requests: 0,
    }
}

fn snapshot(minutes: Vec<LiveMinute>, project_id: Option<&str>) -> OpenAiLiveUsage {
    OpenAiLiveUsage {
        minutes,
        window_minutes: 15,
        fetched_at: NOW,
        project_id: project_id.map(str::to_string),
    }
}

/// Upstream `frontierSnapshot`: 15 minutes from 119160 with one model per active minute.
fn frontier_snapshot(active: &[(i64, &str, u64)]) -> OpenAiLiveUsage {
    snapshot(
        (0..15_i64)
            .map(|index| LiveMinute {
                start: 119_160 + index * 60,
                models: active
                    .iter()
                    .filter(|(minute, _, _)| *minute == index)
                    .map(|(_, name, tokens)| model(name, *tokens, 0, 0, 1))
                    .collect(),
            })
            .collect(),
        None,
    )
}

// ---- On-demand Live Usage window -------------------------------------------------------

#[tokio::test]
async fn window_requests_minute_buckets_model_and_batch_grouping_project_scope_and_pages() {
    let mut server = Server::new_async().await;
    let first_query = live_query_string(119_160, NOW, 15, Some("proj_&test"));
    let first = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(first_query.clone()),
        200,
        &page("", true, Some("next & page")),
    )
    .await
    .expect(1);
    let second = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(format!("{first_query}&page=next+%26+page")),
        200,
        &page("", false, None),
    )
    .await
    .expect(1);

    let live = provider(&server)
        .fetch_live_window("test-admin-key", Some("proj_&test"), 15, at(NOW))
        .await
        .unwrap();

    first.assert_async().await;
    second.assert_async().await;
    assert_eq!(live.project_id.as_deref(), Some("proj_&test"));
    assert_eq!(live.minutes.len(), 15);
    assert_eq!(live.minutes.first().unwrap().start, 119_160);
    assert_eq!(live.minutes.last().unwrap().start, 120_000);
}

#[tokio::test]
async fn window_parsing_merges_pages_floors_buckets_drops_outside_minutes_and_defaults_counts() {
    let mut server = Server::new_async().await;
    let first_query = live_query_string(119_160, NOW, 15, None);
    let _first = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(first_query.clone()),
        200,
        &page(
            r#"{"start_time":119947,"results":[
                 {"model":"model-a","input_tokens":100,"input_cached_tokens":20,"output_tokens":50,"num_model_requests":2},
                 {"model":"model-b","output_tokens":30}, {"model":null}, {"model":""}]},
               {"start_time":119100,"results":[{"input_tokens":999}]},
               {"start_time":120060,"results":[{"input_tokens":999}]}"#,
            true,
            Some("two"),
        ),
    )
    .await;
    let _second = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(format!("{first_query}&page=two")),
        200,
        &page(
            r#"{"start_time":119940,"results":[{"model":"model-a","input_tokens":5,"output_tokens":5}]},
               {"start_time":120000,"results":[{"model":"model-a","input_tokens":500}]}"#,
            false,
            None,
        ),
    )
    .await;

    let live = provider(&server)
        .fetch_live_window("test-admin-key", None, 15, at(NOW))
        .await
        .unwrap();

    assert_eq!(live.model_names(), ["model-a", "model-b", "unknown"]);
    let all = live.series(LiveModelFilter::AllModels);
    assert_eq!(all.len(), 15);
    assert!(all[..13].iter().all(|point| point.total_tokens == 0));
    assert_eq!(all[13].total_tokens, 190);
    assert_eq!(all[13].uncached_input_tokens, 85);
    assert_eq!(all[13].cached_input_tokens, 20);
    assert_eq!(all[13].output_tokens, 85);
    assert_eq!(all[13].requests, 2);
    assert!(!all[13].is_in_progress);
    assert!(all[14].is_in_progress);
    assert_eq!(
        live.series(LiveModelFilter::Model("model-b"))[13].total_tokens,
        30
    );
    assert!(
        live.series(LiveModelFilter::Model("absent"))
            .iter()
            .all(|point| point.total_tokens == 0)
    );
    let summary = live.summary(LiveModelFilter::AllModels);
    assert_eq!(summary.latest_tpm, 190);
    assert_eq!(summary.latest_minute_start, Some(all[13].start));
    assert_eq!(summary.five_minute_average_tpm, 38.0);
    // The in-progress minute's partial 500 tokens count toward totals but never become the peak.
    assert_eq!(summary.peak_tpm, 190);
    assert_eq!(summary.peak_minute_start, Some(all[13].start));
    assert_eq!(summary.total_tokens, 690);
    assert_eq!(summary.input_tokens, 605);
    assert_eq!(summary.cached_input_tokens, 20);
    assert_eq!(summary.output_tokens, 85);
    assert_eq!(summary.requests, 2);
    let row = &live.model_rows()[0];
    assert_eq!(
        (
            row.name.as_str(),
            row.latest_tpm,
            row.peak_tpm,
            row.total_tokens,
            row.requests
        ),
        ("model-a", 160, 160, 660, 2)
    );
}

#[tokio::test]
async fn window_model_ordering_breaks_ties_by_name_and_stacks_five_models_plus_other() {
    let mut server = Server::new_async().await;
    let entries = (1..=7)
        .map(|index| format!(r#"{{"model":"model-{index}","input_tokens":{index}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let _page = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Any,
        200,
        &page(
            &format!(r#"{{"start_time":119940,"results":[{entries}]}}"#),
            false,
            None,
        ),
    )
    .await;

    let live = provider(&server)
        .fetch_live_window("test-admin-key", None, 15, at(NOW))
        .await
        .unwrap();

    assert_eq!(
        live.model_names(),
        [
            "model-7", "model-6", "model-5", "model-4", "model-3", "model-2", "model-1"
        ]
    );
    let stacks = live.stacked_by_model(5);
    assert_eq!(stacks.len(), 15 * 6);
    let other: u64 = stacks
        .iter()
        .filter(|stack| stack.model_or_other == "Other")
        .map(|stack| stack.tokens)
        .sum();
    assert_eq!(other, 3);
    assert_eq!(
        stacks.iter().map(|stack| stack.tokens).sum::<u64>(),
        live.summary(LiveModelFilter::AllModels).total_tokens
    );

    let tied = snapshot(
        vec![LiveMinute {
            start: NOW,
            models: vec![model("z", 1, 0, 0, 0), model("a", 1, 0, 0, 0)],
        }],
        None,
    );
    assert_eq!(tied.model_names(), ["a", "z"]);
}

#[tokio::test]
async fn window_http_errors_map_to_typed_errors() {
    for (status, expected) in [
        (401, LiveUsageError::AdminKeyRequired),
        (403, LiveUsageError::AdminKeyRequired),
        (429, LiveUsageError::RateLimited),
        (500, LiveUsageError::ApiFailure(500)),
    ] {
        let mut server = Server::new_async().await;
        let _page = mock(&mut server, COMPLETIONS_PATH, Matcher::Any, status, "{}").await;
        let error = provider(&server)
            .fetch_live_window("test-admin-key", None, 60, at(NOW))
            .await
            .unwrap_err();
        assert_eq!(error, expected, "{status}");
    }
}

#[tokio::test]
async fn window_malformed_responses_and_invalid_counts_are_parse_errors() {
    for body in [
        "not json",
        "{}",
        r#"{"data":null}"#,
        r#"{"data":[{"results":[]}]}"#,
        r#"{"data":[{"start_time":120000}]}"#,
        r#"{"data":[{"start_time":120000,"results":[{"input_tokens":-1}]}]}"#,
        r#"{"data":[{"start_time":120000,"results":[{"output_tokens":1.5}]}]}"#,
        r#"{"data":[{"start_time":120000,"results":[{"num_model_requests":true}]}]}"#,
        r#"{"data":[{"start_time":120000,"results":[{"input_audio_tokens":-1}]}]}"#,
        r#"{"data":[{"start_time":120000,"results":[{"input_tokens":9223372036854775807,"output_tokens":1}]}]}"#,
        r#"{"data":[{"start_time":119940,"results":[{"batch":"yes"}]}]}"#,
        r#"{"data":[{"start_time":119940,"results":[{"batch":1}]}]}"#,
        r#"{"data":[{"start_time":119940,"results":[{"batch":[]}]}]}"#,
        r#"{"data":[{"start_time":119940,"results":[{"batch":{}}]}]}"#,
        r#"{"data":[{"start_time":119940,"results":[{"batch":true,"input_tokens":9007199254740991,"output_tokens":1}]}]}"#,
        r#"{"data":[{"start_time":119940,"results":[{"batch":true,"input_tokens":9223372036854775807,"output_tokens":1}]}]}"#,
    ] {
        let mut server = Server::new_async().await;
        let _page = mock(&mut server, COMPLETIONS_PATH, Matcher::Any, 200, body).await;
        let error = provider(&server)
            .fetch_live_window("test-admin-key", None, 60, at(NOW))
            .await
            .unwrap_err();
        assert_eq!(error, LiveUsageError::ParseFailure, "{body}");
    }
}

#[tokio::test]
async fn window_pagination_rejects_missing_repeated_and_excessive_cursors() {
    let first_query = live_query_string(116_460, NOW, 60, None);

    let mut server = Server::new_async().await;
    let _missing = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(first_query.clone()),
        200,
        &page("", true, None),
    )
    .await;
    let error = provider(&server)
        .fetch_live_window("test-admin-key", None, 60, at(NOW))
        .await
        .unwrap_err();
    assert_eq!(error, LiveUsageError::ParseFailure);

    let mut server = Server::new_async().await;
    let _first = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(first_query.clone()),
        200,
        &page("", true, Some("same")),
    )
    .await;
    let repeated = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(format!("{first_query}&page=same")),
        200,
        &page("", true, Some("same")),
    )
    .await
    .expect(1);
    let error = provider(&server)
        .fetch_live_window("test-admin-key", None, 60, at(NOW))
        .await
        .unwrap_err();
    assert_eq!(error, LiveUsageError::ParseFailure);
    repeated.assert_async().await;

    let mut server = Server::new_async().await;
    let first = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(first_query.clone()),
        200,
        &page("", true, Some("page-0")),
    )
    .await
    .expect(1);
    let mut later = Vec::new();
    for index in 0..19 {
        let next = format!("page-{}", index + 1);
        later.push(
            mock(
                &mut server,
                COMPLETIONS_PATH,
                Matcher::Exact(format!("{first_query}&page=page-{index}")),
                200,
                &page("", true, Some(&next)),
            )
            .await
            .expect(1),
        );
    }
    let beyond = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(format!("{first_query}&page=page-19")),
        200,
        EMPTY_PAGE,
    )
    .await
    .expect(0);
    let error = provider(&server)
        .fetch_live_window("test-admin-key", None, 60, at(NOW))
        .await
        .unwrap_err();
    assert_eq!(error, LiveUsageError::ParseFailure);
    // Twenty requests in total: the first page plus nineteen cursors, never a 21st.
    first.assert_async().await;
    for mock in &later {
        mock.assert_async().await;
    }
    beyond.assert_async().await;
}

#[tokio::test]
async fn window_unsupported_ranges_never_request_the_network() {
    let mut server = Server::new_async().await;
    let never = mock(&mut server, COMPLETIONS_PATH, Matcher::Any, 200, EMPTY_PAGE)
        .await
        .expect(0);
    let provider = provider(&server);
    for window in [0, 30, 61] {
        assert_eq!(
            provider
                .fetch_live_window("test-admin-key", None, window, at(NOW))
                .await
                .unwrap_err(),
            LiveUsageError::InvalidWindow
        );
        assert_eq!(
            provider
                .fetch_live_usage(Some("test-admin-key"), None, window)
                .await
                .unwrap_err(),
            LiveUsageError::InvalidWindow
        );
    }
    never.assert_async().await;
}

#[tokio::test]
async fn window_unscoped_empty_windows_have_exact_ascending_grids() {
    for minutes in [15_u32, 60, 360] {
        let mut server = Server::new_async().await;
        let first = NOW / 60 * 60 - (i64::from(minutes) - 1) * 60;
        let unscoped = mock(
            &mut server,
            COMPLETIONS_PATH,
            Matcher::Exact(live_query_string(first, NOW, minutes, None)),
            200,
            &page("", false, None),
        )
        .await
        .expect(1);
        let live = provider(&server)
            .fetch_live_window("test-admin-key", Some("  "), minutes, at(NOW))
            .await
            .unwrap();
        unscoped.assert_async().await;
        assert_eq!(live.minutes.len(), minutes as usize);
        assert_eq!(live.project_id, None);
        let summary = live.summary(LiveModelFilter::AllModels);
        assert_eq!(summary.total_tokens, 0);
        assert_eq!(summary.five_minute_average_tpm, 0.0);
        assert!(
            live.minutes
                .windows(2)
                .all(|pair| pair[1].start - pair[0].start == 60)
        );
        assert_eq!(
            live.series(LiveModelFilter::AllModels)
                .iter()
                .filter(|point| point.is_in_progress)
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn window_batch_decoding_keeps_audio_inside_totals_and_defaults_to_standard() {
    for batch in ["true", "false", "null", "missing"] {
        let field = if batch == "missing" {
            String::new()
        } else {
            format!(r#""batch":{batch},"#)
        };
        let body = page(
            &format!(
                r#"{{"start_time":119940,"results":[{{{field}"model":"model-fixture",
                "input_tokens":100,"input_cached_tokens":60,"output_tokens":20,"num_model_requests":3,
                "input_audio_tokens":40,"output_audio_tokens":10}}]}}"#
            ),
            false,
            None,
        );
        let mut server = Server::new_async().await;
        let _page = mock(&mut server, COMPLETIONS_PATH, Matcher::Any, 200, &body).await;
        let live = provider(&server)
            .fetch_live_window("test-admin-key", None, 15, at(NOW))
            .await
            .unwrap();
        let point = live
            .series(LiveModelFilter::AllModels)
            .into_iter()
            .find(|point| point.total_tokens > 0)
            .unwrap();
        let is_batch = batch == "true";
        assert_eq!(point.total_tokens, 120, "{batch}");
        assert_eq!(point.batch_tokens, if is_batch { 120 } else { 0 });
        assert_eq!(point.batch_requests, if is_batch { 3 } else { 0 });
        assert_eq!(point.limit_counted_tokens(), if is_batch { 0 } else { 120 });
        assert_eq!(point.limit_counted_requests(), if is_batch { 0 } else { 3 });
    }
}

// ---- Snapshot math ---------------------------------------------------------------------

#[test]
fn reporting_frontier_skips_lagging_trailing_zeros_across_all_models() {
    let live = frontier_snapshot(&[(8, "model-a", 80), (11, "model-b", 110)]);
    assert_eq!(live.reporting_frontier(), Some(live.minutes[11].start));
    assert_eq!(live.summary(LiveModelFilter::AllModels).latest_tpm, 110);
    let pending: Vec<i64> = live
        .series(LiveModelFilter::AllModels)
        .iter()
        .filter(|point| point.is_pending)
        .map(|point| point.start)
        .collect();
    let tail: Vec<i64> = live.minutes[12..]
        .iter()
        .map(|minute| minute.start)
        .collect();
    assert_eq!(pending, tail);
    assert_eq!(
        live.stacked_by_model(5)
            .iter()
            .filter(|stack| stack.is_pending)
            .count(),
        6
    );
}

#[test]
fn peaks_ignore_partial_minutes_after_the_reporting_frontier() {
    let live = frontier_snapshot(&[(8, "model-a", 80), (14, "model-a", 900)]);
    assert_eq!(live.reporting_frontier(), Some(live.minutes[8].start));
    let summary = live.summary(LiveModelFilter::AllModels);
    assert_eq!(summary.peak_tpm, 80);
    assert_eq!(summary.peak_minute_start, Some(live.minutes[8].start));
    let limits = OrgLimitResolver::new(&[ModelRateLimit::new("model-a", 1000, Some(10))], None);
    let utilization = live
        .utilization(LiveModelFilter::Model("model-a"), &limits)
        .unwrap();
    assert_eq!(utilization.peak_tpm_percent, 8.0);
    assert_eq!(utilization.peak_rpm_percent, Some(10.0));
    let throughput = live.throughput();
    assert_eq!(throughput.peak_tokens_per_minute, 80);
    assert_eq!(throughput.models[0].peak_limit_counted_tokens, 80);
}

#[test]
fn model_ranking_ignores_pending_only_volume_for_the_top_series() {
    let mut minutes: Vec<LiveMinute> = (0..15)
        .map(|index| LiveMinute {
            start: 119_160 + index * 60,
            models: Vec::new(),
        })
        .collect();
    minutes[8].models = (1..=6)
        .map(|index| model(&format!("model-{index}"), 100 - index, 0, 0, 1))
        .collect();
    minutes[14].models = vec![model("pending-model", 10_000, 0, 0, 1)];
    let live = snapshot(minutes, None);
    let names = live.model_names();
    assert_eq!(
        names[..5],
        ["model-1", "model-2", "model-3", "model-4", "model-5"]
    );
    assert_eq!(names.last().map(String::as_str), Some("pending-model"));
    let top: HashSet<String> = live
        .stacked_by_model(5)
        .into_iter()
        .map(|stack| stack.model_or_other)
        .collect();
    assert!(top.contains("model-5"));
    assert!(!top.contains("pending-model"));
    assert_eq!(live.throughput().models[0].name, "model-1");
}

#[test]
fn reporting_frontier_resets_to_real_zeros_after_ten_idle_minutes() {
    let live = frontier_snapshot(&[(3, "model-a", 100)]);
    assert_eq!(live.reporting_frontier(), Some(live.minutes[13].start));
    let summary = live.summary(LiveModelFilter::AllModels);
    assert_eq!(summary.latest_tpm, 0);
    assert_eq!(summary.five_minute_average_tpm, 0.0);
    assert_eq!(
        live.series(LiveModelFilter::AllModels)
            .iter()
            .filter(|point| point.is_pending)
            .count(),
        1
    );
    // The oldest of the ten complete minutes is still eligible.
    let boundary = frontier_snapshot(&[(4, "model-a", 100)]);
    assert_eq!(
        boundary.reporting_frontier(),
        Some(boundary.minutes[4].start)
    );
}

#[test]
fn frontier_is_shared_across_filters_so_an_idle_model_reports_zero() {
    let live = frontier_snapshot(&[(8, "model-a", 80), (11, "model-b", 110)]);
    let idle = live.summary(LiveModelFilter::Model("model-a"));
    assert_eq!(idle.latest_tpm, 0);
    assert_eq!(idle.latest_minute_start, Some(live.minutes[11].start));
    assert_eq!(
        live.summary(LiveModelFilter::Model("absent"))
            .latest_minute_start,
        idle.latest_minute_start
    );
}

#[test]
fn five_minute_average_ends_at_the_reporting_frontier_including_idle_minutes() {
    let live = frontier_snapshot(&[
        (6, "model-a", 999),
        (7, "model-a", 50),
        (8, "model-a", 100),
        (11, "model-b", 150),
    ]);
    assert_eq!(
        live.summary(LiveModelFilter::AllModels)
            .five_minute_average_tpm,
        60.0
    );
    assert_eq!(
        live.summary(LiveModelFilter::Model("model-a"))
            .five_minute_average_tpm,
        30.0
    );
    assert_eq!(
        live.summary(LiveModelFilter::Model("model-b"))
            .five_minute_average_tpm,
        30.0
    );
    let limits = OrgLimitResolver::new(&[ModelRateLimit::new("model-b", 300, None)], None);
    assert_eq!(
        live.utilization(LiveModelFilter::Model("model-b"), &limits)
            .unwrap()
            .latest_tpm_percent,
        50.0
    );
}

fn batch_row(input: u64, cached: u64, output: u64, requests: u64, batch: bool) -> LiveModelUsage {
    LiveModelUsage {
        batch_tokens: if batch { input + output } else { 0 },
        batch_requests: if batch { requests } else { 0 },
        ..model("model-fixture", input, cached, output, requests)
    }
}

#[test]
fn batch_traffic_stays_in_totals_while_utilization_uses_standard_peaks() {
    let live = build(
        vec![
            LiveMinute {
                start: 119_880,
                models: vec![
                    batch_row(900, 800, 100, 90, true),
                    batch_row(80, 70, 20, 2, false),
                ],
            },
            LiveMinute {
                start: 119_940,
                models: vec![
                    batch_row(40, 30, 10, 5, true),
                    batch_row(160, 150, 40, 4, false),
                    model("model-second", 90, 0, 0, 1),
                ],
            },
        ],
        15,
        NOW,
        None,
    )
    .unwrap();
    let points = live.series(LiveModelFilter::Model("model-fixture"));
    let tail = &points[points.len() - 3..];
    let field = |get: fn(&super::live::LivePoint) -> u64| tail.iter().map(get).collect::<Vec<_>>();
    assert_eq!(field(|point| point.total_tokens), [1100, 250, 0]);
    assert_eq!(field(|point| point.batch_tokens), [1000, 50, 0]);
    assert_eq!(field(|point| point.batch_requests), [90, 5, 0]);
    assert_eq!(field(|point| point.limit_counted_tokens()), [100, 200, 0]);
    assert_eq!(field(|point| point.limit_counted_requests()), [2, 4, 0]);
    assert_eq!(live.minutes[12].models.len(), 1);
    assert_eq!(live.model_names(), ["model-fixture", "model-second"]);
    assert_eq!(live.reporting_frontier(), Some(119_940));
    let summary = live.summary(LiveModelFilter::Model("model-fixture"));
    assert_eq!(summary.total_tokens, 1350);
    assert_eq!(summary.batch_tokens, 1050);
    assert_eq!(summary.input_tokens, 1180);
    assert_eq!(summary.cached_input_tokens, 1050);
    assert_eq!(summary.output_tokens, 170);
    assert_eq!(summary.requests, 101);
    assert_eq!(summary.latest_tpm, 250);
    assert_eq!(summary.peak_tpm, 1100);
    assert_eq!(summary.peak_minute_start, Some(119_880));
    let all = live.summary(LiveModelFilter::AllModels);
    assert_eq!((all.total_tokens, all.batch_tokens), (1440, 1050));
    assert_eq!(
        live.stacked_by_model(5)
            .iter()
            .map(|stack| stack.tokens)
            .sum::<u64>(),
        1440
    );
    assert_eq!(live.model_rows()[0].total_tokens, 1350);
    assert_eq!(live.model_rows()[0].requests, 101);

    let limits = OrgLimitResolver::new(
        &[
            ModelRateLimit::new("model-fixture", 400, Some(8)),
            ModelRateLimit::new("model-second", 100, None),
        ],
        None,
    );
    let utilization = live
        .utilization(LiveModelFilter::Model("model-fixture"), &limits)
        .unwrap();
    assert_eq!(utilization.latest_tpm_percent, 50.0);
    assert_eq!(utilization.peak_tpm_percent, 50.0);
    assert_eq!(utilization.peak_rpm_percent, Some(50.0));
    let rows = live.model_rows_with_limits(&limits);
    assert_eq!(rows[0].peak_tpm_percent, Some(50.0));
    assert_eq!(rows[0].latest_tpm_percent, Some(50.0));
    let highest = live.highest_utilization(&limits).unwrap();
    assert_eq!(highest.name, "model-second");
    assert_eq!(highest.peak_tpm_percent, Some(90.0));

    let throughput = live.throughput();
    let tail = &throughput.minutes[throughput.minutes.len() - 3..];
    assert_eq!(
        tail.iter()
            .map(|minute| minute.total_tokens)
            .collect::<Vec<_>>(),
        [1100, 340, 0]
    );
    assert_eq!(
        tail.iter()
            .map(|minute| minute.requests)
            .collect::<Vec<_>>(),
        [92, 10, 0]
    );
    assert_eq!(throughput.models[0].peak_tpm, 1100);
    assert_eq!(throughput.models[0].peak_limit_counted_tokens, 200);
}

#[test]
fn builder_rejects_invalid_batch_subsets_and_bad_windows() {
    for (tokens, requests) in [(121, 0), (0, 4)] {
        let row = LiveModelUsage {
            batch_tokens: tokens,
            batch_requests: requests,
            ..model("model-fixture", 100, 60, 20, 3)
        };
        let error = build(
            vec![LiveMinute {
                start: NOW - 85,
                models: vec![row],
            }],
            15,
            NOW,
            None,
        )
        .unwrap_err();
        assert_eq!(error, LiveUsageError::ParseFailure);
    }
    assert!(build(Vec::new(), 30, NOW, None).is_err());
    assert!(build(Vec::new(), 15, -1, None).is_err());
    let cached_above_input = LiveMinute {
        start: NOW - 85,
        models: vec![model("m", 1, 2, 0, 0)],
    };
    assert!(build(vec![cached_above_input], 15, NOW, None).is_err());
}

#[test]
fn compact_throughput_keeps_every_model_with_aligned_limit_traffic_including_pending() {
    let models: Vec<LiveModelUsage> = (1..=7_u64)
        .map(|index| {
            model(
                &format!("model-{index}"),
                index * 10_000_000,
                index * 9_500_000,
                1_000_000,
                index,
            )
        })
        .collect();
    let live = build(
        vec![
            LiveMinute {
                start: NOW - 85,
                models,
            },
            LiveMinute {
                start: NOW - 25,
                models: vec![LiveModelUsage {
                    batch_tokens: 50,
                    ..model("model-1", 200, 0, 0, 1)
                }],
            },
        ],
        60,
        NOW,
        None,
    )
    .unwrap();
    let throughput = live.throughput();
    let names: Vec<&str> = throughput.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "model-7", "model-6", "model-5", "model-4", "model-3", "model-2", "model-1"
        ]
    );
    for compact in &throughput.models {
        let expected: Vec<u64> = live
            .series(LiveModelFilter::Model(&compact.name))
            .iter()
            .map(|point| point.limit_counted_tokens())
            .collect();
        assert_eq!(compact.limit_counted_tokens_by_minute, expected);
        assert_eq!(
            compact.limit_counted_tokens_by_minute.len(),
            throughput.minutes.len()
        );
    }
    assert!(throughput.minutes.last().unwrap().is_pending);
    assert_eq!(
        throughput
            .models
            .last()
            .unwrap()
            .limit_counted_tokens_by_minute
            .last(),
        Some(&150)
    );

    let manual = [ModelRateLimit::new("model-7", 40_000_000, None)];
    let limits = OrgLimitResolver::new(&manual, None);
    let utilization = live
        .utilization(LiveModelFilter::Model("model-7"), &limits)
        .unwrap();
    assert_eq!(
        live.summary(LiveModelFilter::Model("model-7")).peak_tpm,
        71_000_000
    );
    // Cached input counts toward TPM: 71M of a 40M limit.
    assert_eq!(utilization.peak_tpm_percent, 177.5);
    assert_eq!(utilization.latest_tpm_percent, 177.5);
    assert_eq!(
        live.highest_utilization(&limits).unwrap().peak_tpm_percent,
        Some(177.5)
    );
    assert_eq!(
        live.model_rows_with_limits(&limits)[0].peak_tpm_percent,
        Some(177.5)
    );

    let tile = throughput_limit_utilization(&throughput, &manual).unwrap();
    assert_eq!(tile.label, "model-7");
    assert_eq!(tile.peak_tokens, 71_000_000);
    assert_eq!(tile.tokens_per_minute_limit, 40_000_000);
    assert_eq!(tile.percent, 177.5);
}

#[test]
fn throughput_json_round_trips_through_serde() {
    let throughput = frontier_snapshot(&[(8, "model-a", 80)]).throughput();
    let encoded = serde_json::to_string(&throughput).unwrap();
    let decoded: crate::core::ProviderThroughput = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, throughput);
}

// ---- Per-refresh Tokens/min ------------------------------------------------------------

/// Upstream `livePage`, shifted onto the refresh clock.
fn refresh_live_page() -> String {
    format!(
        r#"{{"data":[
          {{"start_time":{a},"end_time":{b},"results":[{{
            "model":"model-fixture","input_tokens":96000000,"input_cached_tokens":92400000,
            "output_tokens":1000000,"num_model_requests":50}}]}},
          {{"start_time":{b},"end_time":{c},"results":[{{
            "model":"model-fixture","input_tokens":74000000,"input_cached_tokens":70400000,
            "output_tokens":1000000,"num_model_requests":50}}]}}
        ],"has_more":false}}"#,
        a = BASE - 240,
        b = BASE - 180,
        c = BASE - 120,
    )
}

/// One refresh bucket at `BASE - 180` with the given results.
fn refresh_bucket(results: &str) -> String {
    format!(
        r#"{{"data":[{{"start_time":{start},"end_time":{end},"results":[{results}]}}],"has_more":false}}"#,
        start = BASE - 180,
        end = BASE - 120,
    )
}

struct RefreshServer {
    server: ServerGuard,
    live: mockito::Mock,
}

async fn refresh_server(live_status: usize, live_body: &str) -> RefreshServer {
    let mut server = Server::new_async().await;
    let _costs = mock(&mut server, COSTS_PATH, Matcher::Any, 200, EMPTY_PAGE).await;
    let _daily = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::UrlEncoded("bucket_width".into(), "1d".into()),
        200,
        EMPTY_PAGE,
    )
    .await;
    let live = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::UrlEncoded("bucket_width".into(), "1m".into()),
        live_status,
        live_body,
    )
    .await;
    RefreshServer { server, live }
}

async fn refresh(server: &RefreshServer, project: Option<&str>, live: bool) -> ProviderFetchResult {
    provider(&server.server)
        .fetch_refresh(&admin(), project, at(REFRESH_NOW), live)
        .await
        .expect("the daily admin usage should succeed")
}

#[tokio::test]
async fn refresh_requests_one_scoped_minute_page_and_maps_lagging_throughput() {
    let mut server = Server::new_async().await;
    let _costs = mock(&mut server, COSTS_PATH, Matcher::Any, 200, EMPTY_PAGE).await;
    let _daily = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::UrlEncoded("bucket_width".into(), "1d".into()),
        200,
        EMPTY_PAGE,
    )
    .await;
    let live = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::Exact(live_query_string(
            REFRESH_START,
            REFRESH_NOW,
            60,
            Some("proj_fixture & scope"),
        )),
        200,
        &refresh_live_page(),
    )
    .await
    .expect(1);

    let result = provider(&server)
        .fetch_refresh(
            &admin(),
            Some("proj_fixture & scope"),
            at(REFRESH_NOW),
            true,
        )
        .await
        .unwrap();

    live.assert_async().await;
    assert!(result.open_ai_api_usage.is_some());
    let throughput = result.throughput.expect("live minutes attach on refresh");
    assert_eq!(throughput.minutes.len(), 60);
    assert_eq!(throughput.window_minutes, 60);
    assert_eq!(throughput.fetched_at, REFRESH_NOW);
    assert_eq!(throughput.frontier, Some(BASE - 180));
    assert_eq!(throughput.latest_tokens_per_minute, Some(75_000_000));
    assert_eq!(throughput.peak_tokens_per_minute, 97_000_000);
    assert_eq!(throughput.peak_at, Some(BASE - 240));
    assert_eq!(
        throughput.five_minute_average_tokens_per_minute,
        34_400_000.0
    );
    assert!(
        throughput.minutes[57..]
            .iter()
            .all(|minute| minute.is_pending)
    );
    assert_eq!(throughput.models[0].peak_limit_counted_tokens, 97_000_000);
    assert_eq!(
        throughput.project_id.as_deref(),
        Some("proj_fixture & scope")
    );
    // Project-scoped usage never reports organization limit utilization.
    assert!(
        throughput_limit_utilization(
            &throughput,
            &[ModelRateLimit::new("model-fixture", 1, None)]
        )
        .is_none()
    );

    // The refresh and the on-demand 60-minute window agree on the same buckets.
    let window = provider(&server)
        .fetch_live_window(
            "test-admin-key",
            Some("proj_fixture & scope"),
            60,
            at(REFRESH_NOW),
        )
        .await
        .unwrap();
    assert_eq!(window.throughput(), throughput);
}

#[tokio::test]
async fn refresh_live_failures_keep_daily_data_and_the_disabled_setting_skips_the_request() {
    for (status, body) in [
        (500, "{}"),
        (429, "{}"),
        (401, "{}"),
        (200, "malformed"),
        (200, r#"{"data":false}"#),
    ] {
        let server = refresh_server(status, body).await;
        let result = refresh(&server, None, true).await;
        assert!(result.throughput.is_none(), "{status} {body}");
        assert!(result.open_ai_api_usage.is_some());
        assert_eq!(result.source_label, "admin-api");
    }

    let mut server = refresh_server(200, &refresh_live_page()).await;
    server.live = server.live.expect(0);
    let result = refresh(&server, None, false).await;
    assert!(result.throughput.is_none());
    assert!(result.open_ai_api_usage.is_some());
    server.live.assert_async().await;
}

#[tokio::test]
async fn refresh_balance_fallback_never_requests_live_minutes() {
    let mut server = Server::new_async().await;
    let _costs = mock(&mut server, COSTS_PATH, Matcher::Any, 403, "{}").await;
    let _grants = server
        .mock("GET", "/v1/dashboard/billing/credit_grants")
        .with_body(r#"{"total_granted":100.0,"total_used":25.0,"total_available":75.0}"#)
        .create_async()
        .await;
    let live = mock(
        &mut server,
        COMPLETIONS_PATH,
        Matcher::UrlEncoded("bucket_width".into(), "1m".into()),
        200,
        &refresh_live_page(),
    )
    .await
    .expect(0);

    let result = provider(&server)
        .fetch_refresh(&admin(), None, at(REFRESH_NOW), true)
        .await
        .unwrap();

    assert_eq!(result.source_label, "billing-api");
    assert!(result.throughput.is_none());
    live.assert_async().await;
}

#[tokio::test]
async fn refresh_throughput_does_not_count_audio_breakdowns_twice() {
    let server = refresh_server(
        200,
        &refresh_bucket(
            r#"{"model":"model-fixture","input_tokens":100,"input_audio_tokens":40,"input_cached_tokens":60,
                "output_tokens":15,"output_audio_tokens":5,"num_model_requests":2}"#,
        ),
    )
    .await;
    let throughput = refresh(&server, None, true).await.throughput.unwrap();
    assert_eq!(throughput.peak_tokens_per_minute, 115);
    assert_eq!(throughput.models[0].peak_limit_counted_tokens, 115);
}

#[tokio::test]
async fn refresh_batch_rows_merge_by_trimmed_model_name() {
    let server = refresh_server(
        200,
        &refresh_bucket(
            r#"{"model":" model-fixture ","batch":true,"input_tokens":100,"input_cached_tokens":60,
                "output_tokens":20,"num_model_requests":3,"input_audio_tokens":40,"output_audio_tokens":10},
               {"model":"model-fixture","batch":false,"input_tokens":200,"input_cached_tokens":150,
                "output_tokens":30,"num_model_requests":2},
               {"model":"model-second","batch":null,"input_tokens":40,"num_model_requests":1}"#,
        ),
    )
    .await;
    let throughput = refresh(&server, None, true).await.throughput.unwrap();
    assert_eq!(throughput.models[0].name, "model-fixture");
    assert_eq!(throughput.models[0].peak_tpm, 350);
    assert_eq!(throughput.models[0].peak_limit_counted_tokens, 230);
    assert_eq!(throughput.peak_tokens_per_minute, 390);
}

#[tokio::test]
async fn refresh_invalid_live_payloads_discard_only_throughput() {
    let bucket = |start: i64, end: &str, results: &str| {
        format!(r#"{{"start_time":{start}{end},"results":[{results}]}}"#)
    };
    let row = r#"{"model":"m","input_tokens":10}"#;
    let aligned = BASE - 180;
    let too_many_buckets = (0..61)
        .map(|_| bucket(aligned, &format!(r#","end_time":{}"#, aligned + 60), row))
        .collect::<Vec<_>>()
        .join(",");
    let too_many_results = vec![row; 401].join(",");
    let mut bodies = vec![
        format!(r#"{{"data":[{too_many_buckets}],"has_more":false}}"#),
        format!(
            r#"{{"data":[{}]}}"#,
            bucket(aligned, &format!(r#","end_time":{}"#, aligned + 60), row)
        ),
        format!(
            r#"{{"data":[{}],"has_more":false}}"#,
            bucket(
                aligned + 1,
                &format!(r#","end_time":{}"#, aligned + 60),
                row
            )
        ),
        format!(
            r#"{{"data":[{}],"has_more":false}}"#,
            bucket(
                REFRESH_START - 60,
                &format!(r#","end_time":{REFRESH_START}"#),
                row
            )
        ),
        format!(
            r#"{{"data":[{}],"has_more":false}}"#,
            bucket(BASE + 60, &format!(r#","end_time":{}"#, BASE + 120), row)
        ),
        format!(
            r#"{{"data":[{}],"has_more":false}}"#,
            bucket(aligned, "", row)
        ),
        format!(
            r#"{{"data":[{}],"has_more":false}}"#,
            bucket(aligned, &format!(r#","end_time":{aligned}"#), row)
        ),
        format!(
            r#"{{"data":[{}],"has_more":false}}"#,
            bucket(
                aligned,
                &format!(r#","end_time":{}"#, aligned + 60),
                &too_many_results
            )
        ),
        refresh_bucket(r#"{"model":"m","input_tokens":1,"input_cached_tokens":2}"#),
        refresh_bucket(r#"{"model":"m","input_tokens":900719925475}"#),
    ];
    for batch in [r#""yes""#, "1", "[]", "{}"] {
        bodies.push(refresh_bucket(&format!(
            r#"{{"model":"m","batch":{batch},"input_tokens":10}}"#
        )));
    }
    for body in bodies {
        let server = refresh_server(200, &body).await;
        let result = refresh(&server, None, true).await;
        assert!(
            result.throughput.is_none(),
            "{}",
            &body[..body.len().min(160)]
        );
        assert!(result.open_ai_api_usage.is_some());
    }
}

#[tokio::test]
async fn refresh_model_bounds_count_merged_entries() {
    for count in [200, 201] {
        let rows: Vec<String> = (0..count)
            .flat_map(|index| {
                [true, false].map(|batch| {
                    format!(
                        r#"{{"model":"model-fixture-{index}","batch":{batch},"input_tokens":10}}"#
                    )
                })
            })
            .collect();
        // 201 unique models fit the raw cap but exceed the merged cap.
        let raw: Vec<String> = if count == 200 {
            rows
        } else {
            rows.into_iter().step_by(2).collect()
        };
        let server = refresh_server(200, &refresh_bucket(&raw.join(","))).await;
        let result = refresh(&server, None, true).await;
        if count == 200 {
            assert_eq!(result.throughput.unwrap().peak_tokens_per_minute, 4000);
        } else {
            assert!(result.throughput.is_none());
        }
        assert!(result.open_ai_api_usage.is_some());
    }
}

// ---- Organization rate limits ----------------------------------------------------------

fn list_page(rows: &str, more: bool, last_id: Option<&str>) -> String {
    let last = last_id.map_or_else(|| "null".to_string(), |id| format!("\"{id}\""));
    format!(r#"{{"object":"list","data":[{rows}],"has_more":{more},"last_id":{last}}}"#)
}

fn limit_row(model: &str, tpm: u64, rpm: u64) -> String {
    format!(
        r#"{{"model":"{model}","max_tokens_per_1_minute":{tpm},"max_requests_per_1_minute":{rpm}}}"#
    )
}

fn rate_limits_path(project: &str) -> String {
    format!("{PROJECTS_PATH}/{project}/rate_limits")
}

#[tokio::test]
async fn org_discovery_paginates_both_endpoints_and_takes_the_max_cap() {
    let mut server = Server::new_async().await;
    let mut mocks = vec![
        mock(
            &mut server,
            PROJECTS_PATH,
            Matcher::Exact("limit=100".into()),
            200,
            &list_page(
                r#"{"id":"proj_a","status":"active"},{"id":"proj_archived","status":"archived"}"#,
                true,
                Some("proj_page & 2"),
            ),
        )
        .await,
        mock(
            &mut server,
            PROJECTS_PATH,
            Matcher::Exact("limit=100&after=proj_page+%26+2".into()),
            200,
            &list_page(r#"{"id":"proj_b","status":"active"}"#, false, None),
        )
        .await,
        mock(
            &mut server,
            &rate_limits_path("proj_a"),
            Matcher::Exact("limit=100".into()),
            200,
            &list_page(&limit_row("gpt-5", 100, 20), true, Some("rl_next")),
        )
        .await,
        mock(
            &mut server,
            &rate_limits_path("proj_a"),
            Matcher::Exact("limit=100&after=rl_next".into()),
            200,
            &list_page(&limit_row("gpt-4o-mini", 50, 5), false, None),
        )
        .await,
        mock(
            &mut server,
            &rate_limits_path("proj_b"),
            Matcher::Exact("limit=100".into()),
            200,
            &list_page(&limit_row("gpt-5", 200, 10), false, None),
        )
        .await,
    ];
    let never = mock(
        &mut server,
        &rate_limits_path("proj_archived"),
        Matcher::Any,
        200,
        &list_page("", false, None),
    )
    .await
    .expect(0);

    let limits = provider(&server)
        .fetch_org_rate_limits_with("test-admin-key", at(NOW))
        .await
        .unwrap();

    for mock in mocks.drain(..) {
        mock.expect(1).assert_async().await;
    }
    never.assert_async().await;
    assert_eq!(limits.projects_sampled, 2);
    assert!(!limits.truncated);
    assert_eq!(limits.fetched_at, NOW);
    assert_eq!(
        limits.by_model.get("gpt-5"),
        Some(&OrgRateLimit {
            tokens_per_minute: 200,
            requests_per_minute: Some(20)
        })
    );
    assert_eq!(
        limits.by_model.get("gpt-4o-mini"),
        Some(&OrgRateLimit {
            tokens_per_minute: 50,
            requests_per_minute: Some(5)
        })
    );
}

#[tokio::test]
async fn org_projects_without_a_status_are_active() {
    let mut server = Server::new_async().await;
    let _projects = mock(
        &mut server,
        PROJECTS_PATH,
        Matcher::Any,
        200,
        &list_page(
            r#"{"id":"proj_a"},{"id":"proj_b","status":null},{"id":"proj_c","status":"archived"}"#,
            false,
            None,
        ),
    )
    .await;
    let _a = mock(
        &mut server,
        &rate_limits_path("proj_a"),
        Matcher::Any,
        200,
        &list_page(&limit_row("gpt-5", 100, 2), false, None),
    )
    .await;
    let _b = mock(
        &mut server,
        &rate_limits_path("proj_b"),
        Matcher::Any,
        200,
        &list_page(&limit_row("gpt-5", 300, 4), false, None),
    )
    .await;

    let limits = provider(&server)
        .fetch_org_rate_limits_with("test-admin-key", at(NOW))
        .await
        .unwrap();
    assert_eq!(limits.projects_sampled, 2);
    assert_eq!(
        limits.by_model.get("gpt-5"),
        Some(&OrgRateLimit {
            tokens_per_minute: 300,
            requests_per_minute: Some(4)
        })
    );
}

#[tokio::test]
async fn org_project_sample_truncates_after_twenty_five_active_projects() {
    for (total, truncated) in [(30, true), (25, false)] {
        let mut server = Server::new_async().await;
        let projects = (0..total)
            .map(|index| format!(r#"{{"id":"proj_{index}","status":"active"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let _projects = mock(
            &mut server,
            PROJECTS_PATH,
            Matcher::Exact("limit=100".into()),
            200,
            &list_page(&projects, false, None),
        )
        .await;
        let rate_limits = server
            .mock(
                "GET",
                Matcher::Regex(r"^/v1/organization/projects/proj_\d+/rate_limits$".into()),
            )
            .match_query(Matcher::Any)
            .with_body(list_page("", false, None))
            .expect(25)
            .create_async()
            .await;

        let limits = provider(&server)
            .fetch_org_rate_limits_with("test-admin-key", at(NOW))
            .await
            .unwrap();
        rate_limits.assert_async().await;
        assert_eq!(limits.projects_sampled, 25);
        assert_eq!(limits.truncated, truncated, "{total}");
    }
}

#[tokio::test]
async fn org_malformed_project_responses_fail_parsing() {
    for body in [
        "not json",
        "{}",
        r#"{"data":[],"has_more":true}"#,
        r#"{"data":[{"id":"proj_a"}],"has_more":false}"#,
    ] {
        let mut server = Server::new_async().await;
        let _projects = mock(&mut server, PROJECTS_PATH, Matcher::Any, 200, body).await;
        let _limits = mock(
            &mut server,
            &rate_limits_path("proj_a"),
            Matcher::Any,
            200,
            "{}",
        )
        .await;
        let error = provider(&server)
            .fetch_org_rate_limits_with("test-admin-key", at(NOW))
            .await
            .unwrap_err();
        assert_eq!(error, OrgRateLimitsError::ParseFailure, "{body}");
    }
}

async fn one_project_with_rows(rows: &str) -> Result<OrgRateLimits, OrgRateLimitsError> {
    let mut server = Server::new_async().await;
    let _projects = mock(
        &mut server,
        PROJECTS_PATH,
        Matcher::Any,
        200,
        &list_page(r#"{"id":"proj_a","status":"active"}"#, false, None),
    )
    .await;
    let _limits = mock(
        &mut server,
        &rate_limits_path("proj_a"),
        Matcher::Any,
        200,
        &list_page(rows, false, None),
    )
    .await;
    provider(&server)
        .fetch_org_rate_limits_with("test-admin-key", at(NOW))
        .await
}

#[tokio::test]
async fn org_rows_without_token_or_request_limits_are_tolerated() {
    let limits = one_project_with_rows(
        r#"{"model":"gpt-5","max_tokens_per_1_minute":100},
           {"model":"gpt-5","max_tokens_per_1_minute":80,"max_requests_per_1_minute":3},
           {"model":"sora-2","max_requests_per_1_minute":5},
           {"model":"gpt-4o","max_tokens_per_1_minute":0,"max_requests_per_1_minute":0}"#,
    )
    .await
    .unwrap();
    assert_eq!(limits.by_model.len(), 1);
    assert_eq!(
        limits.by_model.get("gpt-5"),
        Some(&OrgRateLimit {
            tokens_per_minute: 100,
            requests_per_minute: Some(3)
        })
    );
}

#[tokio::test]
async fn org_required_rate_limit_fields_are_validated() {
    for row in [
        r#"{"max_tokens_per_1_minute":100,"max_requests_per_1_minute":2}"#,
        r#"{"model":"","max_tokens_per_1_minute":100,"max_requests_per_1_minute":2}"#,
        r#"{"model":"gpt-5","max_tokens_per_1_minute":-1,"max_requests_per_1_minute":2}"#,
        r#"{"model":"gpt-5","max_tokens_per_1_minute":1.5,"max_requests_per_1_minute":2}"#,
        r#"{"model":"gpt-5","max_tokens_per_1_minute":100,"max_requests_per_1_minute":-2}"#,
    ] {
        assert_eq!(
            one_project_with_rows(row).await.unwrap_err(),
            OrgRateLimitsError::ParseFailure,
            "{row}"
        );
    }
}

#[tokio::test]
async fn org_repeated_and_excessive_cursors_are_rejected() {
    let mut server = Server::new_async().await;
    let _first = mock(
        &mut server,
        PROJECTS_PATH,
        Matcher::Exact("limit=100".into()),
        200,
        &list_page("", true, Some("same")),
    )
    .await;
    let _again = mock(
        &mut server,
        PROJECTS_PATH,
        Matcher::Exact("limit=100&after=same".into()),
        200,
        &list_page("", true, Some("same")),
    )
    .await;
    assert_eq!(
        provider(&server)
            .fetch_org_rate_limits_with("test-admin-key", at(NOW))
            .await
            .unwrap_err(),
        OrgRateLimitsError::ParseFailure
    );

    let mut server = Server::new_async().await;
    let _projects = mock(
        &mut server,
        PROJECTS_PATH,
        Matcher::Any,
        200,
        &list_page(r#"{"id":"proj_a","status":"active"}"#, false, None),
    )
    .await;
    let _first = mock(
        &mut server,
        &rate_limits_path("proj_a"),
        Matcher::Exact("limit=100".into()),
        200,
        &list_page("", true, Some("page-0")),
    )
    .await;
    let mut later = Vec::new();
    for index in 0..19 {
        let next = format!("page-{}", index + 1);
        later.push(
            mock(
                &mut server,
                &rate_limits_path("proj_a"),
                Matcher::Exact(format!("limit=100&after=page-{index}")),
                200,
                &list_page("", true, Some(&next)),
            )
            .await
            .expect(1),
        );
    }
    let beyond = mock(
        &mut server,
        &rate_limits_path("proj_a"),
        Matcher::Exact("limit=100&after=page-19".into()),
        200,
        &list_page("", false, None),
    )
    .await
    .expect(0);
    assert_eq!(
        provider(&server)
            .fetch_org_rate_limits_with("test-admin-key", at(NOW))
            .await
            .unwrap_err(),
        OrgRateLimitsError::ParseFailure
    );
    for mock in &later {
        mock.assert_async().await;
    }
    beyond.assert_async().await;
}

#[tokio::test]
async fn org_http_failures_have_typed_errors() {
    for (status, expected) in [
        (401, OrgRateLimitsError::AdminKeyRequired),
        (403, OrgRateLimitsError::AdminKeyRequired),
        (429, OrgRateLimitsError::RateLimited),
        (500, OrgRateLimitsError::ApiFailure(500)),
    ] {
        let mut server = Server::new_async().await;
        let _projects = mock(&mut server, PROJECTS_PATH, Matcher::Any, status, "{}").await;
        assert_eq!(
            provider(&server)
                .fetch_org_rate_limits_with("test-admin-key", at(NOW))
                .await
                .unwrap_err(),
            expected,
            "{status}"
        );
    }
}

#[test]
fn org_manual_limits_and_project_caps_match_by_exact_then_dated_snapshot_family() {
    let cap = |tpm| OrgRateLimit {
        tokens_per_minute: tpm,
        requests_per_minute: None,
    };
    let project_caps = OrgRateLimits {
        by_model: [
            ("gpt-5-2025-08-07".to_string(), cap(1000)),
            ("gpt-4o".to_string(), cap(200)),
            ("gpt-4o-mini".to_string(), cap(300)),
        ]
        .into_iter()
        .collect(),
        projects_sampled: 2,
        truncated: false,
        fetched_at: NOW,
    };
    let resolver = OrgLimitResolver::new(
        &[ModelRateLimit::new("gpt-5", 100, None)],
        Some(project_caps),
    );
    assert_eq!(resolver.project_cap("gpt-5-2025-08-07"), Some(cap(1000)));
    assert_eq!(resolver.resolve("gpt-5-2025-08-07"), Some(cap(100)));
    assert_eq!(resolver.manual_limit_key("gpt-5-2025-08-07"), Some("gpt-5"));
    assert_eq!(resolver.manual_limit_key("gpt-4o"), None);
    assert_eq!(resolver.resolve("gpt-4o"), None);
    assert_eq!(resolver.project_cap("gpt-4o"), Some(cap(200)));
    assert_eq!(
        resolver.project_cap("gpt-4o-mini-2024-07-18"),
        Some(cap(300))
    );
    assert_eq!(resolver.resolve("absent"), None);

    let manual = OrgLimitResolver::new(
        &[
            ModelRateLimit::new("gpt-4", 1, None),
            ModelRateLimit::new("gpt-4o", 2, None),
            ModelRateLimit::new("gpt-4o-mini", 3, None),
            ModelRateLimit::new("gpt-4o-mini-2024-07-18", 4, None),
        ],
        None,
    );
    assert_eq!(manual.resolve("gpt-4o-mini-2024-07-18"), Some(cap(4)));
    assert_eq!(
        manual.manual_limit_key("gpt-4o-mini-2024-07-18"),
        Some("gpt-4o-mini-2024-07-18")
    );
    assert_eq!(
        manual.manual_limit_key("gpt-4o-mini-2024-08-01"),
        Some("gpt-4o-mini")
    );
    assert_eq!(manual.resolve("gpt-4o-mini-next"), None);
    assert_eq!(manual.resolve("gpt-4-0613"), Some(cap(1)));
    let gpt4 = OrgLimitResolver::new(&[ModelRateLimit::new("gpt-4", 1, None)], None);
    assert_eq!(gpt4.resolve("gpt-4o-mini"), None);
    // A family limit covers dated snapshots only, never distinct sibling models.
    let gpt5 = OrgLimitResolver::new(&[ModelRateLimit::new("gpt-5", 100, None)], None);
    assert_eq!(gpt5.resolve("gpt-5-mini"), None);
    assert_eq!(gpt5.resolve("gpt-5-mini-2025-08-07"), None);
    assert_eq!(gpt5.resolve("gpt-5-2025-08-07"), Some(cap(100)));
}

#[test]
fn org_utilization_uses_complete_minute_peaks_and_rpm_with_a_deterministic_highest_model() {
    let live = snapshot(
        vec![
            LiveMinute {
                start: NOW - 90,
                models: vec![model("b", 80, 0, 20, 4), model("a", 50, 0, 0, 1)],
            },
            LiveMinute {
                start: NOW - 30,
                models: vec![
                    model("b", 150, 0, 0, 2),
                    model("a", 75, 0, 0, 1),
                    model("no-limit", 1000, 0, 0, 1),
                ],
            },
        ],
        None,
    );
    let limits = OrgLimitResolver::new(
        &[
            ModelRateLimit::new("a", 100, None),
            ModelRateLimit::new("b", 200, Some(8)),
        ],
        None,
    );
    let b = live
        .utilization(LiveModelFilter::Model("b"), &limits)
        .unwrap();
    assert_eq!(b.latest_tpm_percent, 50.0);
    // The in-progress minute (150 TPM, 75%) is pending, so the peak is the complete minute.
    assert_eq!(b.peak_tpm_percent, 50.0);
    assert_eq!(b.peak_rpm_percent, Some(50.0));
    assert!(
        live.utilization(LiveModelFilter::AllModels, &limits)
            .is_none()
    );
    assert!(
        live.utilization(LiveModelFilter::Model("no-limit"), &limits)
            .is_none()
    );
    assert_eq!(live.highest_utilization(&limits).unwrap().name, "a");
    let rows = live.model_rows_with_limits(&limits);
    let row = rows.iter().find(|row| row.name == "b").unwrap();
    assert_eq!(row.tpm_limit, Some(200));
    assert_eq!(row.rpm_limit, Some(8));
    assert_eq!(row.project_cap, None);
    assert_eq!(row.peak_tpm_percent, Some(50.0));
    assert_eq!(row.latest_tpm_percent, Some(50.0));
    assert_eq!(
        live.utilization(LiveModelFilter::Model("a"), &limits)
            .unwrap()
            .peak_rpm_percent,
        None
    );
}

#[test]
fn org_shared_manual_limits_pool_minute_traffic_while_exact_limits_stay_independent() {
    let minutes = vec![
        LiveMinute {
            start: NOW - 150,
            models: vec![
                LiveModelUsage {
                    batch_tokens: 40,
                    batch_requests: 4,
                    ..model("gpt-5", 90, 50, 10, 10)
                },
                model("gpt-5-2025-08-07", 60, 0, 0, 6),
                model("gpt-5-2025-08-08", 70, 0, 0, 7),
            ],
        },
        LiveMinute {
            start: NOW - 90,
            models: vec![
                model("gpt-5", 20, 0, 0, 1),
                model("gpt-5-2025-08-07", 30, 0, 0, 2),
            ],
        },
        LiveMinute {
            start: NOW - 30,
            models: vec![
                model("gpt-5", 1000, 0, 0, 100),
                model("gpt-5-2025-08-07", 1000, 0, 0, 100),
            ],
        },
    ];
    let manual = [
        ModelRateLimit::new("gpt-5", 100, Some(10)),
        ModelRateLimit::new("gpt-5-2025-08-08", 200, Some(20)),
    ];
    let limits = OrgLimitResolver::new(&manual, None);
    let live = snapshot(minutes.clone(), None);
    for name in ["gpt-5", "gpt-5-2025-08-07"] {
        let utilization = live
            .utilization(LiveModelFilter::Model(name), &limits)
            .unwrap();
        assert_eq!(utilization.pooled_models, ["gpt-5", "gpt-5-2025-08-07"]);
        assert_eq!(utilization.latest_tpm_percent, 50.0);
        assert_eq!(utilization.peak_tpm_percent, 120.0);
        assert_eq!(utilization.peak_rpm_percent, Some(120.0));
    }
    let exact = live
        .utilization(LiveModelFilter::Model("gpt-5-2025-08-08"), &limits)
        .unwrap();
    assert_eq!(exact.pooled_models, ["gpt-5-2025-08-08"]);
    assert_eq!(exact.peak_tpm_percent, 35.0);
    assert_eq!(exact.latest_tpm_percent, 0.0);
    assert_eq!(exact.peak_rpm_percent, Some(35.0));
    assert_eq!(
        live.highest_utilization(&limits).unwrap().peak_tpm_percent,
        Some(120.0)
    );
    let rows = live.model_rows_with_limits(&limits);
    assert_eq!(
        rows[..2]
            .iter()
            .map(|row| row.peak_tpm_percent)
            .collect::<Vec<_>>(),
        [Some(120.0), Some(120.0)]
    );

    // The menu tile pools the alias with its snapshot under the configured ID.
    let tile = throughput_limit_utilization(&live.throughput(), &manual).unwrap();
    assert_eq!(tile.label, "gpt-5");
    assert_eq!(tile.peak_tokens, 120);
    assert_eq!(tile.tokens_per_minute_limit, 100);
    assert_eq!(tile.percent, 120.0);
    assert!(throughput_limit_utilization(&live.throughput(), &[]).is_none());

    let scoped = snapshot(minutes, Some("proj_fixture"));
    assert!(
        scoped
            .utilization(LiveModelFilter::Model("gpt-5"), &limits)
            .is_none()
    );
    assert!(throughput_limit_utilization(&scoped.throughput(), &manual).is_none());
}

#[test]
fn org_traffic_above_every_project_cap_never_implies_an_org_limit() {
    let caps = OrgRateLimits {
        by_model: [(
            "gpt-4o".to_string(),
            OrgRateLimit {
                tokens_per_minute: 30_000_000,
                requests_per_minute: None,
            },
        )]
        .into_iter()
        .collect(),
        projects_sampled: 1,
        truncated: false,
        fetched_at: NOW,
    };
    let live = snapshot(
        vec![LiveMinute {
            start: NOW - 90,
            models: vec![model("gpt-4o", 75_000_000, 0, 0, 10)],
        }],
        None,
    );
    let limits = OrgLimitResolver::new(&[], Some(caps));
    let row = &live.model_rows_with_limits(&limits)[0];
    assert_eq!(row.project_cap, Some(30_000_000));
    assert_eq!(row.tpm_limit, None);
    assert_eq!(row.peak_tpm_percent, None);
    assert!(live.highest_utilization(&limits).is_none());
}
