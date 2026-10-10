//! Wire-level pins for the MiniMax console and remains GET requests.

use super::*;

pub(super) const JSON_ACCEPT: &str = "application/json, text/plain, */*";

/// Console GET with the cookie, JSON accept, XHR and browser headers.
pub(super) fn console_mock(server: &mut mockito::Server, path: &str) -> mockito::Mock {
    browser_mock(server, path, JSON_ACCEPT, "XMLHttpRequest".into())
}

/// Console GET with the cookie, `accept`, `xhr` marker and browser headers.
fn browser_mock(
    server: &mut mockito::Server,
    path: &str,
    accept: &str,
    xhr: mockito::Matcher,
) -> mockito::Mock {
    let base = MiniMaxRegion::Global.base_url();
    server
        .mock("GET", path)
        .match_header("cookie", "session=fixture")
        .match_header("accept", accept)
        .match_header("x-requested-with", xhr)
        .match_header("user-agent", MiniMaxProvider::WEB_USER_AGENT)
        .match_header("accept-language", "en-US,en;q=0.9")
        .match_header("origin", base)
        .match_header(
            "referer",
            format!("{base}/user-center/payment/coding-plan").as_str(),
        )
}

#[tokio::test]
async fn cookie_remains_request_maps_statuses() {
    let cases: [(usize, Option<&str>); 5] = [
        (401, None),
        (403, None),
        (
            404,
            Some("Parse:MiniMax remains returned status 404 Not Found"),
        ),
        (
            405,
            Some("Parse:MiniMax remains returned status 405 Method Not Allowed"),
        ),
        (
            500,
            Some("Other:MiniMax remains returned status 500 Internal Server Error"),
        ),
    ];
    for (status, expected) in cases {
        let mut server = mockito::Server::new_async().await;
        let mock = console_mock(&mut server, "/remains")
            .with_status(status)
            .expect(1)
            .create_async()
            .await;
        let result = MiniMaxProvider::new()
            .fetch_remains_once(
                "session=fixture",
                &format!("{}/remains", server.url()),
                MiniMaxRegion::Global,
                Utc::now(),
            )
            .await;
        mock.assert_async().await;
        assert_status_error(result.map(|_| ()), expected, status);
    }
}

#[tokio::test]
async fn cookie_remains_request_parses_a_json_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = console_mock(&mut server, "/remains")
        .with_header("content-type", "application/json")
        .with_body(r#"{"base_resp":{"status_code":2062,"status_msg":"no plan"}}"#)
        .expect(1)
        .create_async()
        .await;
    let result = MiniMaxProvider::new()
        .fetch_remains_once(
            "session=fixture",
            &format!("{}/remains", server.url()),
            MiniMaxRegion::Global,
            Utc::now(),
        )
        .await;
    mock.assert_async().await;
    match result {
        Err(ProviderError::Other(message)) => assert_eq!(message, "no plan"),
        other => panic!("expected Other(\"no plan\"), got {other:?}"),
    }
}

#[tokio::test]
async fn coding_plan_page_request_sends_html_accept_without_the_xhr_marker() {
    let mut server = mockito::Server::new_async().await;
    let mock = browser_mock(
        &mut server,
        "/coding-plan",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        mockito::Matcher::Missing,
    )
    .expect(1)
    .create_async()
    .await;
    let client = http_client().expect("client");
    let response = console_get(
        &client,
        &format!("{}/coding-plan", server.url()),
        "session=fixture",
        MiniMaxRegion::Global,
        HTML_ACCEPT,
        false,
    )
    .send()
    .await
    .expect("send");
    mock.assert_async().await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

/// `None` expects AuthRequired; otherwise `"<Variant>:<message>"`.
pub(super) fn assert_status_error(
    result: Result<(), ProviderError>,
    expected: Option<&str>,
    status: usize,
) {
    let actual = match result {
        Err(ProviderError::AuthRequired) => None,
        Err(ProviderError::Parse(message)) => Some(format!("Parse:{message}")),
        Err(ProviderError::Other(message)) => Some(format!("Other:{message}")),
        other => panic!("status {status}: unexpected {other:?}"),
    };
    assert_eq!(actual.as_deref(), expected, "status {status}");
}
