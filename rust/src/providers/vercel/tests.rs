use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// Upstream fixture shape (`APIBalancePluginTests.swift`).
fn credits_payload(balance: &str) -> String {
    format!(r#"{{"balance":"{balance}","total_used":"4.50"}}"#)
}

fn assert_parse_failure(result: Result<Credits, ProviderError>) {
    match result {
        Err(ProviderError::Parse(message)) => {
            assert!(!message.contains("private-response"), "{message}");
        }
        other => panic!("expected parse failure, got {other:?}"),
    }
}

#[test]
fn parses_upstream_fixture_into_typed_amounts() {
    let credits = parse_credits(credits_payload("95.50").as_bytes()).unwrap();
    assert_eq!(credits.balance, 95.5);
    assert_eq!(credits.total_used, 4.5);
}

#[test]
fn accepts_zero_negative_and_unpadded_balances() {
    for (value, expected) in [("0.00", 0.0), ("-1.250000", -1.25), ("7", 7.0)] {
        let credits = parse_credits(credits_payload(value).as_bytes()).unwrap();
        assert_eq!(credits.balance, expected, "{value}");
    }
}

#[test]
fn rejects_malformed_amounts_without_echoing_response_data() {
    for value in ["", " ", "NaN", "1e999", "0x10", "private-response"] {
        assert_parse_failure(parse_credits(credits_payload(value).as_bytes()));
    }
    // Decimal strings that overflow f64 are not finite and must fail too.
    assert_parse_failure(parse_credits(credits_payload(&"9".repeat(400)).as_bytes()));
    for value in ["+1", ".5", "1.", "1e3", "Infinity", "1.2.3", "-", "--1"] {
        assert_parse_failure(parse_credits(credits_payload(value).as_bytes()));
    }
}

#[test]
fn rejects_wrong_shapes_without_echoing_response_data() {
    for body in [
        "private-response",
        "{}",
        "null",
        "",
        r#"{"balance":12,"total_used":"0"}"#,
        r#"{"balance":"1.00","total_used":0}"#,
        r#"{"balance":"1.00"}"#,
        r#"{"total_used":"1.00"}"#,
        r#"{"balance":null,"total_used":"1.00"}"#,
        r#"["private-response"]"#,
    ] {
        assert_parse_failure(parse_credits(body.as_bytes()));
    }
}

#[test]
fn rejects_negative_lifetime_spend() {
    assert_parse_failure(parse_credits(br#"{"balance":"1.00","total_used":"-1.00"}"#));
}

#[test]
fn ignores_unknown_fields_without_inventing_them() {
    let credits =
        parse_credits(br#"{"balance":"1.00","total_used":"2.00","extra":"private-response"}"#)
            .unwrap();
    assert_eq!(credits.balance, 1.0);
    assert_eq!(credits.total_used, 2.0);
}

#[test]
fn formats_usd_with_sign_before_the_symbol() {
    assert_eq!(format::usd_signed(95.5), "$95.50");
    assert_eq!(format::usd_signed(-1.25), "-$1.25");
    assert_eq!(format::usd_signed(0.0), "$0.00");
    assert_eq!(format::usd_signed(-0.001), "$0.00");
    assert_eq!(format::usd_signed(4.5), "$4.50");
}

#[test]
fn maps_statuses_without_response_bodies() {
    assert!(matches!(
        status_error(StatusCode::UNAUTHORIZED),
        ProviderError::AuthRequired
    ));
    for (status, needle) in [
        (StatusCode::FORBIDDEN, "HTTP 403"),
        (StatusCode::TOO_MANY_REQUESTS, "rate limit"),
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        (StatusCode::INTERNAL_SERVER_ERROR, "unavailable"),
        (StatusCode::BAD_REQUEST, "HTTP 400"),
    ] {
        match status_error(status) {
            ProviderError::Other(message) => {
                assert!(message.contains(needle), "{status}: {message}");
                assert!(message.contains(&status.as_u16().to_string()), "{message}");
            }
            other => panic!("{status}: unexpected {other:?}"),
        }
    }
}

#[test]
fn balance_result_is_informational_with_typed_balance_and_rows() {
    let result = build_result(Credits {
        balance: 95.5,
        total_used: 4.5,
    });
    assert!(result.usage.primary.is_informational);
    assert!(result.usage.secondary.is_none());
    assert_eq!(result.usage.login_method.as_deref(), Some("API"));
    let cost = result.cost.as_ref().expect("typed cost");
    assert_eq!(cost.balance, Some(95.5));
    assert_eq!(cost.used, 4.5);
    assert_eq!(cost.currency_code, "USD");
    assert!(!cost.always_visible, "lifetime spend is not a 30-day spend");
    let rows: Vec<_> = result
        .display_details()
        .iter()
        .map(|row| (row.title(), row.value()))
        .collect();
    assert_eq!(
        rows,
        [("Available balance", "$95.50"), ("Lifetime spend", "$4.50")]
    );
}

#[test]
fn zero_balance_is_a_typed_zero_and_negative_stays_display_only() {
    let zero = build_result(Credits {
        balance: 0.0,
        total_used: 4.5,
    });
    assert_eq!(zero.cost.as_ref().unwrap().balance, Some(0.0));
    assert_eq!(zero.display_details()[0].value(), "$0.00");

    let negative = build_result(Credits {
        balance: -1.25,
        total_used: 4.5,
    });
    assert_eq!(negative.cost.as_ref().unwrap().balance, None);
    assert_eq!(negative.display_details()[0].value(), "-$1.25");
}

#[test]
fn metadata_matches_upstream_descriptor() {
    let provider = VercelProvider::new();
    let metadata = provider.metadata();
    assert_eq!(metadata.display_name, "Vercel AI Gateway");
    assert_eq!(metadata.session_label, "Balance");
    assert_eq!(metadata.weekly_label, "Balance");
    assert!(!metadata.default_enabled);
    assert_eq!(
        metadata.dashboard_url,
        Some("https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai-gateway")
    );
    assert_eq!(ProviderId::Vercel.cli_name(), "vercel");
    assert_eq!(
        ProviderId::from_cli_name("vercel-ai-gateway"),
        Some(ProviderId::Vercel)
    );
    assert_eq!(
        provider.available_sources(),
        [SourceMode::Auto, SourceMode::OAuth]
    );
}

#[tokio::test]
async fn unsupported_sources_fail_before_any_request() {
    let provider = VercelProvider::with_client("http://127.0.0.1:1/v1/credits", Client::new());
    let context = FetchContext {
        source_mode: SourceMode::Web,
        api_key: Some("test-key".into()),
        ..FetchContext::default()
    };
    assert!(matches!(
        provider.fetch_usage(&context).await,
        Err(ProviderError::UnsupportedSource(SourceMode::Web))
    ));
}

/// One-shot local server; returns the lower-cased request head it received.
fn serve_once(
    status_line: &'static str,
    body: String,
) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).expect("read request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        write!(
            stream,
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len(),
        )
        .expect("write response");
        request
    });
    (format!("http://{address}/v1/credits"), server)
}

