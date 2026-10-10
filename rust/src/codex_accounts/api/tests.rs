use super::*;
use base64::Engine;

#[test]
fn credential_error_states_exclude_permission_and_transport_failures() {
    assert_eq!(
        CodexApiError::AuthenticationRequired("missing credentials".to_string()).state_kind(),
        ProviderStateKind::NeedsAuthentication
    );
    assert_eq!(
        CodexApiError::SessionExpired("unauthorized".to_string()).state_kind(),
        ProviderStateKind::ExpiredSession
    );
    assert_eq!(
        CodexApiError::PermissionDenied("forbidden".to_string()).state_kind(),
        ProviderStateKind::Unknown
    );
    assert_eq!(
        CodexApiError::Network("offline".to_string()).state_kind(),
        ProviderStateKind::Unknown
    );
}

#[tokio::test]
async fn active_fetches_use_and_sync_ambient_credentials_even_when_usage_fails() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};

    for (target_id, managed_newer, expected_token) in [
        ("active", false, "ambient-token"),
        ("active", true, "managed-token"),
        ("other", true, "managed-token"),
    ] {
        let ambient = tempfile::tempdir().unwrap();
        let managed = tempfile::tempdir().unwrap();
        let credentials = |account: &str, token: &str| AuthCredentials {
            access_token: token.into(),
            refresh_token: format!("refresh-{token}"),
            id_token: None,
            account_id: Some(account.into()),
            last_refresh: Some(Utc::now()),
        };
        save_credentials(ambient.path(), &credentials("active", "ambient-token")).unwrap();
        save_credentials(managed.path(), &credentials(target_id, "managed-token")).unwrap();
        let now = Utc::now();
        for (home, refreshed) in [
            (ambient.path(), now - chrono::TimeDelta::hours(1)),
            (
                managed.path(),
                if managed_newer {
                    now
                } else {
                    now - chrono::TimeDelta::hours(2)
                },
            ),
        ] {
            let path = home.join("auth.json");
            let mut json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            json["last_refresh"] = serde_json::json!(refreshed.to_rfc3339());
            std::fs::write(path, json.to_string()).unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = format!(
            "chatgpt_base_url = \"http://{}\"\n",
            listener.local_addr().unwrap()
        );
        for home in [ambient.path(), managed.path()] {
            std::fs::write(home.join("config.toml"), &config).unwrap();
        }
        super::super::file_locations::with_ambient_codex_home(ambient.path().to_owned());
        let api = CodexAccountApi {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
        };
        let server = async {
            let (mut stream, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(
                    timeout(Duration::from_secs(5), stream.read_u8())
                        .await
                        .unwrap()
                        .unwrap(),
                );
            }
            stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
            String::from_utf8(headers).unwrap().to_lowercase()
        };
        let (result, headers) =
            tokio::join!(api.fetch_snapshot(managed.path(), None, false), server);
        super::super::file_locations::clear_ambient_codex_home_override();
        assert!(result.is_err());
        assert!(headers.contains(&format!("authorization: bearer {expected_token}")));
        let saved = load_credentials(managed.path()).unwrap();
        assert_eq!(saved.access_token, expected_token);
        assert_eq!(saved.refresh_token, format!("refresh-{expected_token}"));
        assert_eq!(
            load_credentials(ambient.path()).unwrap().access_token,
            if target_id == "active" {
                expected_token
            } else {
                "ambient-token"
            }
        );
    }
}

