//! Kimi web (`kimi.com`) cookie auth and the web-token resolution chain.
//!
//! Upstream 0.48.0 policies ported here (#2623 / `KimiBrowserImportPolicy`):
//! browser cookie import — and reading the Kimi Desktop session store — are
//! disabled when the Kimi cookie source is `off`. The shared token chain is
//! manual cookie header → Kimi Desktop session → browser cookie import →
//! Chromium local-storage `access_token` (upstream `KimiWebEnrichmentTokenResolver`
//! and #3923), used both by the web fetch itself and
//! by the Code-API/CLI monthly enrichment.

use reqwest::Client;

use super::desktop_token::KimiDesktopAuthToken;
use super::local_storage::local_storage_tokens;
use super::{
    KIMI_SUBSCRIPTION_SERVICE, KIMI_SUBSCRIPTION_STATS_SERVICE, KIMI_WEB_USAGE_SERVICE,
    KimiProvider, KimiRegion, KimiSubscriptionResponse, KimiSubscriptionStatsResponse,
    KimiWebUsageResponse, apply_subscription_windows, kimi_web_post,
};
use crate::browser::cookies::get_cookie_header;
use crate::core::{ProviderError, ProviderId, UsageSnapshot};

/// Persisted Kimi cookie-source value ("manual" default; matches the
/// `claude`/settings convention of a fresh read at fetch time).
pub(crate) fn cookie_source() -> String {
    crate::settings::Settings::load()
        .cookie_source(ProviderId::Kimi)
        .to_string()
}

/// Upstream `KimiBrowserImportPolicy.allowsImport`: automatic discovery is
/// allowed only when the user selected the automatic source.
fn browser_import_allowed(cookie_source: &str) -> bool {
    cookie_source.eq_ignore_ascii_case("auto") || cookie_source.eq_ignore_ascii_case("browser")
}

fn browser_import_error(cookie_source: &str) -> ProviderError {
    let message = if cookie_source.eq_ignore_ascii_case("manual") {
        "Kimi cookie source is Manual; provide a valid manual cookie header."
    } else {
        "Kimi cookie source is Off; provide a manual cookie header or enable browser import."
    };
    ProviderError::Other(message.into())
}

/// A failed web fetch. `had_token` records whether web auth had a token to
/// send (upstream `KimiWebFetchStrategy.isAvailable`); Auto mode reports an
/// earlier CLI failure only when web auth had nothing to try.
#[derive(Debug)]
pub(super) struct WebFetchFailure {
    pub(super) error: ProviderError,
    pub(super) had_token: bool,
}

impl WebFetchFailure {
    fn after_token(error: ProviderError) -> Self {
        Self {
            error,
            had_token: true,
        }
    }
}

/// Web auth token chain for both the web fetch and the Code-API enrichment
/// (upstream `KimiWebEnrichmentTokenResolver.resolve`):
/// 1. Manual cookie header (its `kimi-auth`/auth cookie), source-independent.
/// 2. Kimi Desktop session token (automatic source only).
/// 3. Browser cookie import (automatic source only).
/// 4. Chromium local-storage `access_token` for the region (automatic source only).
pub(crate) fn web_auth_tokens(manual_header: Option<&str>, region: KimiRegion) -> Vec<String> {
    resolve_web_tokens(&WebTokenInput {
        manual_header,
        cookie_source: &cookie_source(),
        region,
        desktop_token: KimiDesktopAuthToken::load_for_region,
        browser_token: browser_auth_token,
        local_storage_tokens,
    })
    .into_iter()
    .map(|candidate| candidate.token)
    .collect()
}

struct WebTokenInput<'a> {
    manual_header: Option<&'a str>,
    cookie_source: &'a str,
    region: KimiRegion,
    desktop_token: fn(KimiRegion) -> Option<String>,
    browser_token: fn(KimiRegion) -> Option<String>,
    local_storage_tokens: fn(KimiRegion) -> Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebTokenSource {
    Manual,
    Desktop,
    Browser,
    LocalStorage,
}

#[derive(PartialEq, Eq)]
struct WebTokenCandidate {
    token: String,
    source: WebTokenSource,
}

