use super::*;
use crate::providers::test_support::{mock_response, mock_status};
use mockito::Matcher;

const NODE_BODY: &str = r#"{"memory":40000000000,"loaded":{"fixture/small:Q4_K_M":2000000000,"fixture/large:Q4_K_M":8000000000},"stored":{"fixture/small:Q4_K_M":2000000000,"fixture/large:Q4_K_M":8000000000,"fixture/idle":500000000}}"#;
const VERSION_BODY: &str = r#"{"version":"0.9.0","pid":1}"#;

fn client() -> Client {
    Client::builder().no_proxy().build().unwrap()
}

fn row<'a>(result: &'a ProviderFetchResult, id: &str) -> &'a ProviderDisplayDetail {
    result
        .display_details()
        .iter()
        .find(|row| row.id() == id)
        .unwrap_or_else(|| panic!("missing detail row {id}"))
}

// -- Endpoint policy ---------------------------------------------------------

#[test]
fn accepted_endpoints_normalize() {
    for (input, expected) in [
        ("localhost", "http://localhost:17434"),
        ("127.0.0.1", "http://127.0.0.1:17434"),
        ("0.0.0.0", "http://127.0.0.1:17434"),
        ("0.0.0.0:18000", "http://127.0.0.1:18000"),
        ("http://0.0.0.0:18000", "http://127.0.0.1:18000"),
        ("192.168.1.10", "http://192.168.1.10:17434"),
        ("daemon.local", "http://daemon.local:17434"),
        ("[::1]", "http://[::1]:17434"),
        ("127.0.0.1:18000", "http://127.0.0.1:18000"),
        ("http://localhost", "http://localhost"),
        ("https://llmman.example.com", "https://llmman.example.com"),
        ("http://127.0.0.1:17434///", "http://127.0.0.1:17434"),
        ("http://127.0.0.1:17434/v1/", "http://127.0.0.1:17434/v1"),
        ("  \"localhost\"  ", "http://localhost:17434"),
        ("", "http://127.0.0.1:17434"),
    ] {
        assert_eq!(
            validated_llmman_base_url(input).as_deref(),
            Ok(expected),
            "input {input:?}"
        );
    }
}

#[test]
fn private_network_http_hosts_are_accepted() {
    for host in [
        "http://10.1.2.3",
        "http://172.16.0.5",
        "http://172.31.255.255",
        "http://192.168.0.1",
        "http://169.254.10.10",
        "http://127.9.9.9",
        "http://[fd12:3456::1]",
        "http://[fe80::1]",
        "http://workstation.local",
    ] {
        assert!(validated_llmman_base_url(host).is_ok(), "{host}");
    }
}

#[test]
fn rejected_endpoints_fail_the_policy() {
    for input in [
        "http://127.0.0.1:17434?probe=1",
        "http://127.0.0.1:17434#x",
        "localhost?probe=1",
        "127.0.0.1#x",
        "http://127.0.0.1:17434?",
        "http://127.0.0.1:17434#",
        "http://public.example.com",
        "public.example.com",
        "http://user:password@127.0.0.1:17434",
        "https://user:password@example.com",
        "file:///tmp/llmman",
        "ftp://127.0.0.1",
        "http://172.32.0.1",
        "http://8.8.8.8",
        "http://[2001:db8::1]",
        "http://.local",
        "http://exa mple.local",
    ] {
        assert!(
            validated_llmman_base_url(input).is_err(),
            "input {input:?} should be rejected"
        );
    }
}

#[test]
fn requests_drop_a_trailing_v1() {
    assert_eq!(
        request_base("http://127.0.0.1:17434/v1"),
        "http://127.0.0.1:17434"
    );
    assert_eq!(
        request_base("http://127.0.0.1:17434"),
        "http://127.0.0.1:17434"
    );
    assert_eq!(
        request_base("https://h.example.com/proxy/v1"),
        "https://h.example.com/proxy"
    );
}

// -- Body mapping ------------------------------------------------------------

