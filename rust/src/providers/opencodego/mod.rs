//! OpenCode Go provider implementation
//!
//! Separate workspace surface that shares the `opencode.ai` cookie domain with
//! the OpenCode provider. Auto prefers local SQLite usage (upstream #2316)
//! unless a workspace override scopes the fetch to web first; Web is cookie
//! scrape only; Cli is local-only.

mod console;
mod legacy;
pub(crate) mod local;
mod usage_api;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use std::time::Duration;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const BASE_URL: &str = "https://opencode.ai";
/// Source label for quota values reconstructed from the device-local SQLite history.
pub const LOCAL_ESTIMATE_SOURCE_LABEL: &str = "local estimate";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

// Upstream 0.48.0 #2583 (F15) optional-Zen-balance bounds.
/// `optionalZenBalanceTimeout`: outer bound of the billing lookup.
const ZEN_BALANCE_TIMEOUT: Duration = Duration::from_secs(5);
/// `optionalZenBalanceStartDelay`: the usage page gets a head start.
const ZEN_BALANCE_START_DELAY: Duration = Duration::from_millis(25);
/// `optionalZenBalanceJoinGrace`: join bound for background/UI reads.
const ZEN_BALANCE_JOIN_GRACE: Duration = Duration::from_millis(250);

pub struct OpenCodeGoProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl OpenCodeGoProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::OpenCodeGo,
                display_name: "OpenCode Go",
                session_label: "5-hour",
                weekly_label: "Weekly",
                supports_opus: true,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://opencode.ai"),
                status_page_url: None,
                tertiary_label_key: Some("ProviderMonthly"),
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn workspace_id_from_context(workspace_id: Option<&str>) -> Option<String> {
        console::normalize_workspace_id(workspace_id)
    }

    async fn fetch_workspace_id(
        client: &Client,
        cookie_header: &str,
    ) -> Result<String, ProviderError> {
        console::fetch_workspace_id(client, cookie_header, Duration::from_secs(30)).await
    }

    /// Upstream `fetchZenBalance`: the dashboard HTML embeds the balance for
    /// some page states; the dedicated billing server-fn report (raw 1e-8 USD
    /// units behind a customerID marker) is the fallback. Optional enrichment
    /// — every failure degrades to `None`, never to a fetch error.
    async fn fetch_zen_balance(
        client: &Client,
        workspace_id: &str,
        cookie_header: &str,
        timeout: Duration,
    ) -> Option<f64> {
        let request_timeout = timeout.min(ZEN_BALANCE_TIMEOUT);
        match console::fetch_balance(client, workspace_id, cookie_header, request_timeout).await {
            Ok(balance) => return balance,
            Err(error) if legacy::can_recover(cookie_header, &error) => {}
            Err(_) => return None,
        }
        legacy::LegacySession::new(client, cookie_header)
            .fetch_balance(request_timeout)
            .await
            .unwrap_or(None)
    }

    /// Spawn the optional Zen balance task (25 ms start delay so the usage
    /// fetch gets the head start, per upstream). Resolves the workspace id
    /// inside the task when no override is pinned.
    fn spawn_zen_balance_task(
        &self,
        cookie_header: &str,
        workspace_id_override: Option<&str>,
        web_timeout: u64,
    ) -> (tokio::task::JoinHandle<Option<f64>>, std::time::Instant) {
        let client = self.client.clone();
        let cookie_header = cookie_header.to_string();
        let workspace_id_override = workspace_id_override.map(str::to_string);
        let timeout = Duration::from_secs(web_timeout.max(1));
        let started_at = std::time::Instant::now();
        let task = tokio::spawn(async move {
            tokio::time::sleep(ZEN_BALANCE_START_DELAY).await;
            let workspace_id = match workspace_id_override {
                Some(id) => id,
                None => match Self::fetch_workspace_id(&client, &cookie_header).await {
                    Ok(id) => id,
                    Err(error) if legacy::can_recover(&cookie_header, &error) => {
                        return legacy::LegacySession::new(&client, &cookie_header)
                            .fetch_balance(timeout.min(ZEN_BALANCE_TIMEOUT))
                            .await
                            .unwrap_or(None);
                    }
                    Err(_) => return None,
                },
            };
            Self::fetch_zen_balance(&client, &workspace_id, &cookie_header, timeout).await
        });
        (task, started_at)
    }

    fn spawn_legacy_balance_task(
        &self,
        cookie_header: &str,
        web_timeout: u64,
    ) -> (tokio::task::JoinHandle<Option<f64>>, std::time::Instant) {
        let client = self.client.clone();
        let cookie_header = cookie_header.to_string();
        let timeout = Duration::from_secs(web_timeout.max(1)).min(ZEN_BALANCE_TIMEOUT);
        let started_at = std::time::Instant::now();
        let task = tokio::spawn(async move {
            tokio::time::sleep(ZEN_BALANCE_START_DELAY).await;
            legacy::LegacySession::new(&client, &cookie_header)
                .fetch_balance(timeout)
                .await
                .unwrap_or(None)
        });
        (task, started_at)
    }

    /// Join the optional Zen balance task within the policy budget. A budget
    /// expiry cancels the in-flight HTTP work instead of leaking it.
    async fn join_zen_balance(
        mut task: tokio::task::JoinHandle<Option<f64>>,
        started_at: std::time::Instant,
        requires_optional_usage_completeness: bool,
    ) -> Option<f64> {
        let budget = zen_balance_join_budget(started_at, requires_optional_usage_completeness);
        match tokio::time::timeout(budget, &mut task).await {
            Ok(Ok(balance)) => balance,
            Ok(Err(_)) | Err(_) => {
                task.abort();
                None
            }
        }
    }

    async fn fetch_with_cookies(
        &self,
        ctx: &FetchContext,
        cookie_header: &str,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let workspace_id = match Self::workspace_id_from_context(ctx.workspace_id.as_deref()) {
            Some(workspace_id) => workspace_id,
            None => match Self::fetch_workspace_id(&self.client, cookie_header).await {
                Ok(workspace_id) => workspace_id,
                Err(console_error) if legacy::can_recover(cookie_header, &console_error) => {
                    return self
                        .fetch_legacy_with_cookies(ctx, cookie_header, console_error)
                        .await;
                }
                Err(error) => return Err(error),
            },
        };
        // F15 (#2583): start the optional Zen balance fetch in parallel with the
        // usage page and bound the join from task creation, so a slow balance
        // still lands in CLI/serve usage reads without stacking a second wait.
        let (zen_task, zen_started) =
            self.spawn_zen_balance_task(cookie_header, Some(&workspace_id), ctx.web_timeout);
        let console_result = console::fetch_usage(
            &self.client,
            &workspace_id,
            cookie_header,
            Duration::from_secs(ctx.web_timeout.max(1)),
        )
        .await;
        let (usage, embedded_balance) = match console_result {
            Ok(console::ConsoleUsage::Snapshot(usage)) => (*usage, None),
            Ok(console::ConsoleUsage::NoSubscription) => {
                zen_task.abort();
                return Err(ProviderError::Parse(
                    "No OpenCode Go subscription is available".to_string(),
                ));
            }
            Err(console_error) if legacy::can_recover(cookie_header, &console_error) => {
                let legacy_result = legacy::LegacySession::new(&self.client, cookie_header)
                    .fetch_usage()
                    .await
                    .map(|result| (result.usage, result.embedded_balance));
                match legacy::select_result(cookie_header, console_error, legacy_result) {
                    Ok(result) => result,
                    Err(error) => {
                        zen_task.abort();
                        return Err(error);
                    }
                }
            }
            Err(error) => {
                zen_task.abort();
                return Err(error);
            }
        };
        let balance = match embedded_balance {
            Some(balance) => {
                zen_task.abort();
                Some(balance)
            }
            None => {
                Self::join_zen_balance(
                    zen_task,
                    zen_started,
                    ctx.requires_optional_usage_completeness,
                )
                .await
            }
        };
        Ok(Self::with_zen_balance(usage, "web", balance))
    }

    async fn fetch_legacy_with_cookies(
        &self,
        ctx: &FetchContext,
        cookie_header: &str,
        console_error: ProviderError,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let (zen_task, zen_started) =
            self.spawn_legacy_balance_task(cookie_header, ctx.web_timeout);
        let legacy_result = legacy::LegacySession::new(&self.client, cookie_header)
            .fetch_usage()
            .await;
        let result = match legacy::select_result(cookie_header, console_error, legacy_result) {
            Ok(result) => result,
            Err(error) => {
                zen_task.abort();
                return Err(error);
            }
        };
        let balance = match result.embedded_balance {
            Some(balance) => {
                zen_task.abort();
                Some(balance)
            }
            None => {
                Self::join_zen_balance(
                    zen_task,
                    zen_started,
                    ctx.requires_optional_usage_completeness,
                )
                .await
            }
        };
        Ok(Self::with_zen_balance(result.usage, "web", balance))
    }

    /// Attach an optional Zen balance to the snapshot: informational extra
    /// window plus the cost row (existing bridge shape).
    fn with_zen_balance(
        mut usage: UsageSnapshot,
        source: &str,
        balance: Option<f64>,
    ) -> ProviderFetchResult {
        let Some(balance) = balance else {
            return ProviderFetchResult::new(usage, source);
        };
        usage = usage.with_extra_rate_window(
            "zen-balance",
            "Zen balance",
            RateWindow::with_details(0.0, None, None, Some(format!("${balance:.2}"))),
        );
        ProviderFetchResult::new(usage, source).with_cost(CostSnapshot::new(
            balance,
            "USD",
            "Zen balance",
        ))
    }
}