impl std::fmt::Debug for WebTokenCandidate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebTokenCandidate")
            .field("token", &"[REDACTED]")
            .field("source", &self.source)
            .finish()
    }
}

fn resolve_web_tokens(input: &WebTokenInput<'_>) -> Vec<WebTokenCandidate> {
    if let Some(header) = input.manual_header
        && let Ok(token) = KimiProvider::auth_token_from_cookie_header(header)
    {
        return vec![WebTokenCandidate {
            token,
            source: WebTokenSource::Manual,
        }];
    }
    if !browser_import_allowed(input.cookie_source) {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(token) = (input.desktop_token)(input.region)
        && seen.insert(token.clone())
    {
        candidates.push(WebTokenCandidate {
            token,
            source: WebTokenSource::Desktop,
        });
    }
    if let Some(token) = (input.browser_token)(input.region)
        && seen.insert(token.clone())
    {
        candidates.push(WebTokenCandidate {
            token,
            source: WebTokenSource::Browser,
        });
    }
    for token in (input.local_storage_tokens)(input.region) {
        if seen.insert(token.clone()) {
            candidates.push(WebTokenCandidate {
                token,
                source: WebTokenSource::LocalStorage,
            });
        }
    }
    candidates
}

/// Browser import only: the first usable `kimi-auth`-class token from any of
/// the registered Kimi cookie domains.
fn browser_auth_token(region: KimiRegion) -> Option<String> {
    region
        .cookie_domains()
        .iter()
        .find_map(|domain| {
            get_cookie_header(domain)
                .ok()
                .filter(|header| !header.is_empty())
        })
        .and_then(|header| KimiProvider::auth_token_from_cookie_header(&header).ok())
}

/// Fetch usage via Kimi web API (weekly quota + rate limit + subscription).
pub(crate) async fn fetch_via_web(
    cookie_header: Option<&str>,
    region: KimiRegion,
    account_isolated: bool,
) -> Result<UsageSnapshot, ProviderError> {
    fetch_web_session_isolated(cookie_header, region, account_isolated)
        .await
        .map_err(|failure| failure.error)
}

/// [`fetch_via_web`] entry point. A selected token account is an identity
/// boundary: fetch only its own session cookie and never fall back to another
/// ambient credential or browser account (upstream per-provider token account
/// routing).
pub(super) async fn fetch_web_session_isolated(
    cookie_header: Option<&str>,
    region: KimiRegion,
    account_isolated: bool,
) -> Result<UsageSnapshot, WebFetchFailure> {
    // An explicitly selected token account is an identity boundary: fetch only
    // its own session cookie and never fall back to another ambient credential
    // or browser account (upstream per-provider token account routing).
    if account_isolated {
        let token = match selected_account_auth_token(cookie_header) {
            Ok(token) => token,
            Err(error) => {
                return Err(WebFetchFailure {
                    error,
                    had_token: false,
                });
            }
        };
        let http = match client() {
            Ok(http) => http,
            Err(error) => {
                return Err(WebFetchFailure {
                    error,
                    had_token: false,
                });
            }
        };
        return fetch_via_web_token(&http, &token, region)
            .await
            .map_err(WebFetchFailure::after_token);
    }
    fetch_web_session(cookie_header, region).await
}

/// [`fetch_via_web`], also reporting whether web auth had a token to try.
pub(super) async fn fetch_web_session(
    cookie_header: Option<&str>,
    region: KimiRegion,
) -> Result<UsageSnapshot, WebFetchFailure> {
    let source = cookie_source();
    let input = WebTokenInput {
        manual_header: cookie_header,
        cookie_source: &source,
        region,
        desktop_token: KimiDesktopAuthToken::load_for_region,
        browser_token: browser_auth_token,
        local_storage_tokens: crate::providers::kimi::local_storage::local_storage_tokens,
    };
    // One HTTP client for every token attempt, built on first use.
    let mut shared_client: Option<Client> = None;
    fetch_with_web_tokens(&input, |token| {
        let http = match &shared_client {
            Some(http) => Ok(http.clone()),
            None => client().inspect(|http| shared_client = Some(http.clone())),
        };
        async move { fetch_via_web_token(&http?, &token, region).await }
    })
    .await
}

/// The web fetch over the token chain. An explicit manual token is
/// authoritative; otherwise (automatic source only) the Kimi Desktop session
/// is tried, then browser import. Only a server rejection moves on to the
/// next automatic token.
async fn fetch_with_web_tokens<F, Fut>(
    input: &WebTokenInput<'_>,
    mut fetch: F,
) -> Result<UsageSnapshot, WebFetchFailure>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<UsageSnapshot, ProviderError>>,
{
    if let Some(token) = input
        .manual_header
        .and_then(|header| KimiProvider::auth_token_from_cookie_header(header).ok())
    {
        // An explicit manual credential is authoritative. A rejected manual
        // token must not silently switch accounts underneath the user.
        return fetch(token).await.map_err(WebFetchFailure::after_token);
    }

    if !browser_import_allowed(input.cookie_source) {
        return Err(WebFetchFailure {
            error: browser_import_error(input.cookie_source),
            had_token: false,
        });
    }

    // Read and try the desktop session first. Browser cookies are intentionally
    // read only after the server rejects this automatic session, so a healthy
    // desktop account never causes another credential store to be touched.
    let desktop_token = (input.desktop_token)(input.region);
    if let Some(token) = desktop_token.clone() {
        match fetch(token).await {
            Ok(usage) => return Ok(usage),
            Err(ProviderError::AuthRequired) => {}
            Err(error) => return Err(WebFetchFailure::after_token(error)),
        }
    }

    let browser_token =
        (input.browser_token)(input.region).filter(|token| desktop_token.as_ref() != Some(token));
    if let Some(token) = browser_token.clone() {
        match fetch(token).await {
            Ok(usage) => return Ok(usage),
            Err(ProviderError::AuthRequired) => {}
            Err(error) => return Err(WebFetchFailure::after_token(error)),
        }
    }

    // Local-storage tokens are read last, only after every cookie source
    // was rejected.
    let mut seen = std::collections::HashSet::new();
    if let Some(token) = desktop_token.clone() {
        seen.insert(token);
    }
    if let Some(token) = browser_token.clone() {
        seen.insert(token);
    }
    for candidate in resolve_web_tokens(input).into_iter().filter(|candidate| {
        candidate.source == WebTokenSource::LocalStorage && seen.insert(candidate.token.clone())
    }) {
        match fetch(candidate.token).await {
            Ok(usage) => return Ok(usage),
            Err(ProviderError::AuthRequired) => {}
            Err(error) => return Err(WebFetchFailure::after_token(error)),
        }
    }

    Err(WebFetchFailure {
        error: ProviderError::AuthRequired,
        had_token: desktop_token.is_some()
            || browser_token.is_some()
            || (input.local_storage_tokens)(input.region)
                .iter()
                .any(|token| !seen.contains(token)),
    })
}

fn selected_account_auth_token(cookie_header: Option<&str>) -> Result<String, ProviderError> {
    cookie_header
        .and_then(|header| KimiProvider::auth_token_from_cookie_header(header).ok())
        .ok_or(ProviderError::AuthRequired)
}

pub(super) fn client() -> Result<reqwest::Client, ProviderError> {
    crate::core::credentialed_http_client_builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ProviderError::Other(e.to_string()))
}

