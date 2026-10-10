//! LLM Proxy provider implementation.
//!
//! Fetches enterprise quota statistics from an LLM Proxy `/v1/quota-stats` endpoint.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde::Deserialize;
use std::collections::HashMap;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::format;

const LLM_PROXY_CREDENTIAL_TARGET: &str = "codexbar-llmproxy";
const LLM_PROXY_BASE_URL_ENV: &str = "LLM_PROXY_BASE_URL";

#[derive(Debug, Deserialize)]
struct QuotaStatsResponse {
    providers: HashMap<String, ProviderStats>,
    summary: Option<SummaryStats>,
}

#[derive(Debug, Deserialize)]
struct ProviderStats {
    credential_count: Option<u64>,
    active_count: Option<u64>,
    exhausted_count: Option<u64>,
    total_requests: Option<u64>,
    tokens: Option<TokenStats>,
    #[serde(rename = "approx_cost")]
    approximate_cost: Option<f64>,
    #[serde(default, deserialize_with = "lenient_quota_groups")]
    quota_groups: Option<QuotaGroups>,
}

#[derive(Debug, Deserialize)]
struct TokenStats {
    input_cached: Option<u64>,
    input_uncached: Option<u64>,
    output: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SummaryStats {
    total_requests: Option<u64>,
    total_tokens: Option<u64>,
    #[serde(rename = "approx_cost")]
    approximate_cost: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum QuotaGroups {
    List(Vec<QuotaGroup>),
    Map(HashMap<String, QuotaGroup>),
}

/// Upstream treats malformed `quota_groups` as absent without discarding the
/// rest of the provider's usage (`llmproxy.ts`, native decoding note).
fn lenient_quota_groups<'de, D>(deserializer: D) -> Result<Option<QuotaGroups>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok().flatten())
}

#[derive(Debug, Clone, Deserialize)]
struct QuotaGroup {
    remaining_percent: Option<f64>,
    reset_time: Option<String>,
}

#[derive(Debug, Clone)]
struct ProviderSummary {
    name: String,
    requests: u64,
    tokens: u64,
    approximate_cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
struct LLMProxySummary {
    provider_count: usize,
    credential_count: u64,
    active_credential_count: u64,
    exhausted_credential_count: u64,
    total_requests: u64,
    total_tokens: u64,
    approximate_cost_usd: Option<f64>,
    minimum_remaining_percent: Option<f64>,
    next_reset_at: Option<DateTime<Utc>>,
    top_providers: Vec<ProviderSummary>,
}

pub struct LLMProxyProvider {
    client: Client,
}

impl LLMProxyProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(
        &self,
        api_key: &str,
        base_url: Url,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let response = self
            .client
            .get(quota_stats_url(base_url))
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "LLM Proxy quota-stats returned status {}",
                response.status()
            )));
        }

        let body = response.bytes().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to read LLM Proxy quota-stats: {e}"))
        })?;
        let summary = parse_summary(&body)?;
        let mut result = ProviderFetchResult::new(snapshot_from_summary(&summary), "api");
        if let Some(cost) = cost_from_summary(&summary) {
            result = result.with_cost(cost);
        }
        Ok(result)
    }
}

