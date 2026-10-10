use super::{
    ClaudeOAuthCredentials, ClaudeOAuthFetcher, OAuthUsageResponse, UsageWindow,
    credential_identity, is_rate_limited_error,
};
use crate::core::{ProviderError, UsageSnapshot};
use base64::Engine;
use reqwest::header::HeaderValue;
use std::time::Duration;

#[test]
fn saved_account_refresh_errors_distinguish_reauthentication_from_retry() {
    assert!(matches!(
        super::account_refresh_error(ProviderError::OAuth("invalid grant".into())),
        ProviderError::OAuthRevoked(_)
    ));
    assert!(matches!(
        super::account_refresh_error(ProviderError::OAuthTransient("cooldown".into())),
        ProviderError::OAuthTransient(_)
    ));
}

fn test_credentials(access_token: &str) -> ClaudeOAuthCredentials {
    ClaudeOAuthCredentials {
        access_token: access_token.to_string(),
        refresh_token: None,
        expires_at: None,
        scopes: vec!["user:profile".to_string()],
        rate_limit_tier: None,
    }
}

fn creds(rate_limit_tier: Option<&str>) -> ClaudeOAuthCredentials {
    ClaudeOAuthCredentials {
        scopes: vec![],
        rate_limit_tier: rate_limit_tier.map(str::to_string),
        ..test_credentials("token")
    }
}

fn snapshot(json: &str, credentials: &ClaudeOAuthCredentials) -> UsageSnapshot {
    let response: OAuthUsageResponse = serde_json::from_str(json).expect("OAuth usage body");
    ClaudeOAuthFetcher::new().build_usage_snapshot(&response, credentials)
}

#[test]
fn credential_identity_uses_jwt_subject_when_available() {
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"sub":"account-123"}"#);
    let identity = credential_identity(&test_credentials(&format!("header.{payload}.signature")));

    assert_eq!(identity.as_deref(), Some("claude-account:account-123"));
}

#[test]
fn opaque_credential_identity_is_a_non_secret_fingerprint() {
    let token = "opaque-claude-token";
    let identity = credential_identity(&test_credentials(token)).expect("identity");

    assert_eq!(
        identity,
        format!(
            "claude-credential:{}",
            crate::core::sha256_hex(token.as_bytes())
        )
    );
}

#[test]
fn keeps_utilization_in_percent_units() {
    // 1.0 is a 1% session, not a full quota.
    for utilization in [0.23, 1.0, 23.0] {
        let window = UsageWindow {
            utilization: Some(utilization),
            resets_at: None,
        };

        let rate = ClaudeOAuthFetcher::to_rate_window(&window, Some(300)).expect("rate window");

        assert!(
            (rate.used_percent - utilization).abs() < f64::EPSILON,
            "session was {}, expected {utilization}% (not 100%)",
            rate.used_percent
        );
    }
}

#[test]
fn missing_oauth_session_is_informational_and_keeps_weekly_lane() {
    let usage = snapshot(
        r#"{
            "seven_day": {"utilization": 51.0, "resets_at": "2026-08-20T12:00:00Z"}
        }"#,
        &test_credentials("token"),
    );

    assert!(usage.primary.is_informational);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("No active 5h session")
    );
    assert_eq!(usage.secondary.expect("weekly lane").used_percent, 51.0);
}

#[test]
fn parses_current_snake_case_oauth_usage_response() {
    let credentials = ClaudeOAuthCredentials {
        rate_limit_tier: Some("default_claude_ai".to_string()),
        ..test_credentials("token")
    };
    let usage = snapshot(
        r#"{
            "five_hour": {"utilization": 1.0, "resets_at": "2026-05-22T22:10:00Z"},
            "seven_day": {"utilization": 0.14, "resets_at": "2026-05-29T10:00:00Z"},
            "seven_day_oauth_apps": {"utilization": 0.0},
            "limits": [{
                "kind": "weekly_scoped",
                "group": "weekly",
                "percent": 7,
                "resets_at": "2026-05-29T10:00:00Z",
                "scope": {"model": {"id": null, "display_name": "Fable"}},
                "is_active": false
            }],
            "extra_usage": {"is_enabled": true, "used_credits": 0, "monthly_limit": 1000, "currency": "USD"}
        }"#,
        &credentials,
    );

    assert_eq!(usage.primary.used_percent, 1.0);
    assert!((usage.secondary.expect("weekly").used_percent - 0.14).abs() < 0.001);
    let scoped = usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == "claude-weekly-scoped-fable")
        .expect("Fable scoped weekly limit");
    assert_eq!(scoped.title, "Fable only");
    assert_eq!(scoped.window.used_percent, 7.0);
}

