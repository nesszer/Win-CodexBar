//! DeepInfra provider implementation.
//!
//! Fetches prepaid balance and monthly spend from DeepInfra billing APIs:
//! - `GET https://api.deepinfra.com/payment/checklist?compute_owed=true`
//! - `GET https://api.deepinfra.com/payment/usage?from=current`
//!
//! Each GET follows upstream's `transientIdempotent` policy: one retry for
//! transient failures, bounded by an overall fetch budget (see
//! `DeepInfraProvider::send_with_retry`).
//!
//! Ported from steipete/CodexBar `DeepInfraUsageFetcher`.

use std::{
    error::Error,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};

const CHECKLIST_URL: &str = "https://api.deepinfra.com/payment/checklist?compute_owed=true";
const USAGE_URL: &str = "https://api.deepinfra.com/payment/usage?from=current";
const CREDENTIAL_TARGET: &str = "codexbar-deepinfra";
const CENTS_PER_DOLLAR: f64 = 100.0;
const ENV_KEYS: &[&str] = &["DEEPINFRA_API_KEY", "DEEPINFRA_TOKEN"];

/// Per-request timeout (upstream `timeoutSeconds: 30`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Wall-clock budget for the whole refresh (both GETs plus any retry). The
/// desktop shell drops a provider fetch after 35 s, so retries must not push
/// past it; a retry that cannot finish inside the budget is not attempted.
const FETCH_BUDGET: Duration = Duration::from_secs(33);
/// Upstream `ProviderHTTPRetryPolicy.transientIdempotent`: a single retry.
const MAX_RETRIES: u32 = 1;
const RETRYABLE_STATUSES: [StatusCode; 6] = [
    StatusCode::REQUEST_TIMEOUT,
    StatusCode::TOO_MANY_REQUESTS,
    StatusCode::INTERNAL_SERVER_ERROR,
    StatusCode::BAD_GATEWAY,
    StatusCode::SERVICE_UNAVAILABLE,
    StatusCode::GATEWAY_TIMEOUT,
];
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(10);

/// Checklist monetary fields are USD. Negative `stripe_balance` means prepaid funds.
#[derive(Debug, Deserialize, Clone)]
struct ChecklistResponse {
    stripe_balance: f64,
    recent: f64,
    limit: Option<f64>,
    #[serde(default)]
    suspended: bool,
    suspend_reason: Option<String>,
}

/// Usage endpoint reports `total_cost` in cents.
#[derive(Debug, Deserialize, Clone)]
struct UsageResponse {
    months: Vec<UsageMonth>,
    #[serde(default)]
    initial_month: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct UsageMonth {
    #[allow(
        dead_code,
        reason = "field present in the DeepInfra billing payload; kept so serde preserves it"
    )]
    period: String,
    /// Cost in cents (upstream field name is `total_cost`).
    total_cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct DeepInfraSnapshot {
    available_balance_usd: f64,
    amount_owed_usd: f64,
    current_month_cost_usd: f64,
    recent_cost_usd: f64,
    spending_limit_usd: Option<f64>,
    suspended: bool,
    suspend_reason: Option<String>,
}

impl DeepInfraSnapshot {
    fn from_responses(checklist: &ChecklistResponse, usage: &UsageResponse) -> Self {
        let recent_cost = checklist.recent.max(0.0);
        let current_month_cost = usage
            .months
            .last()
            .map(|m| (m.total_cost / CENTS_PER_DOLLAR).max(0.0))
            .unwrap_or(recent_cost);
        let net_balance = checklist.stripe_balance + recent_cost;
        let spending_limit = checklist
            .limit
            .and_then(|limit| (limit > 0.0).then_some(limit));

        Self {
            available_balance_usd: (-net_balance).max(0.0),
            amount_owed_usd: net_balance.max(0.0),
            current_month_cost_usd: current_month_cost,
            recent_cost_usd: recent_cost,
            spending_limit_usd: spending_limit,
            suspended: checklist.suspended,
            suspend_reason: checklist.suspend_reason.clone(),
        }
    }

