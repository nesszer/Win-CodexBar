//! Cursor provider implementation
//!
//! Fetches usage data from Cursor's API using browser cookies

mod api;
mod app_auth;
mod cost_cooldown;
pub mod local_csv;
mod team_budget;
mod token_cost;

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};

pub use api::CursorApi;
use cost_cooldown::{CostCooldown, credential_fingerprint};
use token_cost::TokenCostError;

/// Cursor provider for fetching AI usage limits
pub struct CursorProvider {
    api: CursorApi,
    /// Per-credential back-off for forbidden cost requests. The store is
    /// process-wide because the shell builds a fresh provider per refresh.
    cost_cooldown: Arc<CostCooldown>,
    /// Injectable so tests can advance past the cooldown without sleeping.
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl CursorProvider {
    pub fn new() -> Self {
        Self {
            api: CursorApi::new(),
            cost_cooldown: CostCooldown::shared(),
            clock: Arc::new(Instant::now),
        }
    }

    async fn fetch_web_usage(
        &self,
        ctx: &FetchContext,
    ) -> Result<
        (
            api::CursorUsageResult,
            Option<token_cost::CursorTokenCostReport>,
        ),
        ProviderError,
    > {
        let cookie_header = if let Some(cookie_header) = ctx.manual_cookie_header.as_deref() {
            cookie_header.to_string()
        } else {
            // Upstream 0.50.0 #2398: Automatic mode prefers the signed-in
            // Cursor app's read-only local session over browser cookies.
            // A rejected app session (stale token, account mismatch)
            // surfaces in the log and falls back to the browser import.
            if ctx.source_mode == SourceMode::Auto
                && let Some(app_result) = self.fetch_via_app_session().await
            {
                return Ok(app_result);
            }
            crate::providers::browser_cookie_header(&["cursor.com", "cursor.sh"])?
        };

        self.fetch_usage_and_token_report(&cookie_header).await
    }

    /// One usage pass with the app's local session; `None` means the app
    /// session was unavailable or rejected (caller falls back to cookies).
    async fn fetch_via_app_session(
        &self,
    ) -> Option<(
        api::CursorUsageResult,
        Option<token_cost::CursorTokenCostReport>,
    )> {
        let app_cookie = app_auth::preferred_auto_cookie_header()?;
        let usage = match self.api.fetch_usage_with_cookie_header(&app_cookie).await {
            Ok(usage) => usage,
            Err(err) => {
                tracing::debug!(
                    "Cursor app session rejected ({err}); falling back to browser cookies"
                );
                return None;
            }
        };
        app_auth::store_validated_app_session(&app_cookie);
        let token_report = self.fetch_token_report_best_effort(&app_cookie).await;
        Some((usage, token_report))
    }

    async fn fetch_usage_and_token_report(
        &self,
        cookie_header: &str,
    ) -> Result<
        (
            api::CursorUsageResult,
            Option<token_cost::CursorTokenCostReport>,
        ),
        ProviderError,
    > {
        let usage = self
            .api
            .fetch_usage_with_cookie_header(cookie_header)
            .await?;
        let token_report = self.fetch_token_report_best_effort(cookie_header).await;
        Ok((usage, token_report))
    }

    /// Best-effort token-cost page; never fail the main usage fetch.
    ///
    /// A 403 is a cost-only rejection: the events request is skipped for six
    /// hours for that credential while quota usage keeps refreshing, and
    /// nothing about the working session is invalidated.
    async fn fetch_token_report_best_effort(
        &self,
        cookie_header: &str,
    ) -> Option<token_cost::CursorTokenCostReport> {
        let credential = credential_fingerprint(cookie_header);
        if self
            .cost_cooldown
            .is_cooling_down(&credential, (self.clock)())
        {
            tracing::debug!("Cursor token-cost events skipped: cooling down after HTTP 403");
            return None;
        }
        match token_cost::fetch_token_cost_report(
            self.api.client(),
            self.api.base_url(),
            cookie_header,
            Some(token_cost::default_since()),
            Some(chrono::Utc::now()),
        )
        .await
        {
            Ok(report) => {
                self.cost_cooldown.clear(&credential);
                Some(report)
            }
            Err(TokenCostError::CostRequestForbidden) => {
                self.cost_cooldown
                    .record_forbidden(&credential, (self.clock)());
                tracing::debug!(
                    "Cursor token-cost events forbidden (HTTP 403); retrying in six hours"
                );
                None
            }
            Err(err) => {
                tracing::debug!("Cursor token-cost events unavailable: {err}");
                None
            }
        }
    }

    fn build_usage_snapshot(
        primary: RateWindow,
        secondary: Option<RateWindow>,
        model_specific: Option<RateWindow>,
        email: Option<String>,
        plan_type: Option<String>,
        token_report: Option<&token_cost::CursorTokenCostReport>,
    ) -> UsageSnapshot {
        let mut usage = UsageSnapshot::new(primary);
        if let Some(sec) = secondary {
            usage = usage.with_secondary(sec);
        }
        if let Some(ms) = model_specific {
            usage = usage.with_model_specific(ms);
        }
        if let Some(e) = email {
            usage = usage.with_email(e);
        }
        if let Some(plan) = plan_type {
            usage = usage.with_login_method(plan);
        }
        if let Some(report) = token_report {
            for window in report.to_extra_windows() {
                usage.extra_rate_windows.push(window);
            }
        }
        usage
    }

    fn build_fetch_result(
        usage: UsageSnapshot,
        cost: Option<CostSnapshot>,
        token_report: Option<&token_cost::CursorTokenCostReport>,
        include_credits: bool,
    ) -> ProviderFetchResult {
        // On-demand / plan cost follows the shared optional-usage setting
        // (`FetchContext.include_credits` ↔ upstream showOptionalCreditsAndExtraUsage).
        let cost = if include_credits {
            token_report
                .and_then(|r| r.merge_into_cost(cost.clone()))
                .or(cost)
        } else {
            None
        };
        let mut result = ProviderFetchResult::new(usage, "web");
        if let Some(c) = cost {
            result = result.with_cost(c);
        }
        result
    }
}

#[cfg(test)]
impl CursorProvider {
    /// Mock-server provider with its own cooldown store and clock.
    fn for_test(
        base_url: &str,
        cost_cooldown: Arc<CostCooldown>,
        clock: Arc<dyn Fn() -> Instant + Send + Sync>,
    ) -> Self {
        Self {
            api: CursorApi::with_base_url(base_url),
            cost_cooldown,
            clock,
        }
    }
}

impl Default for CursorProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CursorProvider {
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    fn id(&self) -> ProviderId {
        ProviderId::Cursor
    }

