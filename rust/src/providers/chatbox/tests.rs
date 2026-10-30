//! Fixture tests for the Chatbox AI license provider, ported from upstream
//! `Tests/CodexBarTests/ChatboxPluginTests.swift`. Fetch tests use an injected
//! license key, a fixed clock and a local mock server, so no test reads the
//! keyring, the environment, or the real Chatbox API.

use super::*;
use chrono::TimeZone;
use mockito::{Matcher, Mock, Server, ServerGuard};
use serde_json::json;

const KEY: &str = "fixture-key";
const PRIVATE: &str = "private-response-key";

/// Synthetic body in the shape of Chatbox's public license detail schema.
const FIXTURE: &str = r#"{"data":{"key":"private-response-key","name":"Chatbox AI Pro","plan":"pro","status":"active",
"unified_token_usage":300,"unified_token_limit":1500,
"unified_token_usage_details":[{"type":"plan","token_usage":200,"token_limit":1000},
{"type":"invitation_reward","token_usage":100,"token_limit":500}],
"aggregated_reward_details":{"token_usage":100,"token_limit":500},
"expansion_pack_usage":50,"expansion_pack_limit":400,"image_used_count":3,"image_total_quota":20,
"token_refreshed_time":"2026-10-01T00:00:00Z","token_next_refresh_time":"2026-11-01T00:00:00Z",
"token_expire_time":"2027-01-01T00:00:00Z"}}"#;

/// 2026-10-20 00:00 UTC, upstream's fixed test clock.
fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(1_792_454_400, 0)
        .single()
        .expect("fixture clock")
}

fn utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("fixture date")
        .with_timezone(&Utc)
}

fn data() -> Map<String, Value> {
    let root: Value = serde_json::from_str(FIXTURE).expect("fixture JSON");
    root["data"].as_object().expect("fixture data").clone()
}

fn body(data: Map<String, Value>) -> String {
    json!({ "data": data }).to_string()
}

fn context(api_key: Option<&str>, source_mode: SourceMode) -> FetchContext {
    FetchContext {
        source_mode,
        api_key: api_key.map(str::to_owned),
        ..FetchContext::default()
    }
}

/// One mock that only answers the exact upstream request: GET, the raw key
/// as `Authorization`, `Cache-Control: no-store`, no cookie and no body.
async fn mock(server: &mut ServerGuard, status: usize, body: &str) -> Mock {
    server
        .mock("GET", LICENSE_PATH)
        .match_header("authorization", KEY)
        .match_header("cache-control", "no-store")
        .match_header("cookie", Matcher::Missing)
        .match_body(Matcher::Exact(String::new()))
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_header("retry-after", "30")
        .with_body(body)
        .expect(1)
        .create_async()
        .await
}

async fn fetch_status(status: usize, body: &str) -> Result<ProviderFetchResult, ProviderError> {
    let mut server = Server::new_async().await;
    let mock = mock(&mut server, status, body).await;
    let result = ChatboxProvider::with_base_url(&server.url())
        .fetch_at(&context(Some(KEY), SourceMode::Auto), now())
        .await;
    mock.assert_async().await;
    result
}

async fn fetch(body: &str) -> ProviderFetchResult {
    fetch_status(200, body)
        .await
        .expect("fixture fetch succeeds")
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String)> {
    result
        .display_details()
        .iter()
        .map(|row| (row.title().to_owned(), row.value().to_owned()))
        .collect()
}

fn row(result: &ProviderFetchResult, title: &str) -> Option<String> {
    rows(result)
        .into_iter()
        .find(|(label, _)| label == title)
        .map(|(_, value)| value)
}

fn has_primary_meter(result: &ProviderFetchResult) -> bool {
    !result.usage.primary.is_informational
}