    fn to_usage_snapshot(&self) -> UsageSnapshot {
        // Upstream #2822: with a positive spending limit configured, the
        // primary percent reflects billing-cycle spend against that limit so
        // the automatic menu-bar metric tracks it; balance-depletion stays
        // the hard 100% signal.
        let used_percent =
            if self.suspended || self.amount_owed_usd > 0.0 || self.available_balance_usd <= 0.0 {
                100.0
            } else if let Some(limit) = self
                .spending_limit_usd
                .filter(|limit| *limit > 0.0 && limit.is_finite())
            {
                ((self.recent_cost_usd / limit) * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };

        let balance_text = if self.amount_owed_usd > 0.0 {
            format!("${:.2} owed", self.amount_owed_usd)
        } else {
            format!("${:.2} available", self.available_balance_usd)
        };
        let spending_text = format!("${:.2} spent this month", self.current_month_cost_usd);
        let detail = if self.suspended {
            let reason = self
                .suspend_reason
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            match reason {
                Some(reason) => format!("Suspended: {reason} · {balance_text} · {spending_text}"),
                None => format!("Suspended · {balance_text} · {spending_text}"),
            }
        } else {
            format!("{balance_text} · {spending_text}")
        };

        let mut primary = RateWindow::new(used_percent);
        primary.reset_description = Some(detail);

        UsageSnapshot::new(primary).with_login_method(balance_text)
    }

    fn to_cost_snapshot(&self) -> Option<CostSnapshot> {
        self.spending_limit_usd.map(|limit| {
            CostSnapshot::new(self.recent_cost_usd, "USD", "Billing cycle").with_limit(limit)
        })
    }
}

pub struct DeepInfraProvider {
    client: Client,
}

impl DeepInfraProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn resolve_api_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        let raw = crate::providers::resolve_api_key(api_key, CREDENTIAL_TARGET, ENV_KEYS)?;
        clean_api_key(&raw).ok_or_else(|| {
            ProviderError::NotInstalled(
                "DeepInfra API key not found. Set DEEPINFRA_API_KEY / DEEPINFRA_TOKEN or Preferences → Providers."
                    .to_string(),
            )
        })
    }

    async fn fetch_usage_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let api_key = Self::resolve_api_key(ctx.api_key.as_deref())?;
        let deadline = Instant::now() + FETCH_BUDGET;
        let checklist = self
            .fetch_json::<ChecklistResponse>(CHECKLIST_URL, &api_key, deadline)
            .await?;
        let usage = self
            .fetch_json::<UsageResponse>(USAGE_URL, &api_key, deadline)
            .await?;
        let snapshot = DeepInfraSnapshot::from_responses(&checklist, &usage);

        let mut result = ProviderFetchResult::new(snapshot.to_usage_snapshot(), "api");
        if let Some(cost) = snapshot.to_cost_snapshot() {
            result = result.with_cost(cost);
        }
        Ok(result)
    }

    async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        api_key: &str,
        deadline: Instant,
    ) -> Result<T, ProviderError> {
        let resp = self.send_with_retry(url, api_key, deadline).await?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::Other(
                "DeepInfra API key rejected (HTTP 401).".to_string(),
            ));
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::Other(
                "DeepInfra API key cannot access billing data (HTTP 403).".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "DeepInfra API error: HTTP {status}"
            )));
        }

        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse DeepInfra response: {e}")))
    }

    /// Send the billing GET, retrying once on a transient failure.
    ///
    /// Retried: HTTP 408/429/500/502/503/504 and transport failures
    /// (timeout, refused connection, DNS lookup failures). DNS failures are
    /// identified from reqwest's connect-error source chain because
    /// [`ProviderError::is_transport_failure`] deliberately excludes them.
    /// TLS failures are not retried.
    /// 401/403 are returned to the caller unretried. The wait honors
    /// `Retry-After` (seconds, capped at 10 s, default 1 s), and no retry
    /// starts unless it can fit in the remaining `deadline`. Dropping the
    /// future (shell refresh timeout / cancellation) also drops the sleep.
    async fn send_with_retry(
        &self,
        url: &str,
        api_key: &str,
        deadline: Instant,
    ) -> Result<Response, ProviderError> {
        let mut attempt = 0;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ProviderError::Timeout);
            }
            let outcome = self
                .client
                .get(url)
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Accept", "application/json")
                .timeout(remaining.min(REQUEST_TIMEOUT))
                .send()
                .await
                .map_err(ProviderError::Network);

            let delay = match &outcome {
                Ok(resp) if RETRYABLE_STATUSES.contains(&resp.status()) => Some(retry_delay(
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok()),
                )),
                Err(error) if is_retryable_transport_error(error) => Some(retry_delay(None)),
                _ => None,
            };
            let Some(delay) = delay else {
                return outcome;
            };
            let time_left = deadline.saturating_duration_since(Instant::now());
            if attempt >= MAX_RETRIES || delay >= time_left {
                return outcome;
            }
            tracing::debug!(
                attempt = attempt + 1,
                ?delay,
                "DeepInfra billing request failed transiently; retrying once"
            );
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }
}

