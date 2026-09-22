use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct FakeLegacySession {
    workspace_id: String,
}

struct FakeWebTransport {
    console_failures: tokio::sync::Barrier,
    resolver_calls: AtomicUsize,
    console_timeouts: Mutex<Vec<Duration>>,
    usage_sessions: Mutex<Vec<String>>,
    balance_sessions: Mutex<Vec<String>>,
    legacy_balance_timeouts: Mutex<Vec<Duration>>,
    fail_legacy_resolution: bool,
}

impl FakeWebTransport {
    fn new(fail_legacy_resolution: bool) -> Arc<Self> {
        Arc::new(Self {
            console_failures: tokio::sync::Barrier::new(2),
            resolver_calls: AtomicUsize::new(0),
            console_timeouts: Mutex::new(Vec::new()),
            usage_sessions: Mutex::new(Vec::new()),
            balance_sessions: Mutex::new(Vec::new()),
            legacy_balance_timeouts: Mutex::new(Vec::new()),
            fail_legacy_resolution,
        })
    }
}

#[async_trait]
impl WebTransport for FakeWebTransport {
    type LegacySession = FakeLegacySession;

    async fn fetch_workspace_id(
        &self,
        _cookie_header: &str,
        _timeout: Duration,
    ) -> Result<String, ProviderError> {
        panic!("workspace override should bypass discovery")
    }

    async fn fetch_console_usage(
        &self,
        _workspace_id: &str,
        _cookie_header: &str,
        timeout: Duration,
    ) -> Result<console::ConsoleUsage, ProviderError> {
        self.console_timeouts.lock().unwrap().push(timeout);
        self.console_failures.wait().await;
        Err(ProviderError::Other(
            "console usage unavailable".to_string(),
        ))
    }

    async fn fetch_console_balance(
        &self,
        _workspace_id: &str,
        _cookie_header: &str,
        timeout: Duration,
    ) -> Result<Option<f64>, ProviderError> {
        self.console_timeouts.lock().unwrap().push(timeout);
        self.console_failures.wait().await;
        Err(ProviderError::Other(
            "console balance unavailable".to_string(),
        ))
    }

    async fn resolve_legacy_session(
        &self,
        _cookies: &WebCookieSession,
    ) -> Result<Self::LegacySession, ProviderError> {
        self.resolver_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_legacy_resolution {
            return Err(ProviderError::AuthRequired);
        }
        Ok(FakeLegacySession {
            workspace_id: "wrk_legacy".to_string(),
        })
    }

    async fn fetch_legacy_usage(
        &self,
        session: &Self::LegacySession,
    ) -> Result<legacy::LegacyUsage, ProviderError> {
        self.usage_sessions
            .lock()
            .unwrap()
            .push(session.workspace_id.clone());
        Ok(legacy::LegacyUsage {
            usage: UsageSnapshot::new(RateWindow::with_details(25.0, None, None, None)),
            embedded_balance: None,
        })
    }

    async fn fetch_legacy_balance(
        &self,
        session: &Self::LegacySession,
        timeout: Duration,
    ) -> Result<Option<f64>, ProviderError> {
        self.legacy_balance_timeouts.lock().unwrap().push(timeout);
        self.balance_sessions
            .lock()
            .unwrap()
            .push(session.workspace_id.clone());
        Ok(Some(42.5))
    }
}

#[test]
fn cookie_session_classifies_transport_capabilities_once() {
    let both = WebCookieSession::new("auth=legacy; __Host-console_session=console");
    assert_eq!(
        both.capabilities,
        CookieCapabilities {
            console: true,
            legacy: true,
        }
    );
    assert_eq!(
        WebCookieSession::new("__Host-console_session=console").capabilities,
        CookieCapabilities {
            console: true,
            legacy: false,
        }
    );
    assert_eq!(
        WebCookieSession::new("auth=; __Host-console_session=").capabilities,
        CookieCapabilities {
            console: false,
            legacy: false,
        }
    );
}

#[test]
fn legacy_recovery_and_error_precedence_follow_cookie_capabilities() {
    let both = WebCookieSession::new("auth=legacy; __Host-console_session=console");
    assert!(both.can_recover_with_legacy(&ProviderError::AuthRequired));
    assert!(
        !WebCookieSession::new("__Host-console_session=console")
            .can_recover_with_legacy(&ProviderError::AuthRequired)
    );
    assert!(!both.can_recover_with_legacy(&ProviderError::NotInstalled("terminal".to_string())));

    let error = both
        .select_legacy_result::<()>(
            ProviderError::Other("console unavailable".to_string()),
            Err(ProviderError::AuthRequired),
        )
        .unwrap_err();
    assert!(matches!(error, ProviderError::Other(message) if message == "console unavailable"));

    let error = both
        .select_legacy_result::<()>(
            ProviderError::AuthRequired,
            Err(ProviderError::Parse("legacy payload missing".to_string())),
        )
        .unwrap_err();
    assert!(matches!(error, ProviderError::Parse(message) if message == "legacy payload missing"));
}