impl Default for OpenCodeGoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OpenCodeGoProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenCodeGo
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching OpenCode Go usage");

        match ctx.source_mode {
            SourceMode::Auto => {
                // Local-first unless workspace/token scope asks for web first
                // (manual cookie source is already mapped to Web by the shell).
                if Self::auto_prefers_web_first(ctx) {
                    match self.fetch_web(ctx).await {
                        Ok(result) => return Ok(result),
                        Err(e) if Self::web_error_allows_local_fallback(ctx, &e) => {
                            tracing::debug!(
                                "OpenCode Go web failed in scoped Auto; trying local: {e}"
                            );
                        }
                        Err(e) => return Err(e),
                    }
                    return self.fetch_local_with_balance(ctx).await;
                }

                match self.fetch_local_with_balance(ctx).await {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        tracing::debug!("OpenCode Go local failed in Auto; trying API/web: {e}");
                    }
                }
                if let Some(api_key) = usage_api::resolve_api_key(ctx) {
                    match usage_api::fetch(&self.client, ctx, &api_key, "api").await {
                        Ok(result) => return Ok(result),
                        Err(e) => {
                            tracing::debug!("OpenCode Go API failed in Auto; trying web: {e}")
                        }
                    }
                }
                self.fetch_web(ctx).await
            }
            SourceMode::Web => self.fetch_web(ctx).await,
            SourceMode::Cli => self.fetch_local_with_balance(ctx).await,
            SourceMode::OAuth => {
                let api_key = usage_api::resolve_api_key(ctx).ok_or_else(|| {
                    ProviderError::NotInstalled(
                        "Missing OpenCode Go API key. Add one in Settings or set OPENCODE_API_KEY."
                            .to_string(),
                    )
                })?;
                usage_api::fetch(&self.client, ctx, &api_key, "api").await
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::Cli]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        true
    }
}