#[test]
fn node_fixture_maps_memory_and_models() {
    let node = parse_node(NODE_BODY.as_bytes()).unwrap();
    let result = build_result(&node, Some("0.9.0".into()), false);

    let primary = &result.usage.primary;
    assert_eq!(primary.used_percent, 25.0);
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("10.0 GB of 40.0 GB")
    );
    assert!(
        primary.description_is_detail,
        "memory text is a detail line, not reset wording"
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("Local daemon"));

    assert_eq!(row(&result, "loaded").value(), "2 · 10.0 GB");
    assert_eq!(row(&result, "stored").value(), "3 · 10.5 GB");
    assert_eq!(row(&result, "version").value(), "0.9.0");

    let large = row(&result, "loaded-model-0");
    assert_eq!(large.title(), "fixture/large:Q4_K_M");
    assert_eq!(large.value(), "8.0 GB");
    let progress = large.progress().unwrap();
    assert_eq!(progress.used() / progress.total(), 0.2);
    assert_eq!(
        row(&result, "loaded-model-1").title(),
        "fixture/small:Q4_K_M"
    );
}

#[test]
fn api_key_sets_the_login_method() {
    let node = parse_node(NODE_BODY.as_bytes()).unwrap();
    let result = build_result(&node, None, true);
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
    assert!(
        result
            .display_details()
            .iter()
            .all(|row| row.id() != "version")
    );
}

