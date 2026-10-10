//! Tests for the OpenRouter provider.

use super::*;

// Regression guard for the `/auth/credits` 404 bug: the base must be the
// bare `/api/v1` prefix. Credits and key live on DIFFERENT subpaths, so a
// base that bakes in `/auth` (or anything else) silently breaks one of them.
#[test]
fn api_base_is_bare_v1_prefix() {
    assert_eq!(OPENROUTER_API_BASE, "https://openrouter.ai/api/v1");
}

// Credits endpoint: `/api/v1/credits` (verified HTTP 200 against live API).
// The old base `.../api/v1/auth` produced `/api/v1/auth/credits` -> 404.
#[test]
fn credits_url_resolves_to_canonical_path() {
    let url = format!("{}/credits", OPENROUTER_API_BASE);
    assert_eq!(url, "https://openrouter.ai/api/v1/credits");
}

// Key introspection endpoint: `/api/v1/key` (verified HTTP 200), matching
// upstream's `{base}/key` append. (OpenRouter also aliases `/auth/key`, but
// we mirror upstream's canonical path.)
#[test]
fn key_url_resolves_to_canonical_path() {
    let url = format!("{}/key", OPENROUTER_API_BASE);
    assert_eq!(url, "https://openrouter.ai/api/v1/key");
}

#[test]
fn usage_dashboard_opens_activity_history() {
    assert_eq!(
        OpenRouterProvider::new().metadata().dashboard_url,
        Some("https://openrouter.ai/activity")
    );
}

#[test]
fn deprecated_rate_limit_metadata_is_ignored() {
    let response: KeyResponse = serde_json::from_value(serde_json::json!({
        "data": {
            "rate_limit": "deprecated",
            "is_management_key": true,
            "usage": 0.0
        }
    }))
    .expect("deprecated rate_limit must not invalidate /key");

    assert_eq!(response.data.is_management_key, Some(true));
    assert_eq!(response.data.usage, Some(0.0));
}

// ── F14: server-reported current-period remaining drives the key meter ──

fn key_data(
    limit: Option<f64>,
    remaining: Option<f64>,
    reset: Option<&str>,
    usage: Option<f64>,
    daily: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
) -> KeyData {
    KeyData {
        limit,
        limit_remaining: remaining,
        limit_reset: reset.map(str::to_string),
        usage,
        usage_daily: daily,
        usage_weekly: weekly,
        usage_monthly: monthly,
        is_management_key: None,
    }
}

fn key_quota_percent(key_data: KeyData) -> Option<f64> {
    let mut usage = UsageSnapshot::new(RateWindow::new(0.0));
    OpenRouterProvider::add_key_quota(&mut usage, &key_data);
    usage.secondary.map(|window| window.used_percent)
}

#[test]
fn key_limit_copy_stays_distinct_from_account_balance() {
    let provider = OpenRouterProvider::new();
    assert_eq!(provider.metadata().weekly_label, "API key limit");

    let credits = CreditsData {
        total_credits: 5.0,
        total_usage: 3.1,
    };
    let mut usage = OpenRouterProvider::build_credits_usage(&credits);
    OpenRouterProvider::add_key_quota(
        &mut usage,
        &key_data(
            Some(30.0),
            Some(30.0),
            Some("monthly"),
            Some(0.0),
            None,
            None,
            Some(0.0),
        ),
    );
    assert_eq!(usage.login_method.as_deref(), Some("$1.90 balance"));
    let key = usage.secondary.expect("key spending cap");
    assert_eq!(key.used_percent, 0.0);
    assert_eq!(
        key.reset_description.as_deref(),
        Some("$0.00/$30.00 spending cap · Spending cap, not balance")
    );
}

#[test]
fn key_quota_can_stand_in_when_account_credits_are_unavailable() {
    let usage = OpenRouterProvider::build_key_fallback_usage(&key_data(
        Some(20.0),
        None,
        None,
        Some(5.0),
        None,
        None,
        None,
    ))
    .expect("usable key quota");

    assert!(usage.primary.is_informational);
    assert!(usage.primary_label.is_none());
    assert!(usage.login_method.is_none());
    let key_window = usage.secondary.expect("key spending cap");
    assert_eq!(key_window.used_percent, 25.0);
    assert_eq!(usage.secondary_label.as_deref(), Some("API key limit"));
    assert_eq!(
        key_window.reset_description.as_deref(),
        Some("$5.00/$20.00 spending cap · Account balance unavailable")
    );
}

