//! Fixtures are the upstream v0.64.1 `TestsPlugin/NeuralWattPluginTests.swift` quota bodies.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::TimeZone;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;
use crate::providers::test_support::{mock_response_expect, mock_status_expect};

const FIXTURES: [&str; 10] = [
    r#"{
      "snapshot_at": "2026-04-16T18:30:00Z",
      "balance": {"credits_remaining_usd": 32.6774, "total_credits_usd": 52.34, "credits_used_usd": 19.6626, "accounting_method": "energy"},
      "usage": {
        "lifetime": {"cost_usd": 243.9145, "requests": 37801, "tokens": 1235477176, "energy_kwh": 15.6009},
        "current_month": {"cost_usd": 160.1463, "requests": 23902, "tokens": 1116658995, "energy_kwh": 9.7278}
      },
      "limits": {"overage_limit_usd": null, "rate_limit_tier": "standard"},
      "subscription": {
        "plan": "standard", "status": "active", "billing_interval": "month",
        "current_period_start": "2026-04-11T05:05:25Z", "current_period_end": "2026-05-11T05:05:25Z",
        "auto_renew": true, "kwh_included": 20.0, "kwh_used": 13.9023, "kwh_remaining": 6.0977, "in_overage": false
      },
      "key": {"name": "my-production-key", "allowance": {"limit_usd": 50.0, "period": "monthly", "spent_usd": 12.5, "remaining_usd": 37.5, "blocked": false}}
    }"#,
    r#"{
      "snapshot_at": "2026-04-16T18:30:00Z",
      "balance": {"credits_remaining_usd": 4.5, "total_credits_usd": 5.0, "credits_used_usd": 0.5, "accounting_method": "energy"},
      "usage": {
        "lifetime": {"cost_usd": 0.5, "requests": 10, "tokens": 1000, "energy_kwh": 0.01},
        "current_month": {"cost_usd": 0.5, "requests": 10, "tokens": 1000, "energy_kwh": 0.01}
      },
      "limits": {"overage_limit_usd": null, "rate_limit_tier": "free"},
      "subscription": null,
      "key": {"name": "trial", "allowance": null}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 30.0, "total_credits_usd": 100.0, "accounting_method": "energy"},
      "usage": {"lifetime": {}, "current_month": {}},
      "limits": {},
      "subscription": null,
      "key": {"name": "x", "allowance": null}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 0.0, "total_credits_usd": 0.0, "accounting_method": "energy"},
      "usage": {"lifetime": {}, "current_month": {}},
      "limits": {},
      "subscription": null,
      "key": {"name": "x", "allowance": null}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 0.0, "total_credits_usd": 0.0, "accounting_method": "energy"},
      "usage": {"lifetime": {}, "current_month": {}},
      "limits": {},
      "subscription": {
        "plan": "pro_energy", "status": "active",
        "current_period_start": "2026-04-01T00:00:00Z", "current_period_end": "2026-05-01T00:00:00Z",
        "kwh_included": 10.0, "kwh_used": 2.5, "kwh_remaining": 7.5
      },
      "key": {"name": "subscriber", "allowance": null}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 1.0},
      "subscription": {
        "plan": "standard", "status": "active", "current_period_end": "2026-05-01T00:00:00Z",
        "auto_renew": false, "kwh_included": 10.0, "kwh_used": 4.0, "kwh_remaining": 6.0
      },
      "key": {"name": "subscriber", "allowance": null}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 3.0},
      "subscription": null,
      "key": {"name": "blocked", "allowance": {"blocked": true, "period": "monthly"}}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 8.0, "total_credits_usd": 10.0, "credits_used_usd": 2.0, "accounting_method": "energy"},
      "usage": {"lifetime": {}, "current_month": {}},
      "limits": {},
      "subscription": {
        "plan": "standard", "status": "active",
        "current_period_start": "2026-04-11T05:05:25.123Z", "current_period_end": "2026-05-11T05:05:25.456Z"
      },
      "key": {"name": "x", "allowance": null}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 5.0, "total_credits_usd": 10.0, "credits_used_usd": 5.0, "accounting_method": "energy"},
      "usage": {"lifetime": {}, "current_month": {}},
      "limits": {},
      "subscription": null,
      "key": {"name": "k", "allowance": null}
    }"#,
    r#"{
      "balance": {"credits_remaining_usd": 5.0},
      "subscription": null,
      "key": {"name": "retry", "allowance": null}
    }"#,
];

