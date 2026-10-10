use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{TimeZone, Utc};
use mockito::{Matcher, Mock, ServerGuard};
use reqwest::Url;

use super::GroqProvider;
use super::console::{
    self, ConsoleEndpoints, ConsoleSession, daily_usage, history_window, organization_id,
    parse_activity,
};
use crate::core::{FetchContext, ProviderError, ProviderFetchResult, SourceMode};

const ACTIVITY: &str = include_str!("fixtures/activity.json");
/// The pack's unsigned session JWT: org `org_parity_0008`.
const PACK_JWT: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiJ1c2VyX3Bhcml0eV8wMDA4IiwiZW1haWwiOiJwYXJpdHkudXNlckBleGFtcGxlLmNvbSIsImh0dHBzOi8vZ3JvcS5jb20vb3JnYW5pemF0aW9uIjp7ImlkIjoib3JnX3Bhcml0eV8wMDA4In19.";
const PACK_KEY: &str = "gsk_parity_synthetic_0008";
const ACTIVITY_PATH: &str = "/platform/v1/organizations/org_parity_0008/activity";
const METRICS_PATH: &str = "/v1/metrics/prometheus/api/v1/query";

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 10, 12, 0, 0).unwrap()
}

fn pack_result() -> ProviderFetchResult {
    let rows = parse_activity(ACTIVITY.as_bytes()).unwrap();
    let organization = console::organization_name(&rows);
    console::build_result(daily_usage(&rows, &Utc), organization, now())
}

fn details(result: &ProviderFetchResult) -> Vec<(String, String, String, String)> {
    result
        .display_details()
        .iter()
        .map(|row| {
            (
                row.section_title().unwrap_or_default().to_owned(),
                row.title().to_owned(),
                row.value().to_owned(),
                row.secondary_value().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn row(
    section: &str,
    title: &str,
    value: &str,
    secondary: &str,
) -> (String, String, String, String) {
    (section.into(), title.into(), value.into(), secondary.into())
}

fn pack_card() -> Vec<(String, String, String, String)> {
    vec![
        row("Usage summary", "Spend", "$2.85", "Last 30 days"),
        row("Usage summary", "Requests", "1,495", ""),
        row("Usage summary", "Tokens", "686,000", ""),
        row("Usage summary", "Cached input", "60,000", ""),
        row(
            "Models",
            "llama-synthetic-70b",
            "316,000 tokens",
            "590 requests",
        ),
        row(
            "Models",
            "llama-synthetic-8b",
            "312,000 tokens",
            "750 requests",
        ),
        row(
            "Models",
            "mixtral-synthetic",
            "39,000 tokens",
            "95 requests",
        ),
        row("Models", "gemma-synthetic", "19,000 tokens", "60 requests"),
    ]
}

#[test]
fn pack_activity_maps_to_the_mac_card() {
    let result = pack_result();

    assert_eq!(details(&result), pack_card());
    assert_eq!(result.source_label, "console");
    assert_eq!(result.usage.login_method.as_deref(), Some("Console"));
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("Parity Labs")
    );
    assert_eq!(result.usage.account_email, None);
    assert_eq!(result.usage.primary_label.as_deref(), Some("Spend"));
    assert!(result.usage.primary.is_informational);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("$2.85 · Last 30 days")
    );
    let cost = result.cost.as_ref().unwrap();
    assert!((cost.used - 2.85).abs() < 1e-9, "{}", cost.used);
    assert_eq!(cost.currency_code, "USD");
    assert_eq!(cost.period, "Last 30 days");
}

#[test]
fn pack_activity_buckets_six_local_days_for_the_daily_chart() {
    let history = pack_result().open_ai_api_usage.unwrap();
    assert_eq!(history.history_days, 30);
    assert_eq!(history.project_id, None);

    let days: Vec<_> = history
        .daily
        .iter()
        .map(|day| {
            (
                Utc.timestamp_opt(day.start_time, 0)
                    .unwrap()
                    .format("%Y-%m-%d")
                    .to_string(),
                day.end_time - day.start_time,
                format!("{:.2}", day.cost_usd),
                day.requests,
                day.total_tokens,
            )
        })
        .collect();
    assert_eq!(
        days,
        vec![
            (
                "2026-09-20".to_owned(),
                86_400,
                "0.05".to_owned(),
                60,
                19_000
            ),
            (
                "2026-09-26".to_owned(),
                86_400,
                "0.23".to_owned(),
                410,
                192_000
            ),
            (
                "2026-10-01".to_owned(),
                86_400,
                "1.07".to_owned(),
                260,
                153_000
            ),
            (
                "2026-10-04".to_owned(),
                86_400,
                "0.19".to_owned(),
                95,
                39_000
            ),
            (
                "2026-10-07".to_owned(),
                86_400,
                "0.78".to_owned(),
                210,
                101_000
            ),
            (
                "2026-10-09".to_owned(),
                86_400,
                "0.53".to_owned(),
                460,
                182_000
            ),
        ]
    );

    let yesterday = history.daily.last().unwrap();
    assert_eq!(yesterday.input_tokens, 128_000);
    assert_eq!(yesterday.cached_input_tokens, 12_000);
    assert_eq!(yesterday.output_tokens, 42_000);
    assert!(yesterday.line_items.is_empty());
    let models: Vec<_> = yesterday
        .models
        .iter()
        .map(|model| {
            (
                model.name.as_str(),
                model.requests,
                model.input_tokens,
                model.cached_input_tokens,
                model.output_tokens,
                model.total_tokens,
            )
        })
        .collect();
    assert_eq!(
        models,
        vec![
            ("llama-synthetic-8b", 340, 88_000, 2_000, 30_000, 120_000),
            ("llama-synthetic-70b", 120, 40_000, 10_000, 12_000, 62_000),
        ]
    );
}

#[test]
fn missing_counts_follow_upstream_defaults() {
    let rows = parse_activity(
        br#"{"data":[
            {"timestamp":1791547200,"n_context_tokens_total":500,"n_generated_tokens_total":20},
            {"model":"","timestamp":1791547200,"num_requests":3}
        ]}"#,
    )
    .unwrap();
    let daily = daily_usage(&rows, &Utc);

    assert_eq!(console::organization_name(&rows), None);
    let model = &daily[0].models[0];
    assert_eq!(model.name, "unknown");
    assert_eq!(model.requests, 3);
    assert_eq!(model.input_tokens, 500);
    assert_eq!(model.cached_input_tokens, 0);
    assert_eq!(model.total_tokens, 520);
    assert_eq!(daily[0].cost_usd, 0.0);

    let result = console::build_result(daily, None, now());
    let titles: Vec<_> = details(&result)
        .into_iter()
        .map(|(_, title, value, _)| format!("{title}={value}"))
        .collect();
    assert_eq!(
        titles,
        [
            "Spend=$0.00",
            "Requests=3",
            "Tokens=520",
            "unknown=520 tokens"
        ]
    );
}