fn test_provider(url: String) -> VercelProvider {
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("test HTTP client");
    VercelProvider::with_client(url, client)
}

fn keyed_context() -> FetchContext {
    FetchContext {
        api_key: Some("  test-gateway-key  ".into()),
        ..FetchContext::default()
    }
}

#[tokio::test]
async fn sends_trimmed_bearer_request_and_reports_balance() {
    let (url, server) = serve_once("200 OK", credits_payload("-3.25"));
    let result = test_provider(url)
        .fetch_usage(&keyed_context())
        .await
        .expect("credits fetch");
    let request = server.join().expect("test server thread");
    assert!(request.starts_with("get /v1/credits "), "{request}");
    assert!(request.contains("authorization: bearer test-gateway-key\r\n"));
    assert_eq!(result.source_label, "api");
    assert_eq!(result.display_details()[0].value(), "-$3.25");
    assert_eq!(result.display_details()[1].value(), "$4.50");
    assert!(result.usage.primary.is_informational);
}

#[tokio::test]
async fn http_failures_expose_status_but_never_the_body() {
    for (status_line, needle) in [
        ("401 Unauthorized", "Authentication required"),
        ("403 Forbidden", "HTTP 403"),
        ("429 Too Many Requests", "rate limit"),
        ("503 Service Unavailable", "unavailable"),
        ("400 Bad Request", "HTTP 400"),
    ] {
        let (url, server) = serve_once(status_line, "private-response".into());
        let error = test_provider(url)
            .fetch_usage(&keyed_context())
            .await
            .expect_err(status_line);
        server.join().expect("test server thread");
        let message = error.to_string();
        assert!(message.contains(needle), "{status_line}: {message}");
        assert!(!message.contains("private-response"), "{message}");
    }
}

#[tokio::test]
async fn redirects_are_not_followed_with_the_api_key() {
    let (url, server) = serve_once(
        "302 Found\r\nLocation: http://127.0.0.1:9/steal",
        String::new(),
    );
    let error = test_provider(url)
        .fetch_usage(&keyed_context())
        .await
        .expect_err("redirect is not a success");
    server.join().expect("test server thread");
    assert!(error.to_string().contains("HTTP 302"), "{error}");
}

#[tokio::test]
async fn malformed_success_body_is_a_parse_error() {
    let (url, server) = serve_once("200 OK", "private-response".into());
    let error = test_provider(url)
        .fetch_usage(&keyed_context())
        .await
        .expect_err("malformed body");
    server.join().expect("test server thread");
    assert!(matches!(error, ProviderError::Parse(_)), "{error}");
    assert!(!error.to_string().contains("private-response"));
}

#[test]
fn missing_key_uses_the_shared_not_installed_error() {
    let result =
        crate::providers::resolve_api_key(None, "codexbar-vercel-test-without-credential", &[]);
    assert!(matches!(result, Err(ProviderError::NotInstalled(_))));
}
