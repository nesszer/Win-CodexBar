//! Session enrichment for the local estimate (upstream #4410 / #4445).
//!
//! Without an authoritative API result, Auto overlays Console quota from a
//! provided session or, when none is provided, from a browser session found
//! with the shared browser order. A missing, blocked, or rejected discovered
//! session keeps the local estimate. Cookie source Off (signalled by the shell
//! as `manual_cookie_missing`) never reads a browser.

use std::sync::Arc;

use super::transport::WebTransport;
use super::{OpenCodeGoProvider, WebCookieSession};
use crate::core::{FetchContext, ProviderError, ProviderFetchResult};

pub(super) const LOCAL_WEB_SOURCE_LABEL: &str = "local+web";
pub(super) const COOKIE_DOMAINS: &[&str] = &["opencode.ai"];

/// Where the web session for an overlay or Zen balance comes from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SessionCookie {
    /// A stored manual cookie or a selected token account's cookie.
    Provided(String),
    /// No session was provided; the browsers may be read.
    Discover,
    /// The user turned cookies off; no browser may be read.
    Disabled,
}

impl SessionCookie {
    pub(super) fn for_context(ctx: &FetchContext) -> Self {
        match ctx
            .manual_cookie_header
            .as_deref()
            .map(str::trim)
            .filter(|header| !header.is_empty())
        {
            Some(header) => Self::Provided(header.to_string()),
            None if ctx.manual_cookie_missing => Self::Disabled,
            None => Self::Discover,
        }
    }

    /// Resolve to a header. `discover` runs only for [`Self::Discover`].
    pub(super) fn resolve(
        self,
        discover: impl FnOnce() -> Result<String, ProviderError>,
    ) -> Option<String> {
        match self {
            Self::Provided(header) => Some(header),
            Self::Disabled => None,
            Self::Discover => match discover() {
                Ok(header) if !header.trim().is_empty() => Some(header),
                Ok(_) => None,
                Err(error) => {
                    tracing::debug!("OpenCode Go browser session discovery found none: {error}");
                    None
                }
            },
        }
    }
}

pub(super) fn discover_browser_session() -> Result<String, ProviderError> {
    crate::providers::browser_cookie_header(COOKIE_DOMAINS)
}

impl OpenCodeGoProvider {
    /// Overlay authoritative Console quota onto the local estimate. Errors never
    /// escape: a rejected session keeps the estimate, and any other failure
    /// still tries the optional Zen balance with the same session.
    pub(super) async fn overlay_local_with_session<T: WebTransport>(
        ctx: &FetchContext,
        local: ProviderFetchResult,
        cookie_header: Option<String>,
        transport: Arc<T>,
    ) -> ProviderFetchResult {
        let Some(cookie_header) = cookie_header else {
            return local;
        };
        match Self::fetch_with_transport(ctx, &cookie_header, Arc::clone(&transport)).await {
            Ok(mut web) => {
                web.source_label = LOCAL_WEB_SOURCE_LABEL.to_string();
                web
            }
            Err(ProviderError::AuthRequired | ProviderError::NoCookies) => local,
            Err(error) => {
                tracing::debug!(
                    "OpenCode Go session overlay unavailable; keeping estimate: {error}"
                );
                if ctx.include_credits {
                    Self::attach_zen_balance(ctx, local, &cookie_header, transport).await
                } else {
                    local
                }
            }
        }
    }

    /// Attach the optional Zen balance without changing the result's source or
    /// pace authority.
    pub(super) async fn attach_zen_balance<T: WebTransport>(
        ctx: &FetchContext,
        result: ProviderFetchResult,
        cookie_header: &str,
        transport: Arc<T>,
    ) -> ProviderFetchResult {
        let cookies = WebCookieSession::new(cookie_header);
        let (task, started) = Self::spawn_zen_balance_task(
            Arc::clone(&transport),
            &cookies,
            ctx.workspace_id.as_deref(),
            ctx.web_timeout,
        );
        let Some(balance) = Self::finish_zen_balance(
            transport,
            &cookies,
            task,
            started,
            ctx.requires_optional_usage_completeness,
        )
        .await
        else {
            return result;
        };
        let pace_authoritative = result.pace_authoritative;
        let mut enriched =
            Self::with_zen_balance(result.usage, &result.source_label, Some(balance));
        enriched.pace_authoritative = pace_authoritative;
        enriched
    }
}