#[test]
fn key_fallback_does_not_invent_usage_without_a_limit() {
    assert!(
        OpenRouterProvider::build_key_fallback_usage(&key_data(
            None,
            None,
            None,
            Some(5.0),
            None,
            None,
            None,
        ))
        .is_none()
    );
}

#[test]
fn fallback_preserves_key_quota_lane_across_recovery() {
    let credits = || {
        Ok(CreditsResponse {
            data: CreditsData {
                total_credits: 20.0,
                total_usage: 5.0,
            },
        })
    };
    let key = || key_data(Some(20.0), None, None, Some(5.0), None, None, None);

    let normal = OpenRouterProvider::resolve_usage(credits(), Some(key()))
        .expect("account credits should resolve");
    let fallback = OpenRouterProvider::resolve_usage(
        Err(ProviderError::Other("credits unavailable".to_string())),
        Some(key()),
    )
    .expect("key quota should resolve when credits are unavailable");
    let recovered = OpenRouterProvider::resolve_usage(credits(), Some(key()))
        .expect("account credits should recover");

    for usage in [&normal, &fallback, &recovered] {
        assert_eq!(usage.secondary_label.as_deref(), Some("API key limit"));
        assert_eq!(
            usage.secondary.as_ref().map(|window| window.used_percent),
            Some(25.0)
        );
    }
    assert!(!normal.primary.is_informational);
    assert!(fallback.primary.is_informational);
    assert!(!recovered.primary.is_informational);
    assert_eq!(normal.login_method.as_deref(), Some("$15.00 balance"));
    assert!(fallback.login_method.is_none());
    assert_eq!(recovered.login_method.as_deref(), Some("$15.00 balance"));
}

#[test]
fn uncapped_cost_prefers_monthly_key_usage_and_keeps_balance() {
    let credits = CreditsResponse {
        data: CreditsData {
            total_credits: 20.0,
            total_usage: 7.0,
        },
    };
    let key = key_data(Some(0.0), None, None, Some(5.0), None, None, Some(3.5));

    let cost = OpenRouterProvider::build_uncapped_cost(Some(&key), Some(&credits))
        .expect("uncapped key should expose spend");

    assert_eq!(cost.used, 3.5);
    assert_eq!(cost.period, "This month (API key)");
    assert_eq!(cost.balance, Some(13.0));
}

#[test]
fn capped_and_management_keys_do_not_create_payg_costs() {
    let credits = CreditsResponse {
        data: CreditsData {
            total_credits: 20.0,
            total_usage: 7.0,
        },
    };
    let capped = key_data(Some(10.0), None, None, Some(5.0), None, None, Some(3.5));
    assert!(OpenRouterProvider::build_uncapped_cost(Some(&capped), Some(&credits)).is_none());

    let mut management = key_data(Some(0.0), None, None, Some(5.0), None, None, Some(3.5));
    management.is_management_key = Some(true);
    assert!(OpenRouterProvider::build_uncapped_cost(Some(&management), Some(&credits)).is_none());
}

#[test]
fn activity_cost_wins_while_uncapped_key_spend_windows_remain() {
    let credits = CreditsResponse {
        data: CreditsData {
            total_credits: 20.0,
            total_usage: 7.0,
        },
    };
    let key = key_data(
        Some(0.0),
        None,
        None,
        Some(5.0),
        Some(1.0),
        Some(2.0),
        Some(3.0),
    );
    let mut usage = OpenRouterProvider::build_credits_usage(&credits.data);
    OpenRouterProvider::apply_key_lanes(&mut usage, &key, "Spending cap, not balance");

    let activity = CostSnapshot::new(4.0, "USD", "Last 30 days (UTC)");
    let selected = Some(activity)
        .or(OpenRouterProvider::build_uncapped_cost(
            Some(&key),
            Some(&credits),
        ))
        .expect("Activity cost should be selected");

    assert_eq!(selected.used, 4.0);
    assert_eq!(selected.period, "Last 30 days (UTC)");
    for (id, expected) in [
        ("daily-spend", "$1.00 today"),
        ("weekly-spend", "$2.00 this week"),
        ("monthly-spend", "$3.00 this month"),
    ] {
        let window = usage
            .extra_rate_windows
            .iter()
            .find(|window| window.id == id)
            .expect("key spend window");
        assert_eq!(window.window.reset_description.as_deref(), Some(expected));
    }
}

#[test]
fn server_remaining_replaces_lifetime_usage_for_meter() {
    // limit 50, server says 12.50 left this period → 75% used, even though
    // cumulative lifetime usage would imply a different ratio.
    let pct = key_quota_percent(key_data(
        Some(50.0),
        Some(12.5),
        None,
        Some(40.0),
        None,
        None,
        None,
    ));
    assert_eq!(pct, Some(75.0));
}

