use super::{
    AccountResponse, ClaudeWebApiFetcher, UsageWindow, classify_web_http_error, cookie_value,
    describe_json_body_shape, is_cookie_authentication_failure,
};
use crate::core::ProviderError;
use reqwest::StatusCode;
use reqwest::header;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn keeps_utilization_in_percent_units() {
    // 1.0 is a 1% session, not a full quota.
    for utilization in [0.23, 1.0, 23.0] {
        let window = UsageWindow {
            utilization: Some(utilization),
            resets_at: None,
        };

        let rate = ClaudeWebApiFetcher::new().to_rate_window(&window, Some(300));

        assert!(
            (rate.used_percent - utilization).abs() < f64::EPSILON,
            "session was {}, expected {utilization}% (not 100%)",
            rate.used_percent
        );
    }
}

#[test]
fn null_five_hour_session_is_informational_placeholder() {
    let placeholder = crate::core::RateWindow::no_active_session();
    assert!(placeholder.is_informational);
    assert_eq!(placeholder.window_minutes, Some(300));
    assert!((placeholder.used_percent - 0.0).abs() < f64::EPSILON);
    assert_eq!(
        placeholder.reset_description.as_deref(),
        Some("No active 5h session")
    );

    // Real idle session (object present at 0%) stays unflagged.
    let idle = ClaudeWebApiFetcher::new().to_rate_window(
        &UsageWindow {
            utilization: Some(0.0),
            resets_at: None,
        },
        Some(300),
    );
    assert!(!idle.is_informational);
}

#[test]
fn labels_max_5x_and_20x_plans() {
    assert_eq!(
        crate::providers::claude::claude_plan_label("default_claude_max_5x"),
        "Claude Max 5x"
    );
    assert_eq!(
        crate::providers::claude::claude_plan_label("v2_default_claude_max_20x"),
        "Claude Max 20x"
    );
}

#[test]
fn resolves_session_key_from_env_vars() {
    let _guard = env_lock().lock().expect("env lock");
    // (CLAUDE_AI_SESSION_KEY, CLAUDE_WEB_SESSION_KEY, resolved key)
    let rows = [
        (Some("sk-ant-primary"), "sk-ant-secondary", "sk-ant-primary"),
        (
            None,
            "sessionKey=sk-ant-cookie-format",
            "sk-ant-cookie-format",
        ),
    ];
    for (primary, secondary, expected) in rows {
        // SAFETY: env_lock() held for this whole test, so set_var/remove_var
        // cannot race another thread's environment access.
        unsafe {
            std::env::remove_var("CLAUDE_AI_SESSION_KEY");
            std::env::remove_var("CLAUDE_WEB_SESSION_KEY");
            if let Some(primary) = primary {
                std::env::set_var("CLAUDE_AI_SESSION_KEY", primary);
            }
            std::env::set_var("CLAUDE_WEB_SESSION_KEY", secondary);
        }

        let session_key = ClaudeWebApiFetcher::resolve_session_key_from_env();

        assert_eq!(session_key.as_deref(), Some(expected));
    }

    // SAFETY: cleanup while still holding the env_lock() guard.
    unsafe {
        std::env::remove_var("CLAUDE_AI_SESSION_KEY");
        std::env::remove_var("CLAUDE_WEB_SESSION_KEY");
    }
}

#[test]
fn build_headers_include_required_browser_context() {
    let headers = ClaudeWebApiFetcher::build_headers("sessionKey=sk-ant-cookie-format");

    for (name, value) in [
        (header::COOKIE.as_str(), "sessionKey=sk-ant-cookie-format"),
        (header::ACCEPT.as_str(), "application/json"),
        (header::ORIGIN.as_str(), "https://claude.ai"),
        (header::REFERER.as_str(), "https://claude.ai/settings/usage"),
        ("anthropic-client-platform", "web_claude_ai"),
    ] {
        assert_eq!(
            headers.get(name).and_then(|value| value.to_str().ok()),
            Some(value),
            "{name}"
        );
    }
    assert!(headers.contains_key(header::USER_AGENT));
}

#[test]
fn stale_cookie_recovery_retries_only_after_authentication_failure() {
    assert!(is_cookie_authentication_failure(
        &ProviderError::AuthRequired
    ));
    assert!(!is_cookie_authentication_failure(&ProviderError::Timeout));
    assert!(!is_cookie_authentication_failure(&ProviderError::Other(
        "Failed to get organizations: 503 Service Unavailable".to_string(),
    )));
    assert!(!is_cookie_authentication_failure(&classify_web_http_error(
        "organizations",
        StatusCode::FORBIDDEN,
        &header::HeaderMap::new(),
        b"Just a moment...",
    )));
}

