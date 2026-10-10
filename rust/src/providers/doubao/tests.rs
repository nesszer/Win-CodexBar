use super::*;

#[test]
fn selected_ark_account_requires_its_projected_key() {
    let isolated = FetchContext {
        token_account_isolated: true,
        token_account_kind: Some(crate::core::TokenAccountKind::ApiKey),
        ..FetchContext::default()
    };
    assert!(matches!(
        selected_ark_api_key(&isolated),
        Err(ProviderError::AuthRequired)
    ));

    let selected = FetchContext {
        api_key: Some(" selected-key ".into()),
        ..isolated
    };
    assert_eq!(selected_ark_api_key(&selected).unwrap(), "selected-key");
}
use reqwest::header::{HeaderMap, HeaderValue};

/// Probe result for `status` with `(remaining, limit)` rate-limit headers.
fn probe_with(
    status: reqwest::StatusCode,
    remaining: Option<&'static str>,
    limit: Option<&'static str>,
) -> DoubaoProbeResult {
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("x-ratelimit-remaining-requests", remaining),
        ("x-ratelimit-limit-requests", limit),
    ] {
        if let Some(value) = value {
            headers.insert(name, HeaderValue::from_static(value));
        }
    }
    probe_result_from_response(status, &headers, &json!({}))
}

#[test]
fn doubao_snapshot_uses_rate_limit_headers() {
    let snapshot = probe_with(reqwest::StatusCode::OK, Some("25"), Some("100")).snapshot;
    assert_eq!(snapshot.primary.used_percent, 75.0);
}

#[test]
fn doubao_repeated_successful_zero_remaining_falls_back_to_active() {
    let result = probe_with(reqwest::StatusCode::OK, Some("0"), Some("1000"));
    assert!(result.has_ambiguous_zero_remaining());

    let snapshot = snapshot_from_parts(
        result.remaining,
        result.limit,
        result.resets_at,
        result.total_tokens,
        false,
    );
    assert_eq!(snapshot.primary.used_percent, 0.0);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("Active - check dashboard for details")
    );
}

#[test]
fn doubao_rate_limit_with_limit_header_reports_exhausted() {
    let snapshot = probe_with(reqwest::StatusCode::TOO_MANY_REQUESTS, None, Some("1000")).snapshot;

    assert_eq!(snapshot.primary.used_percent, 100.0);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("1000/1000 requests")
    );
}

#[test]
fn doubao_bare_rate_limit_uses_active_fallback() {
    let snapshot = probe_with(reqwest::StatusCode::TOO_MANY_REQUESTS, None, None).snapshot;

    assert_eq!(snapshot.primary.used_percent, 0.0);
    assert_eq!(
        snapshot.primary.reset_description.as_deref(),
        Some("Active - check dashboard for details")
    );
}

#[test]
fn doubao_parses_coding_plan_usage() {
    let body = br#"{
            "Result": {
                "Status": "active",
                "UpdateTimestamp": 1783036800,
                "QuotaUsage": [
                    {"Level": "session", "Percent": 12.5, "ResetTimestamp": 1783040400},
                    {"Level": "weekly", "Percent": 50.0, "ResetTimestamp": 1783641600},
                    {"Level": "monthly", "Percent": 75.0, "ResetTimestamp": 1785628800}
                ]
            }
        }"#;
    let snapshot = coding_plan_snapshot(decode_coding_plan_usage(body).unwrap());
    assert_eq!(snapshot.primary.used_percent, 12.5);
    assert_eq!(snapshot.secondary.unwrap().used_percent, 50.0);
    let monthly = snapshot.tertiary.expect("monthly");
    assert_eq!(monthly.used_percent, 75.0);
    // ResetTimestamp 1785628800 = 2026-08-02 → prior month is 31 days.
    assert_eq!(monthly.window_minutes, Some(31 * 24 * 60));
    assert_eq!(snapshot.login_method.as_deref(), Some("active"));
}

