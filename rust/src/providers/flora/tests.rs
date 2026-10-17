//! Fixture tests for the FLORA workspace provider. Fetch tests use an injected
//! API key, workspace, and a local mock server, so no test reads the keyring,
//! the environment, or the real FLORA API. Dates are fixed through
//! `period_bounds` and `parse_usage`.

use super::*;
use chrono::TimeZone;
use mockito::{Matcher, Server};
use serde_json::json;

const KEY: &str = "flora-test-key";
const WORKSPACE: &str = "ws_abc123";
const USAGE_PATH: &str = "/api/v1/workspaces/ws_abc123/usage";
const FORBIDDEN_MESSAGE: &str =
    "FLORA rejected the key: it needs billing permission on this workspace.";

fn test_provider(base_url: &str) -> FloraProvider {
    FloraProvider::with_base_url(base_url)
}

fn context(source_mode: SourceMode, workspace: Option<&str>, key: Option<&str>) -> FetchContext {
    FetchContext {
        source_mode,
        api_key: key.map(str::to_string),
        workspace_id: workspace.map(str::to_string),
        ..FetchContext::default()
    }
}

/// Title, value, and secondary text for each row, in order.
fn rows(result: &ProviderFetchResult) -> Vec<(String, String, Option<String>)> {
    result
        .display_details
        .iter()
        .map(|detail| {
            assert_eq!(detail.section_title(), Some(SECTION_TITLE));
            (
                detail.title().to_string(),
                detail.value().to_string(),
                detail.secondary_value().map(str::to_string),
            )
        })
        .collect()
}

fn row(title: &str, value: &str, secondary: Option<&str>) -> (String, String, Option<String>) {
    (
        title.to_string(),
        value.to_string(),
        secondary.map(str::to_string),
    )
}

fn assert_other(result: Result<ProviderFetchResult, ProviderError>, expected: &str) {
    match result {
        Err(ProviderError::Other(message)) => assert_eq!(message, expected),
        other => panic!("expected Other({expected:?}), got {other:?}"),
    }
}

fn assert_parse(result: Result<ProviderFetchResult, ProviderError>) {
    match result {
        Err(ProviderError::Parse(message)) => assert_eq!(message, UNRECOGNIZED),
        other => panic!("expected the unrecognized-response error, got {other:?}"),
    }
}

fn parse(value: Value) -> Result<ProviderFetchResult, ProviderError> {
    parse_usage(&serde_json::to_vec(&value).unwrap())
}

#[test]
fn metadata_and_source_contract_match_the_workspace_provider() {
    let provider = FloraProvider::new();
    assert_eq!(provider.id(), ProviderId::Flora);
    assert_eq!(provider.metadata().display_name, "FLORA");
    assert_eq!(
        provider.metadata().dashboard_url,
        Some("https://app.flora.ai")
    );
    assert!(!provider.metadata().default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::OAuth]
    );
    assert!(!provider.supports_web());
    assert_eq!(ProviderId::Flora.cookie_domain(), None);
    assert_eq!(ProviderId::from_cli_name("flora"), Some(ProviderId::Flora));
}

#[test]
fn period_runs_from_the_first_of_the_utc_month_to_today() {
    let mid = Utc.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap();
    assert_eq!(
        period_bounds(mid),
        ("2026-10-01".to_string(), "2026-10-15".to_string())
    );
    let new_year = Utc.with_ymd_and_hms(2027, 1, 1, 0, 30, 0).unwrap();
    assert_eq!(
        period_bounds(new_year),
        ("2027-01-01".to_string(), "2027-01-01".to_string())
    );
}

#[test]
fn workspace_pool_payload_matches_upstream_rows() {
    let result = parse(json!({
        "currency": "USD",
        "available_cost": 120.5,
        "balance_scope": "workspace",
        "member_limit": { "limit_cost": 50, "period": "month" },
        "spend": { "status": "available", "total_cost": 80.25, "classified_runs": 12 }
    }))
    .unwrap();

    assert_eq!(
        rows(&result),
        vec![
            row("Available", "$120.50", Some("Workspace pool")),
            row("Spent this month", "$80.25", None),
            row("Runs this month", "12", None),
        ]
    );
    assert_eq!(result.source_label, "api");
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
    assert!(result.usage.primary.hidden_on_card);
}

#[test]
fn caller_scope_adds_the_member_limit_row_with_its_period() {
    let result = parse(json!({
        "available_cost": 10,
        "balance_scope": "caller",
        "member_limit": { "limit_cost": 50, "period": "month" },
        "spend": { "status": "unavailable", "reason": "permission_missing" }
    }))
    .unwrap();

    assert_eq!(
        rows(&result),
        vec![
            row("Available", "$10.00", Some("Your allowance")),
            row("Member limit", "$50.00", Some("Monthly")),
            row(
                "Spent this month",
                "Unavailable",
                Some("Needs billing permission")
            ),
        ]
    );

    let all_time = parse(json!({
        "available_cost": 10,
        "balance_scope": "caller",
        "member_limit": { "limit_cost": 5, "period": "all_time" }
    }))
    .unwrap();
    assert_eq!(
        rows(&all_time),
        vec![
            row("Available", "$10.00", Some("Your allowance")),
            row("Member limit", "$5.00", Some("All time")),
        ]
    );
}

#[test]
fn member_limit_is_ignored_outside_caller_scope_or_without_a_finite_limit() {
    let workspace_scope = parse(json!({
        "available_cost": 10,
        "balance_scope": "workspace",
        "member_limit": { "limit_cost": 50, "period": "month" }
    }))
    .unwrap();
    assert_eq!(
        rows(&workspace_scope),
        vec![row("Available", "$10.00", Some("Workspace pool"))]
    );

    let no_limit = parse(json!({
        "available_cost": 10,
        "balance_scope": "caller",
        "member_limit": { "period": "month" }
    }))
    .unwrap();
    assert_eq!(
        rows(&no_limit),
        vec![row("Available", "$10.00", Some("Your allowance"))]
    );
}

