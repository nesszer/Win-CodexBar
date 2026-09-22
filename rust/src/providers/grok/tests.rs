use super::*;
use chrono::TimeZone;

fn billing_response_with_percent(percent: f32) -> Vec<u8> {
    let mut payload = vec![0x0a, 0x05, 0x0d];
    payload.extend(percent.to_le_bytes());

    let mut response = vec![0x00];
    response.extend(
        u32::try_from(payload.len())
            .expect("test gRPC-web payload length fits u32")
            .to_be_bytes(),
    );
    response.extend(payload);
    response
}

#[tokio::test]
async fn billing_request_encodes_explicit_false_in_a_nonempty_grpc_web_frame() {
    let mut server = mockito::Server::new_async().await;
    let endpoint = format!("{}/billing", server.url());
    let request = server
        .mock("POST", "/billing")
        .match_body(BILLING_REQUEST_BODY.to_vec())
        .with_status(200)
        .with_body(billing_response_with_percent(37.0))
        .create_async()
        .await;
    let provider = GrokProvider::new().with_billing_endpoint_for_tests(endpoint);

    let snapshot = provider
        .fetch_billing(Some("Bearer local-token".to_string()), None)
        .await
        .unwrap();

    request.assert_async().await;
    assert_eq!(BILLING_REQUEST_BODY, [0, 0, 0, 0, 2, 0x08, 0]);
    assert_eq!(snapshot.used_percent, Some(37.0));
}

#[tokio::test]
async fn oauth_billing_auth_failure_falls_back_to_configured_local_token() {
    let mut server = mockito::Server::new_async().await;
    let endpoint = format!("{}/billing", server.url());
    let rejected = server
        .mock("POST", "/billing")
        .match_header("authorization", "Bearer expired-token")
        .match_body(BILLING_REQUEST_BODY.to_vec())
        .with_status(401)
        .create_async()
        .await;
    let fallback = server
        .mock("POST", "/billing")
        .match_header("authorization", "Bearer local-token")
        .match_body(BILLING_REQUEST_BODY.to_vec())
        .with_status(200)
        .with_body(billing_response_with_percent(42.0))
        .create_async()
        .await;
    let provider = GrokProvider::new().with_billing_endpoint_for_tests(endpoint);
    let credentials = GrokCredentials::from_bearer("expired-token");
    let context = FetchContext {
        include_credits: false,
        api_key: Some("local-token".to_string()),
        ..FetchContext::default()
    };

    let result = provider
        .fetch_with_oauth_fallback(&credentials, &context)
        .await
        .unwrap();

    rejected.assert_async().await;
    fallback.assert_async().await;
    assert_eq!(result.source_label, "grok-oauth");
    assert_eq!(result.usage.primary.used_percent, 42.0);
}

#[test]
fn grok_plan_prefers_subscription_tier_display_names() {
    assert_eq!(
        grok_plan_display_name(Some("SuperGrok Heavy")),
        Some("SuperGrok Heavy".to_string())
    );
    assert_eq!(
        grok_plan_display_name(Some("heavy")),
        Some("SuperGrok Heavy".to_string())
    );
    assert_eq!(
        grok_plan_display_name(Some("SuperGrok")),
        Some("SuperGrok".to_string())
    );
    assert_eq!(
        grok_plan_display_name(Some(" custom ")),
        Some("custom".to_string())
    );
}

#[test]
fn parses_auth_file_prefer_oidc() {
    let auth = r#"{
      "https://accounts.x.ai/sign-in": {"key": "legacy"},
      "https://auth.x.ai::abc": {"key": "oidc", "auth_mode": "oidc", "email": "u@example.com"}
    }"#;
    let parsed = GrokCredentials::parse_for_kind(auth, GrokAuthKind::OAuth).unwrap();
    assert_eq!(parsed.access_token, "oidc");
    assert_eq!(parsed.login_method().as_deref(), Some("SuperGrok"));
}