impl OpenCodeGoProvider {
    /// Auto prefers web when a workspace override or active token-account scope
    /// is present (upstream `requiresScopedWebStrategy`).
    fn auto_prefers_web_first(ctx: &FetchContext) -> bool {
        if ctx
            .workspace_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
        {
            return true;
        }
        // Shell sets this when a token account is active for cookie/web scope.
        ctx.auto_prefer_web
    }

    fn web_error_allows_local_fallback(ctx: &FetchContext, err: &ProviderError) -> bool {
        // Upstream 0.53: an explicitly selected/manual token is an account
        // choice. Never mask that account's auth failure with account-agnostic
        // local SQLite estimates. Workspace-only scoping may still fall back.
        if matches!(err, ProviderError::AuthRequired | ProviderError::NoCookies)
            && (ctx.manual_cookie_header.is_some() || ctx.auto_prefer_web)
        {
            return false;
        }
        matches!(
            err,
            ProviderError::AuthRequired
                | ProviderError::NoCookies
                | ProviderError::Timeout
                | ProviderError::Network(_)
                | ProviderError::Parse(_)
                | ProviderError::Other(_)
        )
    }

    /// Local SQLite snapshot plus the optional bounded Zen balance enrichment
    /// (upstream #2583 waits for the balance in usage-snapshot reads too, not
    /// just web reads; cookie absence or a slow/broken billing lookup degrades
    /// to no balance, never to an error).
    async fn fetch_local_with_balance(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let snap = local::fetch_local_usage(Utc::now())?;
        let mut result = snap.to_fetch_result();
        if let Some(api_key) = usage_api::resolve_api_key(ctx)
            && let Ok(api_result) = usage_api::fetch(&self.client, ctx, &api_key, "local+api").await
        {
            result = api_result;
        }
        if !ctx.include_credits {
            return Ok(result);
        }
        let cookie_header = match ctx.manual_cookie_header.clone() {
            Some(header) => Some(header),
            None => crate::providers::browser_cookie_header(&["opencode.ai"]).ok(),
        };
        let Some(cookie_header) = cookie_header else {
            return Ok(result);
        };
        let (task, started) = self.spawn_zen_balance_task(
            &cookie_header,
            ctx.workspace_id.as_deref(),
            ctx.web_timeout,
        );
        if let Some(balance) =
            Self::join_zen_balance(task, started, ctx.requires_optional_usage_completeness).await
        {
            result =
                Self::with_zen_balance(result.usage, &result.source_label.clone(), Some(balance));
        }
        Ok(result)
    }