#[cfg(test)]
mod tests {
    use super::super::{console, legacy};
    use super::*;
    use crate::core::{RateWindow, UsageSnapshot};
    use async_trait::async_trait;
    use std::cell::Cell;
    use std::sync::Mutex;
    use std::time::Duration;

    const CONSOLE_COOKIE: &str = "__Host-console_session=console";

    enum UsageOutcome {
        Quota(f64),
        Rejected,
        Unavailable,
    }

    struct FakeConsole {
        usage: UsageOutcome,
        balance: Option<f64>,
        usage_cookies: Mutex<Vec<String>>,
    }

    impl FakeConsole {
        fn new(usage: UsageOutcome, balance: Option<f64>) -> Arc<Self> {
            Arc::new(Self {
                usage,
                balance,
                usage_cookies: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl WebTransport for FakeConsole {
        type LegacySession = ();

        async fn fetch_workspace_id(
            &self,
            _cookie_header: &str,
            _timeout: Duration,
        ) -> Result<String, ProviderError> {
            Ok("wrk_discovered".to_string())
        }

        async fn fetch_console_usage(
            &self,
            _workspace_id: &str,
            cookie_header: &str,
            _timeout: Duration,
        ) -> Result<console::ConsoleUsage, ProviderError> {
            self.usage_cookies
                .lock()
                .unwrap()
                .push(cookie_header.to_string());
            match self.usage {
                UsageOutcome::Quota(percent) => Ok(console::ConsoleUsage::Snapshot(Box::new(
                    UsageSnapshot::new(RateWindow::with_details(percent, Some(300), None, None)),
                ))),
                UsageOutcome::Rejected => Err(ProviderError::AuthRequired),
                UsageOutcome::Unavailable => {
                    Err(ProviderError::Other("console unavailable".to_string()))
                }
            }
        }

        async fn fetch_console_balance(
            &self,
            _workspace_id: &str,
            _cookie_header: &str,
            _timeout: Duration,
        ) -> Result<Option<f64>, ProviderError> {
            Ok(self.balance)
        }

        async fn resolve_legacy_session(
            &self,
            _cookies: &WebCookieSession,
        ) -> Result<Self::LegacySession, ProviderError> {
            panic!("console-only sessions never resolve a legacy session")
        }

        async fn fetch_legacy_usage(
            &self,
            _session: &Self::LegacySession,
        ) -> Result<legacy::LegacyUsage, ProviderError> {
            panic!("console-only sessions never fetch legacy usage")
        }

        async fn fetch_legacy_balance(
            &self,
            _session: &Self::LegacySession,
            _timeout: Duration,
        ) -> Result<Option<f64>, ProviderError> {
            panic!("console-only sessions never fetch a legacy balance")
        }
    }

    fn local_estimate() -> ProviderFetchResult {
        ProviderFetchResult::new(
            UsageSnapshot::new(RateWindow::with_details(10.0, Some(300), None, None)),
            super::super::LOCAL_ESTIMATE_SOURCE_LABEL,
        )
        .with_non_authoritative_pace()
    }

    fn completeness_ctx() -> FetchContext {
        FetchContext {
            web_timeout: 1,
            requires_optional_usage_completeness: true,
            ..FetchContext::default()
        }
    }

    #[test]
    fn provided_session_wins_over_discovery_and_off_never_discovers() {
        let discovered = Cell::new(0);
        let discover = || {
            discovered.set(discovered.get() + 1);
            Ok("from=browser".to_string())
        };

        let provided = FetchContext {
            manual_cookie_header: Some("  from=manual ".to_string()),
            manual_cookie_missing: true,
            ..FetchContext::default()
        };
        assert_eq!(
            SessionCookie::for_context(&provided),
            SessionCookie::Provided("from=manual".to_string())
        );
        assert_eq!(
            SessionCookie::for_context(&provided).resolve(discover),
            Some("from=manual".to_string())
        );

        let off = FetchContext {
            manual_cookie_header: Some("   ".to_string()),
            manual_cookie_missing: true,
            ..FetchContext::default()
        };
        assert_eq!(SessionCookie::for_context(&off), SessionCookie::Disabled);
        assert_eq!(SessionCookie::for_context(&off).resolve(discover), None);
        assert_eq!(discovered.get(), 0);

        let auto = FetchContext::default();
        assert_eq!(SessionCookie::for_context(&auto), SessionCookie::Discover);
        assert_eq!(
            SessionCookie::for_context(&auto).resolve(discover),
            Some("from=browser".to_string())
        );
        assert_eq!(discovered.get(), 1);
    }

    #[test]
    fn missing_or_blocked_browser_sessions_resolve_to_none() {
        assert_eq!(
            SessionCookie::Discover.resolve(|| Err(ProviderError::NoCookies)),
            None
        );
        assert_eq!(
            SessionCookie::Discover
                .resolve(|| Err(ProviderError::Other("App-Bound Encryption".to_string()))),
            None
        );
        assert_eq!(
            SessionCookie::Discover.resolve(|| Ok(" ".to_string())),
            None
        );
    }

    #[tokio::test]
    async fn discovered_session_overlays_authoritative_console_quota() {
        let transport = FakeConsole::new(UsageOutcome::Quota(42.0), Some(7.5));
        let result = OpenCodeGoProvider::overlay_local_with_session(
            &completeness_ctx(),
            local_estimate(),
            Some(CONSOLE_COOKIE.to_string()),
            Arc::clone(&transport),
        )
        .await;

        assert_eq!(result.source_label, LOCAL_WEB_SOURCE_LABEL);
        assert!(result.pace_authoritative);
        assert_eq!(result.usage.primary.used_percent, 42.0);
        assert_eq!(result.cost.unwrap().used, 7.5);
        assert_eq!(
            transport.usage_cookies.lock().unwrap().as_slice(),
            [CONSOLE_COOKIE]
        );
    }

    #[tokio::test]
    async fn rejected_session_keeps_the_local_estimate() {
        let transport = FakeConsole::new(UsageOutcome::Rejected, Some(7.5));
        let result = OpenCodeGoProvider::overlay_local_with_session(
            &completeness_ctx(),
            local_estimate(),
            Some(CONSOLE_COOKIE.to_string()),
            transport,
        )
        .await;

        assert_eq!(
            result.source_label,
            super::super::LOCAL_ESTIMATE_SOURCE_LABEL
        );
        assert!(!result.pace_authoritative);
        assert_eq!(result.usage.primary.used_percent, 10.0);
        assert!(result.cost.is_none());
    }

    #[tokio::test]
    async fn unavailable_console_keeps_the_estimate_with_zen_balance() {
        let transport = FakeConsole::new(UsageOutcome::Unavailable, Some(7.5));
        let result = OpenCodeGoProvider::overlay_local_with_session(
            &completeness_ctx(),
            local_estimate(),
            Some(CONSOLE_COOKIE.to_string()),
            transport,
        )
        .await;

        assert_eq!(
            result.source_label,
            super::super::LOCAL_ESTIMATE_SOURCE_LABEL
        );
        assert!(!result.pace_authoritative, "estimate must stay estimated");
        assert_eq!(result.usage.primary.used_percent, 10.0);
        assert_eq!(result.cost.unwrap().used, 7.5);
    }

    #[tokio::test]
    async fn no_session_keeps_the_estimate_without_web_requests() {
        let transport = FakeConsole::new(UsageOutcome::Quota(42.0), Some(7.5));
        let result = OpenCodeGoProvider::overlay_local_with_session(
            &completeness_ctx(),
            local_estimate(),
            None,
            Arc::clone(&transport),
        )
        .await;

        assert_eq!(
            result.source_label,
            super::super::LOCAL_ESTIMATE_SOURCE_LABEL
        );
        assert!(transport.usage_cookies.lock().unwrap().is_empty());
    }
}
