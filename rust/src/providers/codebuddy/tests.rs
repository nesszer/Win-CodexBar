use super::*;

fn api_payload() -> Value {
    serde_json::json!({
        "code": 0,
        "msg": "ok",
        "data": {
            "Response": {
                "Data": {
                    "Accounts": [
                        {
                            "CapacitySizePrecise": "2000",
                            "CapacityUsedPrecise": "100",
                            "CapacityRemainPrecise": "1900",
                            "ExpireTime": "2026-09-01T00:00:00Z"
                        },
                        {
                            "CapacitySize": 1100,
                            "CapacityUsed": 11,
                            "CapacityRemain": 1089
                        }
                    ]
                }
            }
        }
    })
}

#[test]
fn parses_get_user_resource_payload_into_typed_totals() {
    let totals = totals_from_api_payload(&api_payload()).unwrap();
    assert_eq!(totals.total, 3100.0);
    assert_eq!(totals.used, 111.0);
    assert_eq!(totals.remaining, 2989.0);
    assert!(totals.reset.is_some());

    let snapshot = snapshot_from_totals(&totals);
    assert!((snapshot.primary.used_percent - (111.0 / 3100.0 * 100.0)).abs() < 0.01);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("2,989 / 3,100 left")
    );
    assert!(snapshot.primary.resets_at.is_some());
}

#[test]
fn auth_flavoured_payload_maps_to_auth_required() {
    for msg in ["未登录", "登录已过期", "auth token expired"] {
        let payload = serde_json::json!({ "code": 14001, "msg": msg });
        let err = totals_from_api_payload(&payload).unwrap_err();
        assert!(
            matches!(err, ProviderError::AuthRequired),
            "expected AuthRequired for msg={msg:?}, got {err}"
        );
    }
}

#[test]
fn empty_accounts_errors_with_hint() {
    let payload = serde_json::json!({
        "code": 0,
        "data": { "Response": { "Data": { "Accounts": [] } } }
    });
    let err = totals_from_api_payload(&payload).unwrap_err();
    assert!(format!("{err}").contains("PackageCodes") || format!("{err}").contains("package"));
}

#[test]
fn non_zero_code_errors() {
    let payload = serde_json::json!({ "code": 14001, "msg": "quote exceeded" });
    let err = totals_from_api_payload(&payload).unwrap_err();
    assert!(matches!(err, ProviderError::Other(_)), "got {err}");
}

#[test]
fn payloads_with_non_finite_values_are_rejected() {
    // serde_json cannot carry NaN/inf, but an absurd +/-1e308 pair can
    // still overflow the sum to inf — that must be rejected, not cached.
    let payload = serde_json::json!({
        "code": 0,
        "data": { "Response": { "Data": { "Accounts": [
            { "CapacitySize": 1e308, "CapacityUsed": 1e308, "CapacityRemain": 1e308 },
            { "CapacitySize": 1e308, "CapacityUsed": 0, "CapacityRemain": 0 }
        ] } } }
    });
    assert!(totals_from_api_payload(&payload).is_err());
}

#[test]
fn formats_compact_credit_labels() {
    assert_eq!(format_credits_short(1989.0, 3100.0), "1,989 / 3,100 left");
    assert_eq!(format_credits_short(12.5, 100.0), "12.5 / 100 left");
}

#[test]
fn normalizes_cookie_with_caret_escapes() {
    assert_eq!(
        normalize_cookie_header("Cookie: a=1^|2; b=3").as_deref(),
        Some("a=1|2; b=3")
    );
    assert_eq!(normalize_cookie_header("  "), None);
    assert_eq!(normalize_cookie_header("Cookie:"), None);
}

#[test]
fn cache_round_trip_preserves_typed_totals_exactly() {
    let totals = CreditTotals {
        total: 3100.5,
        used: 111.25,
        remaining: 2989.25,
        reset: Some(
            DateTime::parse_from_rfc3339("2026-09-01T08:30:00Z")
                .unwrap()
                .into(),
        ),
    };
    let json = cache_json_from_totals(&totals, Some("0123456789abcdef"));
    let parsed = totals_from_cache_json(&json).unwrap();
    assert_eq!(parsed, totals);
    // The cache carries the fingerprint verbatim.
    assert_eq!(
        json.get("accountHash").and_then(|v| v.as_str()),
        Some("0123456789abcdef")
    );
}

