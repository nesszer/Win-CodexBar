//! Fixture tests for the Parallel balance provider. Fetch tests pass a manual
//! Cookie header or the shell's empty-manual flag, and the browser fallback
//! uses an injected browser closure, so no test reads a real browser or the
//! keyring.

use super::*;
use mockito::{Matcher, Mock, Server, ServerGuard};

const COOKIE: &str = "__Secure-session=fixture";
const OK_BODY: &str = r#"{"kind":"ok","data":{"balance":12345,"pendingDebitBalanceCents":250}}"#;
const RPC_PATH: &str = "/api/rpc/billing/payments/refresh-balance";

fn test_provider(base_url: &str) -> ParallelProvider {
    ParallelProvider::with_balance_url(&format!("{base_url}{RPC_PATH}"))
}

/// `cookie: None` is the shell's off / empty-manual cookie source.
fn context(source_mode: SourceMode, cookie: Option<&str>) -> FetchContext {
    FetchContext {
        source_mode,
        manual_cookie_header: cookie.map(str::to_string),
        manual_cookie_missing: cookie.is_none(),
        ..FetchContext::default()
    }
}

/// No manual header and no missing flag, so the browser closure is consulted.
fn browser_context() -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Auto,
        ..FetchContext::default()
    }
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String)> {
    result
        .display_details
        .iter()
        .map(|detail| {
            assert_eq!(detail.section_title(), Some(SECTION_TITLE));
            (detail.title().to_string(), detail.value().to_string())
        })
        .collect()
}

fn row(title: &str, value: &str) -> (String, String) {
    (title.to_string(), value.to_string())
}

/// A balance request: only the pasted or browser Cookie, the console origin,
/// and an empty JSON body.
async fn balance_mock(
    server: &mut ServerGuard,
    cookie: &str,
    status: usize,
    content_type: &str,
    body: &str,
    hits: usize,
) -> Mock {
    server
        .mock("POST", RPC_PATH)
        .match_header("cookie", cookie)
        .match_header("origin", ORIGIN_URL)
        .match_body(Matcher::Exact("{}".into()))
        .with_status(status)
        .with_header("content-type", content_type)
        .with_body(body)
        .expect(hits)
        .create_async()
        .await
}

fn browsers(
    headers: Vec<(&'static str, &'static str)>,
) -> impl Fn() -> Result<Vec<(String, String)>, ProviderError> + Sync {
    move || {
        Ok(headers
            .iter()
            .map(|(browser, header)| (browser.to_string(), header.to_string()))
            .collect())
    }
}

fn assert_other(result: Result<ProviderFetchResult, ProviderError>, expected: &str) {
    match result {
        Err(ProviderError::Other(message)) => assert_eq!(message, expected),
        other => panic!("expected Other({expected:?}), got {other:?}"),
    }
}

#[test]
fn metadata_and_source_contract_match_the_web_only_provider() {
    let provider = ParallelProvider::new();
    assert_eq!(provider.id(), ProviderId::Parallel);
    assert_eq!(provider.metadata().display_name, "Parallel");
    assert!(!provider.metadata().default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web]
    );
    assert!(provider.supports_web());
    assert!(provider.owns_browser_cookie_resolution());
    assert_eq!(
        provider.manual_empty_cookie_policy(),
        ManualEmptyCookiePolicy::FailClosedWeb
    );
    assert_eq!(
        ProviderId::Parallel.cookie_domain(),
        Some("platform.parallel.ai")
    );
}

#[test]
fn balance_payload_matches_upstream_rows() {
    let result = parse_balance(OK_BODY.as_bytes()).unwrap();
    assert_eq!(
        rows(&result),
        vec![row("Balance", "$123.45"), row("Pending debit", "$2.50"),]
    );
    assert_eq!(result.source_label, "web");
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Browser session")
    );
    assert!(result.usage.primary.hidden_on_card);
    assert!(result.usage.primary.is_informational);
}

#[test]
fn zero_or_absent_pending_debit_adds_no_row() {
    for body in [
        r#"{"kind":"ok","data":{"balance":500,"pendingDebitBalanceCents":0}}"#,
        r#"{"kind":"ok","data":{"balance":500}}"#,
        r#"{"kind":"ok","data":{"balance":500,"pendingDebitBalanceCents":"7"}}"#,
    ] {
        let result = parse_balance(body.as_bytes()).unwrap();
        assert_eq!(rows(&result), vec![row("Balance", "$5.00")], "{body}");
    }
}