async fn fetch_via_web_token(
    client: &reqwest::Client,
    token: &str,
    region: KimiRegion,
) -> Result<UsageSnapshot, ProviderError> {
    let usage_url = region.web_api_url(KIMI_WEB_USAGE_SERVICE);
    let resp = kimi_web_post(
        client,
        &usage_url,
        region,
        token,
        serde_json::json!({ "scope": ["FEATURE_CODING"] }),
    )
    .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        return Err(ProviderError::Other(format!("API error: {}", status)));
    }

    let usage: KimiWebUsageResponse = resp
        .json()
        .await
        .map_err(|e| ProviderError::Parse(e.to_string()))?;

    let (subscription, plan_name) = fetch_subscription_details(client, token, region).await;

    snapshot_from_web_usage_response_with_plan(usage, subscription, plan_name)
}

const SUBSCRIPTION_ENRICHMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

async fn fetch_subscription_details(
    client: &reqwest::Client,
    token: &str,
    region: KimiRegion,
) -> (Option<KimiSubscriptionStatsResponse>, Option<String>) {
    // The quota statistics and the optional title are independent. Keep a
    // completed statistics response when the plan endpoint is slow or absent.
    let stats = tokio::time::timeout(
        SUBSCRIPTION_ENRICHMENT_TIMEOUT,
        fetch_subscription_for_enrichment(client, token, region),
    );
    let plan = tokio::time::timeout(
        SUBSCRIPTION_ENRICHMENT_TIMEOUT,
        fetch_subscription_plan(client, token, region),
    );
    let (stats, plan) = tokio::join!(stats, plan);
    (stats.ok().flatten(), plan.ok().flatten())
}