#[test]
fn unavailable_spend_reasons_map_to_upstream_text() {
    let cases = [
        ("entitlement_missing", "Plan has no usage reporting"),
        ("permission_missing", "Needs billing permission"),
        ("summary_unavailable", "Totals not computed; try again"),
        ("something_new", "Not reported"),
    ];
    for (reason, text) in cases {
        let result = parse(json!({
            "available_cost": 1,
            "spend": { "status": "unavailable", "reason": reason }
        }))
        .unwrap();
        assert_eq!(
            rows(&result).last().unwrap(),
            &row("Spent this month", "Unavailable", Some(text)),
            "reason {reason}"
        );
    }
}

#[test]
fn run_counts_accept_whole_numbers_only() {
    let whole = parse(json!({
        "available_cost": 1,
        "spend": { "status": "available", "total_cost": 0, "classified_runs": 3.0 }
    }))
    .unwrap();
    assert_eq!(rows(&whole).last().unwrap().1, "3");

    for runs in [json!(3.5), json!(-1), json!("3")] {
        let result = parse(json!({
            "available_cost": 1,
            "spend": { "status": "available", "total_cost": 0, "classified_runs": runs }
        }))
        .unwrap();
        assert_eq!(rows(&result).len(), 2, "runs {runs} should add no row");
    }
}

#[test]
fn an_available_spend_without_a_finite_total_fails_parsing() {
    assert_parse(parse(json!({
        "available_cost": 1,
        "spend": { "status": "available", "classified_runs": 2 }
    })));
}

#[test]
fn currency_is_used_only_when_it_is_an_uppercase_iso_code() {
    let result = parse(json!({ "available_cost": 1, "currency": "eur" })).unwrap();
    assert_eq!(rows(&result)[0].1, "$1.00");
    let result = parse(json!({ "available_cost": 1, "currency": "GBP" })).unwrap();
    assert_eq!(rows(&result)[0].1, format_currency(1.0, "GBP"));
}

#[test]
fn malformed_bodies_fail_parsing_without_echoing_them() {
    assert_parse(parse_usage(b"not json"));
    assert_parse(parse_usage(b"[]"));
    assert_parse(parse(json!({ "available_cost": "12" })));
    assert_parse(parse(json!({ "currency": "USD" })));
}

#[test]
fn workspace_ids_must_be_ws_prefixed_alphanumerics() {
    assert_eq!(workspace_id(Some(" ws_abc123 ")).unwrap(), "ws_abc123");
    for invalid in ["abc123", "ws_", "ws_abc-123", "ws_abc/123", "wrk_abc"] {
        match workspace_id(Some(invalid)) {
            Err(ProviderError::NotInstalled(message)) => assert_eq!(message, WORKSPACE_FORMAT),
            other => panic!("{invalid} should be rejected, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn fetch_sends_the_bearer_key_and_the_utc_month_window() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", USAGE_PATH)
        .match_query(Matcher::Regex(
            r"start_date=\d{4}-\d{2}-01&end_date=\d{4}-\d{2}-\d{2}$".into(),
        ))
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"available_cost":3,"balance_scope":"workspace"}"#)
        .expect(1)
        .create_async()
        .await;

    let result = test_provider(&server.url())
        .fetch_usage(&context(SourceMode::Auto, Some(WORKSPACE), Some(KEY)))
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(
        rows(&result),
        vec![row("Available", "$3.00", Some("Workspace pool"))]
    );
}

#[tokio::test]
async fn http_failures_report_their_class_without_a_body() {
    let mut server = Server::new_async().await;
    let provider = test_provider(&server.url());
    let ctx = context(SourceMode::OAuth, Some(WORKSPACE), Some(KEY));

    let cases = [
        (
            401,
            "FLORA rejected the API key. Check or replace it in Settings.",
        ),
        (403, FORBIDDEN_MESSAGE),
        (404, "FLORA did not find this workspace ID."),
        (429, "FLORA returned HTTP 429."),
        (503, "FLORA returned HTTP 503."),
    ];
    for (status, expected) in cases {
        let mock = server
            .mock("GET", USAGE_PATH)
            .match_query(Matcher::Any)
            .with_status(status)
            .with_body("secret echo")
            .expect(1)
            .create_async()
            .await;
        let result = provider.fetch_usage(&ctx).await;
        mock.assert_async().await;
        assert_other(result, expected);
    }
}

#[tokio::test]
async fn an_invalid_workspace_id_sends_nothing() {
    let mut server = Server::new_async().await;
    let never = server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let provider = test_provider(&server.url());

    match provider
        .fetch_usage(&context(SourceMode::Auto, Some("abc123"), Some(KEY)))
        .await
    {
        Err(ProviderError::NotInstalled(message)) => assert_eq!(message, WORKSPACE_FORMAT),
        other => panic!("expected the workspace format error, got {other:?}"),
    }
    never.assert_async().await;
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported_and_send_nothing() {
    let mut server = Server::new_async().await;
    let never = server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let provider = test_provider(&server.url());

    for mode in [SourceMode::Web, SourceMode::Cli] {
        match provider
            .fetch_usage(&context(mode, Some(WORKSPACE), Some(KEY)))
            .await
        {
            Err(ProviderError::UnsupportedSource(source)) => assert_eq!(source, mode),
            other => panic!("expected UnsupportedSource, got {other:?}"),
        }
    }
    never.assert_async().await;
}
