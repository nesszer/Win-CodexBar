//! Kimi web (`kimi.com`) cookie auth and the web-token resolution chain.
//!
//! Upstream 0.48.0 policies ported here (#2623 / `KimiBrowserImportPolicy`):
//! browser cookie import — and reading the Kimi Desktop session store — are
//! disabled when the Kimi cookie source is `off`. The shared token chain is
//! manual cookie header → Kimi Desktop session → browser import (upstream
//! `KimiWebEnrichmentTokenResolver`), used both by the web fetch itself and
//! by the Code-API/CLI monthly enrichment.

use reqwest::Client;

use super::desktop_token::KimiDesktopAuthToken;
use super::{
    KIMI_COOKIE_DOMAINS, KIMI_SUBSCRIPTION_STATS_URL, KIMI_SUBSCRIPTION_URL, KIMI_WEB_USAGE_URL,
    KimiProvider, KimiSubscriptionResponse, KimiSubscriptionStatsResponse, KimiWebUsageResponse,
    apply_subscription_windows, kimi_web_post,
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

/// Upstream `KimiBrowserImportPolicy.allowsImport`: everything but `off`.
fn browser_import_allowed(cookie_source: &str) -> bool {
    !cookie_source.eq_ignore_ascii_case("off")
}

/// Web auth token chain for both the web fetch and the Code-API enrichment
/// (upstream `KimiWebEnrichmentTokenResolver.resolve`):
/// 1. Manual cookie header (its `kimi-auth`/auth cookie), source-independent.
/// 2. Kimi Desktop session token (skipped when cookie source is `off`).
/// 3. Browser cookie import (skipped when cookie source is `off`).
pub(crate) fn web_auth_tokens(manual_header: Option<&str>) -> Vec<String> {
    resolve_web_tokens(WebTokenInput {
        manual_header,
        cookie_source: &cookie_source(),
        desktop_token: KimiDesktopAuthToken::load,
        browser_token: browser_auth_token,
    })
    .into_iter()
    .map(|candidate| candidate.token)
    .collect()
}

struct WebTokenInput<'a> {
    manual_header: Option<&'a str>,
    cookie_source: &'a str,
    desktop_token: fn() -> Option<String>,
    browser_token: fn() -> Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebTokenSource {
    Manual,
    Desktop,
    Browser,
}

#[derive(Debug, PartialEq, Eq)]
struct WebTokenCandidate {
    token: String,
    source: WebTokenSource,
}

fn resolve_web_tokens(input: WebTokenInput) -> Vec<WebTokenCandidate> {
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
    if let Some(token) = (input.desktop_token)()
        && seen.insert(token.clone())
    {
        candidates.push(WebTokenCandidate {
            token,
            source: WebTokenSource::Desktop,
        });
    }
    if let Some(token) = (input.browser_token)()
        && seen.insert(token.clone())
    {
        candidates.push(WebTokenCandidate {
            token,
            source: WebTokenSource::Browser,
        });
    }
    candidates
}

/// Browser import only: the first usable `kimi-auth`-class token from any of
/// the registered Kimi cookie domains.
fn browser_auth_token() -> Option<String> {
    KIMI_COOKIE_DOMAINS
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
) -> Result<UsageSnapshot, ProviderError> {
    let source = cookie_source();
    if let Some(token) =
        cookie_header.and_then(|header| KimiProvider::auth_token_from_cookie_header(header).ok())
    {
        // An explicit manual credential is authoritative. A rejected manual
        // token must not silently switch accounts underneath the user.
        let client = client()?;
        return fetch_via_web_token(&client, &token).await;
    }

    if !browser_import_allowed(&source) {
        return Err(ProviderError::Other(
            "Kimi cookie source is Off; provide a manual cookie header or enable browser import."
                .into(),
        ));
    }

    let client = client()?;
    let mut seen = std::collections::HashSet::new();

    // Read and try the desktop session first. Browser cookies are intentionally
    // read only after the server rejects this automatic session, so a healthy
    // desktop account never causes another credential store to be touched.
    if let Some(token) = KimiDesktopAuthToken::load()
        && seen.insert(token.clone())
    {
        match fetch_via_web_token(&client, &token).await {
            Ok(usage) => return Ok(usage),
            Err(ProviderError::AuthRequired) => {}
            Err(error) => return Err(error),
        }
    }

    if let Some(token) = browser_auth_token()
        && seen.insert(token.clone())
    {
        match fetch_via_web_token(&client, &token).await {
            Ok(usage) => return Ok(usage),
            Err(ProviderError::AuthRequired) => {}
            Err(error) => return Err(error),
        }
    }

    Err(ProviderError::AuthRequired)
}

fn client() -> Result<reqwest::Client, ProviderError> {
    crate::core::credentialed_http_client_builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ProviderError::Other(e.to_string()))
}

