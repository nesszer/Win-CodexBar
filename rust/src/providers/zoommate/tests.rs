use super::*;

fn sample_status_json() -> &'static str {
    r#"{
          "data": {
            "credit_status": {
              "budget_cap": 1000.0,
              "used_credit": 250.0,
              "remaining_credit": 750.0,
              "overage_credit": 0.0,
              "allow_overage": false,
              "cycle_start_date": 1722470400000,
              "cycle_end_date": 1725148800000,
              "is_quota_available": true,
              "is_unlimited": false
            }
          },
          "status_code": 200
        }"#
}

#[test]
fn parses_credits_status_fixture() {
    let envelope: CreditsStatusEnvelope =
        serde_json::from_str(sample_status_json()).expect("fixture parses");
    let status = envelope.data.unwrap().credit_status.unwrap();
    let snap = snapshot_from_credit_status(&status, Some("user@zoom.us"), Utc::now());
    assert!((snap.primary.used_percent - 25.0).abs() < 0.01);
    assert_eq!(snap.primary.reset_description.as_deref(), Some("Credits"));
    assert!(snap.primary.resets_at.is_some());
    // 31 days ≈ 44640 minutes (2024-08-01 → 2024-09-01)
    assert_eq!(snap.primary.window_minutes, Some(44640));
    assert_eq!(snap.account_email.as_deref(), Some("user@zoom.us"));
    assert_eq!(snap.login_method.as_deref(), Some("Cookie"));
}

#[test]
fn unlimited_or_zero_cap_yields_zero_percent() {
    let unlimited = CreditStatus {
        budget_cap: Some(100.0),
        used_credit: Some(50.0),
        is_unlimited: Some(true),
        ..Default::default()
    };
    let snap = snapshot_from_credit_status(&unlimited, None, Utc::now());
    assert_eq!(snap.primary.used_percent, 0.0);
    assert!(snap.primary.resets_at.is_none());

    let zero_cap = CreditStatus {
        budget_cap: Some(0.0),
        used_credit: Some(10.0),
        is_unlimited: Some(false),
        ..Default::default()
    };
    let snap = snapshot_from_credit_status(&zero_cap, None, Utc::now());
    assert_eq!(snap.primary.used_percent, 0.0);
}

#[test]
fn clamps_used_percent_to_100() {
    let status = CreditStatus {
        budget_cap: Some(100.0),
        used_credit: Some(150.0),
        is_unlimited: Some(false),
        cycle_end_date: Some(1_900_000_000_000),
        ..Default::default()
    };
    let snap = snapshot_from_credit_status(&status, None, Utc::now());
    assert_eq!(snap.primary.used_percent, 100.0);
}

#[test]
fn manual_curl_capture_requires_allowed_url_and_authorization() {
    let good = "curl 'https://ai.zoom.us/ai-computer/api/v1/credits/status' \
            -H 'Authorization: Bearer tok-abc' -H 'Cookie: session=xyz'";
    let ctx = request_context_from_manual(good).expect("valid capture");
    assert_eq!(ctx.authorization, "Bearer tok-abc");
    assert_eq!(ctx.preferred_host.as_deref(), Some("ai.zoom.us"));
    assert_eq!(
        ctx.cookie_by_host.get("ai.zoom.us").map(String::as_str),
        Some("session=xyz")
    );

    // Wrong path
    assert!(
        request_context_from_manual(
            "curl 'https://ai.zoom.us/ai-computer/api/v1/other' -H 'Authorization: Bearer x'"
        )
        .is_none()
    );
    // Query rejected
    assert!(
        request_context_from_manual(
            "curl 'https://ai.zoom.us/ai-computer/api/v1/credits/status?x=1' \
                 -H 'Authorization: Bearer x'"
        )
        .is_none()
    );
    // Missing auth
    assert!(
        request_context_from_manual(
            "curl 'https://ai.zoom.us/ai-computer/api/v1/credits/status' -H 'Cookie: a=b'"
        )
        .is_none()
    );
    // Bad host
    assert!(
        request_context_from_manual(
            "curl 'https://evil.example/ai-computer/api/v1/credits/status' \
                 -H 'Authorization: Bearer x'"
        )
        .is_none()
    );
}

#[test]
fn hosts_preferred_promotes_capture_host() {
    assert_eq!(
        hosts_preferred(Some("zoommate.zoom.us")),
        vec!["zoommate.zoom.us", "ai.zoom.us"]
    );
    assert_eq!(
        hosts_preferred(None),
        vec!["ai.zoom.us", "zoommate.zoom.us"]
    );
}