#[test]
fn malformed_success_bodies_fail_closed_as_format_changes() {
    for body in [
        "not json",
        "[]",
        r#"{"kind":"error","data":{"balance":1}}"#,
        r#"{"kind":"ok"}"#,
        r#"{"kind":"ok","data":{}}"#,
        r#"{"kind":"ok","data":{"balance":"12"}}"#,
        r#"{"kind":"ok","data":{"balance":null}}"#,
    ] {
        match parse_balance(body.as_bytes()) {
            Err(ProviderError::Parse(message)) => assert_eq!(message, FORMAT_CHANGED, "{body}"),
            other => panic!("{body}: expected a format change, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn manual_cookie_fetches_the_balance_with_the_console_origin() {
    let mut server = Server::new_async().await;
    let mock = balance_mock(&mut server, COOKIE, 200, "application/json", OK_BODY, 1).await;
    let provider = test_provider(&server.url());

    let result = provider
        .fetch_usage(&context(SourceMode::Web, Some(COOKIE)))
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Browser session")
    );
    assert_eq!(rows(&result)[0], row("Balance", "$123.45"));
}

#[tokio::test]
async fn signed_out_responses_report_an_expired_session() {
    let cases = [
        (401, "application/json", ""),
        (403, "application/json", ""),
        (302, "text/plain", ""),
        (200, "text/html; charset=utf-8", "<html>Log in</html>"),
    ];
    for (status, content_type, body) in cases {
        let mut server = Server::new_async().await;
        let mock = balance_mock(&mut server, COOKIE, status, content_type, body, 1).await;
        let provider = test_provider(&server.url());

        let error = provider
            .fetch_usage(&context(SourceMode::Web, Some(COOKIE)))
            .await
            .unwrap_err();

        mock.assert_async().await;
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession,
            "HTTP {status}"
        );
        assert_other(Err(error), SESSION_EXPIRED);
    }
}

#[tokio::test]
async fn other_http_failures_report_the_status_without_a_body() {
    for status in [404, 408, 429, 500, 503] {
        let mut server = Server::new_async().await;
        let mock = balance_mock(
            &mut server,
            COOKIE,
            status,
            "application/json",
            "secret-body-must-not-leak",
            1,
        )
        .await;
        let provider = test_provider(&server.url());

        let error = provider
            .fetch_usage(&context(SourceMode::Web, Some(COOKIE)))
            .await
            .unwrap_err();

        mock.assert_async().await;
        assert_other(Err(error), &format!("Parallel returned HTTP {status}."));
    }
}

#[tokio::test]
async fn browser_sessions_move_on_after_a_signed_out_session() {
    let mut server = Server::new_async().await;
    let expired = balance_mock(&mut server, "a=expired", 401, "application/json", "", 1).await;
    let valid = balance_mock(&mut server, "a=valid", 200, "application/json", OK_BODY, 1).await;
    let provider = test_provider(&server.url());
    let ctx = browser_context();
    let browser = browsers(vec![("Chrome", "a=expired"), ("Edge", "a=valid")]);

    let result = provider.fetch_web(&ctx, &browser).await.unwrap();

    expired.assert_async().await;
    valid.assert_async().await;
    assert_eq!(rows(&result)[0], row("Balance", "$123.45"));
}

#[tokio::test]
async fn every_browser_session_signed_out_reports_an_expired_session() {
    let mut server = Server::new_async().await;
    let first = balance_mock(&mut server, "a=one", 401, "application/json", "", 1).await;
    let second = balance_mock(&mut server, "a=two", 403, "application/json", "", 1).await;
    let provider = test_provider(&server.url());
    let ctx = browser_context();
    let browser = browsers(vec![("Chrome", "a=one"), ("Edge", "a=two")]);

    let error = provider.fetch_web(&ctx, &browser).await.unwrap_err();

    first.assert_async().await;
    second.assert_async().await;
    assert_other(Err(error), SESSION_EXPIRED);
}

#[tokio::test]
async fn no_usable_session_asks_the_user_to_sign_in() {
    let provider = ParallelProvider::new();
    let no_cookies =
        || -> Result<Vec<(String, String)>, ProviderError> { Err(ProviderError::NoCookies) };
    let error = provider
        .fetch_web(&browser_context(), &no_cookies)
        .await
        .unwrap_err();
    match error {
        ProviderError::BrowserSignInRequired {
            message,
            sign_in_url,
        } => {
            assert_eq!(message, MISSING_SESSION);
            assert_eq!(sign_in_url, SIGN_IN_URL);
        }
        other => panic!("expected a sign-in prompt, got {other:?}"),
    }
}

#[tokio::test]
async fn browser_read_failures_are_reported_instead_of_a_sign_in_prompt() {
    let provider = ParallelProvider::new();
    let broken = || -> Result<Vec<(String, String)>, ProviderError> {
        Err(ProviderError::Other("browser store locked".into()))
    };
    let error = provider
        .fetch_web(&browser_context(), &broken)
        .await
        .unwrap_err();
    assert_other(Err(error), "browser store locked");
}

#[tokio::test]
async fn off_and_oauth_sources_never_send_a_request() {
    let provider = ParallelProvider::new();
    assert_other(
        provider.fetch_usage(&context(SourceMode::Cli, None)).await,
        COOKIES_OFF,
    );
    match provider
        .fetch_usage(&context(SourceMode::OAuth, Some(COOKIE)))
        .await
    {
        Err(ProviderError::UnsupportedSource(SourceMode::OAuth)) => {}
        other => panic!("expected an unsupported OAuth source, got {other:?}"),
    }
}

#[tokio::test]
async fn empty_manual_source_asks_for_a_cookie_without_reading_a_browser() {
    let provider = ParallelProvider::new();
    match provider.fetch_usage(&context(SourceMode::Web, None)).await {
        Err(ProviderError::BrowserSignInRequired { message, .. }) => {
            assert_eq!(message, MISSING_SESSION);
        }
        other => panic!("expected a sign-in prompt, got {other:?}"),
    }
}
