use super::*;

mod dashboard_routes;
mod head_bound;

#[test]
fn rejects_non_loopback_hosts_by_default() {
    assert!(allowed_host("127.0.0.1:8080", "127.0.0.1"));
    assert!(allowed_host("localhost", "127.0.0.1"));
    assert!(allowed_host("[::1]:8080", "127.0.0.1"));
    assert!(!allowed_host("example.com", "127.0.0.1"));
    assert!(!allowed_host("127.0.0.1, example.com", "127.0.0.1"));
}

#[test]
fn allows_configured_non_loopback_host() {
    assert!(allowed_host("192.168.1.10:8080", "192.168.1.10"));
    assert!(allowed_host("192.168.1.10", "192.168.1.10"));
    // Loopback Host headers still work when bound to LAN.
    assert!(allowed_host("127.0.0.1:8080", "192.168.1.10"));
    assert!(!allowed_host("10.0.0.1", "192.168.1.10"));
}

#[test]
fn parses_usage_route_provider_query() {
    let request =
        parse_request("GET /usage?provider=deepseek HTTP/1.1\r\nHost: localhost:8080\r\n\r\n")
            .unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/usage");
    assert_eq!(request.query.get("provider"), Some(&"deepseek".to_string()));
}

#[test]
fn parses_authorization_header() {
    let request = parse_request(
        "GET /usage HTTP/1.1\r\nHost: localhost:8080\r\nAuthorization: Bearer secret-token\r\n\r\n",
    )
    .unwrap();
    assert_eq!(
        request.authorization.as_deref(),
        Some("Bearer secret-token")
    );
}

#[test]
fn validate_startup_requires_token_and_plain_http_for_lan() {
    assert!(validate_serve_startup("127.0.0.1", false, false).is_none());
    assert!(validate_serve_startup("127.0.0.1", true, false).is_none());

    let missing = validate_serve_startup("0.0.0.0", false, false).unwrap();
    assert!(missing.contains("dashboard-token"));

    let plain = validate_serve_startup("192.168.1.5", true, false).unwrap();
    assert!(plain.contains("allow-plain-http"));

    assert!(validate_serve_startup("192.168.1.5", true, true).is_none());
}

#[test]
fn validate_serve_args_accepts_loopback_without_token() {
    let config = validate_serve_args(&ServeArgs {
        port: 8080,
        host: "localhost".into(),
        refresh_interval: 60,
        request_timeout: 0.0,
        dashboard_token: None,
        metrics: false,
        allow_plain_http: false,
        identity: Some("redacted".into()),
    })
    .unwrap();
    assert_eq!(config.host, "127.0.0.1");
    assert!(config.token_digest.is_none());
    assert!(!config.metrics_enabled);
}

#[test]
fn validate_serve_args_rejects_lan_without_token() {
    let err = validate_serve_args(&ServeArgs {
        port: 8080,
        host: "0.0.0.0".into(),
        refresh_interval: 60,
        request_timeout: 0.0,
        dashboard_token: None,
        metrics: false,
        allow_plain_http: true,
        identity: Some("redacted".into()),
    })
    .unwrap_err()
    .to_string();
    assert!(err.contains("dashboard-token"));
}

#[test]
fn validate_serve_args_rejects_lan_without_allow_plain_http() {
    let err = validate_serve_args(&ServeArgs {
        port: 8080,
        host: "192.168.0.2".into(),
        refresh_interval: 60,
        request_timeout: 0.0,
        dashboard_token: Some("tok".into()),
        metrics: true,
        allow_plain_http: false,
        identity: Some("redacted".into()),
    })
    .unwrap_err()
    .to_string();
    assert!(err.contains("allow-plain-http"));
}

fn loopback_args(request_timeout: f64) -> ServeArgs {
    ServeArgs {
        port: 8080,
        host: "127.0.0.1".into(),
        refresh_interval: 60,
        request_timeout,
        dashboard_token: None,
        metrics: false,
        allow_plain_http: false,
        identity: None,
    }
}