fn is_retryable_transport_error(error: &ProviderError) -> bool {
    error.is_transport_failure()
        || matches!(error, ProviderError::Network(error) if is_dns_resolution_error(error))
}

fn is_dns_resolution_error(error: &reqwest::Error) -> bool {
    if !error.is_connect() || error.is_body() || error.is_decode() {
        return false;
    }

    // Reqwest 0.12 does not expose DNS failures as a typed variant. Its
    // hyper-util connector labels the DNS cause in the source chain, allowing
    // this provider-local retry policy to recognize it without broadening the
    // shared transport classifier used by other providers.
    let mut source = error.source();
    while let Some(cause) = source {
        if cause.to_string() == "dns error" {
            return true;
        }
        source = cause.source();
    }
    false
}

/// Retry wait: `Retry-After` seconds (non-negative, capped at 10 s), else 1 s.
fn retry_delay(retry_after: Option<&str>) -> Duration {
    retry_after
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(|seconds| {
            if seconds >= MAX_RETRY_DELAY.as_secs_f64() {
                MAX_RETRY_DELAY
            } else {
                Duration::from_secs_f64(seconds)
            }
        })
        .unwrap_or(DEFAULT_RETRY_DELAY)
}

impl Default for DeepInfraProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for DeepInfraProvider {
    fn id(&self) -> ProviderId {
        ProviderId::DeepInfra
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_api(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn clean_api_key(raw: &str) -> Option<String> {
    let mut value = raw.trim().to_string();
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value = value[1..value.len() - 1].trim().to_string();
    }
    if let Some(stripped) = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
    {
        value = stripped.trim().to_string();
    }
    (!value.is_empty()).then_some(value)
}

/// Parse fixture JSON without network (used by unit tests).
fn parse_snapshot_for_testing(
    checklist_json: &str,
    usage_json: &str,
) -> Result<DeepInfraSnapshot, ProviderError> {
    let checklist: ChecklistResponse = serde_json::from_str(checklist_json)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse DeepInfra checklist: {e}")))?;
    let usage: UsageResponse = serde_json::from_str(usage_json)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse DeepInfra usage: {e}")))?;
    let _ = usage.initial_month.as_ref();
    Ok(DeepInfraSnapshot::from_responses(&checklist, &usage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct FailingDnsResolver(AtomicUsize);

    impl reqwest::dns::Resolve for FailingDnsResolver {
        fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(std::io::Error::other("synthetic DNS failure").into()) })
        }
    }

    fn checklist_json(
        stripe_balance: f64,
        recent: f64,
        limit: Option<f64>,
        suspended: bool,
        suspend_reason: Option<&str>,
    ) -> String {
        let limit_json = match limit {
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        let reason_json = match suspend_reason {
            Some(r) => format!("\"{r}\""),
            None => "null".to_string(),
        };
        format!(
            r#"{{
              "stripe_balance": {stripe_balance},
              "recent": {recent},
              "limit": {limit_json},
              "suspended": {suspended},
              "suspend_reason": {reason_json}
            }}"#
        )
    }