#[tokio::test]
async fn overlapping_fetches_reload_credentials_and_keep_other_homes_parallel() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::{Duration, timeout};

    async fn request(listener: &TcpListener) -> (TcpStream, String) {
        let (mut stream, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(
                timeout(Duration::from_secs(5), stream.read_u8())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        (stream, String::from_utf8(headers).unwrap().to_lowercase())
    }
    async fn respond(mut stream: TcpStream) {
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
    }
    fn configure(home: &Path, address: std::net::SocketAddr, token: &str) {
        std::fs::write(
            home.join("config.toml"),
            format!("chatgpt_base_url = \"http://{address}\"\n"),
        )
        .unwrap();
        std::fs::write(
            home.join("auth.json"),
            serde_json::json!({"OPENAI_API_KEY":token}).to_string(),
        )
        .unwrap();
    }
    fn fetch(
        home: std::path::PathBuf,
    ) -> tokio::task::JoinHandle<Result<AccountUsageSnapshot, CodexApiError>> {
        tokio::spawn(async move {
            let api = CodexAccountApi {
                client: reqwest::Client::builder().no_proxy().build().unwrap(),
            };
            // Exercise per-home concurrency independently of other tests
            // that intentionally take the global account-switch write lock.
            super::super::fetch_coordination::fetch_home_snapshot(
                &api, &home, None, None, false, None,
            )
            .await
        })
    }

    let first_home = tempfile::tempdir().unwrap();
    let other_home = tempfile::tempdir().unwrap();
    let first_server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    configure(
        first_home.path(),
        first_server.local_addr().unwrap(),
        "old-token",
    );
    configure(
        other_home.path(),
        other_server.local_addr().unwrap(),
        "other-token",
    );
    let first = fetch(first_home.path().to_owned());
    let (first_stream, headers) = request(&first_server).await;
    assert!(headers.contains("authorization: bearer old-token"));
    // A lexical alias of the same auth path must share the first lane.
    let second = fetch(first_home.path().join("."));
    let other = fetch(other_home.path().to_owned());
    let (other_stream, headers) = request(&other_server).await;
    assert!(headers.contains("authorization: bearer other-token"));
    respond(other_stream).await;
    timeout(Duration::from_secs(5), other)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        timeout(Duration::from_millis(100), first_server.accept())
            .await
            .is_err()
    );

    // Model a rotated token being persisted by the first in-flight fetch.
    configure(
        first_home.path(),
        first_server.local_addr().unwrap(),
        "rotated-token",
    );
    respond(first_stream).await;
    timeout(Duration::from_secs(5), first)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let (second_stream, headers) = request(&first_server).await;
    assert!(headers.contains("authorization: bearer rotated-token"));
    respond(second_stream).await;
    timeout(Duration::from_secs(5), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[test]
fn parse_credentials_accepts_api_key() {
    let creds = parse_credentials_json(r#"{"OPENAI_API_KEY":"sk-test"}"#).unwrap();
    assert_eq!(creds.access_token, "sk-test");
    assert_eq!(creds.account_id, None);
}

#[test]
fn parse_credentials_accepts_tokens() {
    let creds = parse_credentials_json(
            r#"{"tokens":{"access_token":"at","refresh_token":"rt","account_id":"42"},"last_refresh":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
    assert_eq!(creds.access_token, "at");
    assert_eq!(creds.refresh_token, "rt");
    assert_eq!(creds.account_id.as_deref(), Some("42"));
}

#[test]
fn parse_credentials_missing_tokens_errors() {
    assert!(parse_credentials_json(r#"{"foo":1}"#).is_err());
}

#[test]
fn jwt_payload_decodes() {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"email":"a@b.c"}"#);
    let token = format!("eyJhbGciOiJub25lIn0.{payload}.");
    let parsed = jwt_payload(&token).unwrap();
    assert_eq!(parsed.get("email").and_then(|v| v.as_str()), Some("a@b.c"));
}

#[test]
fn normalize_window_roles_orders_session_first() {
    let weekly = UsageWindowSnapshot::new(10.0, None, 604_800);
    let session = UsageWindowSnapshot::new(10.0, None, 18_000);
    let (p, s) = normalize_window_roles(Some(weekly), Some(session));
    assert_eq!(p.unwrap().limit_window_seconds, 18_000);
    assert_eq!(s.unwrap().limit_window_seconds, 604_800);
}

#[test]
fn limit_reached_forces_100() {
    let payload = make_rate_limit();
    let (p, s) = make_normalized_windows(Some(&payload));
    assert_eq!(p.as_ref().unwrap().used_percent, 100.0);
    assert_eq!(s.as_ref().unwrap().used_percent, 100.0);
}

fn make_rate_limit() -> serde_json::Map<String, serde_json::Value> {
    serde_json::from_str(
            r#"{"allowed":true,"limit_reached":true,"primary_window":{"used_percent":40,"reset_at":0,"limit_window_seconds":18000},"secondary_window":{"used_percent":20,"reset_at":0,"limit_window_seconds":604800}}"#,
        )
        .unwrap()
}

#[test]
fn resolve_usage_url_default() {
    let dir = tempfile::tempdir().unwrap();
    let url = resolve_usage_url(dir.path());
    assert_eq!(url, "https://chatgpt.com/backend-api/wham/usage");
}

#[test]
fn parse_chatgpt_base_url_parses_quoted() {
    let url = parse_chatgpt_base_url(
        "# comment\nchatgpt_base_url = \"https://example.com/backend-api\"\n",
    )
    .unwrap();
    assert_eq!(url, "https://example.com/backend-api");
}

#[test]
fn equivalent_snapshots_match() {
    let mk = || AccountUsageSnapshot {
        email: Some("a@b.c".to_string()),
        provider_account_id: Some("x".to_string()),
        plan: Some("pro".to_string()),
        allowed: Some(true),
        limit_reached: None,
        primary_window: Some(UsageWindowSnapshot::new(12.0, Some(Utc::now()), 18_000)),
        secondary_window: None,
        credits: None,
        cost: None,
        subscription: None,
        updated_at: Utc::now(),
    };
    assert!(is_equivalent(&mk(), &mk()));
    let mut different = mk();
    different.plan = Some("plus".to_string());
    assert!(!is_equivalent(&mk(), &different));
}

#[test]
fn account_id_from_id_token_reads_auth() {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct-99"}}"#);
    let token = format!("h.{payload}.s");
    assert_eq!(
        account_id_from_id_token(Some(&token)).as_deref(),
        Some("acct-99")
    );
}