#[test]
fn weekly_all_limit_wins_over_stale_seven_day_utilization() {
    let credentials = creds(Some("default_claude_max_5x"));
    let usage = snapshot(
        r#"{
            "five_hour": {"utilization": 8.0, "resets_at": "2026-07-20T04:29:59Z"},
            "seven_day": {"utilization": 1.0, "resets_at": "2026-07-26T22:59:59Z"},
            "limits": [
                {
                    "kind": "weekly_all",
                    "group": "weekly",
                    "percent": 1,
                    "resets_at": "2026-07-26T22:59:59Z"
                },
                {
                    "kind": "weekly_scoped",
                    "group": "weekly",
                    "percent": 2,
                    "resets_at": "2026-07-26T22:59:59Z",
                    "scope": {"model": {"display_name": "Fable"}}
                }
            ]
        }"#,
        &credentials,
    );

    assert!((usage.primary.used_percent - 8.0).abs() < f64::EPSILON);
    // seven_day.utilization 1.0 would normalize to 100%; weekly_all wins.
    assert!((usage.secondary.expect("weekly").used_percent - 1.0).abs() < f64::EPSILON);
    assert_eq!(
        usage
            .extra_rate_windows
            .iter()
            .filter(|w| w.id.starts_with("claude-weekly-scoped-"))
            .count(),
        1
    );
}

#[test]
fn issue_210_reporter_shape_secondary_is_one_percent_not_one_hundred() {
    // Mirrors the reporter JSON: session 8%, fable 2%, all-models should be 1%
    // while seven_day.utilization is the stale 1.0 (would display as 100%).
    let credentials = creds(Some("default_claude_max_5x"));
    let usage = snapshot(
        r#"{
            "five_hour": {
                "utilization": 8.0,
                "resets_at": "2026-07-20T04:29:59.671218Z"
            },
            "seven_day": {
                "utilization": 1.0,
                "resets_at": "2026-07-26T22:59:59.671246Z"
            },
            "limits": [
                {
                    "kind": "weekly_all",
                    "group": "weekly",
                    "percent": 1.0,
                    "resets_at": "2026-07-26T22:59:59.671595Z"
                },
                {
                    "kind": "weekly_scoped",
                    "group": "weekly",
                    "percent": 2.0,
                    "resets_at": "2026-07-26T22:59:59.671595Z",
                    "scope": {
                        "model": {
                            "id": "claude-fable",
                            "display_name": "Fable"
                        }
                    }
                }
            ]
        }"#,
        &credentials,
    );

    assert_eq!(usage.login_method.as_deref(), Some("Claude Max 5x"));
    assert!((usage.primary.used_percent - 8.0).abs() < f64::EPSILON);
    let weekly = usage.secondary.expect("secondary weekly");
    assert!(
        (weekly.used_percent - 1.0).abs() < f64::EPSILON,
        "secondary was {}, expected 1% (not 100%)",
        weekly.used_percent
    );
    assert!((weekly.used_percent - 100.0).abs() > 1.0);
    let fable = usage
        .extra_rate_windows
        .iter()
        .find(|w| w.title.contains("Fable"))
        .expect("Fable only window");
    assert!((fable.window.used_percent - 2.0).abs() < f64::EPSILON);
}

#[test]
fn issue_279_session_limits_win_over_stale_five_hour_after_rollover() {
    // Right after a 5h window rollover the legacy five_hour.utilization
    // can transiently report 1.0 (normalizes to 100%) even though
    // claude.ai shows only 5% for the fresh window. The limits[] entry
    // (kind=="session") carries the true value and must win.
    let credentials = creds(Some("default_claude_max_5x"));
    let usage = snapshot(
        r#"{
            "five_hour": {"utilization": 1.0, "resets_at": "2026-08-13T12:49:59.578826Z"},
            "seven_day": {"utilization": 0.01, "resets_at": "2026-07-26T22:59:59Z"},
            "limits": [
                {
                    "kind": "session",
                    "group": "session",
                    "percent": 5,
                    "resets_at": "2026-08-13T12:49:59.578826Z"
                },
                {
                    "kind": "weekly_all",
                    "group": "weekly",
                    "percent": 1,
                    "resets_at": "2026-07-26T22:59:59Z"
                }
            ]
        }"#,
        &credentials,
    );

    // Primary session must be 5%, not the stale 100%.
    assert!(
        (usage.primary.used_percent - 5.0).abs() < f64::EPSILON,
        "primary was {}, expected 5% (not 100%)",
        usage.primary.used_percent
    );
    assert!((usage.primary.used_percent - 100.0).abs() > 1.0);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert!(usage.primary.resets_at.is_some());

    // Weekly lane is unaffected (still prefers limits weekly_all).
    let weekly = usage.secondary.expect("weekly");
    assert!((weekly.used_percent - 1.0).abs() < f64::EPSILON);
}