#[test]
fn malformed_response_shape_does_not_echo_body_contents() {
    let shape = describe_json_body_shape(
        "sessionKey=secret-session-token",
        Some("text/html; charset=utf-8"),
    );

    assert_eq!(
        shape,
        "content_type=text/html; charset=utf-8, body_len=31, body_kind=non-json"
    );
    assert!(!shape.contains("secret-session-token"));

    let object_shape =
        describe_json_body_shape(r#"{"z":"secret-value","a":true}"#, Some("application/json"));
    assert_eq!(
        object_shape,
        "content_type=application/json, body_len=29, json_keys=[a, z]"
    );
    assert!(!object_shape.contains("secret-value"));
}

#[test]
fn extracts_last_active_org_from_cookie_header() {
    let org = cookie_value(
        "foo=bar; sessionKey=sk-ant-session; lastActiveOrg=org-123; other=value",
        "lastActiveOrg",
    );

    assert_eq!(org.as_deref(), Some("org-123"));
}

#[test]
fn account_membership_prefers_nested_organization_uuid() {
    let account: AccountResponse = serde_json::from_str(
        r#"{
                "email_address": "user@example.com",
                "memberships": [
                    {
                        "uuid": "membership-id",
                        "organization": { "uuid": "org-id" }
                    }
                ]
            }"#,
    )
    .unwrap();

    assert_eq!(account.first_membership_org_id().as_deref(), Some("org-id"));
}

#[test]
fn parses_design_and_routines_aliases_preferring_the_named_key() {
    let rows = [
        (
            r#"{
                    "five_hour": { "utilization": 0.1 },
                    "seven_day_omelette": { "utilization": 26 },
                    "seven_day_cowork": { "utilization": 11 }
                }"#,
            26.0,
            11.0,
        ),
        (
            r#"{
                    "seven_day_design": { "utilization": 31 },
                    "seven_day_omelette": { "utilization": 26 },
                    "seven_day_routines": { "utilization": 19 },
                    "seven_day_cowork": { "utilization": 11 }
                }"#,
            31.0,
            19.0,
        ),
    ];
    let fetcher = ClaudeWebApiFetcher::new();
    for (json, design_percent, routines_percent) in rows {
        let usage: super::UsageResponse = serde_json::from_str(json).unwrap();
        let design = usage
            .seven_day_design
            .as_ref()
            .map(|w| fetcher.to_rate_window(w, Some(10080)))
            .expect("design window");
        let routines = usage
            .seven_day_routines
            .as_ref()
            .map(|w| fetcher.to_rate_window(w, Some(10080)))
            .expect("routines window");

        assert!((design.used_percent - design_percent).abs() < f64::EPSILON);
        assert!((routines.used_percent - routines_percent).abs() < f64::EPSILON);
    }
}

#[test]
fn maps_scoped_weekly_limits_even_when_inactive() {
    let usage: super::UsageResponse = serde_json::from_str(
        r#"{
                "limits": [{
                    "kind": "weekly_scoped",
                    "group": "weekly",
                    "percent": 7,
                    "resets_at": "2026-07-16T10:00:00Z",
                    "scope": {"model": {"id": null, "display_name": "Fable"}},
                    "is_active": false
                }]
            }"#,
    )
    .unwrap();

    let windows = super::super::scoped_weekly::scoped_weekly_windows(&usage.limits);
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].id, "claude-weekly-scoped-fable");
    assert_eq!(windows[0].title, "Fable only");
}

#[test]
fn parses_oauth_apps_window_and_embedded_extra_usage() {
    let usage: super::UsageResponse = serde_json::from_str(
        r#"{
                "five_hour": { "utilization": 0.1 },
                "seven_day_oauth_apps": { "utilization": 42 },
                "extra_usage": {
                    "is_enabled": true,
                    "monthly_credit_limit": 2000,
                    "used_credits": 550,
                    "currency": "USD"
                }
            }"#,
    )
    .unwrap();

    let fetcher = ClaudeWebApiFetcher::new();
    let oauth_apps = usage
        .seven_day_oauth_apps
        .as_ref()
        .map(|w| fetcher.to_rate_window(w, Some(10080)))
        .expect("oauth apps window");
    let extra = usage.extra_usage.expect("extra usage");

    assert!((oauth_apps.used_percent - 42.0).abs() < f64::EPSILON);
    assert_eq!(extra.is_enabled, Some(true));
    assert_eq!(extra.monthly_credit_limit, Some(2000.0));
    assert_eq!(extra.used_credits, Some(550.0));
}

