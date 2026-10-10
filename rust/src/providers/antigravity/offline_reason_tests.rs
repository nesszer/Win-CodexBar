//! Offline-explanation tests for failed live Antigravity probes (upstream
//! CodexBar v0.65.0, #3865).

use super::super::AntigravityProvider;
use super::super::cli_print_failure::ExitReason;
use super::super::tests::{offline_result, run_fallback};
use super::*;
use crate::core::{ProviderFetchResult, RateWindow, UsageSnapshot};

const OFFLINE_DETAIL_PREFIX: &str = "Live Antigravity usage is unavailable; showing offline data.";
const HINT: &str = "check Diagnostics for per-source details";

fn offline_detail_value(result: &ProviderFetchResult) -> String {
    let details = result.display_details();
    assert_eq!(details.len(), 1, "exactly one explanation row");
    assert_eq!(details[0].id(), "antigravity-live-unavailable");
    assert_eq!(details[0].title(), "Live usage");
    details[0].value().to_string()
}

/// Resolve `failure` against offline history and return its explanation.
fn offline_detail_for(failure: impl Into<LiveFailure>) -> String {
    let resolved = AntigravityProvider::resolve_probe_failure(failure, Some(offline_result()))
        .expect("offline history is preserved");
    assert_eq!(resolved.source_label, "offline");
    offline_detail_value(&resolved)
}

#[test]
fn offline_fallback_explains_each_typed_failure() {
    let other = |message: &str| ProviderError::Other(message.to_string());
    let cases = [
        (LiveFailure::not_running(), AGY_NOT_FOUND_MESSAGE),
        // Only the not-running marker proves the runtime is absent; other
        // NotInstalled texts (agy exited early, process detection failed) must
        // not claim that agy was not found.
        (
            ProviderError::NotInstalled("Failed to detect Antigravity process".to_string()).into(),
            HINT,
        ),
        (
            ProviderError::Timeout.into(),
            "Antigravity quota request timed out.",
        ),
        (
            LiveFailure::http_status(500, other("API error 500 Internal Server Error: busy")),
            "the usage request failed (HTTP 500)",
        ),
        (
            LiveFailure::http_status(401, other("API error 401 Unauthorized")),
            "Antigravity session expired. Restart Antigravity and retry.",
        ),
        (
            LiveFailure::http_status(403, other("API error 403 Forbidden")),
            "Antigravity session expired. Restart Antigravity and retry.",
        ),
        (
            LiveFailure::cli_report(CliPrintFailure::ExecutableNotFound),
            "Antigravity CLI usage report failed: agy executable not found",
        ),
        (
            LiveFailure::cli_report(CliPrintFailure::Exited {
                code: 1,
                reason: ExitReason::Unspecified,
            }),
            "Antigravity CLI usage report failed: agy exited 1",
        ),
        (ProviderError::Parse("bad json".to_string()).into(), HINT),
        (other("boom").into(), HINT),
        (ProviderError::NoCookies.into(), HINT),
    ];
    for (failure, reason) in cases {
        assert_eq!(
            offline_detail_for(failure),
            format!("{OFFLINE_DETAIL_PREFIX} {reason}")
        );
    }
}

