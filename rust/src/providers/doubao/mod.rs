//! Doubao / Volcengine Ark provider implementation.
//!
//! Probes Ark chat-completions with a one-token request and reads rate-limit headers.
//! Also supports signed Coding Plan API credentials and `arkcli usage plan` (0.45).

mod arkcli;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::core::{
    FetchContext, IconLane, NamedRateWindow, Provider, ProviderError, ProviderFetchResult,
    ProviderId, RateWindow, SourceMode, UsageSnapshot, hex, hmac_sha256, sha256_hex,
};
use crate::providers::resolve_api_key;

#[cfg(test)]
use arkcli::decode_arkcli_usage;
use arkcli::{datetime_from_epoch, fetch_arkcli_usage, resolve_arkcli_binary};

const DOUBAO_API_URL: &str = "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions";
const DOUBAO_CODING_PLAN_URL: &str =
    "https://open.volcengineapi.com/?Action=GetCodingPlanUsage&Version=2024-01-01";
const DOUBAO_CREDENTIAL_TARGET: &str = "codexbar-doubao";
const PROBE_MODELS: &[&str] = &[
    "doubao-seed-2.0-code",
    "doubao-1.5-pro-32k",
    "doubao-lite-32k",
];

pub struct DoubaoProvider {
    client: Client,
}

impl DoubaoProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn api_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        resolve_api_key(
            api_key,
            DOUBAO_CREDENTIAL_TARGET,
            &["ARK_API_KEY", "DOUBAO_API_KEY", "VOLCENGINE_API_KEY"],
        )
    }

    fn coding_plan_credentials(api_key: Option<&str>) -> Option<DoubaoCodingPlanCredentials> {
        api_key
            .and_then(DoubaoCodingPlanCredentials::parse)
            .or_else(DoubaoCodingPlanCredentials::from_env)
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let mut last_error = None;
        for model in PROBE_MODELS {
            match self.probe(api_key, model).await {
                Ok(result) => {
                    return Ok(self
                        .confirm_ambiguous_zero_remaining(api_key, model, result)
                        .await);
                }
                Err(error @ ProviderError::AuthRequired) => return Err(error),
                Err(error) => {
                    last_error = Some(error);
                }
            }
        }
        Err(last_error
            .unwrap_or_else(|| ProviderError::Other("All Doubao probe models failed".into())))
    }

    async fn fetch_coding_plan(
        &self,
        credentials: &DoubaoCodingPlanCredentials,
    ) -> Result<UsageSnapshot, ProviderError> {
        let body = Vec::new();
        let signed = sign_volcengine_request(credentials, &body, Utc::now())?;
        let response = self
            .client
            .post(DOUBAO_CODING_PLAN_URL)
            .header("Accept", "application/json")
            .header("Content-Type", signed.content_type)
            .header("Host", signed.host)
            .header("X-Date", signed.timestamp)
            .header("X-Content-Sha256", signed.payload_hash)
            .header("Authorization", signed.authorization)
            .body(body)
            .send()
            .await?;

        let status = response.status();
        let bytes = response.bytes().await?;
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "Doubao Coding Plan API returned {status}: {}",
                sanitized_body(&String::from_utf8_lossy(&bytes))
            )));
        }

        Ok(coding_plan_snapshot(decode_coding_plan_usage(&bytes)?))
    }

    async fn confirm_ambiguous_zero_remaining(
        &self,
        api_key: &str,
        model: &str,
        initial: DoubaoProbeResult,
    ) -> UsageSnapshot {
        if !initial.has_ambiguous_zero_remaining() {
            return initial.snapshot;
        }

        match self.probe(api_key, model).await {
            Ok(confirmation) if confirmation.status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                initial.snapshot
            }
            Ok(confirmation) if confirmation.has_ambiguous_zero_remaining() => snapshot_from_parts(
                confirmation.remaining,
                confirmation.limit,
                confirmation.resets_at,
                confirmation.total_tokens,
                false,
            ),
            Ok(confirmation) => confirmation.snapshot,
            Err(error) => {
                tracing::warn!(
                    "Doubao zero-remaining confirmation failed; preserving initial exhausted state: {error}"
                );
                initial.snapshot
            }
        }
    }

    async fn probe(&self, api_key: &str, model: &str) -> Result<DoubaoProbeResult, ProviderError> {
        let response = self
            .client
            .post(DOUBAO_API_URL)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "hi"}],
            }))
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }

        let status = response.status();
        if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::Other(format!(
                "Doubao probe model {model} returned status {status}"
            )));
        }

        let headers = response.headers().clone();
        let body: serde_json::Value = response.json().await.unwrap_or_else(|_| json!({}));
        Ok(probe_result_from_response(status, &headers, &body))
    }
}