pub(super) fn snapshot_from_web_usage_response(
    response: KimiWebUsageResponse,
    subscription: Option<KimiSubscriptionStatsResponse>,
) -> Result<UsageSnapshot, ProviderError> {
    snapshot_from_web_usage_response_with_plan(response, subscription, None)
}

fn snapshot_from_web_usage_response_with_plan(
    response: KimiWebUsageResponse,
    subscription: Option<KimiSubscriptionStatsResponse>,
    plan_name: Option<String>,
) -> Result<UsageSnapshot, ProviderError> {
    let coding = response
        .usages
        .into_iter()
        .find(|usage| usage.scope == "FEATURE_CODING")
        .ok_or_else(|| ProviderError::Parse("Kimi FEATURE_CODING usage missing".into()))?;
    let primary = KimiProvider::rate_window_from_usage_detail(&coding.detail, Some(10080))?;
    let mut usage = UsageSnapshot::new(primary).with_login_method("Kimi");

    if let Some(limit) = coding.limits.unwrap_or_default().into_iter().next() {
        let window_minutes = limit.window.as_ref().and_then(super::kimi_window_minutes);
        let rate_limit =
            KimiProvider::rate_window_from_usage_detail(&limit.detail, window_minutes)?;
        usage = usage.with_secondary(rate_limit);
    }

    if let Some(subscription) = subscription.as_ref() {
        usage = apply_subscription_windows(usage, subscription);
    }
    if let Some(plan_name) = plan_name {
        usage = usage.with_login_method(plan_name);
    }

    Ok(usage)
}

pub(super) async fn fetch_subscription_plan(
    client: &Client,
    token: &str,
    region: KimiRegion,
) -> Option<String> {
    let url = region.web_api_url(KIMI_SUBSCRIPTION_SERVICE);
    match kimi_web_post(client, &url, region, token, serde_json::json!({})).await {
        Ok(response) if response.status().is_success() => response
            .json::<KimiSubscriptionResponse>()
            .await
            .ok()
            .and_then(|response| response.plan_name()),
        _ => None,
    }
}

// Kept for `code_api`: resolve the subscription stats snapshot with a web
// token; any failure means "no enrichment", never an error.
pub(super) async fn fetch_subscription_for_enrichment(
    client: &Client,
    token: &str,
    region: KimiRegion,
) -> Option<KimiSubscriptionStatsResponse> {
    fetch_subscription_for_enrichment_result(client, token, region)
        .await
        .ok()
        .flatten()
}