#[test]
fn metadata_and_source_contract_match_the_license_key_provider() {
    let provider = ChatboxProvider::new();
    assert_eq!(provider.id(), ProviderId::Chatbox);
    assert_eq!(provider.metadata().display_name, "Chatbox AI");
    assert_eq!(provider.metadata().session_label, "Plan");
    assert_eq!(provider.metadata().weekly_label, "Top-up credits");
    assert_eq!(
        provider.metadata().dashboard_url,
        Some("https://chatboxai.app")
    );
    assert!(!provider.metadata().default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::OAuth]
    );
    assert_eq!(ProviderId::Chatbox.cookie_domain(), None);
    assert_eq!(
        ProviderId::from_cli_name("chatbox"),
        Some(ProviderId::Chatbox)
    );
    assert_eq!(
        ProviderId::from_cli_name("Chatbox AI"),
        Some(ProviderId::Chatbox)
    );
}

#[tokio::test]
async fn plan_rewards_expansion_and_images_preserve_separate_quota_pools() {
    let result = fetch(FIXTURE).await;
    let primary = &result.usage.primary;
    assert!(has_primary_meter(&result));
    assert_eq!(primary.used_percent, 20.0);
    assert_eq!(primary.window_minutes, None);
    assert_eq!(primary.resets_at, Some(utc("2026-11-01T00:00:00Z")));
    let secondary = result.usage.secondary.as_ref().expect("top-up meter");
    assert_eq!(secondary.used_percent, 12.5);
    assert_eq!(secondary.resets_at, None);
    assert!(result.usage.tertiary.is_none());
    assert_eq!(result.usage.login_method.as_deref(), Some("Chatbox AI Pro"));
    assert_eq!(result.usage.account_email, None);
    assert_eq!(result.account_identity(), None);
    assert_eq!(
        rows(&result),
        vec![
            ("Plan".into(), "800 / 1,000".into()),
            ("Credits".into(), "400 / 500".into()),
            ("Top-up credits".into(), "350 / 400".into()),
            ("Image Generation".into(), "17 / 20".into()),
        ]
    );
    assert!(
        result
            .display_details()
            .iter()
            .all(|row| row.section_title() == Some("Remaining") && row.progress().is_none())
    );
    assert_eq!(
        result.usage.subscription,
        Some(SubscriptionMetadata::new(
            None,
            Some(utc("2027-01-01T00:00:00Z")),
            None
        ))
    );
    assert_eq!(result.source_label, "api");
    assert!(!format!("{result:?}").contains(PRIVATE));
}

#[tokio::test]
async fn free_token_resets_do_not_become_image_resets() {
    let mut data = data();
    data.insert("plan".into(), json!("free"));
    data.remove("token_expire_time");
    data.insert(
        "token_next_refresh_time".into(),
        json!("2026-10-21T16:00:00Z"),
    );
    let result = fetch(&body(data)).await;
    assert_eq!(result.usage.primary.window_minutes, None);
    assert_eq!(
        result.usage.primary.resets_at,
        Some(utc("2026-10-21T16:00:00Z"))
    );
    assert!(result.usage.tertiary.is_none());
    assert!(result.usage.extra_rate_windows.is_empty());
    assert_eq!(result.usage.subscription, None);
}

#[tokio::test]
async fn trial_rewards_and_combined_legacy_totals_never_invent_recurring_plan_quotas() {
    let mut data = data();
    data.insert(
        "unified_token_usage_details".into(),
        json!([
            {"type": "plan", "token_usage": 0, "token_limit": 0},
            {"type": "trial", "token_usage": 100, "token_limit": 500},
        ]),
    );
    let trial = fetch(&body(data.clone())).await;
    assert!(!has_primary_meter(&trial));
    assert!(trial.usage.primary.hidden_on_card);
    assert_eq!(trial.usage.primary.resets_at, None);

    data.remove("unified_token_usage_details");
    let legacy = fetch(&body(data)).await;
    assert!(!has_primary_meter(&legacy));
    assert_eq!(
        rows(&legacy).first(),
        Some(&("Quota".to_owned(), "1,200 / 1,500".to_owned()))
    );
}