#[derive(Debug)]
struct DoubaoProbeResult {
    snapshot: UsageSnapshot,
    status: reqwest::StatusCode,
    remaining: Option<i64>,
    limit: Option<i64>,
    resets_at: Option<DateTime<Utc>>,
    total_tokens: Option<i64>,
    request_limits_reliable: bool,
}

impl DoubaoProbeResult {
    fn has_ambiguous_zero_remaining(&self) -> bool {
        self.status == reqwest::StatusCode::OK
            && self.request_limits_reliable
            && self.limit.is_some_and(|limit| limit > 0)
            && self.remaining == Some(0)
    }
}

fn probe_result_from_response(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &serde_json::Value,
) -> DoubaoProbeResult {
    let remaining = int_header(headers, "x-ratelimit-remaining-requests");
    let limit = int_header(headers, "x-ratelimit-limit-requests");
    let resets_at = string_header(headers, "x-ratelimit-reset-requests").and_then(parse_reset_time);
    let total_tokens = body
        .get("usage")
        .and_then(|usage| usage.get("total_tokens"))
        .and_then(|value| value.as_i64());
    let request_limits_reliable = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        limit.is_some()
    } else {
        limit.is_some() && remaining.is_some()
    };

    let snapshot = snapshot_from_parts(
        remaining,
        limit,
        resets_at,
        total_tokens,
        request_limits_reliable,
    );

    DoubaoProbeResult {
        snapshot,
        status,
        remaining,
        limit,
        resets_at,
        total_tokens,
        request_limits_reliable,
    }
}

fn snapshot_from_parts(
    remaining: Option<i64>,
    limit: Option<i64>,
    resets_at: Option<DateTime<Utc>>,
    total_tokens: Option<i64>,
    request_limits_reliable: bool,
) -> UsageSnapshot {
    let effective_remaining = remaining.unwrap_or(0);

    let (used_percent, detail) = if let (Some(remaining), Some(limit)) = (remaining, limit) {
        if request_limits_reliable && limit > 0 {
            let used = (limit - remaining).max(0);
            let percent = used as f64 / limit as f64 * 100.0;
            (percent, format!("{used}/{limit} requests"))
        } else {
            (0.0, "Active - check dashboard for details".to_string())
        }
    } else if let Some(limit) = limit.filter(|limit| request_limits_reliable && *limit > 0) {
        let used = (limit - effective_remaining).max(0);
        let percent = if limit > 0 {
            used as f64 / limit as f64 * 100.0
        } else {
            0.0
        };
        (percent, format!("{used}/{limit} requests"))
    } else if let Some(total_tokens) = total_tokens {
        (0.0, format!("Active - {total_tokens} tokens observed"))
    } else {
        (0.0, "Active - check dashboard for details".to_string())
    };

    let mut window = RateWindow::with_details(used_percent, None, resets_at, Some(detail));
    if window.used_percent.is_nan() {
        window.used_percent = 0.0;
    }
    UsageSnapshot::new(window)
}

fn int_header(headers: &reqwest::header::HeaderMap, name: &str) -> Option<i64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
}