#[test]
fn session_falls_back_to_legacy_five_hour_without_limits_entry() {
    // When no limits[] session entry exists, the legacy five_hour field
    // is still the source of truth (backwards compatible).
    let credentials = creds(None);
    let usage = snapshot(
        r#"{
            "five_hour": {"utilization": 10.0, "resets_at": "2026-08-13T12:49:59Z"}
        }"#,
        &credentials,
    );

    assert!((usage.primary.used_percent - 10.0).abs() < f64::EPSILON);
    assert_eq!(usage.primary.window_minutes, Some(300));
}

#[test]
fn retry_after_parses_seconds_and_falls_back_to_default_backoff() {
    for (value, expected) in [
        ("17", Duration::from_secs(17)),
        ("not-a-date", ClaudeOAuthFetcher::DEFAULT_RATE_LIMIT_BACKOFF),
    ] {
        let header = HeaderValue::from_static(value);
        let duration = ClaudeOAuthFetcher::retry_after_duration(Some(&header));

        assert_eq!(duration, expected, "{value}");
    }
}

#[test]
fn rate_limited_error_preserves_credentials_language() {
    let error = ClaudeOAuthFetcher::rate_limited_error(Duration::from_secs(5));
    let message = error.to_string();

    assert!(matches!(error, ProviderError::OAuthTransient(_)));
    assert!(message.contains("rate limited"));
    assert!(message.contains("credentials were preserved"));
}

#[test]
fn only_the_rate_limit_refusal_counts_as_rate_limited() {
    let refusal = ClaudeOAuthFetcher::rate_limited_error(Duration::from_secs(5));
    assert!(is_rate_limited_error(&refusal));

    let same_text_other_variant = ProviderError::OAuth(match refusal {
        ProviderError::OAuthTransient(message) => message,
        other => panic!("unexpected {other:?}"),
    });
    assert!(!is_rate_limited_error(&same_text_other_variant));
    assert!(!is_rate_limited_error(&ProviderError::OAuthTransient(
        "Claude OAuth token expired and token refresh is cooling down after a failed attempt."
            .to_string()
    )));
}

#[test]
fn oauth_extras_put_scoped_weekly_before_routines() {
    // With and without a scoped reset time.
    for json in [
        r#"{
                "five_hour": {"utilization": 10.0},
                "seven_day_routines": {"utilization": 5.0},
                "limits": [{
                    "kind": "weekly_scoped",
                    "group": "weekly",
                    "percent": 7,
                    "resets_at": "2026-05-29T10:00:00Z",
                    "scope": {"model": {"display_name": "Fable"}}
                }]
            }"#,
        r#"{
                "five_hour": {"utilization": 10.0},
                "seven_day_routines": {"utilization": 5.0},
                "limits": [{
                    "kind": "weekly_scoped",
                    "group": "weekly",
                    "percent": 7,
                    "scope": {"model": {"display_name": "Fable"}}
                }]
            }"#,
    ] {
        let usage = snapshot(json, &creds(None));

        let ids: Vec<&str> = usage
            .extra_rate_windows
            .iter()
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(ids, vec!["claude-weekly-scoped-fable", "claude-routines"]);
    }
}

// ── Refresh-token backoff (upstream 0.48.0 #2650 mapping) ───

fn unique_source(tag: &str) -> super::credentials_store::CredentialSource {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    super::credentials_store::CredentialSource::File(std::path::PathBuf::from(format!(
        "f3-refresh-backoff-{tag}-{nanos}.json"
    )))
}

#[test]
fn terminal_refresh_rejection_stays_blocked_until_credential_changes() {
    let source = unique_source("terminal");
    let now = std::time::Instant::now();
    super::record_refresh_backoff(
        &source,
        super::refresh::RefreshFailureKind::Terminal,
        now,
        Some("dead-refresh-token"),
    );
    // Terminal gate is indefinite: still blocked far in the future as
    // long as the same refresh token is presented.
    assert_eq!(
        super::active_refresh_backoff(
            &source,
            now + Duration::from_secs(3600),
            Some("dead-refresh-token")
        ),
        Some(super::refresh::RefreshFailureKind::Terminal)
    );
    // A different refresh token (CLI re-auth rotated it) clears the gate.
    assert_eq!(
        super::active_refresh_backoff(&source, now, Some("new-refresh-token")),
        None,
        "credential change clears the terminal gate"
    );
    // Re-record with the new token; explicit clear re-allows attempts.
    super::record_refresh_backoff(
        &source,
        super::refresh::RefreshFailureKind::Terminal,
        now,
        Some("new-refresh-token"),
    );
    super::clear_refresh_backoff(&source);
    assert_eq!(
        super::active_refresh_backoff(&source, now, Some("new-refresh-token")),
        None,
        "explicit clear re-allows attempts (e.g. after re-login)"
    );
}

