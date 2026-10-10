//! Wire tests for the Claude Web requests: the `cedar_ember` usage opt-in
//! and the status and parse mapping of each GET.

use super::ClaudeWebApiFetcher;
use crate::core::ProviderError;
use crate::providers::test_support::{mock_response, mock_response_expect};
use mockito::{Matcher, Mock, Server, ServerGuard};

const USAGE_PATH: &str = "/organizations/org-123/usage";
const CLOUDFLARE_MESSAGE: &str = crate::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE;

const USAGE_BODY: &str = r#"{"five_hour": {"utilization": 11}}"#;
const OPTED_IN_BODY: &str = r#"{"five_hour": {"utilization": 11},
    "cedar_ember": {"eligible": true, "grants": [
        {"resets_left": 1, "resets_total": 1, "paused": false, "ends_at": null}]}}"#;

async fn get_usage(server: &ServerGuard) -> Result<super::UsageResponse, ProviderError> {
    let fetcher = ClaudeWebApiFetcher::new().with_base_url(server.url());
    let headers = ClaudeWebApiFetcher::build_headers("sessionKey=sk-ant-fixture-token");
    fetcher.get_usage("org-123", &headers).await
}

async fn opted_in(server: &mut ServerGuard, status: usize, body: &str) -> Mock {
    server
        .mock("GET", USAGE_PATH)
        .match_query(Matcher::UrlEncoded("cedar_ember".into(), "1".into()))
        .match_header("cookie", "sessionKey=sk-ant-fixture-token")
        .with_status(status)
        .with_body(body)
        .expect(1)
        .create_async()
        .await
}

async fn plain(server: &mut ServerGuard, status: usize, expected_calls: usize) -> Mock {
    server
        .mock("GET", USAGE_PATH)
        .match_query(Matcher::Missing)
        .match_header("cookie", "sessionKey=sk-ant-fixture-token")
        .with_status(status)
        .with_body(USAGE_BODY)
        .expect(expected_calls)
        .create_async()
        .await
}

#[tokio::test]
async fn successful_opt_in_keeps_the_reset_block_and_does_not_retry() {
    let mut server = Server::new_async().await;
    let first = opted_in(&mut server, 200, OPTED_IN_BODY).await;
    let retry = plain(&mut server, 200, 0).await;

    let usage = get_usage(&server).await.unwrap();

    assert!(usage.five_hour.is_some());
    assert!(usage.cedar_ember.is_some());
    first.assert_async().await;
    retry.assert_async().await;
}

#[tokio::test]
async fn rejected_opt_in_retries_once_without_it_and_keeps_usage_windows() {
    for status in [400, 403, 404, 422, 500, 503] {
        let mut server = Server::new_async().await;
        let first = opted_in(
            &mut server,
            status,
            r#"{"error": "unknown query parameter"}"#,
        )
        .await;
        let retry = plain(&mut server, 200, 1).await;

        let usage = get_usage(&server).await.unwrap();

        assert!(usage.five_hour.is_some(), "status {status}");
        assert!(usage.cedar_ember.is_none(), "status {status}");
        first.assert_async().await;
        retry.assert_async().await;
    }
}

#[tokio::test]
async fn retry_failure_keeps_normal_error_handling() {
    let mut server = Server::new_async().await;
    let first = opted_in(&mut server, 422, "{}").await;
    let retry = plain(&mut server, 500, 1).await;

    let error = get_usage(&server).await.err().unwrap();

    assert!(matches!(error, ProviderError::Other(message) if message.contains("500")));
    first.assert_async().await;
    retry.assert_async().await;
}

#[tokio::test]
async fn unauthorized_and_rate_limited_responses_are_not_retried() {
    for status in [401, 429] {
        let mut server = Server::new_async().await;
        let first = opted_in(&mut server, status, "{}").await;
        let retry = plain(&mut server, 200, 0).await;

        let error = get_usage(&server).await.err().unwrap();

        match status {
            401 => assert!(matches!(error, ProviderError::AuthRequired)),
            _ => assert!(matches!(error, ProviderError::Other(message) if message.contains("429"))),
        }
        first.assert_async().await;
        retry.assert_async().await;
    }
}

#[tokio::test]
async fn cloudflare_challenge_is_not_retried() {
    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", USAGE_PATH)
        .match_query(Matcher::UrlEncoded("cedar_ember".into(), "1".into()))
        .with_status(403)
        .with_header("cf-mitigated", "challenge")
        .with_body("challenge page")
        .expect(1)
        .create_async()
        .await;
    let retry = plain(&mut server, 200, 0).await;

    let error = get_usage(&server).await.err().unwrap();

    assert!(matches!(error, ProviderError::Other(message) if message == CLOUDFLARE_MESSAGE));
    first.assert_async().await;
    retry.assert_async().await;
}

#[tokio::test]
async fn ordinary_forbidden_retries_and_a_second_forbidden_is_an_auth_failure() {
    let mut server = Server::new_async().await;
    let first = opted_in(&mut server, 403, "permission denied").await;
    let retry = plain(&mut server, 403, 1).await;

    let error = get_usage(&server).await.err().unwrap();

    assert!(matches!(error, ProviderError::AuthRequired));
    first.assert_async().await;
    retry.assert_async().await;
}

