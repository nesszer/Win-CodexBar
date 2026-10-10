use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

fn balance_payload(amount: &str) -> String {
    format!(
        r#"{{"object":"balance","scope":"account","available":{{"currency":"usd","value":"{amount}"}}}}"#
    )
}

#[test]
fn parses_zero_and_negative_balances_without_clamping() {
    for (amount, expected) in [
        ("12.340", 12.34),
        ("0", 0.0),
        ("0.00", 0.0),
        ("-1.250000", -1.25),
    ] {
        assert_eq!(parse_balance(&balance_payload(amount)).unwrap(), expected);
    }
}

#[test]
fn formats_signed_usd_like_upstream() {
    for (amount, expected) in [
        (95.5, "$95.50"),
        (0.0, "$0.00"),
        (-1.25, "-$1.25"),
        (-0.001, "$0.00"),
        (1234.5678, "$1234.57"),
    ] {
        assert_eq!(format::usd_signed(amount), expected);
    }
}

#[test]
fn rejects_wrong_envelope_currency_and_non_string_amounts() {
    let wrong_object = balance_payload("1").replace("balance", "credits");
    let wrong_scope = balance_payload("1").replace("account", "project");
    let wrong_currency = balance_payload("1").replace("usd", "eur");

    for body in [
        wrong_object.as_str(),
        wrong_scope.as_str(),
        wrong_currency.as_str(),
        r#"{"object":"balance","scope":"account","available":{"currency":"usd","value":1}}"#,
        r#"{"object":"balance","scope":"account","available":{}}"#,
        "not json",
    ] {
        assert!(matches!(parse_balance(body), Err(ProviderError::Parse(_))));
    }
}

#[test]
fn accepts_only_signed_decimal_strings_that_parse_to_finite_numbers() {
    for amount in [
        "",
        " ",
        "-",
        "+1",
        ".5",
        "1.",
        "1e3",
        "1e999",
        "0x10",
        "NaN",
        "Infinity",
        "1.2.3",
        "private-response",
    ] {
        let error = parse_balance(&balance_payload(amount)).expect_err(amount);
        let ProviderError::Parse(message) = error else {
            panic!("unexpected error kind for {amount:?}");
        };
        assert!(
            !message.contains("private-response"),
            "error echoed response text: {message}"
        );
    }
    let overflow = "9".repeat(400);
    assert!(matches!(
        parse_balance(&balance_payload(&overflow)),
        Err(ProviderError::Parse(_))
    ));
}

#[test]
fn maps_auth_permission_rate_limit_and_server_statuses() {
    assert!(matches!(
        status_error(StatusCode::UNAUTHORIZED),
        ProviderError::AuthRequired
    ));
    assert!(matches!(
        status_error(StatusCode::FORBIDDEN),
        ProviderError::Other(message) if message.contains("permissions")
    ));
    assert!(matches!(
        status_error(StatusCode::TOO_MANY_REQUESTS),
        ProviderError::Other(message) if message.contains("rate limit")
    ));
    assert!(matches!(
        status_error(StatusCode::BAD_REQUEST),
        ProviderError::Other(message) if message.contains("400")
    ));
    for status in [
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        assert!(matches!(
            status_error(status),
            ProviderError::Other(message) if message.contains("unavailable")
        ));
    }
}

/// Serve one canned HTTP response; the handle yields the lowercased request.
fn provider_serving(
    status_line: &'static str,
    body: String,
) -> (AtlasCloudProvider, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).expect("read request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        write!(
            stream,
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write response");
        request
    });
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("test HTTP client");
    let provider =
        AtlasCloudProvider::with_client(format!("http://{address}/public/v1/balance"), client);
    (provider, server)
}

fn keyed_context() -> FetchContext {
    FetchContext {
        api_key: Some("test-atlas-key".into()),
        ..FetchContext::default()
    }
}

#[tokio::test]
async fn sends_bearer_request_and_exposes_typed_balance_with_signed_display_row() {
    let (provider, server) = provider_serving("200 OK", balance_payload("-3.25"));

    let result = provider
        .fetch_usage(&keyed_context())
        .await
        .expect("balance fetch");
    let request = server.join().expect("test server thread");
    assert!(request.starts_with("get /public/v1/balance "));
    assert!(request.contains("authorization: bearer test-atlas-key"));
    assert_eq!(result.display_details().len(), 1);
    assert_eq!(result.display_details()[0].title(), "Available balance");
    assert_eq!(result.display_details()[0].value(), "-$3.25");
    assert!(result.usage.primary.is_informational);
    assert!(result.usage.secondary.is_none());
    assert_eq!(result.usage.login_method.as_deref(), Some("API"));
    let cost = result.cost.expect("typed balance carrier");
    assert_eq!(cost.currency_code, "USD");
    // `with_balance` clamps a deficit to zero; the display row keeps the sign.
    assert_eq!(cost.balance, Some(0.0));
}

#[tokio::test]
async fn positive_balance_is_typed_and_formatted() {
    let (provider, server) = provider_serving("200 OK", balance_payload("95.5"));

    let result = provider
        .fetch_usage(&keyed_context())
        .await
        .expect("balance fetch");
    server.join().expect("test server thread");
    assert_eq!(result.display_details()[0].value(), "$95.50");
    let cost = result.cost.expect("typed balance carrier");
    assert_eq!(cost.balance, Some(95.5));
    assert_eq!(cost.format_balance().as_deref(), Some("$95.50"));
}

#[tokio::test]
async fn non_ok_statuses_do_not_echo_the_response_body() {
    for status_line in [
        "400 Bad Request",
        "401 Unauthorized",
        "403 Forbidden",
        "429 Too Many Requests",
        "503 Service Unavailable",
    ] {
        let (provider, server) = provider_serving(status_line, "private-response".into());
        let error = provider
            .fetch_usage(&keyed_context())
            .await
            .expect_err(status_line);
        server.join().expect("test server thread");
        let message = error.to_string();
        assert!(
            !message.contains("private-response"),
            "{status_line}: {message}"
        );
    }
}

#[tokio::test]
async fn unparseable_success_body_does_not_echo_the_response_body() {
    let (provider, server) = provider_serving("200 OK", "private-response".into());
    let error = provider
        .fetch_usage(&keyed_context())
        .await
        .expect_err("invalid body");
    server.join().expect("test server thread");
    assert!(matches!(error, ProviderError::Parse(_)));
    assert!(!error.to_string().contains("private-response"));
}

#[test]
fn metadata_uses_the_single_canonical_console_url() {
    let provider = AtlasCloudProvider::new();
    assert_eq!(
        provider.metadata().dashboard_url,
        Some("https://www.atlascloud.ai/console")
    );
    let configured = crate::settings::get_api_key_providers()
        .into_iter()
        .find(|info| info.id == ProviderId::AtlasCloud)
        .expect("Atlas Cloud API-key catalog entry");
    assert_eq!(configured.dashboard_url, provider.metadata().dashboard_url);
}

#[test]
fn missing_key_uses_the_shared_not_installed_error() {
    let result =
        crate::providers::resolve_api_key(None, "codexbar-atlascloud-test-without-credential", &[]);
    assert!(matches!(result, Err(ProviderError::NotInstalled(_))));
}