#[test]
fn doubao_parses_coding_plan_credentials() {
    let creds = DoubaoCodingPlanCredentials::parse("ak-test|sk-test|cn-shanghai").expect("creds");
    assert_eq!(creds.access_key_id, "ak-test");
    assert_eq!(creds.secret_access_key, "sk-test");
    assert_eq!(creds.region, "cn-shanghai");

    let json = r#"{"accessKeyId":"ak-json","secretAccessKey":"sk-json","region":"cn-beijing"}"#;
    let creds = DoubaoCodingPlanCredentials::parse(json).expect("json creds");
    assert_eq!(creds.access_key_id, "ak-json");
    assert_eq!(creds.secret_access_key, "sk-json");
}

#[test]
fn doubao_signer_sets_required_volcengine_headers() {
    let creds = DoubaoCodingPlanCredentials {
        access_key_id: "AKID".into(),
        secret_access_key: "SECRET".into(),
        region: "cn-beijing".into(),
    };
    let signed = sign_volcengine_request(
        &creds,
        b"",
        Utc.with_ymd_and_hms(2026, 7, 3, 0, 0, 0).unwrap(),
    )
    .unwrap();
    assert_eq!(signed.host, "open.volcengineapi.com");
    assert_eq!(signed.timestamp, "20260703T000000Z");
    assert_eq!(signed.payload_hash, sha256_hex(b""));
    assert!(
        signed
            .authorization
            .starts_with("HMAC-SHA256 Credential=AKID/20260703/cn-beijing/ark/request")
    );
}

#[test]
fn arkcli_usage_parses_coding_and_agent_plans() {
    let raw = br#"{
          "viewer": { "auth_method": "oauth" },
          "items": [
            {
              "product": "coding-plan",
              "subscribed": true,
              "updated_at": 1720000000,
              "periods": [
                { "label": "session", "percent": 12.5, "reset_at": "2026-07-21T10:00:00Z" },
                { "label": "weekly", "percent": 40.0, "reset_at": "2026-07-28T00:00:00Z" }
              ]
            },
            {
              "product": "agent-plan",
              "subscribed": true,
              "periods": [
                { "label": "session", "percent": 5.0 },
                { "label": "weekly", "percent": 15.0 }
              ]
            }
          ]
        }"#;
    let usage = decode_arkcli_usage(raw).expect("arkcli json");
    let snap = coding_plan_snapshot(usage);
    assert!((snap.primary.used_percent - 12.5).abs() < 0.01);
    assert!((snap.secondary.as_ref().unwrap().used_percent - 40.0).abs() < 0.01);
    assert!(
        snap.extra_rate_windows
            .iter()
            .any(|w| w.id == "doubao-agent-session")
    );
    assert!(
        snap.extra_rate_windows
            .iter()
            .any(|w| (w.window.used_percent - 5.0).abs() < 0.01)
    );
}

#[test]
fn arkcli_auth_none_is_auth_required() {
    let raw = br#"{ "viewer": { "auth_method": "none" }, "items": [] }"#;
    let err = decode_arkcli_usage(raw).unwrap_err();
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn arkcli_presence_maps_to_local_runtime_offline_but_api_key_stays_default() {
    assert_eq!(
        DoubaoProvider::new().error_state_kind(&ProviderError::NotInstalled(
            "arkcli was not found. Install arkcli, run 'arkcli auth login', or configure \
                 Doubao API credentials."
                .into(),
        )),
        crate::core::ProviderStateKind::LocalRuntimeOffline
    );
    // The shared API-key producer keeps the default mapping.
    assert_eq!(
        DoubaoProvider::new().error_state_kind(&ProviderError::NotInstalled(
            "API key not found. Set ARK_API_KEY in Preferences or environment.".into(),
        )),
        crate::core::ProviderStateKind::NeedsAuthentication
    );
}