pub(super) async fn fetch_subscription_for_enrichment_result(
    client: &Client,
    token: &str,
    region: KimiRegion,
) -> Result<Option<KimiSubscriptionStatsResponse>, ProviderError> {
    let url = region.web_api_url(KIMI_SUBSCRIPTION_STATS_SERVICE);
    match kimi_web_post(client, &url, region, token, serde_json::json!({})).await {
        Ok(response) if response.status().is_success() => response
            .json()
            .await
            .map(Some)
            .map_err(|error| ProviderError::Parse(error.to_string())),
        Ok(response) if response.status().as_u16() == 401 || response.status().as_u16() == 403 => {
            Err(ProviderError::AuthRequired)
        }
        Ok(_) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_session_rejects_missing_or_invalid_cookie_without_fallback() {
        assert!(matches!(
            selected_account_auth_token(None),
            Err(ProviderError::AuthRequired)
        ));
        assert!(matches!(
            selected_account_auth_token(Some("locale=en-US")),
            Err(ProviderError::AuthRequired)
        ));
        assert_eq!(
            selected_account_auth_token(Some("Cookie: kimi-auth=selected")).unwrap(),
            "selected"
        );
    }

    fn static_desktop(_: KimiRegion) -> Option<String> {
        Some("desktop-token".to_string())
    }

    fn static_browser(_: KimiRegion) -> Option<String> {
        Some("browser-token".to_string())
    }

    fn no_token(_: KimiRegion) -> Option<String> {
        None
    }

    fn unread(_: KimiRegion) -> Option<String> {
        panic!("automatic Kimi token sources must not be read here")
    }

    fn usage(percent: f64) -> UsageSnapshot {
        UsageSnapshot::new(crate::core::RateWindow::new(percent))
    }

    fn reject_all(_: &str) -> Result<UsageSnapshot, ProviderError> {
        Err(ProviderError::AuthRequired)
    }

    fn accept_all(_: &str) -> Result<UsageSnapshot, ProviderError> {
        Ok(usage(25.0))
    }

    fn accept_browser_only(token: &str) -> Result<UsageSnapshot, ProviderError> {
        if token == "browser-token" {
            Ok(usage(25.0))
        } else {
            Err(ProviderError::AuthRequired)
        }
    }

    fn server_error(_: &str) -> Result<UsageSnapshot, ProviderError> {
        Err(ProviderError::Other(
            "API error: 500 Internal Server Error".into(),
        ))
    }

    /// Runs the web token chain against a scripted server; returns the
    /// outcome and every token the server saw, in order.
    async fn run_chain(
        input: WebTokenInput<'_>,
        respond: fn(&str) -> Result<UsageSnapshot, ProviderError>,
    ) -> (Result<UsageSnapshot, WebFetchFailure>, Vec<String>) {
        let sent = std::cell::RefCell::new(Vec::new());
        let result = fetch_with_web_tokens(&input, |token| {
            let response = respond(&token);
            sent.borrow_mut().push(token);
            async move { response }
        })
        .await;
        (result, sent.into_inner())
    }

    #[tokio::test]
    async fn web_auth_without_a_token_reports_that_none_was_tried() {
        for source in ["off", "manual"] {
            for manual in [None, Some("not-a-token")] {
                let (result, sent) =
                    run_chain(input(manual, source, unread, unread), accept_all).await;
                let failure = result.expect_err("no web token to try");
                assert!(!failure.had_token);
                assert_eq!(
                    failure.error.to_string(),
                    browser_import_error(source).to_string()
                );
                assert!(sent.is_empty());
            }
        }

        let (result, sent) = run_chain(input(None, "auto", no_token, no_token), accept_all).await;
        let failure = result.expect_err("no automatic token found");
        assert!(!failure.had_token);
        assert!(matches!(failure.error, ProviderError::AuthRequired));
        assert!(sent.is_empty());
    }

    #[tokio::test]
    async fn rejected_manual_token_is_authoritative_and_counts_as_tried() {
        let (result, sent) = run_chain(
            input(Some("kimi-auth=synthetic-web"), "auto", unread, unread),
            reject_all,
        )
        .await;
        let failure = result.expect_err("manual token rejected");
        assert!(failure.had_token);
        assert!(matches!(failure.error, ProviderError::AuthRequired));
        assert_eq!(sent, ["synthetic-web"]);
    }

    #[tokio::test]
    async fn rejected_desktop_session_falls_through_to_browser_import() {
        let (result, sent) = run_chain(
            input(None, "auto", static_desktop, static_browser),
            accept_browser_only,
        )
        .await;
        assert_eq!(
            result.expect("browser token accepted").primary.used_percent,
            25.0
        );
        assert_eq!(sent, ["desktop-token", "browser-token"]);

        let (result, sent) = run_chain(
            input(None, "auto", static_desktop, static_browser),
            reject_all,
        )
        .await;
        let failure = result.expect_err("every automatic token rejected");
        assert!(failure.had_token);
        assert!(matches!(failure.error, ProviderError::AuthRequired));
        assert_eq!(sent, ["desktop-token", "browser-token"]);
    }

    #[tokio::test]
    async fn healthy_desktop_session_never_reads_browser_cookies() {
        let (result, sent) =
            run_chain(input(None, "auto", static_desktop, unread), accept_all).await;
        assert!(result.is_ok());
        assert_eq!(sent, ["desktop-token"]);
    }

    #[tokio::test]
    async fn duplicate_browser_token_is_not_sent_twice() {
        let (result, sent) = run_chain(
            input(None, "auto", static_desktop, duplicate_browser),
            reject_all,
        )
        .await;
        assert!(result.expect_err("desktop token rejected").had_token);
        assert_eq!(sent, ["desktop-token"]);
    }

    #[tokio::test]
    async fn non_auth_web_error_stops_the_token_chain() {
        let (result, sent) =
            run_chain(input(None, "auto", static_desktop, unread), server_error).await;
        let failure = result.expect_err("server error");
        assert!(failure.had_token);
        assert!(matches!(failure.error, ProviderError::Other(message) if message.contains("500")));
        assert_eq!(sent, ["desktop-token"]);
    }

    fn no_local_storage(_: KimiRegion) -> Vec<String> {
        Vec::new()
    }

    fn static_local_storage(_: KimiRegion) -> Vec<String> {
        vec!["browser-token".to_string(), "storage-token".to_string()]
    }

    fn input<'a>(
        manual_header: Option<&'a str>,
        cookie_source: &'a str,
        desktop_token: fn(KimiRegion) -> Option<String>,
        browser_token: fn(KimiRegion) -> Option<String>,
    ) -> WebTokenInput<'a> {
        WebTokenInput {
            manual_header,
            cookie_source,
            region: KimiRegion::China,
            desktop_token,
            browser_token,
            local_storage_tokens: no_local_storage,
        }
    }

    fn duplicate_browser(_: KimiRegion) -> Option<String> {
        Some("desktop-token".to_string())
    }

    #[test]
    fn manual_cookie_header_wins_regardless_of_source() {
        let candidates = resolve_web_tokens(&input(
            Some("kimi-auth=manual-token"),
            "off",
            static_desktop,
            static_browser,
        ));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                token: "manual-token".to_string(),
                source: WebTokenSource::Manual,
            }]
        );
    }

    #[test]
    fn desktop_token_precedes_browser_import() {
        let candidates =
            resolve_web_tokens(&input(None, "browser", static_desktop, static_browser));
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].source, WebTokenSource::Desktop);
        assert_eq!(candidates[0].token, "desktop-token");
        assert_eq!(candidates[1].source, WebTokenSource::Browser);
        assert_eq!(candidates[1].token, "browser-token");
    }

    #[test]
    fn off_and_manual_sources_block_automatic_discovery() {
        for source in ["off", "manual"] {
            assert_eq!(
                resolve_web_tokens(&input(None, source, static_desktop, static_browser)),
                Vec::new()
            );
            assert_eq!(
                resolve_web_tokens(&input(
                    Some("not-a-token"),
                    source,
                    no_token,
                    static_browser
                )),
                Vec::new()
            );
        }
    }

    #[test]
    fn browser_token_used_when_desktop_absent() {
        let candidates = resolve_web_tokens(&input(None, "auto", no_token, static_browser));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                token: "browser-token".to_string(),
                source: WebTokenSource::Browser,
            }]
        );
    }

    #[test]
    fn explicit_manual_token_stays_authoritative() {
        let candidates = resolve_web_tokens(&input(
            Some("kimi-auth=manual-token"),
            "manual",
            static_desktop,
            static_browser,
        ));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                token: "manual-token".to_string(),
                source: WebTokenSource::Manual,
            }]
        );
    }

    #[test]
    fn duplicate_automatic_tokens_are_deduplicated() {
        let candidates =
            resolve_web_tokens(&input(None, "auto", static_desktop, duplicate_browser));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                token: "desktop-token".to_string(),
                source: WebTokenSource::Desktop,
            }]
        );
    }

    #[test]
    fn local_storage_tokens_follow_cookie_sources_without_duplicates() {
        let candidates = resolve_web_tokens(&WebTokenInput {
            local_storage_tokens: static_local_storage,
            ..input(None, "auto", static_desktop, static_browser)
        });
        let ordered: Vec<_> = candidates
            .iter()
            .map(|candidate| (candidate.source, candidate.token.as_str()))
            .collect();
        assert_eq!(
            ordered,
            vec![
                (WebTokenSource::Desktop, "desktop-token"),
                (WebTokenSource::Browser, "browser-token"),
                (WebTokenSource::LocalStorage, "storage-token"),
            ]
        );
    }

    #[test]
    fn local_storage_is_not_read_for_manual_credentials_or_blocked_sources() {
        let manual = resolve_web_tokens(&WebTokenInput {
            local_storage_tokens: static_local_storage,
            ..input(Some("kimi-auth=manual-token"), "auto", no_token, no_token)
        });
        assert_eq!(manual.len(), 1);
        assert_eq!(manual[0].source, WebTokenSource::Manual);

        for source in ["off", "manual"] {
            assert!(
                resolve_web_tokens(&WebTokenInput {
                    local_storage_tokens: static_local_storage,
                    ..input(None, source, no_token, no_token)
                })
                .is_empty()
            );
        }
    }

    #[test]
    fn browser_import_gate_is_case_insensitive() {
        assert!(!browser_import_allowed("OFF"));
        assert!(browser_import_allowed("browser"));
        assert!(browser_import_allowed("AUTO"));
        assert!(!browser_import_allowed("manual"));
    }

    #[test]
    fn browser_import_error_matches_rejected_cookie_source() {
        assert!(matches!(
            browser_import_error("manual"),
            ProviderError::Other(message)
                if message == "Kimi cookie source is Manual; provide a valid manual cookie header."
        ));
        assert!(matches!(
            browser_import_error("off"),
            ProviderError::Other(message)
                if message == "Kimi cookie source is Off; provide a manual cookie header or enable browser import."
        ));
        assert!(matches!(
            browser_import_error("unexpected"),
            ProviderError::Other(message)
                if message == "Kimi cookie source is Off; provide a manual cookie header or enable browser import."
        ));
    }

    #[test]
    fn subscription_stats_do_not_invent_a_membership_label() {
        let usage: KimiWebUsageResponse = serde_json::from_value(serde_json::json!({
            "usages": [{
                "scope": "FEATURE_CODING",
                "detail": { "limit": "1000", "used": "125" }
            }]
        }))
        .unwrap();
        let subscription: KimiSubscriptionStatsResponse =
            serde_json::from_value(serde_json::json!({
                "subscriptionBalance": {
                    "amountUsedRatio": 0.25,
                    "expireTime": "2026-09-30T00:00:00Z"
                },
                "ratelimitCode7d": {
                    "ratio": 0.1,
                    "enabled": true,
                    "resetTime": "2026-09-14T00:00:00Z"
                }
            }))
            .unwrap();

        let snapshot = snapshot_from_web_usage_response(usage, Some(subscription)).unwrap();

        assert_eq!(snapshot.login_method.as_deref(), Some("Kimi"));
        assert!(
            snapshot
                .extra_rate_windows
                .iter()
                .any(|window| window.id == "kimi-monthly")
        );
    }

    #[test]
    fn active_subscription_title_is_used_but_inactive_title_is_ignored() {
        let active: KimiSubscriptionResponse = serde_json::from_value(serde_json::json!({
            "subscription": {
                "active": true,
                "status": "SUBSCRIPTION_STATUS_ACTIVE",
                "goods": { "title": "  Allegro  " }
            }
        }))
        .unwrap();
        assert_eq!(active.plan_name().as_deref(), Some("Allegro"));

        let inactive: KimiSubscriptionResponse = serde_json::from_value(serde_json::json!({
            "subscription": {
                "active": false,
                "status": "SUBSCRIPTION_STATUS_EXPIRED",
                "goods": { "title": "Allegro" }
            }
        }))
        .unwrap();
        assert_eq!(inactive.plan_name(), None);
    }
}