#[test]
fn cli_and_oauth_select_distinct_auth_entries() {
    let auth = r#"{
      "https://accounts.x.ai/sign-in": {"key": "cli-token", "auth_mode": "session"},
      "https://auth.x.ai::abc": {"key": "oauth-token", "auth_mode": "oidc"}
    }"#;
    assert_eq!(
        GrokCredentials::parse_for_kind(auth, GrokAuthKind::Cli)
            .unwrap()
            .access_token,
        "cli-token"
    );
    assert_eq!(
        GrokCredentials::parse_for_kind(auth, GrokAuthKind::OAuth)
            .unwrap()
            .access_token,
        "oauth-token"
    );
}
#[test]
fn auto_tries_switched_login_before_cookies() {
    assert_eq!(
        grok_auto_steps(true, true, true),
        vec![
            GrokAutoStep::AmbientOAuth,
            GrokAutoStep::AmbientCli,
            GrokAutoStep::ApiKey,
            GrokAutoStep::ManualCookie,
            GrokAutoStep::CookieRefresh,
        ]
    );
    assert_eq!(
        grok_auto_steps(false, false, true),
        vec![
            GrokAutoStep::AmbientOAuth,
            GrokAutoStep::AmbientCli,
            GrokAutoStep::CookieRefresh,
        ]
    );
    assert_eq!(
        grok_auto_steps(false, false, false),
        vec![GrokAutoStep::AmbientOAuth, GrokAutoStep::AmbientCli]
    );
    assert_eq!(
        grok_auto_steps(true, true, false),
        vec![
            GrokAutoStep::AmbientOAuth,
            GrokAutoStep::AmbientCli,
            GrokAutoStep::ApiKey,
        ]
    );
}

#[test]
fn cookie_refresh_uses_cache_when_present() {
    assert_eq!(
        cookie_refresh_action(true, None),
        CookieRefreshAction::UseCached
    );
}

#[test]
fn cookie_refresh_reimports_on_auth_failure() {
    assert_eq!(
        cookie_refresh_action(true, Some(&ProviderError::AuthRequired)),
        CookieRefreshAction::ReimportBrowser
    );
    assert_eq!(
        cookie_refresh_action(false, None),
        CookieRefreshAction::ReimportBrowser
    );
}

#[test]
fn cookie_refresh_gives_up_on_non_auth_errors() {
    assert_eq!(
        cookie_refresh_action(true, Some(&ProviderError::Other("network down".into()))),
        CookieRefreshAction::GiveUp
    );
}

#[test]
fn is_cookie_auth_failure_only_auth_required() {
    assert!(is_cookie_authentication_failure(
        &ProviderError::AuthRequired
    ));
    assert!(!is_cookie_authentication_failure(&ProviderError::NoCookies));
}

#[test]
fn include_credits_false_does_not_start_reset_lookup() {
    let ctx = FetchContext {
        include_credits: false,
        ..FetchContext::default()
    };

    let lookup = GrokProvider::spawn_remaining_resets(
        &ctx,
        None,
        None,
        crate::providers::grok::GrokProvider::new().client_for_tests(),
    );
    assert!(lookup.task.is_none());
}

#[test]
fn cookie_billing_stays_siloed_from_auth_file_identity() {
    let result = result_from_cookie_billing(GrokBillingSnapshot {
        used_percent: Some(23.0),
        used_percent_is_wire_published: true,
        used_percent_is_implicit_zero: false,
        resets_at: None,
        window_minutes: None,
    });
    assert_eq!(result.source_label, "grok-browser");
    assert!(result.usage.account_email.is_none());
    assert!(result.usage.account_organization.is_none());
    assert!(result.usage.login_method.is_none());
}
#[test]
fn billing_snapshot_uses_full_weekly_cycle_for_pace() {
    let now = Utc::now();
    let resets = now + chrono::Duration::days(2);
    let result = result_from_billing(
        GrokBillingSnapshot {
            used_percent: Some(12.0),
            used_percent_is_wire_published: true,
            used_percent_is_implicit_zero: false,
            resets_at: Some(resets),
            window_minutes: Some(crate::core::WEEKLY_WINDOW_MINUTES),
        },
        "web",
        None,
        None,
        Some("SuperGrok".into()),
    );
    assert_eq!(
        result.usage.primary.window_minutes,
        Some(crate::core::WEEKLY_WINDOW_MINUTES)
    );
    assert_eq!(result.usage.primary_label.as_deref(), Some("Weekly"));
    let pace = crate::core::UsagePace::weekly(
        &result.usage.primary,
        Some(now),
        crate::core::WEEKLY_WINDOW_MINUTES,
    );
    assert!(pace.is_some(), "weekly window + reset must yield pace");
}