#[test]
fn offline_explanation_keeps_only_the_http_status() {
    let body = offline_detail_for(LiveFailure::http_status(
        500,
        ProviderError::Other(
            r#"API error 500 Internal Server Error: {"secret":"token-value"}"#.to_string(),
        ),
    ));
    assert!(body.contains("HTTP 500"), "{body}");
    assert!(!body.contains("token-value"), "{body}");

    let expired = offline_detail_for(LiveFailure::http_status(
        403,
        ProviderError::Other(r#"API error 403 Forbidden: {"detail":"private"}"#.to_string()),
    ));
    assert!(expired.contains("session expired"), "{expired}");
    assert!(!expired.contains("private"), "{expired}");
}

#[test]
fn http_failure_without_history_surfaces_the_original_error() {
    let resolved = AntigravityProvider::resolve_probe_failure(
        LiveFailure::http_status(
            500,
            ProviderError::Other("API error 500 Internal Server Error: busy".to_string()),
        ),
        None,
    );
    assert!(matches!(
        resolved,
        Err(ProviderError::Other(message)) if message == "API error 500 Internal Server Error: busy"
    ));
}

#[tokio::test]
async fn offline_fallback_explains_the_masked_cli_failure() {
    let (result, _) = run_fallback(
        Err(LiveFailure::not_running()),
        Err(LiveFailure::cli_report(CliPrintFailure::Exited {
            code: 1,
            reason: ExitReason::EligibilityNetwork,
        })),
        Some(offline_result()),
    )
    .await;
    let result = result.expect("offline history should survive a failed CLI probe");
    assert_eq!(result.source_label, "offline");
    assert_eq!(
        offline_detail_value(&result),
        "Live Antigravity usage is unavailable; showing offline data. Antigravity CLI usage report failed: agy exited 1; the eligibility check failed on a network request (check network or proxy settings)"
    );
}

#[tokio::test]
async fn unavailable_cli_keeps_the_local_failure_reason() {
    let (result, _) = run_fallback(
        Err(LiveFailure::http_status(
            500,
            ProviderError::Other("API error 500 Internal Server Error: busy".to_string()),
        )),
        Ok(None),
        Some(offline_result()),
    )
    .await;
    let result = result.expect("offline history is preserved");
    assert_eq!(
        offline_detail_value(&result),
        format!("{OFFLINE_DETAIL_PREFIX} the usage request failed (HTTP 500)")
    );
}

#[tokio::test]
async fn request_timeouts_are_classified_without_the_request_url() {
    // Accepted by the OS backlog but never answered, so the client times out.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let url = format!(
        "http://127.0.0.1:{}/private-path",
        listener.local_addr().expect("listener address").port()
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_millis(200))
        .build()
        .expect("test client");
    let error = client
        .post(&url)
        .send()
        .await
        .expect_err("an unanswered request must time out");

    let failure = LiveFailure::request("API request failed", &error);
    assert_eq!(failure.reason(), LiveFailureReason::TimedOut);
    let detail = failure
        .offline_detail()
        .expect("offline explanation")
        .value()
        .to_string();
    assert_eq!(
        detail,
        format!("{OFFLINE_DETAIL_PREFIX} Antigravity quota request timed out.")
    );
    assert!(!detail.contains("private-path"), "{detail}");
    drop(listener);
}

#[tokio::test]
async fn refused_requests_are_classified_without_the_request_url() {
    // Bind and release a loopback port so nothing listens on it.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("loopback port")
        .port();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("test client");
    let error = client
        .post(format!("http://127.0.0.1:{port}/private-path"))
        .send()
        .await
        .expect_err("nothing listens on the released port");

    let failure = LiveFailure::request("API request failed", &error);
    assert_eq!(failure.reason(), LiveFailureReason::CannotConnect);
    let detail = failure
        .offline_detail()
        .expect("offline explanation")
        .value()
        .to_string();
    assert_eq!(
        detail,
        format!("{OFFLINE_DETAIL_PREFIX} Could not connect to the server.")
    );
    assert!(!detail.contains("private-path"), "{detail}");
    assert!(!detail.contains(&port.to_string()), "{detail}");
}

#[test]
fn offline_explanation_never_echoes_error_text() {
    let secrets = [
        "user@example.com",
        "https://lh3.googleusercontent.com/a/photo",
        r"C:\Users\someone\agy.exe",
        "HTTP 500",
    ];
    let leaky = secrets.join(" ");
    let errors = [
        ProviderError::NotInstalled(leaky.clone()),
        ProviderError::Parse(leaky.clone()),
        ProviderError::Other(leaky.clone()),
        ProviderError::OAuth(leaky),
    ];
    for error in errors {
        let value = offline_detail_for(error);
        for secret in secrets {
            assert!(!value.contains(secret), "{secret} leaked into {value}");
        }
    }
}

#[test]
fn auth_required_adds_no_offline_explanation() {
    assert!(
        LiveFailure::from(ProviderError::AuthRequired)
            .offline_detail()
            .is_none()
    );
    let resolved = AntigravityProvider::resolve_probe_failure(
        ProviderError::AuthRequired,
        Some(offline_result()),
    );
    assert!(matches!(resolved, Err(ProviderError::AuthRequired)));
}

#[tokio::test]
async fn successful_live_fallback_carries_no_offline_explanation() {
    let live = ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(10.0)), "cli");
    let (result, _) = run_fallback(
        Err(ProviderError::Timeout.into()),
        Ok(Some(live)),
        Some(offline_result()),
    )
    .await;
    let result = result.expect("live CLI fallback wins over offline history");
    assert_eq!(result.source_label, "cli");
    assert!(result.display_details().is_empty());
}

#[tokio::test]
async fn failed_cli_fallback_offline_result_explains_failure() {
    let (result, _) = run_fallback(
        Err(ProviderError::AuthRequired.into()),
        Err(LiveFailure::from(ProviderError::Timeout)),
        Some(offline_result()),
    )
    .await;
    let result = result.expect("offline history should survive a failed CLI probe");
    assert_eq!(
        offline_detail_value(&result),
        format!("{OFFLINE_DETAIL_PREFIX} Antigravity quota request timed out.")
    );
}
