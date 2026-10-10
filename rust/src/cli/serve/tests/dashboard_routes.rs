//! Upstream 0.48.0 A1–A5: dashboard routes.

use super::head_bound::{connected_pair, fast_budget, head_test_config};
use super::*;
use crate::cli::serve::collection::SnapshotCollection;
use dashboard::coordinator::{SnapshotArtifacts, SnapshotArtifactsBuildFn, SnapshotBuildFn};
use dashboard::snapshot::{
    AccountFetchEnvelope, ClaudeAccountsInput, DashboardIdentity as DashboardIdMode,
    ProviderFetchEnvelope, SnapshotInput, build_snapshot,
};

fn stub_build(identity: DashboardIdMode, with_accounts: bool, delay: Duration) -> SnapshotBuildFn {
    std::sync::Arc::new(move || {
        Box::pin(async move {
            if delay > Duration::ZERO {
                tokio::time::sleep(delay).await;
            }
            let mut usage = crate::core::UsageSnapshot::new(crate::core::RateWindow::new(11.0));
            usage.account_email = Some("dev@example.com".to_string());
            usage.login_method = Some("Claude Max".to_string());
            let claude_accounts = with_accounts.then(|| ClaudeAccountsInput {
                accounts: Ok(vec![AccountFetchEnvelope {
                    id: "u-1".to_string(),
                    label: "Work".to_string(),
                    active: true,
                    fetch: Ok(crate::core::ProviderFetchResult::new(usage.clone(), "test")),
                }]),
            });
            Ok(build_snapshot(&SnapshotInput {
                collection: SnapshotCollection {
                    providers: vec![ProviderFetchEnvelope {
                        id: "claude".to_string(),
                        display_name: "Claude".to_string(),
                        session_label: "Session".to_string(),
                        weekly_label: "Weekly".to_string(),
                        fetch: Ok(crate::core::ProviderFetchResult::new(usage, "test")),
                    }],
                    costs: std::collections::HashMap::new(),
                    claude_accounts,
                    generated_at: chrono::Utc::now(),
                    refresh_seconds: 60,
                    order: vec![],
                    enabled: std::collections::BTreeSet::new(),
                },
                identity,
                version: Some("test".to_string()),
                usage_bars_show_used: None,
            }))
        })
    })
}

fn stub_state_ok() -> dashboard::DashboardState {
    let build: SnapshotArtifactsBuildFn<metrics::MetricsSnapshot> =
        std::sync::Arc::new(|| {
            Box::pin(async move {
                let mut usage = crate::core::UsageSnapshot::new(
                    crate::core::RateWindow::with_details(11.0, Some(300), None, None),
                );
                usage.updated_at = chrono::Utc::now();
                let input = SnapshotInput {
                    collection: SnapshotCollection {
                        providers: vec![ProviderFetchEnvelope {
                            id: "codex".to_string(),
                            display_name: "Codex".to_string(),
                            session_label: "Session".to_string(),
                            weekly_label: "Weekly".to_string(),
                            fetch: Ok(crate::core::ProviderFetchResult::new(usage, "test")),
                        }],
                        costs: std::collections::HashMap::new(),
                        claude_accounts: None,
                        generated_at: chrono::Utc::now(),
                        refresh_seconds: 60,
                        order: vec!["codex".to_string()],
                        enabled: ["codex".to_string()].into_iter().collect(),
                    },
                    identity: DashboardIdMode::Redacted,
                    version: Some("test".to_string()),
                    usage_bars_show_used: None,
                };
                Ok(SnapshotArtifacts {
                    dashboard: build_snapshot(&input),
                    sidecar: Some(metrics::MetricsSnapshot::from_collection(&input.collection)),
                })
            })
        });
    dashboard::DashboardState::stub_with_artifacts(build, 3600, Some(DashboardIdMode::Redacted))
}

fn dashboard_test_config(
    token: Option<&str>,
    state: Option<dashboard::DashboardState>,
) -> ServeConfig {
    let mut config = head_test_config(fast_budget(), token);
    config.dashboard = state;
    config
}