#[test]
fn days_are_bucketed_by_local_date_and_labelled_with_it() {
    let rows = parse_activity(
        br#"{"data":[
            {"model":"m","timestamp":1791597600,"num_requests":1},
            {"model":"m","timestamp":1791612000,"num_requests":2}
        ]}"#,
    )
    .unwrap();
    let new_york = chrono::FixedOffset::west_opt(4 * 3600).unwrap();

    let days: Vec<_> = daily_usage(&rows, &new_york)
        .iter()
        .map(|day| (day.start_time, day.end_time, day.requests))
        .collect();

    // 02:00Z on Oct 10 is still Oct 9 in New York; 06:00Z is Oct 10.
    let oct9 = Utc
        .with_ymd_and_hms(2026, 10, 9, 0, 0, 0)
        .unwrap()
        .timestamp();
    let oct10 = oct9 + 86_400;
    assert_eq!(days, vec![(oct9, oct10, 1), (oct10, oct10 + 86_400, 2)]);
}

#[test]
fn history_window_covers_thirty_local_days_including_today() {
    assert_eq!(
        history_window(&now(), 30),
        (
            Utc.with_ymd_and_hms(2026, 9, 11, 0, 0, 0)
                .unwrap()
                .timestamp(),
            Utc.with_ymd_and_hms(2026, 10, 11, 0, 0, 0)
                .unwrap()
                .timestamp(),
        )
    );
    let tokyo = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
    assert_eq!(
        history_window(&now().with_timezone(&tokyo), 1),
        (
            Utc.with_ymd_and_hms(2026, 10, 9, 15, 0, 0)
                .unwrap()
                .timestamp(),
            Utc.with_ymd_and_hms(2026, 10, 10, 15, 0, 0)
                .unwrap()
                .timestamp(),
        )
    );
}