#[test]
fn issue_279_session_limits_win_over_stale_five_hour_after_rollover() {
    // Right after a 5h window rollover the legacy five_hour.utilization
    // can transiently report 1.0 (normalizes to 100%) even though
    // claude.ai shows only 5% for the fresh window. The limits[] entry
    // (kind=="session") carries the true value and must win.
    let usage: super::UsageResponse = serde_json::from_str(
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
    )
    .expect("issue 279 body");

    let fetcher = ClaudeWebApiFetcher::new();
    let (primary, secondary, _) = fetcher.build_rate_windows(&usage);

    // Primary session must be 5%, not the stale 100%.
    assert!(
        (primary.used_percent - 5.0).abs() < f64::EPSILON,
        "primary was {}, expected 5% (not 100%)",
        primary.used_percent
    );
    assert!((primary.used_percent - 100.0).abs() > 1.0);
    assert_eq!(primary.window_minutes, Some(300));
    assert!(primary.resets_at.is_some());

    // Weekly lane is unaffected (still prefers limits weekly_all).
    let weekly = secondary.expect("weekly");
    assert!((weekly.used_percent - 1.0).abs() < f64::EPSILON);
}

#[test]
fn session_falls_back_to_legacy_five_hour_without_limits_entry() {
    // When no limits[] session entry exists, the legacy five_hour field
    // is still the source of truth (backwards compatible).
    let usage: super::UsageResponse = serde_json::from_str(
        r#"{
                "five_hour": {"utilization": 10.0, "resets_at": "2026-08-13T12:49:59Z"}
            }"#,
    )
    .expect("legacy-only body");

    let fetcher = ClaudeWebApiFetcher::new();
    let (primary, _, _) = fetcher.build_rate_windows(&usage);

    assert!((primary.used_percent - 10.0).abs() < f64::EPSILON);
    assert_eq!(primary.window_minutes, Some(300));
}

#[test]
fn parse_prepaid_balance_converts_cents_to_dollars() {
    let balance = super::parse_prepaid_balance(r#"{"amount": 2550, "currency": "usd"}"#)
        .expect("prepaid balance");
    assert!((balance.amount_dollars - 25.5).abs() < f64::EPSILON);
    assert_eq!(balance.currency_code, "USD");
}

#[test]
fn parse_prepaid_balance_rejects_negative_or_non_finite() {
    assert!(super::parse_prepaid_balance(r#"{"amount": -1, "currency": "USD"}"#).is_none());
    assert!(super::parse_prepaid_balance(r#"{"amount": 10, "currency": "  "}"#).is_none());
}

#[test]
fn apply_prepaid_balance_attaches_to_same_currency_cost() {
    let existing = crate::core::CostSnapshot::new(1.0, "USD", "Monthly").with_limit(20.0);
    let balance = super::PrepaidBalance {
        amount_dollars: 12.34,
        currency_code: "USD".into(),
    };
    let cost = super::apply_prepaid_balance(balance, Some(existing));
    assert_eq!(cost.balance, Some(12.34));
    assert!((cost.used - 1.0).abs() < f64::EPSILON);
    assert_eq!(cost.limit, Some(20.0));
    assert_eq!(cost.period, "Monthly");
}

#[test]
fn apply_prepaid_balance_creates_extra_usage_when_missing_or_mismatch() {
    let balance = super::PrepaidBalance {
        amount_dollars: 5.0,
        currency_code: "USD".into(),
    };
    let created = super::apply_prepaid_balance(balance.clone(), None);
    assert_eq!(created.balance, Some(5.0));
    assert_eq!(created.period, "Extra usage");
    assert!((created.used - 0.0).abs() < f64::EPSILON);

    let eur = crate::core::CostSnapshot::new(2.0, "EUR", "Monthly");
    let replaced = super::apply_prepaid_balance(balance, Some(eur));
    assert_eq!(replaced.currency_code, "USD");
    assert_eq!(replaced.period, "Extra usage");
    assert_eq!(replaced.balance, Some(5.0));
}

#[test]
fn web_extras_order_oauth_scoped_then_routines() {
    use crate::core::{NamedRateWindow, RateWindow, UsageSnapshot};

    let mut snapshot = UsageSnapshot::new(RateWindow::new(10.0));
    super::append_web_extra_windows(
        &mut snapshot,
        Some(RateWindow::new(1.0)),
        vec![NamedRateWindow::new(
            "claude-weekly-scoped-fable",
            "Fable only",
            RateWindow::new(2.0),
        )],
        Some(RateWindow::new(3.0)),
    );

    let ids: Vec<&str> = snapshot
        .extra_rate_windows
        .iter()
        .map(|w| w.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![
            "claude-oauth-apps",
            "claude-weekly-scoped-fable",
            "claude-routines"
        ]
    );
}