fn parse(body: &str) -> Result<(UsageSnapshot, Option<CostSnapshot>), ProviderError> {
    let quota: QuotaResponse = serde_json::from_str(body)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Neuralwatt quota: {e}")))?;
    snapshot_from_quota(&quota)
}

fn test_client() -> Client {
    Client::builder()
        .no_proxy()
        .build()
        .expect("the test client should build")
}

fn mock_url(server: &mockito::ServerGuard) -> Url {
    Url::parse(&format!("{}/v1/quota", server.url())).expect("the mock server URL should be valid")
}

#[test]
fn upstream_fixtures_keep_windows_balances_and_identity() {
    let balances = [32.6774, 4.5, 30.0, 0.0, 0.0, 1.0, 3.0, 8.0, 5.0, 5.0];
    let percentages = [
        Some(13.9023 / 20.0 * 100.0),
        None,
        None,
        None,
        Some(25.0),
        Some(40.0),
        None,
        None,
        None,
        None,
    ];
    let labels = [
        Some("Standard plan"),
        Some("Energy"),
        Some("Energy"),
        Some("Energy"),
        Some("Pro Energy plan"),
        Some("Standard plan"),
        None,
        Some("Standard plan"),
        Some("Energy"),
        None,
    ];
    for (index, fixture) in FIXTURES.iter().enumerate() {
        let (snap, cost) = parse(fixture).unwrap_or_else(|e| panic!("fixture {index}: {e}"));
        let cost = cost.unwrap_or_else(|| panic!("fixture {index} has a prepaid balance"));
        assert_eq!(cost.balance, Some(balances[index]), "fixture {index}");
        assert_eq!(cost.currency_code, "USD");
        assert_eq!(
            snap.login_method.as_deref(),
            labels[index],
            "fixture {index}"
        );
        match percentages[index] {
            Some(percent) => {
                assert!(!snap.primary.is_informational, "fixture {index}");
                assert!(
                    (snap.primary.used_percent - percent).abs() < 1e-9,
                    "fixture {index}"
                );
            }
            None => assert!(snap.primary.is_informational, "fixture {index}"),
        }
        match index {
            0 => {
                assert_eq!(
                    snap.primary.reset_description.as_deref(),
                    Some("13.90 / 20 kWh")
                );
                assert_eq!(snap.primary.window_minutes, Some(43_200));
                assert_eq!(snap.extra_rate_windows.len(), 1);
                assert_eq!(snap.extra_rate_windows[0].title, "Key Monthly");
                assert!((snap.extra_rate_windows[0].window.used_percent - 25.0).abs() < 1e-9);
            }
            4 => assert_eq!(
                snap.primary.reset_description.as_deref(),
                Some("2.50 / 10 kWh")
            ),
            6 => {
                assert_eq!(snap.extra_rate_windows.len(), 1);
                assert!((snap.extra_rate_windows[0].window.used_percent - 100.0).abs() < 1e-9);
            }
            _ => assert!(snap.extra_rate_windows.is_empty(), "fixture {index}"),
        }
    }
}

#[test]
fn prepaid_balance_keeps_limit_and_used_credits() {
    let (_, cost) = parse(FIXTURES[0]).unwrap();
    let cost = cost.unwrap();
    assert!((cost.used - 19.6626).abs() < 1e-9);
    assert_eq!(cost.limit, Some(52.34));
    assert_eq!(cost.balance, Some(32.6774));
}