#[tokio::test]
async fn expired_disabled_and_terminal_periods_retain_balances_without_claiming_a_reset() {
    for status in ["expired", "disabled", "refunded", "unknown"] {
        let mut data = data();
        data.insert("status".into(), json!(status));
        let result = fetch(&body(data)).await;
        assert!(!has_primary_meter(&result), "{status}");
        assert_eq!(
            result
                .usage
                .secondary
                .as_ref()
                .map(|window| window.used_percent),
            Some(12.5),
            "{status}"
        );
        assert_eq!(row(&result, "Plan").as_deref(), Some("800 / 1,000"));
    }
    for next in [
        "0001-01-01T00:00:00Z",
        "invalid",
        "2026-09-01T00:00:00Z",
        "2027-01-01T00:00:00Z",
    ] {
        let mut data = data();
        data.insert("token_next_refresh_time".into(), json!(next));
        let result = fetch(&body(data)).await;
        assert!(has_primary_meter(&result), "{next}");
        assert_eq!(result.usage.primary.resets_at, None, "{next}");
    }
}

#[tokio::test]
async fn exhaustion_clamps_progress_while_absent_optional_pools_stay_unknown() {
    let result = fetch(
        r#"{"data":{"name":"Fixture plan","status":"active","unified_token_usage_details":
        [{"type":"plan","token_usage":1200,"token_limit":1000}]}}"#,
    )
    .await;
    assert_eq!(result.usage.primary.used_percent, 100.0);
    assert!(result.usage.secondary.is_none());
    assert_eq!(
        rows(&result),
        vec![("Plan".to_owned(), "0 / 1,000".to_owned())]
    );
}

#[tokio::test]
async fn empty_pools_stay_hidden_while_exhausted_balances_remain_visible() {
    let mut data = data();
    data.insert("expansion_pack_usage".into(), json!(0));
    data.insert("expansion_pack_limit".into(), json!(0));
    data.insert("image_used_count".into(), json!(20));
    let result = fetch(&body(data)).await;
    assert!(result.usage.secondary.is_none());
    assert_eq!(row(&result, "Top-up credits"), None);
    assert_eq!(row(&result, "Image Generation").as_deref(), Some("0 / 20"));
}

#[tokio::test]
async fn reward_fallback_excludes_expired_grants_and_never_inherits_plan_resets() {
    let mut data = data();
    data.remove("aggregated_reward_details");
    data.insert(
        "unified_token_usage_details".into(),
        json!([
            {"type": "plan", "token_usage": 200, "token_limit": 1000},
            {"type": "invitation_reward", "token_usage": 100, "token_limit": 500},
            {"type": "trial", "token_usage": 25, "token_limit": 200,
             "expires_at": "2026-11-01T00:00:00Z"},
            {"type": "invitation_reward", "token_usage": 0, "token_limit": 900,
             "expires_at": "2026-10-19T00:00:00Z"},
        ]),
    );
    let result = fetch(&body(data)).await;
    assert_eq!(row(&result, "Credits").as_deref(), Some("575 / 700"));
    assert_eq!(result.usage.primary.used_percent, 20.0);
    assert!(result.usage.extra_rate_windows.is_empty());
    assert!(result.usage.tertiary.is_none());
}

#[tokio::test]
async fn expired_plan_dates_retain_balances_without_a_primary_meter() {
    let mut data = data();
    data.insert("token_expire_time".into(), json!("2026-10-19T00:00:00Z"));
    let result = fetch(&body(data)).await;
    assert!(!has_primary_meter(&result));
    assert_eq!(row(&result, "Plan").as_deref(), Some("800 / 1,000"));
    assert_eq!(
        result
            .usage
            .secondary
            .as_ref()
            .map(|window| window.used_percent),
        Some(12.5)
    );
}

#[tokio::test]
async fn absent_license_status_retains_the_public_client_quota_shape() {
    let mut data = data();
    data.remove("status");
    let result = fetch(&body(data)).await;
    assert_eq!(result.usage.primary.used_percent, 20.0);
    assert_eq!(
        result.usage.primary.resets_at,
        Some(utc("2026-11-01T00:00:00Z"))
    );
}

