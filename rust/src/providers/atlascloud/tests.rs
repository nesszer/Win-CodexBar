use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

fn balance_payload(amount: &str) -> String {
    format!(
        r#"{{"object":"balance","scope":"account","available":{{"currency":"usd","value":"{amount}"}}}}"#
    )
}

#[test]
fn accepts_decimal_zero_and_negative_balances_without_normalizing_text() {
    for amount in ["12.340", "0", "-0.25"] {
        assert_eq!(parse_balance(&balance_payload(amount)).unwrap(), amount);
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
    for amount in ["", "-", "+1", ".5", "1.", "1e3", "NaN", "Infinity", "1.2.3"] {
        assert!(
            matches!(
                parse_balance(&balance_payload(amount)),
                Err(ProviderError::Parse(_))
            ),
            "unexpectedly accepted {amount:?}"
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
    assert!(matches!(
        status_error(StatusCode::INTERNAL_SERVER_ERROR),
        ProviderError::Other(message) if message.contains("unavailable")
    ));
}

#[tokio::test]
async fn sends_bearer_request_and_exposes_balance_as_display_only_detail() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).expect("read request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        let body = balance_payload("-3.25");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    let context = FetchContext {
        api_key: Some("test-atlas-key".into()),
        ..FetchContext::default()
    };

    let result = provider.fetch_usage(&context).await.expect("balance fetch");
    let request = server.join().expect("test server thread");
    assert!(request.starts_with("get /public/v1/balance "));
    assert!(request.contains("authorization: bearer test-atlas-key"));
    assert_eq!(result.display_details().len(), 1);
    assert_eq!(result.display_details()[0].title(), "Available");
    assert_eq!(result.display_details()[0].value(), "-3.25");
    assert!(result.usage.primary.is_informational);
    assert!(result.usage.secondary.is_none());
    assert!(result.cost.is_none());
}

#[test]
fn missing_key_uses_the_shared_not_installed_error() {
    let result =
        crate::providers::resolve_api_key(None, "codexbar-atlascloud-test-without-credential", &[]);
    assert!(matches!(result, Err(ProviderError::NotInstalled(_))));
}