#[test]
fn renewal_date_follows_period_end_unless_auto_renew_is_false() {
    let period_end = chrono::Utc.with_ymd_and_hms(2026, 5, 11, 5, 5, 25).unwrap();
    let (snap, _) = parse(FIXTURES[0]).unwrap();
    assert_eq!(snap.primary.resets_at, Some(period_end));
    assert_eq!(
        snap.subscription,
        Some(SubscriptionMetadata::new(None, None, Some(period_end)))
    );

    // A missing `auto_renew` still renews (fixture 4).
    let (snap, _) = parse(FIXTURES[4]).unwrap();
    assert!(snap.subscription.is_some_and(|s| s.renews_at.is_some()));

    // `auto_renew: false` keeps the reset clock but drops the renewal date.
    let (snap, _) = parse(FIXTURES[5]).unwrap();
    assert!(snap.primary.resets_at.is_some());
    assert_eq!(snap.subscription, None);

    // No kWh window means nothing renews.
    let (snap, _) = parse(FIXTURES[1]).unwrap();
    assert_eq!(snap.subscription, None);
}

#[test]
fn derived_totals_and_fixed_decimal_kwh_match_upstream() {
    let (snap, cost) = parse(
        r#"{"balance":{"total_credits_usd":10,"credits_used_usd":3},
            "subscription":{"kwh_used":1.125,"kwh_remaining":2.675}}"#,
    )
    .unwrap();
    assert_eq!(cost.unwrap().balance, Some(7.0));
    assert_eq!(
        snap.primary.reset_description.as_deref(),
        Some("1.12 / 3.80 kWh")
    );

    let (snap, cost) = parse(r#"{"balance":{"total_credits_usd":10}}"#).unwrap();
    assert!(snap.primary.is_informational);
    assert!(cost.is_none());
    assert_eq!(snap.login_method, None);
}

#[test]
fn accounting_method_is_title_cased_only_without_a_plan() {
    let (snap, _) = parse(
        r#"{"balance":{"credits_remaining_usd":1,"accounting_method":"TOKEN"},"subscription":null}"#,
    )
    .unwrap();
    assert_eq!(snap.login_method.as_deref(), Some("Token"));

    let (snap, _) = parse(
        r#"{"balance":{"credits_remaining_usd":1,"accounting_method":"token"},
            "subscription":{"plan":"  "}}"#,
    )
    .unwrap();
    assert_eq!(snap.login_method.as_deref(), Some("Token"));

    let (snap, _) = parse(
        r#"{"balance":{"credits_remaining_usd":1,"accounting_method":"token"},
            "subscription":{"plan":"pro_ENERGY"}}"#,
    )
    .unwrap();
    assert_eq!(snap.login_method.as_deref(), Some("Pro Energy plan"));
}

#[test]
fn malformed_bodies_remain_parse_failures() {
    for body in [
        "{}",
        "[]",
        "null",
        "not JSON",
        r#"{"balance":{}}"#,
        r#"{"balance":{"credits_remaining_usd":-1}}"#,
        r#"{"balance":{"total_credits_usd":0,"credits_used_usd":-2}}"#,
        r#"{"balance":{"credits_remaining_usd":"1"}}"#,
        r#"{"balance":{"credits_remaining_usd":1},"subscription":{"current_period_end":"2026"}}"#,
        r#"{"balance":{"credits_remaining_usd":1},"subscription":{"current_period_start":"2026-05-01 00:00:00Z"}}"#,
        r#"{"balance":{"credits_remaining_usd":1},"subscription":{"current_period_end":""}}"#,
        r#"{"balance":{"credits_remaining_usd":1},"usage":{"current_month":{"requests":1.2}}}"#,
    ] {
        assert!(
            matches!(parse(body), Err(ProviderError::Parse(_))),
            "expected a parse failure for {body}"
        );
    }
}