#[test]
fn monthly_cycle_stays_monthly_with_six_days_remaining() {
    let now = Utc::now();
    let resets = now + chrono::Duration::days(6);
    let monthly_minutes = 31 * 24 * 60;
    let result = result_from_billing(
        GrokBillingSnapshot {
            used_percent: Some(40.0),
            used_percent_is_wire_published: true,
            used_percent_is_implicit_zero: false,
            resets_at: Some(resets),
            window_minutes: Some(monthly_minutes),
        },
        "cli",
        None,
        None,
        Some("SuperGrok Heavy".into()),
    );
    assert_eq!(result.usage.primary_label.as_deref(), Some("Monthly"));
    assert_eq!(result.usage.primary.window_minutes, Some(monthly_minutes));
}

#[test]
fn reset_distance_alone_does_not_invent_a_cadence() {
    let resets = Utc::now() + chrono::Duration::days(6);
    let result = result_from_billing(
        GrokBillingSnapshot {
            used_percent: Some(80.0),
            used_percent_is_wire_published: true,
            used_percent_is_implicit_zero: false,
            resets_at: Some(resets),
            window_minutes: None,
        },
        "web",
        None,
        None,
        Some("SuperGrok".into()),
    );
    assert_eq!(result.usage.primary.window_minutes, None);
    assert_eq!(result.usage.primary_label, None);
}

#[test]
fn period_only_billing_is_informational_not_zero_usage() {
    let resets = Utc::now() + chrono::Duration::days(6);
    let result = result_from_billing(
        GrokBillingSnapshot {
            used_percent: None,
            used_percent_is_wire_published: false,
            used_percent_is_implicit_zero: false,
            resets_at: Some(resets),
            window_minutes: None,
        },
        "cli",
        Some("user@example.com".into()),
        None,
        Some("SuperGrok Heavy".into()),
    );

    assert!(result.usage.primary.is_informational);
    assert_eq!(result.usage.primary.resets_at, Some(resets));
    assert_eq!(
        result.usage.account_email.as_deref(),
        Some("user@example.com")
    );
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("SuperGrok Heavy")
    );
}

#[test]
fn unpublished_zero_does_not_reach_the_usage_surface() {
    let result = result_from_billing(
        GrokBillingSnapshot {
            used_percent: Some(0.0),
            used_percent_is_wire_published: false,
            used_percent_is_implicit_zero: false,
            resets_at: Some(Utc.timestamp_opt(1_789_000_000, 0).single().unwrap()),
            window_minutes: None,
        },
        "grok-web",
        None,
        None,
        None,
    );

    assert!(result.usage.primary.is_informational);
}

#[test]
fn account_usage_marks_informational_windows_unavailable() {
    let result = result_from_billing(
        GrokBillingSnapshot {
            used_percent: None,
            used_percent_is_wire_published: false,
            used_percent_is_implicit_zero: false,
            resets_at: None,
            window_minutes: Some(crate::core::WEEKLY_WINDOW_MINUTES),
        },
        "grok-cli",
        None,
        None,
        Some("SuperGrok".into()),
    );

    let usage = account_usage_from_result(&result);
    assert!(!usage.usage_available);
    assert_eq!(usage.used_percent, None);
}