fn string_header(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

fn parse_reset_time(value: String) -> Option<DateTime<Utc>> {
    let trimmed = value.trim();
    if let Ok(ts) = trimmed.parse::<i64>() {
        return Utc.timestamp_opt(ts, 0).single();
    }
    DateTime::parse_from_rfc3339(trimmed)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[derive(Debug)]
struct DoubaoCodingPlanCredentials {
    access_key_id: String,
    secret_access_key: String,
    region: String,
}

impl DoubaoCodingPlanCredentials {
    fn from_env() -> Option<Self> {
        let access_key_id = cleaned_env("VOLCENGINE_ACCESS_KEY_ID")
            .or_else(|| cleaned_env("DOUBAO_ACCESS_KEY_ID"))?;
        let secret_access_key = cleaned_env("VOLCENGINE_SECRET_ACCESS_KEY")
            .or_else(|| cleaned_env("DOUBAO_SECRET_ACCESS_KEY"))?;
        let region = cleaned_env("VOLCENGINE_REGION")
            .or_else(|| cleaned_env("DOUBAO_REGION"))
            .unwrap_or_else(|| "cn-beijing".to_string());
        Some(Self {
            access_key_id,
            secret_access_key,
            region,
        })
    }

    fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.starts_with('{') {
            let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
            let access_key_id = string_key(
                &value,
                &["accessKeyID", "accessKeyId", "access_key_id", "ak"],
            )?;
            let secret_access_key = string_key(
                &value,
                &[
                    "secretAccessKey",
                    "secret_access_key",
                    "secretKey",
                    "secret_key",
                    "sk",
                ],
            )?;
            let region =
                string_key(&value, &["region"]).unwrap_or_else(|| "cn-beijing".to_string());
            return Some(Self {
                access_key_id,
                secret_access_key,
                region,
            });
        }

        let parts = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(Self {
                access_key_id: parts[0].to_string(),
                secret_access_key: parts[1].to_string(),
                region: parts
                    .get(2)
                    .copied()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("cn-beijing")
                    .to_string(),
            });
        }
        None
    }
}

fn cleaned_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn string_key(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    })
}

#[derive(Debug, Deserialize)]
struct CodingPlanUsageResponse {
    #[serde(rename = "Result")]
    result: CodingPlanResult,
}

#[derive(Debug, Deserialize)]
struct CodingPlanResult {
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "UpdateTimestamp")]
    update_timestamp: Option<f64>,
    #[serde(rename = "QuotaUsage", default)]
    quota_usage: Vec<CodingPlanQuota>,
}

#[derive(Debug, Deserialize)]
struct CodingPlanQuota {
    #[serde(rename = "Level")]
    level: String,
    #[serde(rename = "Percent")]
    percent: f64,
    #[serde(rename = "ResetTimestamp")]
    reset_timestamp: Option<f64>,
}

fn decode_coding_plan_usage(bytes: &[u8]) -> Result<CodingPlanResult, ProviderError> {
    let response: CodingPlanUsageResponse = serde_json::from_slice(bytes)
        .map_err(|e| ProviderError::Parse(format!("Failed to parse Doubao Coding Plan: {e}")))?;
    Ok(response.result)
}

fn coding_plan_snapshot(usage: CodingPlanResult) -> UsageSnapshot {
    let primary = coding_plan_window(
        &usage,
        &["session", "5-hour", "five_hour", "5h"],
        Some(5 * 60),
    )
    // A missing Coding Plan session is not a measured 0%: keep the canonical
    // informational placeholder so Agent Plan-only accounts do not draw one.
    .unwrap_or_else(RateWindow::no_active_session);
    let mut snapshot = UsageSnapshot::new(primary);
    if let Some(weekly) = coding_plan_window(&usage, &["weekly", "week"], Some(7 * 24 * 60)) {
        snapshot = snapshot.with_secondary(weekly);
    }
    if let Some(monthly) = coding_plan_window(&usage, &["monthly", "month"], Some(30 * 24 * 60)) {
        snapshot = snapshot.with_tertiary(monthly);
    }
    // Agent / team plan buckets from arkcli (level prefixes).
    for (prefix, id_prefix) in [
        ("agent_", "doubao-agent"),
        ("coding_team_", "doubao-coding-team"),
        ("agent_team_", "doubao-agent-team"),
    ] {
        if let Some(w) = coding_plan_window(
            &usage,
            &[
                &format!("{prefix}session"),
                &format!("{prefix}5-hour"),
                &format!("{prefix}five_hour"),
                &format!("{prefix}5h"),
            ],
            Some(5 * 60),
        ) {
            snapshot.extra_rate_windows.push(with_agent_icon_lane(
                NamedRateWindow::new(format!("{id_prefix}-session"), "5-hour", w),
                prefix,
                IconLane::Primary,
            ));
        }
        if let Some(w) = coding_plan_window(
            &usage,
            &[&format!("{prefix}weekly"), &format!("{prefix}week")],
            Some(7 * 24 * 60),
        ) {
            snapshot.extra_rate_windows.push(with_agent_icon_lane(
                NamedRateWindow::new(format!("{id_prefix}-weekly"), "Weekly", w),
                prefix,
                IconLane::Secondary,
            ));
        }
        if let Some(w) = coding_plan_window(
            &usage,
            &[&format!("{prefix}monthly"), &format!("{prefix}month")],
            Some(30 * 24 * 60),
        ) {
            snapshot.extra_rate_windows.push(NamedRateWindow::new(
                format!("{id_prefix}-monthly"),
                "Monthly",
                w,
            ));
        }
    }
    if let Some(status) = usage.status.filter(|s| !s.trim().is_empty()) {
        snapshot = snapshot.with_login_method(status);
    }
    if let Some(update) = usage.update_timestamp.and_then(datetime_from_epoch) {
        snapshot.updated_at = update;
    }
    snapshot
}