#[test]
fn request_timeout_is_off_by_default_and_capped_at_one_day() {
    #[derive(clap::Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: ServeArgs,
    }
    let parse = |argv: &[&str]| {
        <Wrapper as clap::Parser>::try_parse_from(
            std::iter::once("serve").chain(argv.iter().copied()),
        )
        .map(|wrapper| wrapper.args.request_timeout)
    };
    assert_eq!(parse(&[]).unwrap(), 0.0);
    assert_eq!(parse(&["--request-timeout", "2.5"]).unwrap(), 2.5);
    assert_eq!(parse(&["--request-timeout", "-1"]).unwrap(), -1.0);
    assert!(parse(&["--request-timeout", "soon"]).is_err());

    let timeout = |secs: f64| validate_serve_args(&loopback_args(secs)).map(|c| c.request_timeout);
    assert_eq!(timeout(0.0).unwrap(), None, "0 waits for every provider");
    assert_eq!(timeout(30.0).unwrap(), Some(Duration::from_secs(30)));
    assert_eq!(timeout(0.25).unwrap(), Some(Duration::from_millis(250)));
    assert_eq!(
        timeout(1.0e9).unwrap(),
        Some(Duration::from_secs(86_400)),
        "clamped to one day"
    );
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        let err = timeout(invalid).unwrap_err().to_string();
        assert_eq!(err, "--request-timeout must be zero or greater.");
    }
}

#[tokio::test(start_paused = true)]
async fn data_routes_answer_504_after_the_request_deadline() {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let response = within_request_deadline(Some(deadline), std::future::pending::<String>()).await;
    assert!(
        response.starts_with("HTTP/1.1 504 Gateway Timeout\r\n"),
        "{response}"
    );
    assert!(response.ends_with("\r\n\r\n{\"error\":\"request timed out\"}"));

    let finished = within_request_deadline(Some(deadline), async { "done".to_string() }).await;
    assert_eq!(finished, "done");
    let unbounded = within_request_deadline(None, async {
        tokio::time::sleep(Duration::from_secs(3_600)).await;
        "late".to_string()
    })
    .await;
    assert_eq!(unbounded, "late", "no deadline waits for the route");
}

#[test]
fn auth_gate_constant_time_compare() {
    let digest = sha256_digest(b"correct-token");
    assert!(authorize_request(
        Some("Bearer correct-token"),
        Some(&digest)
    ));
    assert!(!authorize_request(
        Some("Bearer wrong-token"),
        Some(&digest)
    ));
    assert!(!authorize_request(None, Some(&digest)));
    assert!(!authorize_request(
        Some("Basic correct-token"),
        Some(&digest)
    ));
    // No configured token → open.
    assert!(authorize_request(None, None));
}

#[test]
fn bearer_token_extraction() {
    assert_eq!(bearer_token(Some("Bearer abc")), Some("abc".to_string()));
    assert_eq!(bearer_token(Some("bearer  xyz  ")), Some("xyz".to_string()));
    assert_eq!(bearer_token(Some("Bearer")), None);
    assert_eq!(bearer_token(Some("Token abc")), None);
}

#[test]
fn rejects_empty_dashboard_token() {
    let err = resolve_dashboard_token(Some("   "))
        .unwrap_err()
        .to_string();
    assert!(err.contains("empty"));
}

#[test]
fn usage_route_without_provider_follows_enabled_providers() {
    use crate::cli::usage::ProviderSelection;
    use crate::core::ProviderId;

    let enabled = || vec![ProviderId::Codex, ProviderId::Cursor];
    for absent in [None, Some("")] {
        assert_eq!(
            data::usage_selection(absent, enabled).unwrap(),
            ProviderSelection::Custom(vec![ProviderId::Codex, ProviderId::Cursor])
        );
    }
    assert_eq!(
        data::usage_selection(Some("claude"), || panic!(
            "explicit provider reads no settings"
        ))
        .unwrap(),
        ProviderSelection::Single(ProviderId::Claude)
    );
    assert!(data::usage_selection(Some("nope"), enabled).is_err());
}