#[test]
fn failover_skips_auth_and_parse() {
    assert!(!should_failover(&ProviderError::AuthRequired));
    assert!(!should_failover(&ProviderError::Parse("x".into())));
    assert!(should_failover(&ProviderError::Other("HTTP 500".into())));
    assert!(should_failover(&ProviderError::NoCookies));
}

#[test]
fn bearer_header_normalizes_prefix() {
    assert_eq!(bearer_header_value("tok"), "Bearer tok");
    assert_eq!(bearer_header_value("Bearer tok"), "Bearer tok");
    assert_eq!(bearer_header_value("bearer tok"), "bearer tok");
}

#[test]
fn jwt_exp_reads_payload() {
    // header.payload.sig — payload = {"exp": 2000000000}
    let payload = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        br#"{"exp":2000000000}"#,
    );
    let token = format!("aaa.{payload}.sig");
    assert_eq!(jwt_exp_unix(&token), Some(2_000_000_000));
    assert_eq!(jwt_exp_unix("not-a-jwt"), None);
}

#[test]
fn cookie_fingerprint_is_stable() {
    let mut a = HashMap::new();
    a.insert("ai.zoom.us".into(), "c=1".into());
    a.insert("zoommate.zoom.us".into(), "c=2".into());
    let mut b = HashMap::new();
    b.insert("zoommate.zoom.us".into(), "c=2".into());
    b.insert("ai.zoom.us".into(), "c=1".into());
    assert_eq!(cookie_fingerprint(&a), cookie_fingerprint(&b));
    assert_eq!(cookie_fingerprint(&a).len(), 64);
}

// ── F16: browser cookie scope preservation (upstream #2627) ───

#[derive(Deserialize)]
struct CookieScopeFixture {
    records: Vec<FixtureRecord>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureRecord {
    source_domain: String,
    domain: String,
    scope: String,
    name: String,
    value: String,
}

/// Upstream fixture `issue-2507-cookie-scope.json`, copied verbatim: the raw
/// browser host key lives in `sourceDomain`; our `Cookie.domain` carries the
/// same raw form, and the leading dot encodes the fixture's explicit scope.
fn issue_2507_records() -> Vec<Cookie> {
    let fixture: CookieScopeFixture = serde_json::from_str(include_str!(
        "../fixtures/zoommate/issue-2507-cookie-scope.json"
    ))
    .unwrap();
    fixture
        .records
        .into_iter()
        .map(|record| {
            // Guard the dot↔scope derivation against fixture drift.
            let derived = if record.source_domain.starts_with('.') {
                "domain"
            } else {
                "hostOnly"
            };
            assert_eq!(derived, record.scope, "{}", record.name);
            assert_eq!(
                record.source_domain.trim_start_matches('.'),
                record.domain,
                "{}",
                record.name
            );
            Cookie {
                name: record.name,
                value: record.value,
                domain: record.source_domain,
                path: "/".into(),
                expires: None,
                is_secure: true,
                is_http_only: true,
            }
        })
        .collect()
}

#[test]
fn issue_2507_fixture_routes_parent_cookie_to_both_hosts_without_leaks() {
    let records = issue_2507_records();
    assert_eq!(
        cookie_header_for_host(&records, "ai.zoom.us").as_deref(),
        Some("parent=fake; ai-only=fake")
    );
    assert_eq!(
        cookie_header_for_host(&records, "zoommate.zoom.us").as_deref(),
        Some("parent=fake; mate-only=fake")
    );
}

#[test]
fn cookie_scope_filter_follows_rfc_6265_scope() {
    assert!(cookie_is_sendable_to_host("ai.zoom.us", "ai.zoom.us"));
    assert!(!cookie_is_sendable_to_host(
        "ai.zoom.us",
        "zoommate.zoom.us"
    ));
    assert!(cookie_is_sendable_to_host(".zoom.us", "ai.zoom.us"));
    assert!(cookie_is_sendable_to_host(".zoom.us", "zoommate.zoom.us"));
    // Plain zoom.us is host-only: never sent to leaf API hosts.
    assert!(!cookie_is_sendable_to_host("zoom.us", "ai.zoom.us"));
    // Sibling subdomains are not destinations, and the host-only
    // marketing cookie doesn't roam either.
    assert!(!cookie_is_sendable_to_host(
        "marketing.zoom.us",
        "ai.zoom.us"
    ));
    assert!(cookie_header_for_host(&issue_2507_records(), "marketing.zoom.us").is_none());
    // Suffix-lookalike attackers and empty domains never match.
    assert!(!cookie_is_sendable_to_host(
        "zoom.us.attacker.com",
        "ai.zoom.us"
    ));
    assert!(!cookie_is_sendable_to_host("", "ai.zoom.us"));
}