#[test]
fn negative_server_remaining_reads_exhausted() {
    // Upstream: "treat negative remaining as exhausted quota".
    let pct = key_quota_percent(key_data(
        Some(50.0),
        Some(-3.0),
        None,
        Some(10.0),
        None,
        None,
        None,
    ));
    assert_eq!(pct, Some(100.0));
}

#[test]
fn above_limit_server_remaining_reads_zero() {
    // Inclusive [0, keyLimit] clamp: a server remaining above the
    // configured limit renders 0% used, not a suppressed meter.
    let pct = key_quota_percent(key_data(
        Some(50.0),
        Some(75.0),
        None,
        Some(10.0),
        None,
        None,
        None,
    ));
    assert_eq!(pct, Some(0.0));
}

#[test]
fn reset_window_usage_is_the_preferred_fallback() {
    // No remaining: `limit_reset: "monthly"` picks usage_monthly (25/50).
    let pct = key_quota_percent(key_data(
        Some(50.0),
        None,
        Some("monthly"),
        Some(40.0),
        Some(1.0),
        Some(2.0),
        Some(25.0),
    ));
    assert_eq!(pct, Some(50.0));
    // Case-insensitive reset label.
    let pct = key_quota_percent(key_data(
        Some(50.0),
        None,
        Some("WEEKLY"),
        Some(40.0),
        Some(1.0),
        Some(2.0),
        Some(25.0),
    ));
    assert_eq!(pct, Some(4.0));
}

#[test]
fn cumulative_usage_is_the_last_fallback() {
    let pct = key_quota_percent(key_data(
        Some(50.0),
        None,
        None,
        Some(20.0),
        Some(1.0),
        None,
        None,
    ));
    assert_eq!(pct, Some(40.0));
}

#[test]
fn no_usable_quota_source_hides_the_meter() {
    assert_eq!(
        key_quota_percent(key_data(Some(50.0), None, None, None, None, None, None)),
        None
    );
    assert_eq!(
        key_quota_percent(key_data(
            Some(0.0),
            Some(5.0),
            None,
            Some(1.0),
            None,
            None,
            None
        )),
        None
    );
    assert_eq!(
        key_quota_percent(key_data(None, Some(5.0), None, Some(1.0), None, None, None)),
        None
    );
}

#[test]
fn parsed_key_wire_fields_decode() {
    let parsed: KeyResponse = serde_json::from_str(
        r#"{"data":{"limit":50,"limit_remaining":12.5,"limit_reset":"monthly","usage":40,"usage_monthly":25}}"#,
    )
    .unwrap();
    assert_eq!(parsed.data.limit, Some(50.0));
    assert_eq!(parsed.data.limit_remaining, Some(12.5));
    assert_eq!(parsed.data.limit_reset.as_deref(), Some("monthly"));
}

// ── 0.61.0 (#3272, #3733): optional-request diagnostics and detail rows ──

use super::activity::ActivitySummary;
use super::diagnostics::{
    ACTIVITY_KEY_REQUIRED, ACTIVITY_NOT_CONFIGURED, Observations, build_display_details,
};

type DetailRow = (String, String, Option<String>);

fn detail_rows(
    credits: Result<CreditsData, String>,
    key: Result<KeyData, String>,
    activity: Result<ActivitySummary, String>,
) -> Vec<DetailRow> {
    build_display_details(&Observations {
        credits: &credits,
        key: &key,
        activity: &activity,
    })
    .iter()
    .map(|row| {
        (
            row.title().to_string(),
            row.value().to_string(),
            row.secondary_value().map(str::to_string),
        )
    })
    .collect()
}

fn row_of<'a>(rows: &'a [DetailRow], title: &str) -> &'a DetailRow {
    rows.iter()
        .find(|row| row.0 == title)
        .unwrap_or_else(|| panic!("missing row {title}: {rows:?}"))
}

#[test]
fn optional_request_deadline_is_four_seconds() {
    assert_eq!(
        OPENROUTER_REQUEST_TIMEOUT,
        std::time::Duration::from_secs(4)
    );
}

