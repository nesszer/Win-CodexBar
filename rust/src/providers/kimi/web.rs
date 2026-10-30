//! Kimi web (`kimi.com`) cookie auth and the web-token resolution chain.
//!
//! Upstream 0.48.0 policies ported here (#2623 / `KimiBrowserImportPolicy`):
//! browser cookie import — and reading the Kimi Desktop session store — are
//! disabled when the Kimi cookie source is `off`. The shared token chain is
//! manual cookie header → Kimi Desktop session → browser cookie import →
//! Chromium local-storage `access_token` (upstream `KimiWebEnrichmentTokenResolver`
//! and #3923), used both by the web fetch itself and
//! by the Code-API/CLI monthly enrichment. A Desktop session keeps its
//! provenance as a [`KimiWebCredential`] and is revalidated before each
//! request (upstream #4416); a changed session stops the chain instead of
//! falling back to another credential.

use reqwest::Client;

use super::code_api::MISSING_WEEKLY_DESCRIPTION;
use super::credential::KimiWebCredential;
use super::desktop_session::desktop_credential;
use super::local_storage::local_storage_tokens;
use super::{
    KIMI_SUBSCRIPTION_SERVICE, KIMI_SUBSCRIPTION_STATS_SERVICE, KIMI_WEB_USAGE_SERVICE,
    KimiProvider, KimiRegion, KimiSubscriptionResponse, KimiSubscriptionStatsResponse,
    KimiWebUsageResponse, apply_subscription_windows, gift_display_details, kimi_web_post,
};
use crate::browser::cookies::get_cookie_header;
use crate::core::{ProviderError, ProviderFetchResult, ProviderId, RateWindow, UsageSnapshot};

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
    fn before_token(error: ProviderError) -> Self {
        Self {
            error,
            had_token: false,
        }
    }

    fn after_token(error: ProviderError) -> Self {
        Self {
            error,
            had_token: true,
        }
    }
}

/// One automatic token attempt. `None` means the server rejected the token
/// and the chain moves on; anything else ends it.
fn settle_attempt<T>(result: Result<T, ProviderError>) -> Option<Result<T, WebFetchFailure>> {
    match result {
        Ok(usage) => Some(Ok(usage)),
        Err(ProviderError::AuthRequired) => None,
        Err(error) => Some(Err(WebFetchFailure::after_token(error))),
    }
}

/// Web auth token chain for both the web fetch and the Code-API enrichment
/// (upstream `KimiWebEnrichmentTokenResolver.resolve`):
/// 1. Manual cookie header (its `kimi-auth`/auth cookie), source-independent.
/// 2. Kimi Desktop session token (automatic source only).
/// 3. Browser cookie import (automatic source only).
/// 4. Chromium local-storage `access_token` for the region (automatic source only).
pub(super) fn web_auth_credentials(
    manual_header: Option<&str>,
    region: KimiRegion,
) -> Vec<KimiWebCredential> {
    resolve_web_tokens(&WebTokenInput {
        manual_header,
        cookie_source: &cookie_source(),
        region,
        desktop_token: desktop_credential,
        browser_token: browser_auth_token,
        local_storage_tokens,
    })
    .into_iter()
    .map(|candidate| candidate.credential)
    .collect()
}

struct WebTokenInput<'a> {
    manual_header: Option<&'a str>,
    cookie_source: &'a str,
    region: KimiRegion,
    desktop_token: fn(KimiRegion) -> Option<KimiWebCredential>,
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

#[derive(Debug, PartialEq, Eq)]
struct WebTokenCandidate {
    credential: KimiWebCredential,
    source: WebTokenSource,
}