#[tokio::test]
async fn expires_at_stands_in_when_token_expire_time_is_absent() {
    let mut data = data();
    data.remove("token_expire_time");
    data.insert("expires_at".into(), json!("2026-12-01T00:00:00Z"));
    let result = fetch(&body(data)).await;
    assert_eq!(
        result
            .usage
            .subscription
            .as_ref()
            .and_then(|subscription| subscription.expires_at),
        Some(utc("2026-12-01T00:00:00Z"))
    );
}

#[tokio::test]
async fn zero_allowances_stay_explicit_without_an_unlimited_meter() {
    let result = fetch(r#"{"data":{"unified_token_usage":0,"unified_token_limit":0}}"#).await;
    assert!(!has_primary_meter(&result));
    assert!(result.usage.secondary.is_none());
    assert_eq!(
        rows(&result),
        vec![("Quota".to_owned(), "0 / 0".to_owned())]
    );
}

#[tokio::test]
async fn license_name_is_cleaned_and_bounded() {
    let mut data = data();
    data.insert(
        "name".into(),
        json!(format!(" Pro\u{7}plan{} ", "x".repeat(200))),
    );
    let result = fetch(&body(data.clone())).await;
    let name = result.usage.login_method.expect("name");
    assert!(name.starts_with("Pro plan"));
    assert_eq!(name.encode_utf16().count(), 100);

    data.insert("name".into(), json!("   "));
    assert_eq!(fetch(&body(data)).await.usage.login_method, None);
}

#[tokio::test]
async fn malformed_payloads_fail_without_echoing_secrets() {
    for body in [
        PRIVATE,
        "null",
        "[]",
        "{}",
        r#"{"data":{}}"#,
        r#"{"data":{"unified_token_usage":-1,"unified_token_limit":100}}"#,
        r#"{"data":{"unified_token_usage":true,"unified_token_limit":100}}"#,
        r#"{"data":{"unified_token_usage":1.5,"unified_token_limit":100}}"#,
        r#"{"data":{"unified_token_usage":"1","unified_token_limit":100}}"#,
        r#"{"data":{"unified_token_usage":9007199254740992,"unified_token_limit":100}}"#,
        r#"{"data":{"unified_token_usage_details":{}}}"#,
        r#"{"data":{"unified_token_usage_details":[{"token_usage":0,"token_limit":1}]}}"#,
        r#"{"data":{"unified_token_usage_details":[1]}}"#,
        r#"{"data":{"unified_token_limit":1,"unified_token_usage":0,"aggregated_reward_details":[]}}"#,
        r#"{"data":null,"error":{"message":"private-response-key"}}"#,
        r#"{"data":{"unified_token_usage":0,"unified_token_limit":100},"error":{"code":"fixture-error"}}"#,
        r#"{"data":{"unified_token_usage_details":[
        {"type":"plan","token_usage":0,"token_limit":1},
        {"type":"plan","token_usage":0,"token_limit":1}]}}"#,
        r#"{"data":{"unified_token_usage_details":[
        {"type":"reward","token_usage":0,"token_limit":9007199254740991},
        {"type":"trial","token_usage":0,"token_limit":1}]}}"#,
    ] {
        let error = fetch_status(200, body)
            .await
            .expect_err("malformed body must fail");
        assert!(matches!(error, ProviderError::Parse(_)), "{body}: {error}");
        let message = error.to_string();
        assert!(message.ends_with(MALFORMED), "{body}: {message}");
        assert!(!message.contains(PRIVATE), "{message}");
        assert!(!message.contains(KEY), "{message}");
    }
}

#[tokio::test]
async fn too_many_detail_items_fail() {
    let items: Vec<Value> = (0..=MAX_DETAIL_ITEMS)
        .map(|_| json!({"type": "reward", "token_usage": 0, "token_limit": 1}))
        .collect();
    let body = json!({"data": {"unified_token_usage_details": items}}).to_string();
    let error = fetch_status(200, &body).await.expect_err("too many items");
    assert!(matches!(error, ProviderError::Parse(_)));
}