fn describe(result: Result<String, ProviderError>) -> String {
    match result {
        Ok(value) => format!("ok {value}"),
        Err(ProviderError::AuthRequired) => "auth".to_string(),
        Err(ProviderError::Parse(message)) => format!("parse {message}"),
        Err(ProviderError::Other(message)) => format!("other {message}"),
        Err(other) => format!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn organization_lookup_maps_statuses_after_the_account_fallback() {
    let cloudflare = format!("other {CLOUDFLARE_MESSAGE}");
    let rows: [(usize, &str, &str); 6] = [
        (200, r#"[{"uuid": "org-9", "name": "Team"}]"#, "ok org-9"),
        (200, "[]", "parse No organizations found"),
        (401, "{}", "auth"),
        (403, "permission denied", "auth"),
        (403, "<title>Just a moment...</title>", &cloudflare),
        (
            500,
            "{}",
            "other Failed to get organizations: 500 Internal Server Error",
        ),
    ];
    for (status, body, expected) in rows {
        let mut server = Server::new_async().await;
        let account = mock_response(&mut server, "GET", "/account", 500, "{}").await;
        let orgs = mock_response(&mut server, "GET", "/organizations", status, body).await;
        let fetcher = ClaudeWebApiFetcher::new().with_base_url(server.url());
        let headers = ClaudeWebApiFetcher::build_headers("sessionKey=sk-ant-fixture-token");

        let got = describe(
            fetcher
                .get_organization_id("sessionKey=sk-ant-fixture-token", &headers)
                .await,
        );

        assert_eq!(got, expected, "status {status}");
        account.assert_async().await;
        orgs.assert_async().await;
    }
}

#[tokio::test]
async fn organization_lookup_prefers_the_cookie_then_the_account() {
    let mut server = Server::new_async().await;
    let account = mock_response_expect(
        &mut server,
        "GET",
        "/account",
        200,
        r#"{"memberships": [{"organization": {"uuid": " org-acct "}}]}"#,
        1,
    )
    .await;
    let orgs = mock_response_expect(&mut server, "GET", "/organizations", 200, "[]", 0).await;
    let fetcher = ClaudeWebApiFetcher::new().with_base_url(server.url());
    let headers = ClaudeWebApiFetcher::build_headers("sessionKey=sk-ant-fixture-token");

    let from_cookie = fetcher
        .get_organization_id(
            "sessionKey=sk-ant-fixture-token; lastActiveOrg=org-cookie",
            &headers,
        )
        .await;
    let from_account = fetcher
        .get_organization_id("sessionKey=sk-ant-fixture-token", &headers)
        .await;

    assert_eq!(describe(from_cookie), "ok org-cookie");
    assert_eq!(describe(from_account), "ok org-acct");
    account.assert_async().await;
    orgs.assert_async().await;
}

#[tokio::test]
async fn extra_usage_and_account_failures_keep_their_plain_status_text() {
    let rows: [(usize, &str, &str, &str); 4] = [
        (
            401,
            "{}",
            "other Failed to get extra usage: 401 Unauthorized",
            "other Failed to get account: 401 Unauthorized",
        ),
        (
            403,
            "<title>Just a moment...</title>",
            "other Failed to get extra usage: 403 Forbidden",
            "other Failed to get account: 403 Forbidden",
        ),
        (
            500,
            "{}",
            "other Failed to get extra usage: 500 Internal Server Error",
            "other Failed to get account: 500 Internal Server Error",
        ),
        (
            200,
            "not json",
            "parse Failed to parse extra usage: ",
            "parse Failed to parse account: ",
        ),
    ];
    for (status, body, extra_expected, account_expected) in rows {
        let mut server = Server::new_async().await;
        let extra = mock_response(
            &mut server,
            "GET",
            "/organizations/org-123/overage_spend_limit",
            status,
            body,
        )
        .await;
        let account = mock_response(&mut server, "GET", "/account", status, body).await;
        let fetcher = ClaudeWebApiFetcher::new().with_base_url(server.url());
        let headers = ClaudeWebApiFetcher::build_headers("sessionKey=sk-ant-fixture-token");

        let extra_got = describe(
            fetcher
                .get_extra_usage("org-123", &headers)
                .await
                .map(|usage| format!("{:?}", usage.monthly_credit_limit)),
        );
        let account_got = describe(
            fetcher
                .get_account_info(&headers)
                .await
                .map(|info| format!("{:?}", info.email_address)),
        );

        if status == 200 {
            assert!(extra_got.starts_with(extra_expected), "{extra_got}");
            assert!(account_got.starts_with(account_expected), "{account_got}");
        } else {
            assert_eq!(extra_got, extra_expected, "status {status}");
            assert_eq!(account_got, account_expected, "status {status}");
        }
        extra.assert_async().await;
        account.assert_async().await;
    }
}

#[tokio::test]
async fn extra_usage_and_account_parse_successful_bodies() {
    let mut server = Server::new_async().await;
    let extra = mock_response(
        &mut server,
        "GET",
        "/organizations/org-123/overage_spend_limit",
        200,
        r#"{"monthly_credit_limit": 5000, "used_credits": 1200, "currency": "USD", "is_enabled": true}"#,
    )
    .await;
    let account = mock_response(
        &mut server,
        "GET",
        "/account",
        200,
        r#"{"email_address": "a@example.com", "rate_limit_tier": "default_claude_max_5x"}"#,
    )
    .await;
    let fetcher = ClaudeWebApiFetcher::new().with_base_url(server.url());
    let headers = ClaudeWebApiFetcher::build_headers("sessionKey=sk-ant-fixture-token");

    let usage = fetcher.get_extra_usage("org-123", &headers).await.unwrap();
    let info = fetcher.get_account_info(&headers).await.unwrap();

    assert_eq!(usage.monthly_credit_limit, Some(5000.0));
    assert_eq!(usage.used_credits, Some(1200.0));
    assert_eq!(usage.currency.as_deref(), Some("USD"));
    assert_eq!(usage.is_enabled, Some(true));
    assert_eq!(info.email_address.as_deref(), Some("a@example.com"));
    assert_eq!(
        info.rate_limit_tier.as_deref(),
        Some("default_claude_max_5x")
    );
    extra.assert_async().await;
    account.assert_async().await;
}