#[test]
fn uses_context_workspace_id_before_discovery() {
    assert_eq!(
        OpenCodeGoProvider::workspace_id_from_context(Some("wrk_override")),
        Some("wrk_override".to_string())
    );
    assert_eq!(
        OpenCodeGoProvider::workspace_id_from_context(Some("")),
        None
    );
}

#[test]
fn selected_token_auth_failure_does_not_fall_back_to_local_estimate() {
    let mut selected = FetchContext {
        auto_prefer_web: true,
        ..FetchContext::default()
    };
    assert!(!OpenCodeGoProvider::web_error_allows_local_fallback(
        &selected,
        &ProviderError::AuthRequired
    ));

    selected.auto_prefer_web = false;
    selected.workspace_id = Some("wrk_example".to_string());
    assert!(OpenCodeGoProvider::web_error_allows_local_fallback(
        &selected,
        &ProviderError::AuthRequired
    ));
}

// ── F15: bounded optional Zen balance wait (upstream #2583) ───

#[test]
fn zen_join_budget_grace_vs_completeness() {
    let started = std::time::Instant::now();
    // Background/UI reads keep the short join grace.
    assert_eq!(
        zen_balance_join_budget(started, false),
        Duration::from_millis(250)
    );
    // Completeness reads get the remainder of the 5 s optional-balance
    // budget measured from task creation.
    let budget = zen_balance_join_budget(started, true);
    assert!(budget <= ZEN_BALANCE_TIMEOUT, "{budget:?}");
    assert!(budget > Duration::from_secs(4), "{budget:?}");
    // An already-exhausted budget joins immediately.
    let stale = std::time::Instant::now()
        .checked_sub(Duration::from_secs(60))
        .unwrap();
    assert_eq!(zen_balance_join_budget(stale, true), Duration::ZERO);
}

#[tokio::test]
async fn slow_zen_task_is_abandoned_within_grace() {
    let started = std::time::Instant::now();
    let task = tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        OptionalZenBalance::Resolved(Some(42.5))
    });
    // UI grace (250 ms) never waits out a 30 s balance fetch.
    let balance = OpenCodeGoProvider::join_zen_balance(task, started, false).await;
    assert_eq!(balance, None);
}

#[tokio::test]
async fn fast_zen_task_lands_in_completeness_budget() {
    let started = std::time::Instant::now();
    let task = tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        OptionalZenBalance::Resolved(Some(42.5))
    });
    let balance = OpenCodeGoProvider::join_zen_balance(task, started, true).await;
    assert_eq!(balance, Some(OptionalZenBalance::Resolved(Some(42.5))));
}

#[tokio::test]
async fn simultaneous_console_failures_resolve_one_legacy_session() {
    let transport = FakeWebTransport::new(false);
    let ctx = FetchContext {
        workspace_id: Some("wrk_console".to_string()),
        web_timeout: 1,
        requires_optional_usage_completeness: true,
        ..FetchContext::default()
    };

    let result = OpenCodeGoProvider::fetch_with_transport(
        &ctx,
        "auth=legacy; __Host-console_session=console",
        Arc::clone(&transport),
    )
    .await
    .unwrap();

    assert_eq!(transport.resolver_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        transport.usage_sessions.lock().unwrap().as_slice(),
        ["wrk_legacy"]
    );
    assert_eq!(
        transport.balance_sessions.lock().unwrap().as_slice(),
        ["wrk_legacy"]
    );
    assert_eq!(
        transport.console_timeouts.lock().unwrap().as_slice(),
        [Duration::from_secs(1), Duration::from_secs(1)]
    );
    assert_eq!(
        transport.legacy_balance_timeouts.lock().unwrap().as_slice(),
        [Duration::from_secs(1)]
    );
    assert_eq!(result.cost.unwrap().used, 42.5);
}

#[tokio::test]
async fn production_fallback_preserves_console_error_precedence() {
    let transport = FakeWebTransport::new(true);
    let ctx = FetchContext {
        workspace_id: Some("wrk_console".to_string()),
        web_timeout: 1,
        requires_optional_usage_completeness: true,
        ..FetchContext::default()
    };

    let error = OpenCodeGoProvider::fetch_with_transport(
        &ctx,
        "auth=legacy; __Host-console_session=console",
        Arc::clone(&transport),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        ProviderError::Other(message) if message == "console usage unavailable"
    ));
    assert_eq!(transport.resolver_calls.load(Ordering::SeqCst), 1);
    assert!(transport.usage_sessions.lock().unwrap().is_empty());
    assert!(transport.balance_sessions.lock().unwrap().is_empty());
}