#[tokio::test]
async fn a_falsy_error_member_is_ignored() {
    let result =
        fetch(r#"{"data":{"unified_token_usage":0,"unified_token_limit":100},"error":null}"#).await;
    assert_eq!(row(&result, "Quota").as_deref(), Some("100 / 100"));
}

#[tokio::test]
async fn http_failures_preserve_authentication_and_transient_classifications() {
    let provider = ChatboxProvider::new();
    for (status, expected, kind) in [
        (401, INVALID_KEY, ProviderStateKind::ExpiredSession),
        (403, DENIED, ProviderStateKind::Unknown),
        (409, FAILED, ProviderStateKind::Unknown),
        (429, RATE_LIMITED, ProviderStateKind::Unknown),
        (500, FAILED, ProviderStateKind::Unknown),
        (502, FAILED, ProviderStateKind::Unknown),
        (503, FAILED, ProviderStateKind::Unknown),
        (302, FAILED, ProviderStateKind::Unknown),
    ] {
        let error = fetch_status(status, PRIVATE)
            .await
            .expect_err("HTTP failure must surface");
        let message = error.to_string();
        assert_eq!(message, format!("{expected} (HTTP {status})"));
        assert!(!message.contains(PRIVATE));
        assert!(!message.contains(KEY));
        assert_eq!(provider.error_state_kind(&error), kind, "{status}");
    }
}

#[tokio::test]
async fn conflict_responses_make_exactly_one_request() {
    // `fetch_status` asserts the mock was hit exactly once.
    let error = fetch_status(409, r#"{"error":{"code":"fixture-conflict"}}"#)
        .await
        .expect_err("conflict must surface");
    assert_eq!(error.to_string(), format!("{FAILED} (HTTP 409)"));
}

#[tokio::test]
async fn malformed_manual_keys_fail_before_any_http_request() {
    let mut server = Server::new_async().await;
    let unexpected = server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let provider = ChatboxProvider::with_base_url(&server.url());
    for key in [
        "fixture key".to_owned(),
        "fixture\r\nheader".to_owned(),
        "fixture\u{0}suffix".to_owned(),
        "fixture\u{85}suffix".to_owned(),
        "x".repeat(MAX_KEY_LENGTH + 1),
    ] {
        let error = provider
            .fetch_at(&context(Some(&key), SourceMode::Auto), now())
            .await
            .expect_err("invalid key must be rejected");
        assert_eq!(error.to_string(), INVALID_KEY);
        assert!(!error.to_string().contains(&key));
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession
        );
    }
    unexpected.assert_async().await;
}

#[test]
fn key_gate_trims_accepts_the_bound_and_reports_a_missing_key() {
    assert_eq!(
        checked_key(Some("  fixture-key  ".into())).expect("trimmed"),
        "fixture-key"
    );
    assert!(checked_key(Some("x".repeat(MAX_KEY_LENGTH))).is_ok());
    assert!(checked_key(Some("clé-fixture".into())).is_ok());
    for missing in [None, Some(String::new()), Some("   ".into())] {
        let error = checked_key(missing).expect_err("missing key");
        assert!(matches!(&error, ProviderError::NotInstalled(message) if message == MISSING_KEY));
        assert_eq!(error.state_kind(), ProviderStateKind::NeedsAuthentication);
    }
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported_and_oauth_uses_the_key_lane() {
    let provider = ChatboxProvider::new();
    for source in [SourceMode::Web, SourceMode::Cli] {
        let error = provider
            .fetch_usage(&context(Some(KEY), source))
            .await
            .expect_err("unsupported source");
        assert!(matches!(error, ProviderError::UnsupportedSource(_)));
    }

    let mut server = Server::new_async().await;
    let mock = mock(&mut server, 200, FIXTURE).await;
    let result = ChatboxProvider::with_base_url(&server.url())
        .fetch_at(&context(Some(KEY), SourceMode::OAuth), now())
        .await
        .expect("oauth uses the key lane");
    mock.assert_async().await;
    assert_eq!(result.source_label, "api");
}

#[test]
fn counters_accept_only_safe_non_negative_integers() {
    let value = |raw: &str| serde_json::from_str::<Value>(raw).expect("json");
    assert_eq!(counter(None).ok(), Some(None));
    assert_eq!(counter(Some(&Value::Null)).ok(), Some(None));
    assert_eq!(counter(Some(&value("12"))).ok(), Some(Some(12)));
    assert_eq!(counter(Some(&value("12.0"))).ok(), Some(Some(12)));
    assert_eq!(
        counter(Some(&value("9007199254740991"))).ok(),
        Some(Some(MAX_SAFE_INTEGER))
    );
    for raw in [
        "-1",
        "1.5",
        "true",
        "\"1\"",
        "9007199254740992",
        "1e300",
        "[]",
    ] {
        assert!(counter(Some(&value(raw))).is_err(), "{raw}");
    }
}

#[test]
fn dates_need_an_iso_date_time_from_1970() {
    let value = |raw: &str| json!(raw);
    assert_eq!(
        date(Some(&value("2026-11-01T00:00:00Z"))),
        Some(utc("2026-11-01T00:00:00Z"))
    );
    assert_eq!(
        date(Some(&value("2026-11-01T09:00:00.250+09:00"))),
        Some(utc("2026-11-01T00:00:00.250Z"))
    );
    for raw in [
        "0001-01-01T00:00:00Z",
        "invalid",
        "2026-11-01",
        "2026-11-01 00:00:00Z",
    ] {
        assert_eq!(date(Some(&value(raw))), None, "{raw}");
    }
    assert_eq!(date(Some(&json!(1_792_454_400))), None);
}

#[test]
fn base_url_override_accepts_https_and_loopback_http() {
    assert_eq!(
        validated_base_url("https://api.chatboxai.app").as_deref(),
        Some("https://api.chatboxai.app")
    );
    assert_eq!(
        validated_base_url("http://127.0.0.1:4010/").as_deref(),
        Some("http://127.0.0.1:4010")
    );
    assert_eq!(
        validated_base_url("http://localhost:9").as_deref(),
        Some("http://localhost:9")
    );
    assert_eq!(
        validated_base_url("http://[::1]:9").as_deref(),
        Some("http://[::1]:9")
    );
}

#[test]
fn base_url_override_rejects_remote_plain_http_and_unsafe_forms() {
    for raw in [
        "http://api.chatboxai.app",
        "http://localhost.attacker.test",
        "ftp://127.0.0.1",
        "https://user:pass@api.chatboxai.app",
        "https://api.chatboxai.app?x=1",
        "https://api.chatboxai.app#frag",
        "http://127.0.0.1%2f.attacker.test",
        "not a url",
        "",
    ] {
        assert!(
            validated_base_url(raw).is_none(),
            "{raw} should be rejected"
        );
    }
}

#[test]
fn base_url_override_falls_back_to_production_when_unset_or_invalid() {
    assert_eq!(base_url_from_override(None), BASE_URL);
    assert_eq!(base_url_from_override(Some("   ")), BASE_URL);
    assert_eq!(
        base_url_from_override(Some("http://attacker.example.com")),
        BASE_URL
    );
    assert_eq!(
        base_url_from_override(Some("\"http://127.0.0.1:4010\"")),
        "http://127.0.0.1:4010"
    );
}

#[test]
fn api_key_settings_expose_the_license_key_field() {
    let info = crate::settings::get_api_key_providers()
        .into_iter()
        .find(|info| info.id == ProviderId::Chatbox)
        .expect("Chatbox AI api key metadata");
    assert_eq!(info.name, "Chatbox AI");
    assert_eq!(info.api_key_env_var, Some(API_KEY_ENV));
}