/// Only the personal Agent Plan session/weekly lanes may stand in for a missing
/// Coding Plan lane on the tray icon; team and monthly buckets never do.
fn with_agent_icon_lane(
    lane: NamedRateWindow,
    level_prefix: &str,
    icon_lane: IconLane,
) -> NamedRateWindow {
    if level_prefix == "agent_" {
        lane.with_icon_fallback(icon_lane)
    } else {
        lane
    }
}

fn coding_plan_window(
    usage: &CodingPlanResult,
    levels: &[&str],
    minutes: Option<u32>,
) -> Option<RateWindow> {
    let quota = usage.quota_usage.iter().find(|quota| {
        let level = quota.level.to_ascii_lowercase();
        levels.iter().any(|candidate| *candidate == level)
    })?;
    let resets_at = quota.reset_timestamp.and_then(datetime_from_epoch);
    // Monthly windows: expand the 30-day sentinel to the real calendar cycle.
    let window_minutes = match minutes {
        Some(m) if m == 30 * 24 * 60 => RateWindow::monthly_window_minutes(resets_at).or(Some(m)),
        other => other,
    };
    Some(RateWindow::with_details(
        quota.percent,
        window_minutes,
        resets_at,
        None,
    ))
}

struct SignedVolcengineRequest {
    content_type: &'static str,
    host: String,
    timestamp: String,
    payload_hash: String,
    authorization: String,
}