#[test]
fn cache_file_round_trip_via_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cb_credits.json");
    let totals = CreditTotals {
        total: 2000.0,
        used: 100.25,
        remaining: 1899.75,
        reset: None,
    };
    write_credits_cache(&path, &totals, Some("feedbeefcafe0001")).unwrap();

    let value = read_credits_cache(&path).unwrap();
    let parsed = totals_from_cache_json(&value).unwrap();
    assert_eq!(parsed, totals);

    // Raw file exposes typed JSON numbers, not display text.
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("\"total\": 2000.0"));
    assert!(raw.contains("feedbeefcafe0001"));
}

#[test]
fn hostile_cache_inputs_are_rejected() {
    // Missing total.
    let err = totals_from_cache_json(&serde_json::json!({"used": 1})).unwrap_err();
    assert!(format!("{err}").contains("missing total"));
    // Negative total.
    assert!(totals_from_cache_json(&serde_json::json!({"total": -5})).is_err());
    // Non-JSON is a Parse error at the read layer.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.json");
    std::fs::write(&path, b"{not json").unwrap();
    assert!(matches!(
        read_credits_cache(&path),
        Err(ProviderError::Parse(_))
    ));
    // Oversized cache is refused before parsing.
    let big = dir.path().join("big.json");
    #[allow(
        clippy::cast_possible_truncation,
        reason = "MAX_CACHE_BYTES is 1 MiB; +1 stays far inside usize on any supported target"
    )]
    std::fs::write(&big, vec![b' '; (MAX_CACHE_BYTES + 1) as usize]).unwrap();
    assert!(read_credits_cache(&big).is_err());
}

#[test]
fn validated_package_codes_filters_and_caps() {
    let long_code = "x".repeat(MAX_PACKAGE_CODE_LEN + 1);
    let items = vec![
        json!("TCACA_code_001_ok"),
        json!("   "),
        json!(""),
        json!(42),
        json!("aB\u{0007}c"),
        json!(long_code),
        json!("  TCACA_code_002_trimmed  "),
    ];
    let codes = validated_package_codes(&items);
    assert_eq!(
        codes,
        vec![
            "TCACA_code_001_ok".to_string(),
            "TCACA_code_002_trimmed".to_string(),
        ]
    );

    // Cap: more than MAX_PACKAGE_CODES valid entries are truncated.
    let many: Vec<Value> = (0..MAX_PACKAGE_CODES + 10)
        .map(|i| json!(format!("code_{i}")))
        .collect();
    assert_eq!(validated_package_codes(&many).len(), MAX_PACKAGE_CODES);
}

#[test]
fn validate_api_url_rules() {
    assert_eq!(
        validate_api_url("https://www.codebuddy.cn/x").unwrap(),
        "https://www.codebuddy.cn/x"
    );
    assert!(validate_api_url("http://127.0.0.1:8080/x").is_ok());
    assert!(validate_api_url("http://localhost:8080/x").is_ok());
    assert!(validate_api_url("http://[::1]:8080/x").is_ok());
    assert!(validate_api_url("http://example.com/x").is_err());
    assert!(validate_api_url("ftp://example.com/x").is_err());
    assert!(validate_api_url("not a url").is_err());
    assert!(validate_api_url("   ").is_err());
}

#[test]
fn failure_classification_for_http_statuses() {
    assert!(failure_for_status(StatusCode::OK).is_none());

    for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
        let fail = failure_for_status(status).unwrap();
        assert!(!fail.is_transient(), "{status} must be permanent");
        assert!(matches!(fail.into_error(), ProviderError::AuthRequired));
    }
    for status in [
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::BAD_GATEWAY,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        assert!(
            failure_for_status(status).unwrap().is_transient(),
            "{status} must be transient"
        );
    }
    // Other failures (400 etc.) are permanent but not auth errors.
    let fail = failure_for_status(StatusCode::BAD_REQUEST).unwrap();
    assert!(!fail.is_transient());
    assert!(matches!(fail.into_error(), ProviderError::Other(_)));
}