async fn fetch_via_web_token(
    client: &reqwest::Client,
    token: &str,
) -> Result<UsageSnapshot, ProviderError> {
    let resp = kimi_web_post(
        client,
        KIMI_WEB_USAGE_URL,
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

    let (subscription, plan_name) = fetch_subscription_details(client, token).await;

    snapshot_from_web_usage_response_with_plan(usage, subscription, plan_name)
}

const SUBSCRIPTION_ENRICHMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

async fn fetch_subscription_details(
    client: &reqwest::Client,
    token: &str,
) -> (Option<KimiSubscriptionStatsResponse>, Option<String>) {
    // The quota statistics and the optional title are independent. Keep a
    // completed statistics response when the plan endpoint is slow or absent.
    let stats = tokio::time::timeout(
        SUBSCRIPTION_ENRICHMENT_TIMEOUT,
        fetch_subscription_for_enrichment(client, token),
    );
    let plan = tokio::time::timeout(
        SUBSCRIPTION_ENRICHMENT_TIMEOUT,
        fetch_subscription_plan(client, token),
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

pub(super) async fn fetch_subscription_plan(client: &Client, token: &str) -> Option<String> {
    match kimi_web_post(client, KIMI_SUBSCRIPTION_URL, token, serde_json::json!({})).await {
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
) -> Option<KimiSubscriptionStatsResponse> {
    fetch_subscription_for_enrichment_result(client, token)
        .await
        .ok()
        .flatten()
}

pub(super) async fn fetch_subscription_for_enrichment_result(
    client: &Client,
    token: &str,
) -> Result<Option<KimiSubscriptionStatsResponse>, ProviderError> {
    match kimi_web_post(
        client,
        KIMI_SUBSCRIPTION_STATS_URL,
        token,
        serde_json::json!({}),
    )
    .await
    {
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

    fn static_desktop() -> Option<String> {
        Some("desktop-token".to_string())
    }

    fn static_browser() -> Option<String> {
        Some("browser-token".to_string())
    }

    fn no_token() -> Option<String> {
        None
    }

    fn input<'a>(
        manual_header: Option<&'a str>,
        cookie_source: &'a str,
        desktop_token: fn() -> Option<String>,
        browser_token: fn() -> Option<String>,
    ) -> WebTokenInput<'a> {
        WebTokenInput {
            manual_header,
            cookie_source,
            desktop_token,
            browser_token,
        }
    }

    fn duplicate_browser() -> Option<String> {
        Some("desktop-token".to_string())
    }

    #[test]
    fn manual_cookie_header_wins_regardless_of_source() {
        let candidates = resolve_web_tokens(input(
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
        let candidates = resolve_web_tokens(input(None, "browser", static_desktop, static_browser));
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].source, WebTokenSource::Desktop);
        assert_eq!(candidates[0].token, "desktop-token");
        assert_eq!(candidates[1].source, WebTokenSource::Browser);
        assert_eq!(candidates[1].token, "browser-token");
    }

    #[test]
    fn cookie_source_off_blocks_desktop_and_browser_but_not_manual() {
        assert_eq!(
            resolve_web_tokens(input(None, "off", static_desktop, static_browser)),
            Vec::new()
        );
        assert_eq!(
            resolve_web_tokens(input(None, "off", no_token, static_browser)),
            Vec::new()
        );
        assert_eq!(
            resolve_web_tokens(input(Some("kimi-auth=manual"), "off", no_token, no_token)),
            vec![WebTokenCandidate {
                token: "manual".to_string(),
                source: WebTokenSource::Manual,
            }]
        );
    }

    #[test]
    fn browser_token_used_when_desktop_absent() {
        let candidates = resolve_web_tokens(input(None, "auto", no_token, static_browser));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                token: "browser-token".to_string(),
                source: WebTokenSource::Browser,
            }]
        );
    }

    #[test]
    fn manual_default_source_still_allows_desktop_token() {
        // Upstream: desktop-session token applies for any non-off source;
        // the local default ("manual") must keep desktop sessions working.
        let candidates = resolve_web_tokens(input(None, "manual", static_desktop, no_token));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                token: "desktop-token".to_string(),
                source: WebTokenSource::Desktop,
            }]
        );
    }

    #[test]
    fn duplicate_automatic_tokens_are_deduplicated() {
        let candidates = resolve_web_tokens(input(None, "auto", static_desktop, duplicate_browser));
        assert_eq!(
            candidates,
            vec![WebTokenCandidate {
                token: "desktop-token".to_string(),
                source: WebTokenSource::Desktop,
            }]
        );
    }

    #[test]
    fn browser_import_gate_is_case_insensitive() {
        assert!(!browser_import_allowed("OFF"));
        assert!(browser_import_allowed("browser"));
        assert!(browser_import_allowed("manual"));
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