#[test]
fn transient_refresh_failure_gets_5min_backoff() {
    let source = unique_source("transient");
    let now = std::time::Instant::now();
    super::record_refresh_backoff(
        &source,
        super::refresh::RefreshFailureKind::Transient,
        now,
        None,
    );
    assert_eq!(
        super::active_refresh_backoff(&source, now + Duration::from_secs(299), None),
        Some(super::refresh::RefreshFailureKind::Transient)
    );
    assert_eq!(
        super::active_refresh_backoff(&source, now + Duration::from_secs(301), None),
        None
    );
}

#[test]
fn switching_accounts_clears_the_credential_files_transient_cooldown() {
    let source = unique_source("switch");
    let super::credentials_store::CredentialSource::File(path) = &source else {
        panic!("file source")
    };
    let now = std::time::Instant::now();
    super::record_refresh_backoff(
        &source,
        super::refresh::RefreshFailureKind::Transient,
        now,
        Some("old-token"),
    );
    assert!(super::active_refresh_backoff(&source, now, Some("new-token")).is_some());
    super::clear_account_cache(path);
    assert!(super::active_refresh_backoff(&source, now, Some("new-token")).is_none());
}

#[test]
fn backoff_kinds_have_distinct_user_messages() {
    let terminal = super::terminal_refresh_message();
    assert!(terminal.contains("claude login"), "{terminal}");
    // Upstream drops the "then retry" tail for the provably-dead state.
    assert!(!terminal.contains("retry"), "{terminal}");

    let cooldown = super::refresh_cooldown_message();
    assert!(cooldown.contains("retry shortly"), "{cooldown}");
    assert!(cooldown.contains("claude login"), "{cooldown}");
}

#[test]
fn missing_user_profile_scope_recommends_a_usable_credential_source() {
    let recovery = super::ClaudeOAuthFetcher::scope_recovery_message();
    assert!(recovery.contains("Claude Code sign-in"), "{recovery}");
    assert!(recovery.contains("OAuth token override"), "{recovery}");
    assert!(recovery.contains("switch Claude Source"), "{recovery}");
    assert!(!recovery.contains("setup-token"), "{recovery}");

    // Local-scope error: prefixed with the current scopes.
    let credentials = test_credentials("token");
    let local = format!(
        "OAuth token missing 'user:profile' scope (has: {}). {recovery}",
        credentials.scopes.join(", ")
    );
    assert!(local.contains("missing 'user:profile' scope"), "{local}");
    assert!(!local.contains("setup-token"), "{local}");

    // 403 response error.
    let forbidden =
        format!("OAuth token does not meet scope requirement 'user:profile'. {recovery}");
    assert!(forbidden.contains("scope requirement"), "{forbidden}");
    assert!(!forbidden.contains("setup-token"), "{forbidden}");
}

#[test]
fn gate_precheck_blocks_with_unchanged_error_and_serves_cache_without_request() {
    use super::usage_gate::UsageGate;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("CodexBar").join("gate.json");
    let now = 1_000_000_000_000_i64;
    let gate = std::sync::Mutex::new(UsageGate::new(Some(path.clone())));

    assert!(ClaudeOAuthFetcher::gate_precheck(&gate, now, "fp").is_none());

    gate.lock().unwrap().record_success(
        now,
        "fp",
        r#"{"fiveHour":{"utilization":42.0,"resetsAt":"2030-01-01T00:00:00Z"}}"#.into(),
    );
    let cached = ClaudeOAuthFetcher::gate_precheck(&gate, now + 1000, "fp")
        .expect("cached")
        .expect("ok");
    assert_eq!(cached.five_hour.unwrap().utilization, Some(42.0));
    assert!(ClaudeOAuthFetcher::gate_precheck(&gate, now + 1000, "other").is_none());

    gate.lock()
        .unwrap()
        .record_rate_limit(now + 4 * 60_000, "fp", Duration::ZERO);
    let err = ClaudeOAuthFetcher::gate_precheck(&gate, now + 5 * 60_000, "fp")
        .expect("blocked")
        .unwrap_err();
    assert!(is_rate_limited_error(&err));
    assert_eq!(
        err.to_string(),
        ClaudeOAuthFetcher::rate_limited_error(Duration::from_secs(240)).to_string()
    );
}

#[test]
fn gate_survives_a_poisoned_mutex() {
    use super::usage_gate::UsageGate;
    let gate = std::sync::Arc::new(std::sync::Mutex::new(UsageGate::new(None)));
    let g2 = gate.clone();
    let joined = std::thread::spawn(move || {
        let _guard = g2.lock().unwrap();
        panic!("poison");
    })
    .join();
    assert!(joined.is_err());
    assert!(ClaudeOAuthFetcher::gate_precheck(&gate, 1_000_000_000_000, "fp").is_none());
}