#[test]
fn cache_fallback_requires_auto_mode_and_transient_failure() {
    let transient = FetchFailure::Transient(ProviderError::Other("flake".into()));
    let permanent_auth = FetchFailure::Permanent(ProviderError::AuthRequired);
    let transient_ref = &transient;
    let auth_ref = &permanent_auth;

    // Auth never masks behind a stale cache.
    assert!(!cache_fallback_allowed(SourceMode::Auto, auth_ref));
    // Transient failures may fall back in Auto only.
    assert!(cache_fallback_allowed(SourceMode::Auto, transient_ref));
    assert!(!cache_fallback_allowed(SourceMode::Web, transient_ref));
    assert!(!cache_fallback_allowed(SourceMode::Cli, transient_ref));
}

#[test]
fn cookie_fingerprint_is_stable_per_account_and_secret_free() {
    let fp_a = cookie_fingerprint("session=aaa; uid=1");
    let fp_b = cookie_fingerprint("session=bbb; uid=2");
    assert_eq!(fp_a, cookie_fingerprint("session=aaa; uid=1"));
    assert_ne!(fp_a, fp_b);
    assert_eq!(fp_a.len(), 16);
    assert!(fp_a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn parse_datetime_accepts_rfc3339_naive_and_epochs() {
    assert!(parse_datetime("2026-09-01T00:00:00Z").is_some());
    assert!(parse_datetime("2026-09-01 12:30:00").is_some());
    assert_eq!(
        parse_datetime("1767225600").unwrap().to_rfc3339(),
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .to_rfc3339()
    );
    // Millis precision is divided down to seconds.
    assert_eq!(
        parse_datetime("1767225600000"),
        parse_datetime("1767225600")
    );
    assert!(parse_datetime("garbage").is_none());
}

#[test]
fn number_field_does_not_short_circuit_on_missing_keys() {
    let obj = serde_json::json!({"b": "2.5"});
    assert_eq!(number_field(&obj, &["missing", "b"]), Some(2.5));
    assert_eq!(number_field(&obj, &["missing", "other"]), None);
}

const USAGE_PATH: &str = "/billing/meter/get-user-resource";

/// Mock the usage endpoint once; `body` is `(content-type, body)`. The
/// server guard is returned so the mock outlives the call.
async fn mock_usage(
    status: usize,
    body: Option<(&str, &str)>,
    expect: Option<usize>,
) -> (mockito::ServerGuard, mockito::Mock, CodeBuddyProvider) {
    let mut server = mockito::Server::new_async().await;
    let mut mock = server.mock("POST", USAGE_PATH).with_status(status);
    if let Some((content_type, body)) = body {
        mock = mock
            .with_header("content-type", content_type)
            .with_body(body);
    }
    if let Some(hits) = expect {
        mock = mock.expect(hits);
    }
    let mock = mock.create_async().await;
    let mut provider = CodeBuddyProvider::new();
    provider.api_url = format!("{}{USAGE_PATH}", server.url());
    (server, mock, provider)
}

#[tokio::test]
async fn transient_failures_are_retried_exactly_once() {
    // One initial attempt + one retry.
    let (_server, mock, provider) = mock_usage(500, None, Some(2)).await;
    let fail = provider.fetch_web("a=1").await.unwrap_err();
    assert!(fail.is_transient());
    mock.assert_async().await;
}

#[tokio::test]
async fn auth_required_is_permanent_and_never_retried() {
    // Auth failures must not be retried.
    let (_server, mock, provider) = mock_usage(401, None, Some(1)).await;
    let fail = provider.fetch_web("a=1").await.unwrap_err();
    assert!(!fail.is_transient());
    assert!(matches!(fail.into_error(), ProviderError::AuthRequired));
    mock.assert_async().await;
}

#[tokio::test]
async fn waf_html_body_is_treated_as_transient() {
    let html = ("text/html", "<html><body>edgeone block</body></html>");
    let (_server, mock, provider) = mock_usage(200, Some(html), Some(2)).await;
    let fail = provider.fetch_web("a=1").await.unwrap_err();
    assert!(fail.is_transient());
    mock.assert_async().await;
}

/// End-to-end Auto behaviour with a real on-disk cache:
/// success persists typed totals; transient failure falls back to them;
/// auth failure surfaces even with a valid cache; foreign cache rejected.
#[tokio::test]
async fn auto_mode_cache_semantics_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("cb_credits.json");
    // SAFETY: single test using this env var; restored at the end.
    unsafe {
        std::env::set_var("CB_CREDITS_FILE", &cache_path);
    }

    let payload_body = r#"{"code":0,"msg":"ok","data":{"Response":{"Data":{"Accounts":[{"CapacitySize":2000,"CapacityUsed":100,"CapacityRemain":1900,"ExpireTime":"2026-09-01T00:00:00Z"}]}}}}"#;
    let ctx = |mode: SourceMode| FetchContext {
        source_mode: mode,
        manual_cookie_header: Some("session=abc; uid=42".to_string()),
        ..Default::default()
    };

    // Phase A: web success persists typed totals + fingerprint, source=web.
    {
        let json_body = ("application/json", payload_body);
        let (_server, mock, provider) = mock_usage(200, Some(json_body), Some(1)).await;
        let result = provider.fetch_usage(&ctx(SourceMode::Auto)).await.unwrap();
        assert_eq!(result.source_label, "web");
        mock.assert_async().await;

        let file = std::fs::read_to_string(&cache_path).unwrap();
        let json: Value = serde_json::from_str(&file).unwrap();
        assert_eq!(json.get("total").and_then(|v| v.as_f64()), Some(2000.0));
        assert_eq!(json.get("used").and_then(|v| v.as_f64()), Some(100.0));
        assert_eq!(json.get("remaining").and_then(|v| v.as_f64()), Some(1900.0));
        assert_eq!(
            json.get("accountHash").and_then(|v| v.as_str()),
            Some(cookie_fingerprint("session=abc; uid=42").as_str())
        );
        assert!(json.get("resetsAt").and_then(|v| v.as_str()).is_some());
    }

    // Phase B: auth failure must surface — never masked by the valid cache.
    {
        let (_server, mock, provider) = mock_usage(401, None, Some(1)).await;
        let err = provider
            .fetch_usage(&ctx(SourceMode::Auto))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::AuthRequired), "got {err}");
        mock.assert_async().await;
    }

    // Phase C: transient failure in Auto falls back to the cache (source=cli).
    {
        let (_server, mock, provider) = mock_usage(500, None, Some(2)).await;
        let result = provider.fetch_usage(&ctx(SourceMode::Auto)).await.unwrap();
        assert_eq!(result.source_label, "cli");
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("1,900 / 2,000 left")
        );
        assert!(
            (result.usage.primary.used_percent - 5.0).abs() < 0.01,
            "used_percent={}",
            result.usage.primary.used_percent
        );
        mock.assert_async().await;
    }

    // Phase D: a cache belonging to another account is rejected.
    {
        let mut json: Value =
            serde_json::from_str(&std::fs::read_to_string(&cache_path).unwrap()).unwrap();
        json["accountHash"] = json!("deadbeefdeadbeef");
        std::fs::write(&cache_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();

        let (_server, _mock, provider) = mock_usage(500, None, None).await;
        assert!(provider.fetch_usage(&ctx(SourceMode::Auto)).await.is_err());

        // Web mode with a transient failure must not fall back at all.
        let err = provider
            .fetch_usage(&ctx(SourceMode::Web))
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::Other(_)), "got {err}");
    }

    // SAFETY: this test set CB_CREDITS_FILE at its start under the same
    // single-test ownership; removing it restores the shared environment.
    unsafe {
        std::env::remove_var("CB_CREDITS_FILE");
    }
}