#[test]
fn idle_daemon_still_renders() {
    let node = parse_node(br#"{"memory":0,"loaded":{},"stored":{}}"#).unwrap();
    let result = build_result(&node, None, false);
    assert!(result.usage.primary.is_informational);
    assert_eq!(row(&result, "loaded").value(), "0 · 0 B");
    assert_eq!(row(&result, "stored").value(), "0 · 0 B");
    assert!(
        result
            .display_details()
            .iter()
            .all(|row| !row.id().starts_with("loaded-model"))
    );
}

#[test]
fn loaded_models_without_a_memory_budget_have_no_progress() {
    let node = parse_node(br#"{"memory":0,"loaded":{"a":5},"stored":{}}"#).unwrap();
    let result = build_result(&node, None, false);
    let model = row(&result, "loaded-model-0");
    assert_eq!(model.value(), "5 B");
    assert!(model.progress().is_none());
}

#[test]
fn loaded_models_are_largest_first_with_ties_by_name_and_capped() {
    let mut loaded = serde_json::Map::new();
    for index in 0..30 {
        loaded.insert(format!("model-{index:02}"), serde_json::json!(100));
    }
    loaded.insert("big".into(), serde_json::json!(900));
    loaded.insert("bad\nname".into(), serde_json::json!(1_000));
    let body = serde_json::json!({"memory": 10_000, "loaded": loaded, "stored": {}});
    let node = parse_node(body.to_string().as_bytes()).unwrap();
    let rows = loaded_model_rows(&node);

    // The unshowable name is skipped before the 24-row cap is applied.
    assert_eq!(rows.len(), 24);
    assert_eq!(rows[0].title(), "big");
    assert_eq!(rows[1].title(), "model-00");
    assert_eq!(rows[2].title(), "model-01");
    assert_eq!(rows[23].title(), "model-22");
}

#[test]
fn progress_is_capped_at_the_memory_budget() {
    let node = parse_node(br#"{"memory":10,"loaded":{"a":50},"stored":{}}"#).unwrap();
    let result = build_result(&node, None, false);
    assert_eq!(result.usage.primary.used_percent, 100.0);
    let progress = row(&result, "loaded-model-0").progress().unwrap();
    assert_eq!(progress.used(), progress.total());
}

#[test]
fn sizes_use_decimal_units() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(999), "999 B");
    assert_eq!(format_size(1_000), "1.0 kB");
    assert_eq!(format_size(1_500_000), "1.5 MB");
    assert_eq!(format_size(2_000_000_000), "2.0 GB");
    assert_eq!(format_size(40_000_000_000), "40.0 GB");
}

#[test]
fn malformed_bodies_are_parse_failures() {
    for body in [
        "not json",
        "[]",
        "[1,{},{}]",
        "null",
        r#"{"memory":-1,"loaded":{},"stored":{}}"#,
        r#"{"memory":1.5,"loaded":{},"stored":{}}"#,
        r#"{"memory":9007199254740992,"loaded":{},"stored":{}}"#,
        r#"{"memory":"40","loaded":{},"stored":{}}"#,
        r#"{"memory":1,"loaded":[],"stored":{}}"#,
        r#"{"memory":1,"loaded":{"a":"1"},"stored":{}}"#,
        r#"{"memory":1,"loaded":{"a":-1},"stored":{}}"#,
        r#"{"memory":1,"loaded":{}}"#,
        r#"{"loaded":{},"stored":{}}"#,
    ] {
        assert!(
            matches!(parse_node(body.as_bytes()), Err(ProviderError::Parse(_))),
            "body {body:?} should not parse"
        );
    }
}

// -- HTTP behavior -----------------------------------------------------------

#[tokio::test]
async fn fetches_node_and_version_with_a_bearer_key() {
    let mut server = mockito::Server::new_async().await;
    let node = server
        .mock("GET", "/llmman/node")
        .match_header("authorization", "Bearer sk-llmman")
        .with_body(NODE_BODY)
        .create_async()
        .await;
    let version = server
        .mock("GET", "/api/version")
        .match_header("authorization", "Bearer sk-llmman")
        .with_body(VERSION_BODY)
        .create_async()
        .await;

    let result = fetch_daemon(&client(), &server.url(), Some("sk-llmman"))
        .await
        .unwrap();

    node.assert_async().await;
    version.assert_async().await;
    assert_eq!(result.usage.primary.used_percent, 25.0);
    assert_eq!(row(&result, "version").value(), "0.9.0");
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
}

#[tokio::test]
async fn open_daemon_is_queried_without_an_authorization_header() {
    let mut server = mockito::Server::new_async().await;
    let node = server
        .mock("GET", "/llmman/node")
        .match_header("authorization", Matcher::Missing)
        .with_body(NODE_BODY)
        .create_async()
        .await;
    mock_status(&mut server, "GET", "/api/version", 404).await;

    let result = fetch_daemon(&client(), &server.url(), None).await.unwrap();

    node.assert_async().await;
    assert_eq!(result.usage.login_method.as_deref(), Some("Local daemon"));
    // The version request failed, which never fails the fetch.
    assert!(
        result
            .display_details()
            .iter()
            .all(|row| row.id() != "version")
    );
}

#[tokio::test]
async fn a_v1_base_is_dropped_from_request_paths() {
    let mut server = mockito::Server::new_async().await;
    let node = mock_response(&mut server, "GET", "/llmman/node", 200, NODE_BODY).await;
    mock_response(&mut server, "GET", "/api/version", 200, VERSION_BODY).await;

    fetch_daemon(&client(), &format!("{}/v1", server.url()), None)
        .await
        .unwrap();

    node.assert_async().await;
}

#[tokio::test]
async fn unusable_version_replies_are_ignored() {
    for reply in [
        "not json",
        r#"{"version":7}"#,
        r#"{"version":""}"#,
        r#"{"version":"bad\nlabel"}"#,
        r#"{"pid":1}"#,
    ] {
        let mut server = mockito::Server::new_async().await;
        mock_response(&mut server, "GET", "/llmman/node", 200, NODE_BODY).await;
        mock_response(&mut server, "GET", "/api/version", 200, reply).await;

        let result = fetch_daemon(&client(), &server.url(), None).await.unwrap();
        assert!(
            result
                .display_details()
                .iter()
                .all(|row| row.id() != "version"),
            "reply {reply:?}"
        );
    }
}

async fn node_status_error(status: usize, key: Option<&str>) -> ProviderError {
    let mut server = mockito::Server::new_async().await;
    mock_response(
        &mut server,
        "GET",
        "/llmman/node",
        status,
        "sk-echoed-secret",
    )
    .await;
    fetch_daemon(&client(), &server.url(), key)
        .await
        .unwrap_err()
}

#[tokio::test]
async fn rejected_credentials_are_classified_by_key_presence() {
    for status in [401, 403] {
        let error = node_status_error(status, None).await;
        assert!(
            matches!(&error, ProviderError::NotInstalled(message)
                if message == "llmman requires an API key. Set one in Settings or LLMMAN_API_KEY."),
            "{error:?}"
        );

        let error = node_status_error(status, Some("sk-secret")).await;
        assert!(
            matches!(&error, ProviderError::OAuthExpired(message)
                if *message == format!("llmman rejected the API key (HTTP {status}).")),
            "{error:?}"
        );
        assert!(!format!("{error:?}").contains("sk-secret"));
    }
}

#[tokio::test]
async fn other_statuses_map_to_friendly_errors() {
    let busy = node_status_error(429, None).await;
    assert!(matches!(&busy, ProviderError::Other(m) if m == "llmman is busy."));

    let down = node_status_error(503, None).await;
    assert!(matches!(&down, ProviderError::Other(m) if m == "llmman error: HTTP 503"));

    let foreign = node_status_error(404, None).await;
    assert!(
        matches!(&foreign, ProviderError::Other(m)
            if m.starts_with("http://127.0.0.1:") && m.ends_with(" is not an llmman daemon (HTTP 404).")),
        "{foreign:?}"
    );
}

#[tokio::test]
async fn unrecognized_body_is_a_parse_error() {
    let mut server = mockito::Server::new_async().await;
    mock_response(&mut server, "GET", "/llmman/node", 200, "not json").await;
    let error = fetch_daemon(&client(), &server.url(), None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ProviderError::Parse(ref m) if m == "llmman returned an unrecognized /llmman/node response."
    ));
}