#[test]
fn endpoint_override_must_be_https_or_a_bare_host() {
    let url = |raw: Option<&str>| quota_url_for(raw).map(|url| url.to_string());
    assert_eq!(url(None).unwrap(), "https://api.neuralwatt.com/v1/quota");
    assert_eq!(
        url(Some("  ")).unwrap(),
        "https://api.neuralwatt.com/v1/quota"
    );
    for (raw, expected) in [
        (
            "https://api.neuralwatt.test",
            "https://api.neuralwatt.test/v1/quota",
        ),
        (
            "api.neuralwatt.test/",
            "https://api.neuralwatt.test/v1/quota",
        ),
        (
            "https://api.neuralwatt.test/v1/",
            "https://api.neuralwatt.test/v1/quota",
        ),
        (
            "https://api.neuralwatt.test/prefix?tenant=a?b",
            "https://api.neuralwatt.test/prefix/v1/quota?tenant=a?b",
        ),
    ] {
        assert_eq!(url(Some(raw)).unwrap(), expected, "{raw}");
    }
    for raw in [
        "http://api.neuralwatt.test",
        "ftp://api.neuralwatt.test",
        "https://user:pass@api.neuralwatt.test",
        "https://api.neuralwatt.test%2f@evil.test",
        "https://",
    ] {
        let error = url(Some(raw)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Neuralwatt endpoint override NEURALWATT_API_URL must use HTTPS or a bare host.",
            "{raw}"
        );
    }
}

#[test]
fn retry_delay_uses_capped_retry_after_or_the_default() {
    let default = Duration::from_secs(1);
    assert_eq!(retry_delay(None, default), default);
    assert_eq!(retry_delay(Some("bad"), default), default);
    assert_eq!(retry_delay(Some("-3"), default), default);
    assert_eq!(retry_delay(Some("NaN"), default), default);
    assert_eq!(retry_delay(Some(" 0 "), default), Duration::ZERO);
    assert_eq!(
        retry_delay(Some("2.5"), default),
        Duration::from_millis(2500)
    );
    assert_eq!(retry_delay(Some("99"), default), Duration::from_secs(10));
}

#[test]
fn only_transient_statuses_are_retryable() {
    for status in [408, 429, 500, 502, 503, 504] {
        assert!(is_retryable_status(StatusCode::from_u16(status).unwrap()));
    }
    for status in [200, 400, 401, 403, 404, 501] {
        assert!(!is_retryable_status(StatusCode::from_u16(status).unwrap()));
    }
}

#[tokio::test]
async fn http_failure_is_retried_once_then_reported() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/v1/quota")
        .match_header("authorization", "Bearer fixture-key")
        .match_header("accept", "application/json")
        .with_status(503)
        .with_header("retry-after", "0")
        .with_body("unavailable")
        .expect(2)
        .create_async()
        .await;

    let error = fetch_quota(
        &test_client(),
        &mock_url(&server),
        "fixture-key",
        RETRY_DEFAULT_DELAY,
    )
    .await
    .unwrap_err();

    assert_eq!(error.to_string(), "Neuralwatt API error: HTTP 503");
    mock.assert_async().await;
}