    async fn fetch_web(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        if let Some(cookie_header) = &ctx.manual_cookie_header {
            return self.fetch_with_cookies(ctx, cookie_header).await;
        }

        match crate::providers::browser_cookie_header(&["opencode.ai"]) {
            Ok(cookie_header) => self.fetch_with_cookies(ctx, &cookie_header).await,
            Err(ProviderError::NoCookies) => Err(ProviderError::AuthRequired),
            Err(e) => Err(e),
        }
    }
}

// ── F15 helpers ─────────────────────────────────────────────────────────────

/// The optional-balance join bound, measured from task creation (upstream
/// `optionalZenBalanceJoinTimeout`): usage-completeness reads get the remainder
/// of the 5 s optional-balance budget so a slow usage fetch cannot stack a
/// second full wait; background reads keep the short join grace.
fn zen_balance_join_budget(
    started_at: std::time::Instant,
    requires_optional_usage_completeness: bool,
) -> Duration {
    if !requires_optional_usage_completeness {
        return ZEN_BALANCE_JOIN_GRACE;
    }
    ZEN_BALANCE_TIMEOUT.saturating_sub(started_at.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Some(42.5)
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
            Some(42.5)
        });
        let balance = OpenCodeGoProvider::join_zen_balance(task, started, true).await;
        assert_eq!(balance, Some(42.5));
    }
}