#[tokio::test]
async fn unreachable_daemon_reports_offline_runtime() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let base = format!("http://127.0.0.1:{port}");

    let error = fetch_daemon(&client(), &base, None).await.unwrap_err();
    let ProviderError::Other(message) = &error else {
        panic!("expected Other, got {error:?}");
    };
    assert_eq!(
        *message,
        format!("llmman is not reachable at {base}. Start it with llmman serve.")
    );
    assert_eq!(
        LLMManProvider::new().error_state_kind(&error),
        ProviderStateKind::LocalRuntimeOffline
    );
}

#[test]
fn only_the_unreachable_error_is_an_offline_runtime() {
    let provider = LLMManProvider::new();
    for error in [
        ProviderError::Other("llmman is busy.".into()),
        ProviderError::NotInstalled("llmman requires an API key.".into()),
    ] {
        assert_ne!(
            provider.error_state_kind(&error),
            ProviderStateKind::LocalRuntimeOffline
        );
    }
}

#[test]
fn saved_base_url_is_validated() {
    assert_eq!(
        resolve_base_url(Some("192.168.1.10")).as_deref().ok(),
        Some("http://192.168.1.10:17434")
    );
    assert!(resolve_base_url(Some("http://public.example.com")).is_err());
}

#[test]
fn dashboard_follows_the_saved_daemon() {
    assert_eq!(
        dashboard_url(Some("192.168.1.10")),
        "http://192.168.1.10:17434"
    );
    assert_eq!(
        dashboard_url(Some("https://llmman.example.com/v1")),
        "https://llmman.example.com"
    );
    assert_eq!(
        dashboard_url(Some("http://public.example.com")),
        "http://127.0.0.1:17434"
    );
}

#[test]
fn metadata_matches_upstream() {
    let provider = LLMManProvider::new();
    let metadata = provider.metadata();
    assert_eq!(metadata.display_name, "llmman");
    assert_eq!(metadata.session_label, "Memory");
    assert_eq!(metadata.weekly_label, "Models");
    assert!(!metadata.default_enabled);
    assert!(!metadata.supports_credits);
    assert_eq!(metadata.dashboard_url, Some("http://127.0.0.1:17434"));
}