    fn retains_last_good_on_transport_failure(&self) -> bool {
        true
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Cursor usage via web API");

        match ctx.source_mode {
            // Cli is only ever set by the shell for "no cookie yet"; treat it as
            // web so empty-manual users get browser cookie attempt (or AuthRequired)
            // instead of "Source mode 'Cli' not supported" (#212).
            SourceMode::Auto | SourceMode::Web | SourceMode::Cli => {
                match self.fetch_web_usage(ctx).await {
                    Ok((result, token_report)) => {
                        let api::CursorUsageResult {
                            primary,
                            secondary,
                            model_specific,
                            cost,
                            email,
                            plan_type,
                            grok_bot,
                        } = result;
                        let mut usage = Self::build_usage_snapshot(
                            primary,
                            secondary,
                            model_specific,
                            email,
                            plan_type,
                            token_report.as_ref(),
                        );
                        if let Some(grok_bot) = grok_bot {
                            usage.extra_rate_windows.push(grok_bot);
                        }
                        Ok(Self::build_fetch_result(
                            usage,
                            cost,
                            token_report.as_ref(),
                            ctx.include_credits,
                        ))
                    }
                    Err(e) => {
                        tracing::warn!("Cursor API fetch failed: {}", e);
                        Err(e)
                    }
                }
            }
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(ctx.source_mode)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{FetchContext, LastGoodFailurePolicy};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn cli_mode_does_not_return_unsupported_source() {
        let provider = CursorProvider::new();
        let ctx = FetchContext {
            source_mode: SourceMode::Cli,
            manual_cookie_header: None,
            ..FetchContext::default()
        };
        let err = provider
            .fetch_usage(&ctx)
            .await
            .expect_err("no cookies on this machine");
        // Must not be UnsupportedSource — that was the user-visible #212 bug.
        assert!(
            !matches!(err, ProviderError::UnsupportedSource(_)),
            "unexpected UnsupportedSource: {err}"
        );
        assert!(
            matches!(
                err,
                ProviderError::NoCookies | ProviderError::AuthRequired | ProviderError::Other(_)
            ),
            "expected cookie/auth style error, got: {err}"
        );
    }

    #[tokio::test]
    async fn oauth_mode_still_unsupported() {
        let provider = CursorProvider::new();
        let ctx = FetchContext {
            source_mode: SourceMode::OAuth,
            ..FetchContext::default()
        };
        let err = provider
            .fetch_usage(&ctx)
            .await
            .expect_err("oauth unsupported");
        assert!(matches!(
            err,
            ProviderError::UnsupportedSource(SourceMode::OAuth)
        ));
    }

    #[test]
    fn does_not_advertise_unsupported_credits() {
        let provider = CursorProvider::new();
        assert!(!provider.metadata().supports_credits);
    }

    #[test]
    fn transport_failures_retain_but_authentication_failures_replace() {
        let provider = CursorProvider::new();
        assert_eq!(
            provider.last_good_failure_policy_for_error(&ProviderError::Timeout),
            LastGoodFailurePolicy::Preserve
        );
        assert_eq!(
            provider.last_good_failure_policy_for_error(&ProviderError::AuthRequired),
            LastGoodFailurePolicy::Replace
        );
    }

    #[test]
    fn on_demand_cost_follows_include_credits_setting() {
        let usage = UsageSnapshot::new(RateWindow::new(16.0));
        let cost = CostSnapshot::new(3.5, "USD", "On-demand (billing cycle)").with_limit(10.0);

        let shown =
            CursorProvider::build_fetch_result(usage.clone(), Some(cost.clone()), None, true);
        assert!(
            shown.cost.is_some(),
            "include_credits=true keeps on-demand cost"
        );

        let hidden = CursorProvider::build_fetch_result(usage, Some(cost), None, false);
        assert!(
            hidden.cost.is_none(),
            "include_credits=false hides on-demand extra usage"
        );
    }

    const SUMMARY: &str = r#"{
        "billingCycleStart": "2026-03-01T00:00:00Z",
        "billingCycleEnd": "2026-04-01T00:00:00Z",
        "membershipType": "pro",
        "individualUsage": {
            "plan": { "used": 1500, "limit": 5000, "totalPercentUsed": 30.0 }
        }
    }"#;
    const COOKIE_A: &str = "WorkosCursorSessionToken=user-a%3A%3Atoken-a";
    const COOKIE_B: &str = "WorkosCursorSessionToken=user-b%3A%3Atoken-b";
    const EVENTS_PATH: &str = "/api/dashboard/get-filtered-usage-events";

    struct Harness {
        provider: CursorProvider,
        offset: Arc<AtomicU64>,
    }

    impl Harness {
        fn new(server: &mockito::ServerGuard) -> Self {
            let offset = Arc::new(AtomicU64::new(0));
            let base = Instant::now();
            let clock_offset = Arc::clone(&offset);
            let provider = CursorProvider::for_test(
                &server.url(),
                Arc::new(CostCooldown::default()),
                Arc::new(move || base + Duration::from_secs(clock_offset.load(Ordering::SeqCst))),
            );
            Self { provider, offset }
        }

        fn advance(&self, by: Duration) {
            self.offset.fetch_add(by.as_secs(), Ordering::SeqCst);
        }

        async fn fetch(&self, cookie: &str) -> ProviderFetchResult {
            let ctx = FetchContext {
                source_mode: SourceMode::Web,
                manual_cookie_header: Some(cookie.to_string()),
                ..FetchContext::default()
            };
            self.provider
                .fetch_usage(&ctx)
                .await
                .expect("usage must not depend on the cost request")
        }
    }

    async fn usage_mock(server: &mut mockito::ServerGuard, hits: usize) -> mockito::Mock {
        server
            .mock("GET", "/api/usage-summary")
            .with_status(200)
            .with_body(SUMMARY)
            .expect(hits)
            .create_async()
            .await
    }

    async fn events_mock(
        server: &mut mockito::ServerGuard,
        cookie: &str,
        status: usize,
        hits: usize,
    ) -> mockito::Mock {
        let origin = server.url();
        server
            .mock("POST", EVENTS_PATH)
            .match_header("cookie", cookie)
            .match_header("origin", origin.as_str())
            .with_status(status)
            .with_body("{}")
            .expect(hits)
            .create_async()
            .await
    }

    #[tokio::test]
    async fn forbidden_cost_request_is_not_repeated_inside_the_window() {
        let mut server = mockito::Server::new_async().await;
        let usage = usage_mock(&mut server, 2).await;
        let events = events_mock(&mut server, COOKIE_A, 403, 1).await;
        let harness = Harness::new(&server);

        let first = harness.fetch(COOKIE_A).await;
        harness.advance(Duration::from_secs(5 * 60 * 60));
        let second = harness.fetch(COOKIE_A).await;

        usage.assert_async().await;
        events.assert_async().await;
        assert!((first.usage.primary.used_percent - 30.0).abs() < 0.01);
        assert_eq!(
            first.usage.primary.used_percent,
            second.usage.primary.used_percent
        );
        assert_eq!(
            first.cost.as_ref().map(|cost| cost.used),
            second.cost.as_ref().map(|cost| cost.used),
            "quota and plan cost are unaffected by the cost-only rejection"
        );
        assert!(first.usage.extra_rate_windows.is_empty());
    }

    #[tokio::test]
    async fn forbidden_cost_request_is_retried_after_the_window() {
        let mut server = mockito::Server::new_async().await;
        let usage = usage_mock(&mut server, 3).await;
        let events = events_mock(&mut server, COOKIE_A, 403, 2).await;
        let harness = Harness::new(&server);

        harness.fetch(COOKIE_A).await;
        harness.advance(Duration::from_secs(6 * 60 * 60 - 1));
        harness.fetch(COOKIE_A).await;
        harness.advance(Duration::from_secs(1));
        harness.fetch(COOKIE_A).await;

        usage.assert_async().await;
        events.assert_async().await;
    }

    #[tokio::test]
    async fn a_different_credential_retries_immediately() {
        let mut server = mockito::Server::new_async().await;
        let usage = usage_mock(&mut server, 4).await;
        let events_a = events_mock(&mut server, COOKIE_A, 403, 1).await;
        let events_b = events_mock(&mut server, COOKIE_B, 403, 1).await;
        let harness = Harness::new(&server);

        harness.fetch(COOKIE_A).await;
        harness.fetch(COOKIE_B).await;
        harness.fetch(COOKIE_A).await;
        harness.fetch(COOKIE_B).await;

        usage.assert_async().await;
        events_a.assert_async().await;
        events_b.assert_async().await;
    }

    #[tokio::test]
    async fn transient_cost_failures_keep_the_normal_cadence() {
        let mut server = mockito::Server::new_async().await;
        let usage = usage_mock(&mut server, 2).await;
        let events = events_mock(&mut server, COOKIE_A, 503, 2).await;
        let harness = Harness::new(&server);

        harness.fetch(COOKIE_A).await;
        harness.fetch(COOKIE_A).await;

        usage.assert_async().await;
        events.assert_async().await;
    }

    #[tokio::test]
    async fn successful_cost_request_after_the_window_ends_the_cooldown() {
        let mut server = mockito::Server::new_async().await;
        let usage = usage_mock(&mut server, 3).await;
        let harness = Harness::new(&server);

        let forbidden = events_mock(&mut server, COOKIE_A, 403, 1).await;
        harness.fetch(COOKIE_A).await;
        forbidden.assert_async().await;
        forbidden.remove_async().await;

        harness.advance(cost_cooldown::FORBIDDEN_COST_COOLDOWN + Duration::from_secs(1));
        let allowed = events_mock(&mut server, COOKIE_A, 200, 2).await;
        harness.fetch(COOKIE_A).await;
        harness.fetch(COOKIE_A).await;

        usage.assert_async().await;
        allowed.assert_async().await;
    }
}