    fn usage_json(total_cost_cents: f64) -> String {
        format!(
            r#"{{
              "months": [
                {{
                  "period": "2026.07",
                  "items": [],
                  "total_cost": {total_cost_cents}
                }}
              ],
              "initial_month": "2026.07"
            }}"#
        )
    }

    #[test]
    fn converts_monthly_cents_and_deducts_recent_usage_from_prepaid_balance() {
        let snapshot = parse_snapshot_for_testing(
            &checklist_json(-99.75, 3.94, Some(20.0), false, None),
            &usage_json(394.0),
        )
        .unwrap();

        assert!((snapshot.available_balance_usd - 95.81).abs() < 1e-6);
        assert_eq!(snapshot.amount_owed_usd, 0.0);
        assert!((snapshot.current_month_cost_usd - 3.94).abs() < 1e-6);
        assert_eq!(snapshot.recent_cost_usd, 3.94);
        assert_eq!(snapshot.spending_limit_usd, Some(20.0));

        let usage = snapshot.to_usage_snapshot();
        // Upstream #2822: positive spending limit drives the percent from
        // billing-cycle spend (3.94 / 20.00 ≈ 19.7%), not a flat 0%.
        assert!((usage.primary.used_percent - 19.7).abs() < 0.05);
        assert_eq!(
            usage.primary.reset_description.as_deref(),
            Some("$95.81 available · $3.94 spent this month")
        );

        let cost = snapshot.to_cost_snapshot().unwrap();
        assert_eq!(cost.used, 3.94);
        assert_eq!(cost.limit, Some(20.0));
        assert_eq!(cost.period, "Billing cycle");
    }

    #[test]
    fn spend_over_limit_clamps_to_100_and_zero_limit_stays_binary() {
        // Spend beyond the positive limit clamps at 100% instead of exceeding.
        let snapshot = parse_snapshot_for_testing(
            &checklist_json(-50.0, 30.0, Some(20.0), false, None),
            &usage_json(3000.0),
        )
        .unwrap();
        assert_eq!(snapshot.to_usage_snapshot().primary.used_percent, 100.0);

        // No (or non-positive) limit keeps the legacy binary balance signal.
        let snapshot = parse_snapshot_for_testing(
            &checklist_json(-50.0, 1.0, Some(0.0), false, None),
            &usage_json(100.0),
        )
        .unwrap();
        assert_eq!(snapshot.to_usage_snapshot().primary.used_percent, 0.0);
    }

    #[test]
    fn positive_stripe_balance_is_reported_as_amount_owed() {
        let snapshot = parse_snapshot_for_testing(
            &checklist_json(2.75, 7.0, Some(-1.0), false, None),
            &usage_json(650.0),
        )
        .unwrap();

        assert_eq!(snapshot.available_balance_usd, 0.0);
        assert_eq!(snapshot.amount_owed_usd, 9.75);
        assert_eq!(snapshot.spending_limit_usd, None);

        let usage = snapshot.to_usage_snapshot();
        assert_eq!(usage.primary.used_percent, 100.0);
        assert_eq!(
            usage.primary.reset_description.as_deref(),
            Some("$9.75 owed · $6.50 spent this month")
        );
        assert!(snapshot.to_cost_snapshot().is_none());
    }

    #[test]
    fn suspended_account_is_marked_exhausted() {
        let snapshot = parse_snapshot_for_testing(
            &checklist_json(-5.0, 1.0, None, true, Some("Payment review")),
            &usage_json(100.0),
        )
        .unwrap()
        .to_usage_snapshot();

        assert_eq!(snapshot.primary.used_percent, 100.0);
        assert!(
            snapshot
                .primary
                .reset_description
                .as_deref()
                .unwrap_or("")
                .starts_with("Suspended: Payment review")
        );
    }

    #[test]
    fn rejects_malformed_billing_response() {
        let err = parse_snapshot_for_testing("{}", &usage_json(100.0)).unwrap_err();
        match err {
            ProviderError::Parse(msg) => assert!(msg.contains("checklist")),
            other => panic!("expected parse error, got {other:?}"),
        }
    }

    #[test]
    fn cleans_quoted_and_bearer_prefixed_keys() {
        assert_eq!(
            clean_api_key("  \"Bearer sk-test\"  ").as_deref(),
            Some("sk-test")
        );
        assert_eq!(clean_api_key("bearer sk-abc").as_deref(), Some("sk-abc"));
        assert_eq!(clean_api_key("   ").as_deref(), None);
    }

    fn far_deadline() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    #[test]
    fn retry_delay_honors_retry_after_within_bounds() {
        assert_eq!(retry_delay(None), Duration::from_secs(1));
        assert_eq!(retry_delay(Some(" 2 ")), Duration::from_secs(2));
        assert_eq!(retry_delay(Some("0.5")), Duration::from_millis(500));
        assert_eq!(retry_delay(Some("0")), Duration::ZERO);
        assert_eq!(retry_delay(Some("99")), Duration::from_secs(10));
        for unusable in [
            "-3",
            "nan",
            "inf",
            "1e9999",
            "soon",
            "",
            "Wed, 21 Oct 2026 07:28:00 GMT",
        ] {
            assert_eq!(retry_delay(Some(unusable)), Duration::from_secs(1));
        }
    }

    #[tokio::test]
    async fn transient_status_is_retried_once_then_succeeds() {
        for status in [408, 429, 500, 502, 503, 504] {
            let mut server = mockito::Server::new_async().await;
            let failing = server
                .mock("GET", "/payment/checklist")
                .with_status(status)
                .with_header("retry-after", "0")
                .expect(1)
                .create_async()
                .await;
            let ok = server
                .mock("GET", "/payment/checklist")
                .with_status(200)
                .with_body(checklist_json(-5.0, 1.0, None, false, None))
                .expect(1)
                .create_async()
                .await;

            let url = format!("{}/payment/checklist", server.url());
            let checklist = DeepInfraProvider::new()
                .fetch_json::<ChecklistResponse>(&url, "sk-test", far_deadline())
                .await
                .unwrap_or_else(|e| panic!("HTTP {status} should be retried: {e}"));

            assert_eq!(checklist.stripe_balance, -5.0);
            failing.assert_async().await;
            ok.assert_async().await;
        }
    }

    #[tokio::test]
    async fn persistent_transient_status_makes_exactly_two_requests() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/payment/usage")
            .with_status(503)
            .with_header("retry-after", "0")
            .expect(2)
            .create_async()
            .await;

        let url = format!("{}/payment/usage", server.url());
        let error = DeepInfraProvider::new()
            .fetch_json::<UsageResponse>(&url, "sk-test", far_deadline())
            .await
            .unwrap_err();

        assert!(error.to_string().contains("HTTP 503"), "got: {error}");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn auth_and_client_errors_are_not_retried() {
        for (status, message) in [
            (401, "rejected (HTTP 401)"),
            (403, "billing data (HTTP 403)"),
            (404, "HTTP 404"),
        ] {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("GET", "/payment/checklist")
                .with_status(status)
                .with_header("retry-after", "0")
                .expect(1)
                .create_async()
                .await;

            let url = format!("{}/payment/checklist", server.url());
            let error = DeepInfraProvider::new()
                .fetch_json::<ChecklistResponse>(&url, "sk-test", far_deadline())
                .await
                .unwrap_err();

            assert!(error.to_string().contains(message), "got: {error}");
            mock.assert_async().await;
        }
    }

    #[tokio::test]
    async fn retry_that_cannot_fit_in_the_budget_is_skipped() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/payment/checklist")
            .with_status(429)
            .with_header("retry-after", "10")
            .expect(1)
            .create_async()
            .await;

        let url = format!("{}/payment/checklist", server.url());
        let started = Instant::now();
        let error = DeepInfraProvider::new()
            .fetch_json::<ChecklistResponse>(
                &url,
                "sk-test",
                Instant::now() + Duration::from_secs(3),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("HTTP 429"), "got: {error}");
        assert!(started.elapsed() < Duration::from_secs(3));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn expired_budget_fails_without_sending_a_request() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/payment/checklist")
            .expect(0)
            .create_async()
            .await;

        let url = format!("{}/payment/checklist", server.url());
        let error = DeepInfraProvider::new()
            .fetch_json::<ChecklistResponse>(&url, "sk-test", Instant::now())
            .await
            .unwrap_err();

        assert!(matches!(error, ProviderError::Timeout), "got: {error:?}");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn refused_connection_is_retried_once_then_reported() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let started = Instant::now();
        let error = DeepInfraProvider::new()
            .fetch_json::<ChecklistResponse>(
                &format!("http://{address}/payment/checklist"),
                "sk-test",
                far_deadline(),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ProviderError::Network(_)), "got: {error:?}");
        // One default 1 s wait proves the transport failure was retried once.
        assert!(started.elapsed() >= Duration::from_millis(900));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn dns_lookup_failure_is_retried_once_without_network_access() {
        let resolver = Arc::new(FailingDnsResolver(AtomicUsize::new(0)));
        let mut provider = DeepInfraProvider::new();
        provider.client = Client::builder()
            .no_proxy()
            .dns_resolver(Arc::clone(&resolver))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap();

        let error = provider
            .fetch_json::<ChecklistResponse>(
                "http://deepinfra.invalid/payment/checklist",
                "sk-test",
                far_deadline(),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ProviderError::Network(_)), "got: {error:?}");
        assert_eq!(resolver.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn metadata_matches_upstream_descriptor() {
        let provider = DeepInfraProvider::new();
        assert_eq!(provider.id(), ProviderId::DeepInfra);
        assert_eq!(provider.metadata().display_name, "DeepInfra");
        assert_eq!(
            provider.metadata().dashboard_url,
            Some("https://deepinfra.com/dash")
        );
        assert_eq!(
            provider.metadata().status_page_url,
            Some("https://status.deepinfra.com")
        );
        assert!(!provider.metadata().supports_credits);
        assert!(!provider.metadata().default_enabled);
    }
}