#[tokio::test]
async fn transient_status_recovers_on_the_single_retry() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/v1/quota")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    let second = mock_response_expect(&mut server, "GET", "/v1/quota", 200, FIXTURES[0], 1).await;

    let body = fetch_quota(&test_client(), &mock_url(&server), "k", Duration::ZERO)
        .await
        .expect("the retry should succeed");

    assert!(body.balance.is_some());
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn auth_and_client_errors_are_never_retried() {
    for (status, auth) in [(401, true), (403, true), (404, false), (400, false)] {
        let mut server = mockito::Server::new_async().await;
        let mock = mock_status_expect(&mut server, "GET", "/v1/quota", status, 1).await;

        let error = fetch_quota(&test_client(), &mock_url(&server), "k", Duration::ZERO)
            .await
            .unwrap_err();

        assert_eq!(
            matches!(error, ProviderError::AuthRequired),
            auth,
            "{status}"
        );
        if !auth {
            assert_eq!(
                error.to_string(),
                format!("Neuralwatt API error: HTTP {status}")
            );
        }
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn retry_sleep_is_dropped_with_the_fetch_future() {
    let mut server = mockito::Server::new_async().await;
    let mock = mock_status_expect(&mut server, "GET", "/v1/quota", 503, 1).await;

    // Without `Retry-After` the retry waits the 30 s default; cancelling the
    // future (as the refresh timeout does) must end the wait immediately.
    let outcome = tokio::time::timeout(
        Duration::from_millis(500),
        fetch_quota(
            &test_client(),
            &mock_url(&server),
            "k",
            Duration::from_secs(30),
        ),
    )
    .await;

    assert!(
        outcome.is_err(),
        "the fetch should still be waiting to retry"
    );
    mock.assert_async().await;
}

/// How the raw-socket stub treats each accepted connection, in order. The last
/// entry repeats.
#[derive(Clone, Copy)]
enum Connection {
    /// Read the request and close without answering (connection lost).
    Drop,
    /// Read the request and never answer (client timeout).
    Hang,
    /// Answer with the upstream success fixture.
    Respond,
}

async fn stub_server(script: &'static [Connection]) -> (Url, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local ephemeral port should be available");
    let address = listener
        .local_addr()
        .expect("the local listener should expose its address");
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&accepted);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let index = counter.fetch_add(1, Ordering::SeqCst);
            let behavior = script[index.min(script.len() - 1)];
            tokio::spawn(async move {
                let mut request = [0_u8; 2048];
                stream.read(&mut request).await.unwrap_or_default();
                match behavior {
                    Connection::Drop => {}
                    Connection::Hang => tokio::time::sleep(Duration::from_secs(30)).await,
                    Connection::Respond => {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            FIXTURES[0].len(),
                            FIXTURES[0]
                        );
                        stream
                            .write_all(response.as_bytes())
                            .await
                            .unwrap_or_default();
                    }
                }
            });
        }
    });
    let url = Url::parse(&format!("http://{address}/v1/quota")).expect("the stub URL is valid");
    (url, accepted)
}

fn short_timeout_client() -> Client {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(200))
        .build()
        .expect("the test client should build")
}

#[tokio::test]
async fn timeout_is_retried_once() {
    let (url, accepted) = stub_server(&[Connection::Hang, Connection::Respond]).await;
    let body = fetch_quota(&short_timeout_client(), &url, "k", Duration::ZERO)
        .await
        .expect("the retry should succeed");
    assert!(body.balance.is_some());
    assert_eq!(accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn second_timeout_is_not_retried_again() {
    let (url, accepted) = stub_server(&[Connection::Hang]).await;
    let error = fetch_quota(&short_timeout_client(), &url, "k", Duration::ZERO)
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Network(ref e) if e.is_timeout()));
    assert_eq!(accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn lost_connection_is_retried_once() {
    let (url, accepted) = stub_server(&[Connection::Drop, Connection::Respond]).await;
    let body = fetch_quota(&test_client(), &url, "k", Duration::ZERO)
        .await
        .expect("the retry should succeed");
    assert!(body.balance.is_some());
    assert_eq!(accepted.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn refused_connection_is_retried_once() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("a local ephemeral port should be available");
    let address = listener
        .local_addr()
        .expect("the local listener should expose its address");
    drop(listener);
    let url = Url::parse(&format!("http://{address}/v1/quota")).unwrap();

    let error = fetch_quota(&test_client(), &url, "k", Duration::from_millis(1))
        .await
        .unwrap_err();
    let ProviderError::Network(error) = error else {
        panic!("expected a network error");
    };
    assert!(error.is_connect(), "expected a connect error: {error:?}");
    assert!(is_transient_transport_error(&error));
}

#[tokio::test]
async fn tls_handshake_failure_is_not_transient() {
    let (http_url, accepted) = stub_server(&[Connection::Respond]).await;
    let url = Url::parse(&http_url.as_str().replacen("http://", "https://", 1)).unwrap();

    let error = fetch_quota(&short_timeout_client(), &url, "k", Duration::ZERO)
        .await
        .unwrap_err();

    let ProviderError::Network(error) = error else {
        panic!("expected a network error");
    };
    assert!(!is_transient_transport_error(&error), "{error:?}");
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}