#[test]
fn activity_url_replaces_the_api_base_path() {
    for base in [
        "https://api.groq.com/v1",
        "https://api.groq.com/openai/v1/?x=1#frag",
    ] {
        let url = console::activity_url(&Url::parse(base).unwrap(), "org_a", 10, 20).unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.groq.com/platform/v1/organizations/org_a/activity?start_date=10&end_date=20",
            "{base}"
        );
    }
    let url = console::activity_url(&Url::parse("https://api.groq.com/v1").unwrap(), "a/b", 1, 2)
        .unwrap();
    assert_eq!(url.path(), "/platform/v1/organizations/a%2Fb/activity");
}

fn jwt_with(claims: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!(
        "{}.{}.sig",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        URL_SAFE_NO_PAD.encode(claims)
    )
}

#[test]
fn organization_id_reads_the_groq_claim_then_the_stytch_slug() {
    assert_eq!(
        organization_id(PACK_JWT).as_deref(),
        Some("org_parity_0008")
    );
    assert_eq!(
        organization_id(&jwt_with(
            r#"{"https://groq.com/organization":{"id":""},"https://stytch.com/organization":{"slug":"parity-slug"}}"#
        ))
        .as_deref(),
        Some("parity-slug")
    );
    assert_eq!(organization_id(&jwt_with(r#"{"sub":"user"}"#)), None);
    assert_eq!(organization_id("not-a-jwt"), None);
    assert_eq!(organization_id("a.!!!.c"), None);
}

#[test]
fn session_comes_from_the_stytch_cookies() {
    assert_eq!(
        ConsoleSession::from_cookie_header(
            "Cookie: theme=dark; stytch_session=opaque-a; stytch_session_jwt=jwt-a"
        ),
        Some(ConsoleSession {
            session_token: Some("opaque-a".into()),
            direct_jwt: Some("jwt-a".into()),
        })
    );
    assert_eq!(
        ConsoleSession::from_cookie_header("stytch_session_jwt=jwt-b"),
        Some(ConsoleSession {
            session_token: None,
            direct_jwt: Some("jwt-b".into()),
        })
    );
    assert_eq!(
        ConsoleSession::from_cookie_header("theme=dark; stytch_session="),
        None
    );
}

#[test]
fn session_token_env_wins_over_the_jwt_env() {
    let env = |name: &str| match name {
        "GROQ_SESSION_TOKEN" => Some(" opaque ".to_owned()),
        "GROQ_SESSION_JWT" => Some("jwt".to_owned()),
        _ => None,
    };
    assert_eq!(
        ConsoleSession::from_env(env),
        Some(ConsoleSession {
            session_token: Some("opaque".into()),
            direct_jwt: Some("jwt".into()),
        })
    );
    assert_eq!(ConsoleSession::from_env(|_| Some("  ".to_owned())), None);
}

#[test]
fn session_debug_output_redacts_the_tokens() {
    let session = ConsoleSession {
        session_token: Some("opaque-secret".into()),
        direct_jwt: None,
    };
    assert_eq!(
        format!("{session:?}"),
        r#"ConsoleSession { session_token: Some("<redacted>"), direct_jwt: None }"#
    );
}

fn endpoints(server: &ServerGuard) -> ConsoleEndpoints {
    ConsoleEndpoints {
        api_base: Url::parse(&format!("{}/v1", server.url())).unwrap(),
        stytch_base: Url::parse(&server.url()).unwrap(),
        stytch_public_token: "public-token-test".to_owned(),
    }
}

fn context(source_mode: SourceMode, api_key: Option<&str>) -> FetchContext {
    FetchContext {
        source_mode,
        api_key: api_key.map(ToOwned::to_owned),
        manual_cookie_missing: true,
        ..FetchContext::default()
    }
}

fn jwt_env(name: &str) -> Option<String> {
    (name == "GROQ_SESSION_JWT").then(|| PACK_JWT.to_owned())
}

fn no_env(_: &str) -> Option<String> {
    None
}

async fn activity_mock(server: &mut ServerGuard, jwt: &str, status: usize, hits: usize) -> Mock {
    server
        .mock("GET", ACTIVITY_PATH)
        .match_query(Matcher::AllOf(vec![
            Matcher::Regex(r"start_date=\d+".into()),
            Matcher::Regex(r"end_date=\d+".into()),
        ]))
        .match_header("authorization", format!("Bearer {jwt}").as_str())
        .match_header("accept", "application/json")
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(ACTIVITY)
        .expect(hits)
        .create_async()
        .await
}

async fn metrics_mock(server: &mut ServerGuard, hits: usize) -> Mock {
    server
        .mock("GET", METRICS_PATH)
        .match_query(Matcher::Any)
        .match_header("authorization", format!("Bearer {PACK_KEY}").as_str())
        .with_status(200)
        .with_body(r#"{"status":"success","data":{"result":[{"value":[1710000000,"1"]}]}}"#)
        .expect(hits)
        .create_async()
        .await
}

fn no_browser() -> Result<Vec<(String, String)>, ProviderError> {
    panic!("the browser must not be read here")
}

async fn fetch(
    server: &ServerGuard,
    ctx: &FetchContext,
    env: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<ProviderFetchResult, ProviderError> {
    fetch_with_browser(server, ctx, env, &no_browser).await
}

async fn fetch_with_browser(
    server: &ServerGuard,
    ctx: &FetchContext,
    env: &(dyn Fn(&str) -> Option<String> + Sync),
    browser: &super::BrowserCookieHeaders,
) -> Result<ProviderFetchResult, ProviderError> {
    GroqProvider::new()
        .fetch_routed(ctx, &endpoints(server), env, browser, now())
        .await
}

#[tokio::test]
async fn auto_reads_console_activity_with_the_session_jwt_before_metrics() {
    let mut server = mockito::Server::new_async().await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 1).await;
    let metrics = metrics_mock(&mut server, 0).await;

    let result = fetch(
        &server,
        &context(SourceMode::Auto, Some(PACK_KEY)),
        &jwt_env,
    )
    .await
    .unwrap();

    activity.assert_async().await;
    metrics.assert_async().await;
    assert_eq!(result.source_label, "console");
    assert_eq!(details(&result), pack_card());
}

#[tokio::test]
async fn web_reads_a_manual_stytch_cookie() {
    let mut server = mockito::Server::new_async().await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 1).await;
    let ctx = FetchContext {
        manual_cookie_header: Some(format!("stytch_session_jwt={PACK_JWT}")),
        manual_cookie_missing: false,
        ..context(SourceMode::Web, None)
    };

    let result = fetch(&server, &ctx, &no_env).await.unwrap();

    activity.assert_async().await;
    assert_eq!(result.source_label, "console");
}

#[tokio::test]
async fn auto_falls_back_to_metrics_without_a_usable_session() {
    for (status, env) in [
        (401, &jwt_env as &(dyn Fn(&str) -> Option<String> + Sync)),
        (403, &jwt_env),
        (200, &no_env),
    ] {
        let mut server = mockito::Server::new_async().await;
        let activity =
            activity_mock(&mut server, PACK_JWT, status, usize::from(status != 200)).await;
        let metrics = metrics_mock(&mut server, 4).await;

        let result = fetch(&server, &context(SourceMode::Auto, Some(PACK_KEY)), env)
            .await
            .unwrap();

        activity.assert_async().await;
        metrics.assert_async().await;
        assert_eq!(result.source_label, "api", "{status}");
        assert_eq!(
            result.usage.login_method.as_deref(),
            Some("Prometheus metrics")
        );
    }
}

#[tokio::test]
async fn console_api_and_parse_errors_do_not_fall_back() {
    for (status, body, expected) in [
        (500, ACTIVITY, "Groq console API error: HTTP 500"),
        (200, r#"{"rows":[]}"#, "Groq console response"),
    ] {
        let mut server = mockito::Server::new_async().await;
        let _activity = server
            .mock("GET", ACTIVITY_PATH)
            .match_query(Matcher::Any)
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let metrics = metrics_mock(&mut server, 0).await;

        let error = fetch(
            &server,
            &context(SourceMode::Auto, Some(PACK_KEY)),
            &jwt_env,
        )
        .await
        .unwrap_err();

        metrics.assert_async().await;
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[tokio::test]
async fn web_never_uses_the_api_key() {
    let mut server = mockito::Server::new_async().await;
    let metrics = metrics_mock(&mut server, 0).await;

    let error = fetch(&server, &context(SourceMode::Web, Some(PACK_KEY)), &no_env)
        .await
        .unwrap_err();

    metrics.assert_async().await;
    assert_eq!(
        error.to_string(),
        "No Groq console session found. Sign in at console.groq.com in your browser."
    );
}

#[tokio::test]
async fn api_source_uses_only_metrics() {
    let mut server = mockito::Server::new_async().await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 0).await;
    let metrics = metrics_mock(&mut server, 4).await;

    let result = fetch(
        &server,
        &context(SourceMode::OAuth, Some(PACK_KEY)),
        &jwt_env,
    )
    .await
    .unwrap();

    activity.assert_async().await;
    metrics.assert_async().await;
    assert_eq!(result.source_label, "api");
}

fn stytch_mock(server: &mut ServerGuard, status: usize) -> mockito::Mock {
    let credential = STANDARD.encode("public-token-test:opaque-session");
    server
        .mock("POST", "/sdk/v1/b2b/sessions/authenticate")
        .match_header("authorization", format!("Basic {credential}").as_str())
        .match_header("origin", "https://console.groq.com")
        .match_header("x-sdk-parent-host", "https://console.groq.com")
        .match_header(
            "x-sdk-client",
            STANDARD
                .encode(r#"{"app":{"identifier":"console.groq.com"},"sdk":{"identifier":"Stytch.js Javascript SDK","version":"5.43.0"}}"#)
                .as_str(),
        )
        .match_body(Matcher::Json(serde_json::json!({
            "session_token": "opaque-session",
            "session_duration_minutes": 30,
        })))
        .with_status(status)
        .with_body(format!(r#"{{"data":{{"session_jwt":"{PACK_JWT}"}}}}"#))
        .expect(1)
}

#[tokio::test]
async fn opaque_session_token_is_refreshed_through_stytch() {
    let mut server = mockito::Server::new_async().await;
    let stytch = stytch_mock(&mut server, 200).create_async().await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 1).await;
    let env = |name: &str| (name == "GROQ_SESSION_TOKEN").then(|| "opaque-session".to_owned());

    let result = fetch(&server, &context(SourceMode::Web, None), &env)
        .await
        .unwrap();

    stytch.assert_async().await;
    activity.assert_async().await;
    assert_eq!(result.source_label, "console");
}

#[tokio::test]
async fn failed_refresh_uses_the_direct_jwt_or_falls_back() {
    let mut server = mockito::Server::new_async().await;
    let stytch = stytch_mock(&mut server, 401).create_async().await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 1).await;
    let ctx = FetchContext {
        manual_cookie_header: Some(format!(
            "stytch_session=opaque-session; stytch_session_jwt={PACK_JWT}"
        )),
        manual_cookie_missing: false,
        ..context(SourceMode::Web, None)
    };

    let result = fetch(&server, &ctx, &no_env).await.unwrap();

    stytch.assert_async().await;
    activity.assert_async().await;
    assert_eq!(result.source_label, "console");

    let mut server = mockito::Server::new_async().await;
    let stytch = stytch_mock(&mut server, 403).create_async().await;
    let metrics = metrics_mock(&mut server, 4).await;
    let ctx = FetchContext {
        manual_cookie_header: Some("stytch_session=opaque-session".to_owned()),
        manual_cookie_missing: false,
        ..context(SourceMode::Auto, Some(PACK_KEY))
    };

    let result = fetch(&server, &ctx, &no_env).await.unwrap();

    stytch.assert_async().await;
    metrics.assert_async().await;
    assert_eq!(result.source_label, "api");
}

/// Org `org_parity_0008`, but a different token the activity mock rejects.
fn stale_jwt() -> String {
    jwt_with(r#"{"https://groq.com/organization":{"id":"org_parity_0008"}}"#)
}

fn browser_context(source_mode: SourceMode, api_key: Option<&str>) -> FetchContext {
    FetchContext {
        manual_cookie_missing: false,
        ..context(source_mode, api_key)
    }
}

#[tokio::test]
async fn a_rejected_browser_session_moves_on_to_the_next_browser() {
    let mut server = mockito::Server::new_async().await;
    let stale = activity_mock(&mut server, &stale_jwt(), 401, 1).await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 1).await;
    let browser = || {
        Ok(vec![
            (
                "Chrome".to_owned(),
                format!("theme=dark; stytch_session_jwt={}", stale_jwt()),
            ),
            ("Edge".to_owned(), "theme=dark".to_owned()),
            (
                "Firefox".to_owned(),
                format!("stytch_session_jwt={PACK_JWT}"),
            ),
        ])
    };

    let result = fetch_with_browser(
        &server,
        &browser_context(SourceMode::Web, None),
        &no_env,
        &browser,
    )
    .await
    .unwrap();

    stale.assert_async().await;
    activity.assert_async().await;
    assert_eq!(result.source_label, "console");
    assert_eq!(details(&result), pack_card());
}

#[tokio::test]
async fn auto_falls_back_to_metrics_when_every_browser_session_is_rejected() {
    let mut server = mockito::Server::new_async().await;
    let stale = activity_mock(&mut server, &stale_jwt(), 403, 2).await;
    let metrics = metrics_mock(&mut server, 4).await;
    let browser = || {
        Ok(vec![
            (
                "Chrome".to_owned(),
                format!("stytch_session_jwt={}", stale_jwt()),
            ),
            (
                "Edge".to_owned(),
                format!("stytch_session_jwt={}", stale_jwt()),
            ),
        ])
    };

    let result = fetch_with_browser(
        &server,
        &browser_context(SourceMode::Auto, Some(PACK_KEY)),
        &no_env,
        &browser,
    )
    .await
    .unwrap();

    stale.assert_async().await;
    metrics.assert_async().await;
    assert_eq!(result.source_label, "api");

    let error = fetch_with_browser(
        &server,
        &browser_context(SourceMode::Web, None),
        &no_env,
        &browser,
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "Groq console access denied: HTTP 403");
}

#[tokio::test]
async fn a_console_api_error_stops_at_the_first_browser_session() {
    let mut server = mockito::Server::new_async().await;
    let failing = activity_mock(&mut server, &stale_jwt(), 500, 1).await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 0).await;
    let browser = || {
        Ok(vec![
            (
                "Chrome".to_owned(),
                format!("stytch_session_jwt={}", stale_jwt()),
            ),
            ("Edge".to_owned(), format!("stytch_session_jwt={PACK_JWT}")),
        ])
    };

    let error = fetch_with_browser(
        &server,
        &browser_context(SourceMode::Web, None),
        &no_env,
        &browser,
    )
    .await
    .unwrap_err();

    failing.assert_async().await;
    activity.assert_async().await;
    assert_eq!(error.to_string(), "Groq console API error: HTTP 500");
}

#[tokio::test]
async fn env_and_manual_sessions_are_used_without_reading_the_browser() {
    let mut server = mockito::Server::new_async().await;
    let activity = activity_mock(&mut server, PACK_JWT, 200, 2).await;

    let from_env = fetch(&server, &browser_context(SourceMode::Web, None), &jwt_env)
        .await
        .unwrap();
    let manual = FetchContext {
        manual_cookie_header: Some(format!("stytch_session_jwt={PACK_JWT}")),
        ..browser_context(SourceMode::Web, None)
    };
    let from_manual = fetch(&server, &manual, &no_env).await.unwrap();

    activity.assert_async().await;
    assert_eq!(from_env.source_label, "console");
    assert_eq!(from_manual.source_label, "console");

    let error = fetch(
        &server,
        &FetchContext {
            manual_cookie_header: Some("theme=dark".to_owned()),
            ..browser_context(SourceMode::Web, None)
        },
        &no_env,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "No Groq console session found. Sign in at console.groq.com in your browser."
    );
}

#[tokio::test]
async fn an_unreadable_browser_store_reports_a_missing_session() {
    let server = mockito::Server::new_async().await;
    let browser = || {
        Err(ProviderError::Other(
            "no cookies found for groq.com".to_owned(),
        ))
    };

    let error = fetch_with_browser(
        &server,
        &browser_context(SourceMode::Web, None),
        &no_env,
        &browser,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "No Groq console session found. Sign in at console.groq.com in your browser."
    );
}