fn sign_volcengine_request(
    credentials: &DoubaoCodingPlanCredentials,
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<SignedVolcengineRequest, ProviderError> {
    let parsed = reqwest::Url::parse(DOUBAO_CODING_PLAN_URL)
        .map_err(|e| ProviderError::Other(format!("Invalid Doubao Coding Plan URL: {e}")))?;
    let host = parsed
        .host_str()
        .unwrap_or("open.volcengineapi.com")
        .to_string();
    let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let payload_hash = sha256_hex(body);
    let content_type = "application/x-www-form-urlencoded; charset=utf-8";
    let signed_headers = "content-type;host;x-content-sha256;x-date";
    let canonical_request = [
        "POST".to_string(),
        canonical_uri(&parsed),
        canonical_query_string(&parsed),
        format!("content-type:{content_type}"),
        format!("host:{host}"),
        format!("x-content-sha256:{payload_hash}"),
        format!("x-date:{timestamp}"),
        String::new(),
        signed_headers.to_string(),
        payload_hash.clone(),
    ]
    .join("\n");
    let credential_scope = format!("{}/{}/ark/request", date_stamp, credentials.region);
    let string_to_sign = [
        "HMAC-SHA256".to_string(),
        timestamp.clone(),
        credential_scope.clone(),
        sha256_hex(canonical_request.as_bytes()),
    ]
    .join("\n");
    let date_key = hmac_sha256(
        credentials.secret_access_key.as_bytes(),
        date_stamp.as_bytes(),
    );
    let region_key = hmac_sha256(&date_key, credentials.region.as_bytes());
    let service_key = hmac_sha256(&region_key, b"ark");
    let signing_key = hmac_sha256(&service_key, b"request");
    let signature = hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    let authorization = format!(
        "HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        credentials.access_key_id
    );
    Ok(SignedVolcengineRequest {
        content_type,
        host,
        timestamp,
        payload_hash,
        authorization,
    })
}

fn canonical_uri(url: &reqwest::Url) -> String {
    let path = url.path();
    if path.is_empty() {
        "/".to_string()
    } else {
        percent_encode(path, false)
    }
}

fn canonical_query_string(url: &reqwest::Url) -> String {
    let mut pairs = url
        .query_pairs()
        .map(|(key, value)| (percent_encode(&key, true), percent_encode(&value, true)))
        .collect::<Vec<_>>();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode(value: &str, encode_slash: bool) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            let keep = byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.' | b'~')
                || (!encode_slash && byte == b'/');
            if keep {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

fn sanitized_body(body: &str) -> String {
    crate::core::sanitized_body(body, 200)
}

impl Default for DoubaoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for DoubaoProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Doubao
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        if ctx.token_account_isolated
            && ctx.token_account_kind == Some(crate::core::TokenAccountKind::ApiKey)
        {
            let api_key = selected_ark_api_key(ctx)?;
            return Ok(ProviderFetchResult::new(
                self.fetch_api(&api_key).await?,
                "api",
            ));
        }
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                if let Some(credentials) = Self::coding_plan_credentials(ctx.api_key.as_deref()) {
                    return Ok(ProviderFetchResult::new(
                        self.fetch_coding_plan(&credentials).await?,
                        "coding-plan",
                    ));
                }
                // Prefer arkcli when available so Agent Plan / team quotas surface
                // without signed API credentials (upstream 0.45).
                if resolve_arkcli_binary().is_some() {
                    match fetch_arkcli_usage() {
                        Ok(snap) => return Ok(ProviderFetchResult::new(snap, "arkcli")),
                        Err(ProviderError::AuthRequired) => {
                            return Err(ProviderError::AuthRequired);
                        }
                        Err(ProviderError::NotInstalled(_)) => {}
                        Err(_) => {
                            // Fall through to request-header probe if configured.
                        }
                    }
                }
                let api_key = Self::api_key(ctx.api_key.as_deref())?;
                Ok(ProviderFetchResult::new(
                    self.fetch_api(&api_key).await?,
                    "api",
                ))
            }
            SourceMode::Cli => {
                let snap = fetch_arkcli_usage()?;
                Ok(ProviderFetchResult::new(snap, "arkcli"))
            }
            SourceMode::Web => Err(ProviderError::UnsupportedSource(ctx.source_mode)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth, SourceMode::Cli]
    }

    /// Stays `false` although `SourceMode::Cli` is listed (existing behavior).
    fn supports_cli(&self) -> bool {
        false
    }

    /// Doubao's arkcli probe raises `NotInstalled` when the `arkcli`
    /// binary is missing from PATH/`ARKCLI_PATH` ("arkcli was not found.
    /// Install arkcli, ...") — an installation gap, not a credential
    /// problem — so it surfaces as an offline local runtime (matching the
    /// pre-backend classifier's treatment of CLI-presence failures). The
    /// guard is message-scoped: the shared "API key not found" producer
    /// keeps the default sign-in mapping.
    fn error_state_kind(&self, error: &ProviderError) -> crate::core::ProviderStateKind {
        match error {
            ProviderError::NotInstalled(msg) if msg.contains("arkcli was not found") => {
                crate::core::ProviderStateKind::LocalRuntimeOffline
            }
            _ => error.state_kind(),
        }
    }
}

fn selected_ark_api_key(ctx: &FetchContext) -> Result<String, ProviderError> {
    ctx.api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .ok_or(ProviderError::AuthRequired)
}

#[cfg(test)]
mod icon_lane_tests;

#[cfg(test)]
mod tests;