impl Default for LLMProxyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LLMProxyProvider {
    fn id(&self) -> ProviderId {
        ProviderId::LLMProxy
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = resolve_api_key(
                    ctx.api_key.as_deref(),
                    LLM_PROXY_CREDENTIAL_TARGET,
                    &["LLM_PROXY_API_KEY"],
                )?;
                let base_url = resolve_base_url(ctx)?;
                self.fetch_api(&api_key, base_url).await
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// The Settings value wins; `LLM_PROXY_BASE_URL` is the fallback.
fn resolve_base_url(ctx: &FetchContext) -> Result<Url, ProviderError> {
    let raw = ctx
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var(LLM_PROXY_BASE_URL_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| {
            ProviderError::NotInstalled(format!(
                "LLM Proxy base URL not found. Add one in Settings or set {LLM_PROXY_BASE_URL_ENV}."
            ))
        })?;
    crate::providers::validated_https_or_private_http_url(&raw, "LLM Proxy")
}

/// Build `{base}/v1/quota-stats`, keeping any query/fragment at the end.
///
/// `/v1` is appended only when the percent-decoded path does not already end
/// in `/v1` (upstream `llmproxy.ts`).
fn quota_stats_url(mut base_url: Url) -> Url {
    let query = base_url.query().map(str::to_owned);
    let fragment = base_url.fragment().map(str::to_owned);
    base_url.set_query(None);
    base_url.set_fragment(None);

    let path = base_url.path().trim_end_matches('/').to_string();
    let version = if percent_decode(&path).ends_with("/v1") {
        ""
    } else {
        "/v1"
    };
    base_url.set_path(&format!("{path}{version}/quota-stats"));
    base_url.set_query(query.as_deref());
    base_url.set_fragment(fragment.as_deref());
    base_url
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        let decoded = (bytes[idx] == b'%')
            .then(|| bytes.get(idx + 1..idx + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = decoded {
            out.push(byte);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_summary(data: &[u8]) -> Result<LLMProxySummary, ProviderError> {
    let decoded: QuotaStatsResponse = serde_json::from_slice(data)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse LLM Proxy quota-stats: {e}")))?;

    let mut provider_summaries: Vec<_> = decoded
        .providers
        .iter()
        .map(|(name, stats)| ProviderSummary {
            name: name.clone(),
            requests: stats.total_requests.unwrap_or(0),
            tokens: token_total(stats.tokens.as_ref()),
            approximate_cost_usd: stats.approximate_cost,
        })
        .collect();
    provider_summaries.sort_by(|left, right| {
        right
            .requests
            .cmp(&left.requests)
            .then_with(|| left.name.cmp(&right.name))
    });

    let requests = decoded
        .summary
        .as_ref()
        .and_then(|summary| summary.total_requests)
        .unwrap_or_else(|| {
            provider_summaries
                .iter()
                .map(|summary| summary.requests)
                .sum()
        });
    let tokens = decoded
        .summary
        .as_ref()
        .and_then(|summary| summary.total_tokens)
        .unwrap_or_else(|| {
            provider_summaries
                .iter()
                .map(|summary| summary.tokens)
                .sum()
        });
    let cost = decoded
        .summary
        .as_ref()
        .and_then(|summary| summary.approximate_cost)
        .or_else(|| {
            let sum: f64 = provider_summaries
                .iter()
                .filter_map(|summary| summary.approximate_cost_usd)
                .sum();
            (sum > 0.0).then_some(sum)
        });

    let quota_groups: Vec<_> = decoded
        .providers
        .values()
        .flat_map(|stats| {
            stats
                .quota_groups
                .as_ref()
                .map(|groups| match groups {
                    QuotaGroups::List(groups) => groups.clone(),
                    QuotaGroups::Map(groups) => groups.values().cloned().collect(),
                })
                .unwrap_or_default()
        })
        .collect();

    Ok(LLMProxySummary {
        provider_count: decoded.providers.len(),
        credential_count: decoded
            .providers
            .values()
            .map(|stats| stats.credential_count.unwrap_or(0))
            .sum(),
        active_credential_count: decoded
            .providers
            .values()
            .map(|stats| stats.active_count.unwrap_or(0))
            .sum(),
        exhausted_credential_count: decoded
            .providers
            .values()
            .map(|stats| stats.exhausted_count.unwrap_or(0))
            .sum(),
        total_requests: requests,
        total_tokens: tokens,
        approximate_cost_usd: cost,
        minimum_remaining_percent: quota_groups
            .iter()
            .filter_map(|group| group.remaining_percent)
            .min_by(|left, right| left.total_cmp(right)),
        next_reset_at: next_reset_at_from_groups(&quota_groups, Utc::now()),
        top_providers: provider_summaries,
    })
}

fn snapshot_from_summary(summary: &LLMProxySummary) -> UsageSnapshot {
    // Upstream leaves the primary lane out when no quota group reports a
    // remaining percentage. A Windows snapshot always has a primary lane, so
    // it becomes informational instead of a made-up 0%.
    let primary = summary
        .minimum_remaining_percent
        .map(|remaining| {
            RateWindow::with_details(
                (100.0 - remaining).clamp(0.0, 100.0),
                None,
                summary.next_reset_at,
                None,
            )
        })
        .unwrap_or_else(|| RateWindow::informational("No quota reported"));

    // Totals and provider rows are counts, not quotas. Informational rows keep
    // them from rendering as empty bars with a "Resets" prefix.
    let secondary = RateWindow::informational(format!(
        "{} requests",
        format::count(summary.total_requests)
    ));
    let tertiary =
        RateWindow::informational(format!("{} tokens", format::count(summary.total_tokens)));

    let mut snapshot = UsageSnapshot::new(primary)
        .with_secondary(secondary)
        .with_tertiary(tertiary)
        .with_login_method(format!(
            "{} / {} active keys",
            summary.active_credential_count, summary.credential_count
        ))
        .with_organization(format!("{} providers", summary.provider_count));

    for provider in summary.top_providers.iter().take(3) {
        let mut parts = vec![
            format!("{} req", format::count(provider.requests)),
            format!("{} tok", format::count(provider.tokens)),
        ];
        if let Some(cost) = provider.approximate_cost_usd {
            parts.push(format_usd(cost));
        }
        snapshot = snapshot.with_extra_rate_window(
            provider.name.clone(),
            provider.name.clone(),
            RateWindow::informational(parts.join(" · ")),
        );
    }

    snapshot
}

/// Approximate spend, kept even at $0. Upstream gives it the soonest quota
/// reset as its reset time.
fn cost_from_summary(summary: &LLMProxySummary) -> Option<CostSnapshot> {
    let cost = CostSnapshot::new(summary.approximate_cost_usd?, "USD", "Approx. spend");
    Some(match summary.next_reset_at {
        Some(resets_at) => cost.with_resets_at(resets_at),
        None => cost,
    })
}

/// US-dollar amount with thousands separators and cents, like `$1,234.50`.
fn format_usd(value: f64) -> String {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "f64-to-u64 casts saturate in Rust; display amounts never approach u64::MAX"
    )]
    let cents = (value.abs() * 100.0).round() as u64;
    let sign = if value < 0.0 && cents > 0 { "-" } else { "" };
    format!("{sign}${}.{:02}", format::count(cents / 100), cents % 100)
}

fn token_total(tokens: Option<&TokenStats>) -> u64 {
    tokens
        .map(|tokens| {
            tokens.input_cached.unwrap_or(0)
                + tokens.input_uncached.unwrap_or(0)
                + tokens.output.unwrap_or(0)
        })
        .unwrap_or(0)
}

/// Pick the soonest upcoming reset. Already-elapsed times are ignored so a stale
/// past reset cannot win over a real future one (upstream #2335).
fn next_reset_at_from_groups(groups: &[QuotaGroup], now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    groups
        .iter()
        .filter_map(|group| parse_date(group.reset_time.as_deref()))
        .filter(|t| *t > now)
        .min()
}

fn parse_date(raw: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw?)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn resolve_api_key(
    explicit: Option<&str>,
    credential_target: &str,
    env_names: &[&str],
) -> Result<String, ProviderError> {
    if let Some(key) = explicit
        && !key.trim().is_empty()
    {
        return Ok(key.trim().to_string());
    }
    if let Ok(entry) = keyring::Entry::new(credential_target, "api_key")
        && let Ok(key) = entry.get_password()
        && !key.trim().is_empty()
    {
        return Ok(key);
    }
    for env in env_names {
        if let Ok(key) = std::env::var(env)
            && !key.trim().is_empty()
        {
            return Ok(key);
        }
    }
    Err(ProviderError::NotInstalled(format!(
        "API key not found. Set {} in Preferences or environment.",
        env_names.join(" / ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quota_stats_with_keyed_quota_groups() {
        let summary = parse_summary(
            br#"{
                "providers": {
                    "openai": {
                        "credential_count": 2,
                        "active_count": 1,
                        "exhausted_count": 1,
                        "total_requests": 50,
                        "tokens": {"input_cached": 10, "input_uncached": 20, "output": 30},
                        "approx_cost": 1.25,
                        "quota_groups": {
                            "daily": {"remaining_percent": 25.5, "reset_time": "2026-05-20T00:00:00Z"}
                        }
                    },
                    "anthropic": {
                        "credential_count": 1,
                        "active_count": 1,
                        "exhausted_count": 0,
                        "total_requests": 75,
                        "tokens": {"input_cached": 0, "input_uncached": 40, "output": 60},
                        "approx_cost": 2.0,
                        "quota_groups": []
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(summary.credential_count, 3);
        assert_eq!(summary.active_credential_count, 2);
        assert_eq!(summary.total_requests, 125);
        assert_eq!(summary.total_tokens, 160);
        assert_eq!(summary.approximate_cost_usd, Some(3.25));
        assert_eq!(summary.minimum_remaining_percent, Some(25.5));

        let snapshot = snapshot_from_summary(&summary);
        assert_eq!(snapshot.primary.used_percent, 74.5);
        assert_eq!(snapshot.extra_rate_windows.len(), 2);
    }

    fn utc(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Upstream `LLMProxyUsageFetcherTests` "parses quota stats summary", with
    /// the reset moved into the future because the port reads the real clock.
    #[test]
    fn snapshot_matches_upstream_quota_stats_summary() {
        let summary = parse_summary(
            br#"{
              "providers": {
                "openai": {
                  "credential_count": 3, "active_count": 2, "exhausted_count": 1,
                  "total_requests": 120,
                  "tokens": {"input_cached": 1000, "input_uncached": 2000, "output": 3000},
                  "approx_cost": 12.5,
                  "quota_groups": {"default": {"remaining_percent": 42, "reset_time": "2099-05-18T12:00:00Z"}}
                },
                "anthropic": {
                  "credential_count": 1, "active_count": 1, "exhausted_count": 0,
                  "total_requests": 40,
                  "tokens": {"input_cached": 0, "input_uncached": 500, "output": 500},
                  "approx_cost": 3.0,
                  "quota_groups": [{"remaining_percent": 80}]
                }
              },
              "summary": {"total_requests": 160, "total_tokens": 7000, "approx_cost": 15.5}
            }"#,
        )
        .unwrap();
        let snapshot = snapshot_from_summary(&summary);

        assert!(!snapshot.primary.is_informational);
        assert_eq!(snapshot.primary.used_percent, 58.0);
        assert_eq!(
            snapshot.primary.resets_at,
            Some(utc("2099-05-18T12:00:00Z"))
        );
        assert_eq!(snapshot.primary.reset_description, None);

        let secondary = snapshot.secondary.as_ref().unwrap();
        assert!(secondary.is_informational);
        assert_eq!(secondary.reset_description.as_deref(), Some("160 requests"));
        let tertiary = snapshot.tertiary.as_ref().unwrap();
        assert!(tertiary.is_informational);
        assert_eq!(tertiary.reset_description.as_deref(), Some("7,000 tokens"));

        let first = &snapshot.extra_rate_windows[0];
        assert_eq!(first.id, "openai");
        assert!(first.window.is_informational);
        assert_eq!(
            first.window.reset_description.as_deref(),
            Some("120 req · 6,000 tok · $12.50")
        );
        assert_eq!(
            snapshot.extra_rate_windows[1]
                .window
                .reset_description
                .as_deref(),
            Some("40 req · 1,000 tok · $3.00")
        );

        let cost = cost_from_summary(&summary).unwrap();
        assert_eq!(cost.used, 15.5);
        assert_eq!(cost.currency_code, "USD");
        assert_eq!(cost.period, "Approx. spend");
        assert_eq!(cost.resets_at, Some(utc("2099-05-18T12:00:00Z")));
    }

    /// Upstream "zero summary spend is retained and empty providers stay
    /// displayable": no quota groups means no quota lane (here: informational).
    #[test]
    fn empty_providers_keep_zero_spend_without_a_quota_lane() {
        let summary = parse_summary(br#"{"providers":{},"summary":{"approx_cost":0}}"#).unwrap();
        let snapshot = snapshot_from_summary(&summary);

        assert!(snapshot.primary.is_informational);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("No quota reported")
        );
        assert_eq!(
            snapshot
                .secondary
                .as_ref()
                .unwrap()
                .reset_description
                .as_deref(),
            Some("0 requests")
        );
        assert!(snapshot.extra_rate_windows.is_empty());
        let cost = cost_from_summary(&summary).unwrap();
        assert_eq!(cost.used, 0.0);
        assert_eq!(cost.resets_at, None);
    }

    /// Upstream "sums missing summary ignores elapsed resets and limits sorted
    /// provider rows", with the future reset moved past the real clock.
    #[test]
    fn sums_missing_summary_ignores_elapsed_resets_and_keeps_top_three() {
        let summary = parse_summary(
            br#"{"providers":{
              "delta":{"total_requests":1,"tokens":{"output":2},"approx_cost":0},
              "charlie":{"total_requests":3,"approx_cost":2,"quota_groups":"invalid"},
              "bravo":{"total_requests":3,"quota_groups":[{"remaining_percent":-10,"reset_time":"1970-01-01T00:00:00Z"}]},
              "alpha":{"total_requests":4,"approx_cost":3,"quota_groups":{"a":{"reset_time":"2099-05-01T00:00:00Z"}}}
            }}"#,
        )
        .unwrap();
        let snapshot = snapshot_from_summary(&summary);

        assert_eq!(snapshot.primary.used_percent, 100.0);
        assert_eq!(
            snapshot.primary.resets_at,
            Some(utc("2099-05-01T00:00:00Z"))
        );
        assert_eq!(
            snapshot
                .secondary
                .as_ref()
                .unwrap()
                .reset_description
                .as_deref(),
            Some("11 requests")
        );
        assert_eq!(
            snapshot
                .tertiary
                .as_ref()
                .unwrap()
                .reset_description
                .as_deref(),
            Some("2 tokens")
        );
        assert_eq!(cost_from_summary(&summary).unwrap().used, 5.0);
        let ids: Vec<_> = snapshot
            .extra_rate_windows
            .iter()
            .map(|row| row.id.as_str())
            .collect();
        assert_eq!(ids, ["alpha", "bravo", "charlie"]);
        assert_eq!(
            snapshot.extra_rate_windows[1]
                .window
                .reset_description
                .as_deref(),
            Some("3 req · 0 tok")
        );
    }

    #[test]
    fn rejects_malformed_payloads() {
        let bodies: [&[u8]; 3] = [
            b"not json",
            b"{}",
            br#"{"providers":{"a":{"total_requests":"3"}}}"#,
        ];
        for body in bodies {
            assert!(
                matches!(parse_summary(body), Err(ProviderError::Parse(_))),
                "parsed {}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn formats_usd_with_thousands_separators() {
        assert_eq!(format_usd(12.5), "$12.50");
        assert_eq!(format_usd(1234.5), "$1,234.50");
        assert_eq!(format_usd(0.0), "$0.00");
        assert_eq!(format_usd(0.004), "$0.00");
        assert_eq!(format_usd(-3.0), "-$3.00");
    }

    fn stats_url(raw: &str) -> String {
        let base = crate::providers::validated_https_or_private_http_url(raw, "LLM Proxy").unwrap();
        quota_stats_url(base).to_string()
    }

    #[test]
    fn quota_stats_url_appends_v1_only_when_missing() {
        assert_eq!(
            stats_url("https://proxy.example.com"),
            "https://proxy.example.com/v1/quota-stats"
        );
        assert_eq!(
            stats_url("https://proxy.example.com/"),
            "https://proxy.example.com/v1/quota-stats"
        );
        assert_eq!(
            stats_url("https://proxy.example.com/v1"),
            "https://proxy.example.com/v1/quota-stats"
        );
        assert_eq!(
            stats_url("https://proxy.example.com/v1//"),
            "https://proxy.example.com/v1/quota-stats"
        );
        assert_eq!(
            stats_url("https://proxy.example.com/gateway"),
            "https://proxy.example.com/gateway/v1/quota-stats"
        );
        // Percent-decoded path already ends in /v1.
        assert_eq!(
            stats_url("https://proxy.example.com/%76%31"),
            "https://proxy.example.com/%76%31/quota-stats"
        );
    }

    #[test]
    fn quota_stats_url_keeps_query_and_fragment_at_the_end() {
        assert_eq!(
            stats_url("https://proxy.example.com?team=a"),
            "https://proxy.example.com/v1/quota-stats?team=a"
        );
        assert_eq!(
            stats_url("https://proxy.example.com/v1/?team=a#frag"),
            "https://proxy.example.com/v1/quota-stats?team=a#frag"
        );
        assert_eq!(
            stats_url("https://proxy.example.com/gw#frag"),
            "https://proxy.example.com/gw/v1/quota-stats#frag"
        );
    }

    #[test]
    fn base_url_policy_allows_https_and_private_network_http_only() {
        for ok in [
            "https://proxy.example.com",
            "http://127.0.0.1:8000",
            "http://localhost:8000",
            "http://[::1]:8000",
            "http://10.0.0.5",
            "http://172.16.4.2",
            "http://192.168.1.10:8000",
            "http://169.254.1.1",
            "http://proxy.local",
            "http://printer.local.:8000",
            "http://[fd00::1]",
            "http://[fe80::1]",
        ] {
            assert!(
                crate::providers::validated_https_or_private_http_url(ok, "LLM Proxy").is_ok(),
                "rejected {ok}"
            );
        }
        for bad in [
            "http://proxy.example.com",
            "http://172.32.0.1",
            "http://8.8.8.8",
            "ftp://proxy.local",
            "https://user:pass@proxy.example.com",
            "http://user@192.168.1.10",
            // `.local` needs a label in front of it.
            "http://.local",
            "http://.local.",
            "http://app.localhost:8000",
            // Upstream compares `localhost` before dropping a trailing dot.
            "http://localhost.:8000",
            // IPv4-mapped and global IPv6 addresses are not private literals.
            "http://[::ffff:10.0.0.1]",
            "http://[2001:db8::1]",
            "http://proxy.local%2f.evil.test",
            "",
        ] {
            assert!(
                crate::providers::validated_https_or_private_http_url(bad, "LLM Proxy").is_err(),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn settings_base_url_wins_over_env() {
        let ctx = FetchContext {
            workspace_id: Some(" http://192.168.1.10:8000 ".into()),
            ..FetchContext::default()
        };
        assert_eq!(
            resolve_base_url(&ctx).unwrap().as_str(),
            "http://192.168.1.10:8000/"
        );
    }

    #[test]
    fn malformed_quota_groups_are_absent_without_dropping_usage() {
        let summary = parse_summary(
            br#"{
                "providers": {
                    "a": {"total_requests": 5, "quota_groups": "nope"},
                    "b": {"total_requests": 7, "quota_groups": null},
                    "c": {"total_requests": 11, "quota_groups": [1, 2]},
                    "d": {"total_requests": 13, "quota_groups": {"x": {"remaining_percent": "bad"}}},
                    "e": {"total_requests": 17, "quota_groups": [{"remaining_percent": 40.0}]}
                }
            }"#,
        )
        .unwrap();
        assert_eq!(summary.total_requests, 53);
        assert_eq!(summary.minimum_remaining_percent, Some(40.0));
    }

    #[test]
    fn accepts_array_and_object_quota_groups() {
        let summary = parse_summary(
            br#"{
                "providers": {
                    "a": {"quota_groups": [{"remaining_percent": 30.0}]},
                    "b": {"quota_groups": {"k": {"remaining_percent": 20.0}}}
                }
            }"#,
        )
        .unwrap();
        assert_eq!(summary.minimum_remaining_percent, Some(20.0));
    }

    #[test]
    fn next_reset_at_ignores_elapsed_resets() {
        let now = DateTime::parse_from_rfc3339("2026-05-15T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let groups = vec![
            QuotaGroup {
                remaining_percent: Some(10.0),
                reset_time: Some("2026-05-10T00:00:00Z".into()), // past
            },
            QuotaGroup {
                remaining_percent: Some(40.0),
                reset_time: Some("2026-05-20T00:00:00Z".into()), // future
            },
            QuotaGroup {
                remaining_percent: Some(50.0),
                reset_time: Some("2026-05-18T00:00:00Z".into()), // sooner future
            },
        ];
        let next = next_reset_at_from_groups(&groups, now).expect("future reset");
        assert_eq!(
            next,
            DateTime::parse_from_rfc3339("2026-05-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn next_reset_at_none_when_all_elapsed() {
        let now = DateTime::parse_from_rfc3339("2026-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let groups = vec![QuotaGroup {
            remaining_percent: Some(10.0),
            reset_time: Some("2026-05-20T00:00:00Z".into()),
        }];
        assert!(next_reset_at_from_groups(&groups, now).is_none());
    }
}