#[test]
fn resolve_route_maps_paths() {
    let req = |path: &str, query: &[(&str, &str)]| {
        let mut request =
            parse_request(&format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")).unwrap();
        for (k, v) in query {
            request.query.insert(k.to_string(), v.to_string());
        }
        request
    };
    assert_eq!(
        resolve_route(&req("/", &[])),
        Some(ServeRoute::DashboardHome)
    );
    assert_eq!(
        resolve_route(&req("/health", &[])),
        Some(ServeRoute::Health)
    );
    assert_eq!(
        resolve_route(&req("/usage?provider=codex", &[])),
        Some(ServeRoute::Usage {
            provider: Some("codex".to_string())
        })
    );
    assert_eq!(
        resolve_route(&req("/metrics", &[])),
        Some(ServeRoute::Metrics)
    );
    assert_eq!(
        resolve_route(&req("/dashboard/v1/snapshot", &[])),
        Some(ServeRoute::DashboardSnapshot)
    );
    assert_eq!(
        resolve_route(&req("/icons/ProviderIcon-codex.svg", &[])),
        Some(ServeRoute::ProviderIcon {
            name: "ProviderIcon-codex".to_string()
        })
    );
    assert_eq!(resolve_route(&req("/icons/../x.svg", &[])), None);
    assert_eq!(resolve_route(&req("/icons/.svg", &[])), None);
    assert_eq!(resolve_route(&req("/dashboard/v1/other", &[])), None);
    assert_eq!(resolve_route(&req("/usage.json", &[])), None);
}

#[tokio::test]
async fn metrics_route_is_not_found_until_enabled() {
    let config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    let response = request_roundtrip_dashboard(
        b"GET /metrics HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer s3cret\r\n\r\n",
        config,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 404"), "got: {response}");
    assert!(response.contains(r#""error":"not found""#));
}

#[tokio::test]
async fn metrics_route_uses_bearer_gate_and_prometheus_content_type() {
    let mut missing_config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    missing_config.metrics_enabled = true;
    let missing = request_roundtrip_dashboard(
        b"GET /metrics HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        missing_config,
    )
    .await;
    assert!(missing.starts_with("HTTP/1.1 401"), "got: {missing}");
    assert!(missing.contains("WWW-Authenticate: Bearer\r\n"));

    let mut wrong_config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    wrong_config.metrics_enabled = true;
    let wrong = request_roundtrip_dashboard(
        b"GET /metrics HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer nope\r\n\r\n",
        wrong_config,
    )
    .await;
    assert!(wrong.starts_with("HTTP/1.1 401"), "got: {wrong}");

    let ready_state = stub_state_ok();
    ready_state.coordinator.get().await.unwrap();
    let mut ok_config = dashboard_test_config(Some("s3cret"), Some(ready_state));
    ok_config.metrics_enabled = true;
    let ok = request_roundtrip_dashboard(
        b"GET /metrics HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer s3cret\r\n\r\n",
        ok_config,
    )
    .await;
    assert!(ok.starts_with("HTTP/1.1 200"), "got: {ok}");
    assert!(ok.contains("Content-Type: text/plain; version=0.0.4; charset=utf-8\r\n"));
    assert!(ok.contains("Cache-Control: no-store\r\n"));
    assert!(
        ok.contains("codexbar_snapshot_schema_version 1\n"),
        "got: {ok}"
    );
    assert!(
        ok.contains("codexbar_quota_session_used_ratio{provider=\"codex\"}"),
        "got: {ok}"
    );
}

#[tokio::test]
async fn metrics_route_returns_500_when_exporter_state_is_missing() {
    let mut config = dashboard_test_config(Some("s3cret"), None);
    config.metrics_enabled = true;
    let response = request_roundtrip_dashboard(
        b"GET /metrics HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer s3cret\r\n\r\n",
        config,
    )
    .await;

    assert!(response.starts_with("HTTP/1.1 500"), "got: {response}");
    assert!(response.contains(r#""error":"dashboard not configured""#));
}

#[tokio::test]
async fn metrics_route_keeps_the_host_allowlist() {
    let mut config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    config.metrics_enabled = true;
    config.host = "192.0.2.10".to_string();
    let forbidden = request_roundtrip_dashboard(
        b"GET /metrics HTTP/1.1\r\nHost: 192.0.2.11:8080\r\nAuthorization: Bearer s3cret\r\n\r\n",
        config,
    )
    .await;
    assert!(forbidden.starts_with("HTTP/1.1 403"), "got: {forbidden}");
    assert!(forbidden.contains(r#""error":"forbidden host""#));
}

#[tokio::test]
async fn dashboard_home_serves_html_no_store() {
    let config = dashboard_test_config(None, Some(stub_state_ok()));
    let response =
        request_roundtrip_dashboard(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", config).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "got: {}",
        &response[..80.min(response.len())]
    );
    assert!(response.contains("Content-Type: text/html; charset=utf-8\r\n"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("/dashboard/v1/snapshot"));
    assert!(response.contains("ProviderIcon-codex.svg"));
}

#[tokio::test]
async fn icon_route_serves_svg_immutable_and_404s_unknown() {
    let config = dashboard_test_config(None, None);
    let response = request_roundtrip_dashboard(
        b"GET /icons/ProviderIcon-codex.svg HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "got: {}",
        &response[..80.min(response.len())]
    );
    assert!(response.contains("Content-Type: image/svg+xml\r\n"));
    assert!(response.contains("Cache-Control: public, max-age=86400, immutable\r\n"));
    assert!(response.contains("<svg"));

    let config = dashboard_test_config(None, None);
    let missing = request_roundtrip_dashboard(
        b"GET /icons/ProviderIcon-nope.svg HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    assert!(missing.starts_with("HTTP/1.1 404"));
}

#[tokio::test]
async fn snapshot_route_gates_on_token_and_advertises_bearer_on_401() {
    // No token -> 401 + WWW-Authenticate (upstream dashboard-rule parity).
    let config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    let unauthorized = request_roundtrip_dashboard(
        b"GET /dashboard/v1/snapshot HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    assert!(
        unauthorized.starts_with("HTTP/1.1 401"),
        "got: {unauthorized}"
    );
    assert!(unauthorized.contains("WWW-Authenticate: Bearer\r\n"));

    // Valid token -> 200 + schema v1 payload + no-store.
    let config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    let response = request_roundtrip_dashboard(
            b"GET /dashboard/v1/snapshot HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer s3cret\r\n\r\n",
            config,
        )
        .await;
    assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("\"schemaVersion\": 1"));
    assert!(response.contains("\"providers\""));
}

#[tokio::test]
async fn dashboard_home_and_icons_are_public_when_token_configured() {
    let config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    let response =
        request_roundtrip_dashboard(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", config).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "shell stays public: {response}"
    );
    let config = dashboard_test_config(Some("s3cret"), Some(stub_state_ok()));
    let icon = request_roundtrip_dashboard(
        b"GET /icons/ProviderIcon-claude.svg HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    assert!(
        icon.starts_with("HTTP/1.1 200"),
        "icons stay public: {icon}"
    );
    assert!(!icon.contains("WWW-Authenticate"));
}

#[tokio::test]
async fn snapshot_identity_modes_redact_or_expose() {
    // Redacted (default): `redacted@domain`, raw address never leaks.
    let state = dashboard::DashboardState::stub(
        stub_build(DashboardIdMode::Redacted, false, Duration::ZERO),
        3600,
        Some(DashboardIdMode::Redacted),
    );
    let config = dashboard_test_config(None, Some(state));
    let redacted = request_roundtrip_dashboard(
        b"GET /dashboard/v1/snapshot HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    assert!(redacted.contains("redacted@example.com"), "got: {redacted}");
    assert!(
        !redacted.contains("dev@example.com"),
        "raw email leaked: {redacted}"
    );

    // Full opt-in: real account email exposed.
    let state = dashboard::DashboardState::stub(
        stub_build(DashboardIdMode::Full, false, Duration::ZERO),
        3600,
        Some(DashboardIdMode::Full),
    );
    let config = dashboard_test_config(None, Some(state));
    let full = request_roundtrip_dashboard(
        b"GET /dashboard/v1/snapshot HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    assert!(full.contains("dev@example.com"), "got: {full}");
}

#[tokio::test]
async fn snapshot_claude_accounts_nest_under_claude_row() {
    let state = dashboard::DashboardState::stub(
        stub_build(DashboardIdMode::Redacted, true, Duration::ZERO),
        3600,
        Some(DashboardIdMode::Redacted),
    );
    let config = dashboard_test_config(None, Some(state));
    let response = request_roundtrip_dashboard(
        b"GET /dashboard/v1/snapshot HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    assert!(response.contains("\"accounts\""), "got: {response}");
    assert!(response.contains("\"label\": \"Work\""), "got: {response}");
    assert!(response.contains("\"active\": true"), "got: {response}");
    assert!(response.contains("redacted@example.com"));
}

#[tokio::test]
async fn snapshot_late_build_is_delivered_not_discarded() {
    // F9/2717 parity: a slow snapshot build completes and the response carries
    // the finished result — never a discarded-build error.
    let state = dashboard::DashboardState::stub(
        stub_build(DashboardIdMode::Redacted, false, Duration::from_millis(250)),
        3600,
        Some(DashboardIdMode::Redacted),
    );
    let config = dashboard_test_config(None, Some(state));
    let started = std::time::Instant::now();
    let response = request_roundtrip_dashboard(
        b"GET /dashboard/v1/snapshot HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        config,
    )
    .await;
    let elapsed = started.elapsed();
    assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
    assert!(response.contains("\"schemaVersion\": 1"));
    assert!(
        elapsed >= Duration::from_millis(250),
        "response arrived before the build finished: {elapsed:?}"
    );
}

/// Roundtrip helper for route-level tests (separate from head-level helper).
async fn request_roundtrip_dashboard(request: &[u8], config: ServeConfig) -> String {
    let (server, mut client) = connected_pair().await;
    let server_task = tokio::spawn(async move { handle_client(server, &config).await });
    client.write_all(request).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), client.read_to_end(&mut response))
        .await
        .expect("client read timed out")
        .unwrap();
    drop(client);
    server_task.await.unwrap().unwrap();
    String::from_utf8_lossy(&response).into_owned()
}