fn resolve_web_tokens(input: &WebTokenInput<'_>) -> Vec<WebTokenCandidate> {
    if let Some(header) = input.manual_header
        && let Ok(token) = KimiProvider::auth_token_from_cookie_header(header)
    {
        return vec![WebTokenCandidate {
            credential: KimiWebCredential::plain(token),
            source: WebTokenSource::Manual,
        }];
    }
    if !browser_import_allowed(input.cookie_source) {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(credential) = (input.desktop_token)(input.region)
        && seen.insert(credential.token().to_string())
    {
        candidates.push(WebTokenCandidate {
            credential,
            source: WebTokenSource::Desktop,
        });
    }
    if let Some(token) = (input.browser_token)(input.region)
        && seen.insert(token.clone())
    {
        candidates.push(WebTokenCandidate {
            credential: KimiWebCredential::plain(token),
            source: WebTokenSource::Browser,
        });
    }
    for token in (input.local_storage_tokens)(input.region) {
        if seen.insert(token.clone()) {
            candidates.push(WebTokenCandidate {
                credential: KimiWebCredential::plain(token),
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
) -> Result<ProviderFetchResult, ProviderError> {
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
) -> Result<ProviderFetchResult, WebFetchFailure> {
    // An explicitly selected token account is an identity boundary: fetch only
    // its own session cookie and never fall back to another ambient credential
    // or browser account (upstream per-provider token account routing).
    if account_isolated {
        let token =
            selected_account_auth_token(cookie_header).map_err(WebFetchFailure::before_token)?;
        let http = client().map_err(WebFetchFailure::before_token)?;
        return fetch_via_web_token(&http, &KimiWebCredential::plain(token), region)
            .await
            .map_err(WebFetchFailure::after_token);
    }
    fetch_web_session(cookie_header, region).await
}

/// [`fetch_via_web`], also reporting whether web auth had a token to try.
pub(super) async fn fetch_web_session(
    cookie_header: Option<&str>,
    region: KimiRegion,
) -> Result<ProviderFetchResult, WebFetchFailure> {
    let source = cookie_source();
    let input = WebTokenInput {
        manual_header: cookie_header,
        cookie_source: &source,
        region,
        desktop_token: desktop_credential,
        browser_token: browser_auth_token,
        local_storage_tokens,
    };
    // One HTTP client for every token attempt, built on first use.
    let mut shared_client: Option<Client> = None;
    fetch_with_web_tokens(&input, |credential| {
        let http = match &shared_client {
            Some(http) => Ok(http.clone()),
            None => client().inspect(|http| shared_client = Some(http.clone())),
        };
        async move { fetch_via_web_token(&http?, &credential, region).await }
    })
    .await
}

/// The web fetch over the token chain. An explicit manual token is
/// authoritative; otherwise (automatic source only) the Kimi Desktop session
/// is tried, then browser import. Only a server rejection moves on to the
/// next automatic token; a Desktop session that changed before dispatch is a
/// non-auth error and stops the chain.
async fn fetch_with_web_tokens<T, F, Fut>(
    input: &WebTokenInput<'_>,
    mut fetch: F,
) -> Result<T, WebFetchFailure>
where
    F: FnMut(KimiWebCredential) -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    if let Some(token) = input
        .manual_header
        .and_then(|header| KimiProvider::auth_token_from_cookie_header(header).ok())
    {
        // An explicit manual credential is authoritative. A rejected manual
        // token must not silently switch accounts underneath the user.
        return fetch(KimiWebCredential::plain(token))
            .await
            .map_err(WebFetchFailure::after_token);
    }

    if !browser_import_allowed(input.cookie_source) {
        return Err(WebFetchFailure::before_token(browser_import_error(
            input.cookie_source,
        )));
    }

    // Read and try the desktop session first. Browser cookies are intentionally
    // read only after the server rejects this automatic session, so a healthy
    // desktop account never causes another credential store to be touched.
    let desktop_token = (input.desktop_token)(input.region);
    if let Some(credential) = desktop_token.clone()
        && let Some(outcome) = settle_attempt(fetch(credential).await)
    {
        return outcome;
    }

    let browser_token = (input.browser_token)(input.region).filter(|token| {
        desktop_token.as_ref().map(KimiWebCredential::token) != Some(token.as_str())
    });
    if let Some(token) = browser_token.clone()
        && let Some(outcome) = settle_attempt(fetch(KimiWebCredential::plain(token)).await)
    {
        return outcome;
    }

    // Local-storage tokens are read last, only after every cookie source
    // was rejected.
    let mut seen = std::collections::HashSet::new();
    if let Some(credential) = &desktop_token {
        seen.insert(credential.token().to_string());
    }
    if let Some(token) = browser_token.clone() {
        seen.insert(token);
    }
    for candidate in resolve_web_tokens(input).into_iter().filter(|candidate| {
        candidate.source == WebTokenSource::LocalStorage
            && seen.insert(candidate.credential.token().to_string())
    }) {
        if let Some(outcome) = settle_attempt(fetch(candidate.credential).await) {
            return outcome;
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
    credential: &KimiWebCredential,
    region: KimiRegion,
) -> Result<ProviderFetchResult, ProviderError> {
    let usage_url = region.web_api_url(KIMI_WEB_USAGE_SERVICE);
    let resp = kimi_web_post(
        client,
        &usage_url,
        region,
        credential,
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

    let (subscription, plan_name) = fetch_subscription_details(client, credential, region).await;

    snapshot_from_web_usage_response_with_plan(usage, subscription, plan_name)
}

const SUBSCRIPTION_ENRICHMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

async fn fetch_subscription_details(
    client: &reqwest::Client,
    credential: &KimiWebCredential,
    region: KimiRegion,
) -> (Option<KimiSubscriptionStatsResponse>, Option<String>) {
    // The quota statistics and the optional title are independent. Keep a
    // completed statistics response when the plan endpoint is slow or absent.
    let stats = tokio::time::timeout(
        SUBSCRIPTION_ENRICHMENT_TIMEOUT,
        fetch_subscription_for_enrichment_result(client, credential, region),
    );
    let plan = tokio::time::timeout(
        SUBSCRIPTION_ENRICHMENT_TIMEOUT,
        fetch_subscription_plan(client, credential, region),
    );
    let (stats, plan) = tokio::join!(stats, plan);
    (
        stats.ok().and_then(Result::ok).flatten(),
        plan.ok().flatten(),
    )
}

pub(super) fn snapshot_from_web_usage_response(
    response: KimiWebUsageResponse,
    subscription: Option<KimiSubscriptionStatsResponse>,
) -> Result<UsageSnapshot, ProviderError> {
    snapshot_from_web_usage_response_with_plan(response, subscription, None)
        .map(|fetched| fetched.usage)
}

fn snapshot_from_web_usage_response_with_plan(
    response: KimiWebUsageResponse,
    subscription: Option<KimiSubscriptionStatsResponse>,
    plan_name: Option<String>,
) -> Result<ProviderFetchResult, ProviderError> {
    let coding = response
        .usages
        .unwrap_or_default()
        .into_iter()
        .find(|usage| usage.scope == "FEATURE_CODING");
    let has_coding = coding.is_some();
    let mut usage = match coding {
        Some(coding) => {
            let primary = KimiProvider::rate_window_from_usage_detail(&coding.detail, Some(10080))?;
            let mut usage = UsageSnapshot::new(primary).with_login_method("Kimi");

            if let Some(limit) = coding.limits.unwrap_or_default().into_iter().next() {
                let window_minutes = limit.window.as_ref().and_then(super::kimi_window_minutes);
                let rate_limit =
                    KimiProvider::rate_window_from_usage_detail(&limit.detail, window_minutes)?;
                usage = usage.with_secondary(rate_limit);
            }
            usage
        }
        // Upstream #4700: a lapsed plan can omit its Code pools while the
        // membership pools below still show usage.
        None => UsageSnapshot::new(RateWindow::informational(MISSING_WEEKLY_DESCRIPTION))
            .with_login_method("Kimi"),
    };

    let mut details = Vec::new();
    if let Some(subscription) = subscription.as_ref() {
        details = super::monthly_breakdown::monthly_breakdown_details(
            super::monthly_breakdown::subscription_monthly_shares(subscription),
        );
        details.extend(gift_display_details(subscription, usage.updated_at));
        usage = apply_subscription_windows(usage, subscription);
    }
    if let Some(plan_name) = plan_name {
        usage = usage.with_login_method(plan_name);
    }

    // Without Code pools, the membership pools must supply at least one lane.
    if !has_coding && usage.extra_rate_windows.is_empty() {
        return Err(ProviderError::Parse(
            "FEATURE_CODING scope not found in response".into(),
        ));
    }

    Ok(ProviderFetchResult::new(usage, "web").with_display_details(details))
}

pub(super) async fn fetch_subscription_plan(
    client: &Client,
    credential: &KimiWebCredential,
    region: KimiRegion,
) -> Option<String> {
    let url = region.web_api_url(KIMI_SUBSCRIPTION_SERVICE);
    match kimi_web_post(client, &url, region, credential, serde_json::json!({})).await {
        Ok(response) if response.status().is_success() => response
            .json::<KimiSubscriptionResponse>()
            .await
            .ok()
            .and_then(|response| response.plan_name()),
        _ => None,
    }
}

pub(super) async fn fetch_subscription_for_enrichment_result(
    client: &Client,
    credential: &KimiWebCredential,
    region: KimiRegion,
) -> Result<Option<KimiSubscriptionStatsResponse>, ProviderError> {
    let url = region.web_api_url(KIMI_SUBSCRIPTION_STATS_SERVICE);
    match kimi_web_post(client, &url, region, credential, serde_json::json!({})).await {
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

    fn static_desktop(_: KimiRegion) -> Option<KimiWebCredential> {
        Some(KimiWebCredential::plain("desktop-token"))
    }

    fn changed_desktop(_: KimiRegion) -> Option<KimiWebCredential> {
        Some(KimiWebCredential::desktop("desktop-token", || false))
    }

    fn static_browser(_: KimiRegion) -> Option<String> {
        Some("browser-token".to_string())
    }

    fn no_desktop(_: KimiRegion) -> Option<KimiWebCredential> {
        None
    }

    fn no_token(_: KimiRegion) -> Option<String> {
        None
    }

    fn unread_desktop(_: KimiRegion) -> Option<KimiWebCredential> {
        panic!("automatic Kimi token sources must not be read here")
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
        let client = reqwest::Client::new();
        let url = KimiRegion::China.web_api_url(KIMI_WEB_USAGE_SERVICE);
        let result = fetch_with_web_tokens(&input, |credential| {
            let request = super::super::kimi_web_request(
                &client,
                &url,
                KimiRegion::China,
                credential.token(),
                serde_json::json!({}),
            )
            .unwrap();
            let response = credential
                .check_dispatch(&request, KimiRegion::China)
                .and_then(|()| {
                    sent.borrow_mut().push(credential.token().to_string());
                    respond(credential.token())
                });
            async move { response }
        })
        .await;
        (result, sent.into_inner())
    }

    #[tokio::test]
    async fn changed_desktop_session_is_not_sent_and_does_not_fall_back() {
        let (result, sent) =
            run_chain(input(None, "auto", changed_desktop, unread), accept_all).await;
        let failure = result.expect_err("desktop session changed");
        assert!(failure.had_token);
        assert_eq!(
            failure.error.to_string(),
            super::super::credential::DESKTOP_SESSION_CHANGED_MESSAGE
        );
        assert!(sent.is_empty());
    }

    #[tokio::test]
    async fn web_auth_without_a_token_reports_that_none_was_tried() {
        for source in ["off", "manual"] {
            for manual in [None, Some("not-a-token")] {
                let (result, sent) =
                    run_chain(input(manual, source, unread_desktop, unread), accept_all).await;
                let failure = result.expect_err("no web token to try");
                assert!(!failure.had_token);
                assert_eq!(
                    failure.error.to_string(),
                    browser_import_error(source).to_string()
                );
                assert!(sent.is_empty());
            }
        }

        let (result, sent) = run_chain(input(None, "auto", no_desktop, no_token), accept_all).await;
        let failure = result.expect_err("no automatic token found");
        assert!(!failure.had_token);
        assert!(matches!(failure.error, ProviderError::AuthRequired));
        assert!(sent.is_empty());
    }

    #[tokio::test]
    async fn rejected_manual_token_is_authoritative_and_counts_as_tried() {
        let (result, sent) = run_chain(
            input(
                Some("kimi-auth=synthetic-web"),
                "auto",
                unread_desktop,
                unread,
            ),
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

    thread_local! {
        static READS: std::cell::Cell<[u32; 3]> = const { std::cell::Cell::new([0; 3]) };
    }

    fn count_read(source: usize) {
        READS.with(|reads| {
            let mut counts = reads.get();
            counts[source] += 1;
            reads.set(counts);
        });
    }

    /// Desktop, browser and local-storage reads since the last call.
    fn take_reads() -> [u32; 3] {
        READS.with(|reads| reads.replace([0; 3]))
    }

    fn counted_desktop(_: KimiRegion) -> Option<KimiWebCredential> {
        count_read(0);
        Some(KimiWebCredential::plain("desktop-token"))
    }

    fn counted_browser(_: KimiRegion) -> Option<String> {
        count_read(1);
        Some("browser-token".to_string())
    }

    fn counted_no_desktop(_: KimiRegion) -> Option<KimiWebCredential> {
        count_read(0);
        None
    }

    fn counted_no_browser(_: KimiRegion) -> Option<String> {
        count_read(1);
        None
    }

    fn counted_local_storage(_: KimiRegion) -> Vec<String> {
        count_read(2);
        vec!["browser-token".to_string(), "storage-token".to_string()]
    }

    fn accept_storage_only(token: &str) -> Result<UsageSnapshot, ProviderError> {
        if token == "storage-token" {
            Ok(usage(40.0))
        } else {
            Err(ProviderError::AuthRequired)
        }
    }

    #[tokio::test]
    async fn rejected_local_storage_only_token_reports_none_tried() {
        take_reads();
        let (result, sent) = run_chain(
            WebTokenInput {
                local_storage_tokens: counted_local_storage,
                ..input(None, "auto", counted_no_desktop, counted_no_browser)
            },
            reject_all,
        )
        .await;
        let failure = result.expect_err("local-storage token rejected");
        // Kept quirk: a rejected local-storage-only token reads as "no token".
        assert!(!failure.had_token);
        assert!(matches!(failure.error, ProviderError::AuthRequired));
        assert_eq!(sent, ["browser-token", "storage-token"]);
        assert_eq!(take_reads(), [2, 2, 2]);
    }

    #[tokio::test]
    async fn local_storage_tokens_follow_rejected_cookie_tokens() {
        take_reads();
        let (result, sent) = run_chain(
            WebTokenInput {
                local_storage_tokens: counted_local_storage,
                ..input(None, "auto", counted_desktop, counted_browser)
            },
            accept_storage_only,
        )
        .await;
        assert_eq!(
            result.expect("storage token accepted").primary.used_percent,
            40.0
        );
        assert_eq!(sent, ["desktop-token", "browser-token", "storage-token"]);
        assert_eq!(take_reads(), [2, 2, 1]);

        let (result, sent) = run_chain(
            WebTokenInput {
                local_storage_tokens: counted_local_storage,
                ..input(None, "auto", counted_desktop, counted_browser)
            },
            reject_all,
        )
        .await;
        assert!(result.expect_err("every token rejected").had_token);
        assert_eq!(sent, ["desktop-token", "browser-token", "storage-token"]);
        assert_eq!(take_reads(), [2, 2, 1]);

        let (result, sent) = run_chain(
            WebTokenInput {
                local_storage_tokens: counted_local_storage,
                ..input(None, "auto", counted_no_desktop, counted_no_browser)
            },
            server_error,
        )
        .await;
        let failure = result.expect_err("server error");
        assert!(failure.had_token);
        assert!(matches!(failure.error, ProviderError::Other(message) if message.contains("500")));
        assert_eq!(sent, ["browser-token"]);
        assert_eq!(take_reads(), [2, 2, 1]);
    }

    fn static_local_storage(_: KimiRegion) -> Vec<String> {
        vec!["browser-token".to_string(), "storage-token".to_string()]
    }

    fn input<'a>(
        manual_header: Option<&'a str>,
        cookie_source: &'a str,
        desktop_token: fn(KimiRegion) -> Option<KimiWebCredential>,
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
                credential: KimiWebCredential::plain("manual-token"),
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
        assert_eq!(candidates[0].credential.token(), "desktop-token");
        assert_eq!(candidates[1].source, WebTokenSource::Browser);
        assert_eq!(candidates[1].credential.token(), "browser-token");
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
                    no_desktop,
                    static_browser
                )),
                Vec::new()
            );
        }
    }

    #[test]
    fn browser_token_used_when_desktop_absent() {
        let candidates = resolve_web_tokens(&input(None, "auto", no_desktop, static_browser));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                credential: KimiWebCredential::plain("browser-token"),
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
                credential: KimiWebCredential::plain("manual-token"),
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
                credential: KimiWebCredential::plain("desktop-token"),
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
            .map(|candidate| (candidate.source, candidate.credential.token()))
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
            ..input(Some("kimi-auth=manual-token"), "auto", no_desktop, no_token)
        });
        assert_eq!(manual.len(), 1);
        assert_eq!(manual[0].source, WebTokenSource::Manual);

        for source in ["off", "manual"] {
            assert!(
                resolve_web_tokens(&WebTokenInput {
                    local_storage_tokens: static_local_storage,
                    ..input(None, source, no_desktop, no_token)
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

    #[test]
    fn lapsed_web_result_carries_gift_rows_and_membership_pool() {
        // Upstream #4700 + #4691: a lapsed plan keeps the membership pool and
        // the live gift rows in the web result, with the web source label.
        let usage: KimiWebUsageResponse = serde_json::from_value(serde_json::json!({
            "totalQuota": { "limit": "100", "used": "36" }
        }))
        .unwrap();
        let subscription: KimiSubscriptionStatsResponse =
            serde_json::from_value(serde_json::json!({
                "subscriptionBalance": {
                    "feature": "FEATURE_OMNI",
                    "type": "SUBSCRIPTION",
                    "amountUsedRatio": 0.36,
                    "expireTime": "2026-11-22T00:00:00Z"
                },
                "giftBalances": [{
                    "feature": "FEATURE_OMNI",
                    "type": "GIFT",
                    "amountUsedRatio": 0.6,
                    "expireTime": "2099-12-31T15:59:59Z",
                    "displayName": "Welcome Credit"
                }]
            }))
            .unwrap();

        let fetched =
            snapshot_from_web_usage_response_with_plan(usage, Some(subscription), None).unwrap();

        assert_eq!(fetched.source_label, "web");
        assert!(fetched.usage.primary.is_informational);
        assert!(
            fetched
                .usage
                .extra_rate_windows
                .iter()
                .any(|window| window.id == "kimi-monthly")
        );
        assert_eq!(fetched.display_details.len(), 1);
        assert_eq!(fetched.display_details[0].title(), "Welcome Credit");
        assert_eq!(fetched.display_details[0].value(), "40% left");
    }

    #[test]
    fn web_result_lists_the_total_usage_breakdown_before_gifts() {
        // Upstream #4759: the membership pool's Code share splits Total usage.
        let usage: KimiWebUsageResponse = serde_json::from_value(serde_json::json!({
            "totalQuota": { "limit": "100", "used": "36" }
        }))
        .unwrap();
        let subscription: KimiSubscriptionStatsResponse =
            serde_json::from_value(serde_json::json!({
                "subscriptionBalance": {
                    "feature": "FEATURE_OMNI",
                    "type": "SUBSCRIPTION",
                    "amountUsedRatio": 0.36,
                    "kimiCodeUsedRatio": 0.1,
                    "expireTime": "2026-11-22T00:00:00Z"
                },
                "giftBalances": [{
                    "feature": "FEATURE_OMNI",
                    "type": "GIFT",
                    "amountUsedRatio": 0.6,
                    "expireTime": "2099-12-31T15:59:59Z",
                    "displayName": "Welcome Credit"
                }]
            }))
            .unwrap();

        let fetched =
            snapshot_from_web_usage_response_with_plan(usage, Some(subscription), None).unwrap();

        let rows: Vec<(&str, &str)> = fetched
            .display_details
            .iter()
            .map(|row| (row.title(), row.value()))
            .collect();
        assert_eq!(
            rows,
            [
                ("Kimi Code", "10% used"),
                ("Other features", "26% used"),
                ("Welcome Credit", "40% left"),
            ]
        );
    }
}