#[test]
fn successful_sources_render_credits_key_and_activity_rows() {
    let rows = detail_rows(
        Ok(CreditsData {
            total_credits: 5.0,
            total_usage: 3.1,
        }),
        Ok(key_data(
            Some(30.0),
            Some(30.0),
            Some(" monthly "),
            Some(0.0),
            None,
            None,
            None,
        )),
        Ok(ActivitySummary {
            tokens: 22,
            requests: 2,
            models: 2,
        }),
    );

    assert_eq!(row_of(&rows, "Credits remaining").1, "$1.90");
    assert_eq!(row_of(&rows, "Credits used").1, "$3.10");
    assert_eq!(row_of(&rows, "Credits total added").1, "$5.00");
    let limit = row_of(&rows, "API key limit");
    assert_eq!(limit.1, "$30.00");
    assert_eq!(limit.2.as_deref(), Some("Spending cap, not balance"));
    assert_eq!(row_of(&rows, "API key remaining").1, "$30.00");
    assert_eq!(row_of(&rows, "API key used").1, "$0.00");
    assert_eq!(row_of(&rows, "Reset window").1, "monthly");
    assert_eq!(row_of(&rows, "Activity tokens").1, "22");
    assert_eq!(row_of(&rows, "Activity requests").1, "2");
    assert_eq!(row_of(&rows, "Activity models").1, "2");
    assert!(
        rows.iter()
            .all(|row| row.0 != "Spend history (last 30 days)")
    );
}

#[test]
fn uncapped_key_reports_no_limit_and_omits_remaining() {
    let rows = detail_rows(
        Err("Request failed".into()),
        Ok(key_data(None, None, None, Some(1.0), None, None, None)),
        Err(ACTIVITY_NOT_CONFIGURED.into()),
    );

    assert_eq!(row_of(&rows, "API key limit").1, "No limit configured");
    assert!(rows.iter().all(|row| row.0 != "API key remaining"));
    assert!(rows.iter().all(|row| row.0 != "Reset window"));
}

#[test]
fn degraded_sources_keep_safe_reasons_beside_the_unavailable_marker() {
    let rows = detail_rows(
        Err("Request returned HTTP 503".into()),
        Err("Request timed out".into()),
        Err(ACTIVITY_KEY_REQUIRED.into()),
    );

    for (title, reason) in [
        ("Credits balance", "Request returned HTTP 503"),
        ("API key limit", "Request timed out"),
        ("Spend history (last 30 days)", ACTIVITY_KEY_REQUIRED),
    ] {
        let row = row_of(&rows, title);
        assert_eq!(row.1, "Unavailable right now");
        assert_eq!(row.2.as_deref(), Some(reason));
    }
    assert_eq!(rows.len(), 3);
}

#[test]
fn http_failures_keep_their_status_and_auth_typing() {
    use reqwest::StatusCode;

    let rejected = Degraded::http("key", StatusCode::FORBIDDEN, AUTH_REJECTED);
    assert!(matches!(rejected.error, ProviderError::AuthRequired));
    assert_eq!(rejected.reason, "Request returned HTTP 403");

    // Credits only treats 401 as a rejected credential.
    let unavailable = Degraded::http(
        "credits",
        StatusCode::SERVICE_UNAVAILABLE,
        &[StatusCode::UNAUTHORIZED],
    );
    assert!(matches!(unavailable.error, ProviderError::Other(_)));
    assert_eq!(unavailable.reason, "Request returned HTTP 503");

    let activity = Degraded::http("Activity", StatusCode::FORBIDDEN, AUTH_REJECTED)
        .with_reason(ACTIVITY_KEY_REQUIRED);
    assert!(matches!(activity.error, ProviderError::AuthRequired));
    assert_eq!(activity.reason, "Management API key required");
}

#[test]
fn invalid_bodies_are_labelled_without_leaking_the_payload() {
    let degraded = Degraded::invalid(ProviderError::Parse("secret-body".into()));
    assert_eq!(degraded.reason, "Response was invalid");
}

#[tokio::test]
async fn slow_response_reports_a_timeout_and_other_transport_errors_a_failure() {
    // A bound listener that never accepts or replies: the request stalls.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(100))
        .build()
        .unwrap();
    let stalled = Degraded::from(client.get(&url).send().await.unwrap_err());
    assert_eq!(stalled.reason, "Request timed out");

    // A non-timeout transport error (unsupported scheme) is a plain failure.
    let failed = Degraded::from(client.get("ftp://127.0.0.1/").send().await.unwrap_err());
    assert_eq!(failed.reason, "Request failed");
}
// Upstream 0.67.0 `OpenRouterSettingsError.missingToken` copy: a Management key
// is a valid primary key, and the optional Management field never replaces it.
#[test]
fn missing_key_message_explains_primary_and_management_fields() {
    assert_eq!(
        MISSING_API_KEY_MESSAGE,
        "Enter a regular API key or a Management API key in the API key field, or set OPENROUTER_API_KEY. In Settings, the optional Management API key field does not replace it."
    );
}
