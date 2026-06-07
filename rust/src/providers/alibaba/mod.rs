//! Alibaba Cloud Model Studio – Coding Plan provider
//!
//! Uses the same-origin console data gateway (e.g.
//! `modelstudio.console.alibabacloud.com` for the international regions), which
//! is not protected by Alibaba's ISG bot detection.
//!
//! Flow:
//!   1. GET the dashboard page to extract SEC_TOKEN from the page HTML.
//!   2. POST to `/data/api.json` with the full params JSON and sec_token.
//!
//! Auth: browser cookies from the region's console domain.
//!
//! All region-specific behaviour (gateway, region code, cookie domains,
//! dashboard URL and gateway request constants) lives in [`AlibabaRegion`],
//! the single source of truth — mirroring the `MiniMaxRegion` pattern.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use crate::browser::cookies::get_cookie_header;
use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";

/// How long a scraped SEC_TOKEN stays valid in the in-memory cache.
const SEC_TOKEN_TTL: Duration = Duration::from_secs(25 * 60);

// ─── Canonical region model ──────────────────────────────────────────────

/// Per-platform request constants for the console data gateway. These differ
/// between the international Model Studio console and the China-mainland
/// Bailian console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlibabaRequestProfile {
    /// Base origin of the console data gateway (no trailing slash).
    pub gateway: &'static str,
    /// `action` gateway parameter.
    pub api_action: &'static str,
    /// `product` gateway parameter.
    pub api_product: &'static str,
    /// Fully-qualified gateway API method name.
    pub api_method: &'static str,
    /// Coding Plan commodity code queried.
    pub commodity_code: &'static str,
    /// `switchAgent` cornerstone parameter (fixed per console).
    pub switch_agent: u64,
    /// `switchUserType` cornerstone parameter (fixed per console).
    pub switch_user_type: u64,
    /// `consoleSite` cornerstone parameter.
    pub console_site: &'static str,
    /// `domain` cornerstone parameter (the console host).
    pub console_domain: &'static str,
}

/// Verified request profile for the international Model Studio console
/// (Singapore / US / Germany / Hong Kong share this — only the region code
/// in the referer/feURL differs).
const INTL_PROFILE: AlibabaRequestProfile = AlibabaRequestProfile {
    gateway: "https://modelstudio.console.alibabacloud.com",
    api_action: "IntlBroadScopeAspnGateway",
    api_product: "sfm_bailian",
    api_method: "zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2",
    commodity_code: "sfm_codingplan_public_intl",
    switch_agent: 313762,
    switch_user_type: 3,
    console_site: "MODELSTUDIO_ALBABACLOUD",
    console_domain: "modelstudio.console.alibabacloud.com",
};

/// Alibaba Coding Plan region — single source of truth for everything that
/// varies by region: settings value, UI label, gateway, API region code,
/// cookie domains, dashboard URL and gateway request constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlibabaRegion {
    Singapore,
    UsEast,
    Germany,
    HongKong,
    ChinaMainland,
}

impl AlibabaRegion {
    /// Regions exposed in the settings UI.
    ///
    /// China Mainland routes to its own console gateway
    /// (`bailian.console.alibabacloud.com`) and CN cookie domains — it never
    /// shares the international endpoint. Its gateway request constants
    /// (commodity code / action) currently inherit the international values
    /// pending a verified China-mainland capture; see [`Self::request_profile`].
    pub const ALL: &'static [AlibabaRegion] = &[
        Self::Singapore,
        Self::UsEast,
        Self::Germany,
        Self::HongKong,
        Self::ChinaMainland,
    ];

    /// Parse the persisted `api_region` settings value (tolerant of aliases).
    pub fn from_settings_value(value: Option<&str>) -> Self {
        match value.unwrap_or_default().trim().to_lowercase().as_str() {
            "us" | "us-east-1" | "useast" | "us-west-1" => Self::UsEast,
            "germany" | "eu" | "eu-central-1" | "frankfurt" => Self::Germany,
            "hongkong" | "hong-kong" | "hk" | "cn-hongkong" | "ap-east-1" => Self::HongKong,
            "cn" | "china" | "china-mainland" | "china_mainland" | "mainland" => {
                Self::ChinaMainland
            }
            // singapore / intl / ap-southeast-1 / unrecognised → Singapore
            _ => Self::Singapore,
        }
    }

    /// Canonical persisted settings value.
    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Singapore => "singapore",
            Self::UsEast => "us",
            Self::Germany => "germany",
            Self::HongKong => "hongkong",
            Self::ChinaMainland => "cn",
        }
    }

    /// Human-readable label for the settings region picker.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Singapore => "International – Singapore (ap-southeast-1)",
            Self::UsEast => "International – US (us-east-1)",
            Self::Germany => "International – Germany (eu-central-1)",
            Self::HongKong => "International – Hong Kong (cn-hongkong)",
            Self::ChinaMainland => "China Mainland (Bailian)",
        }
    }

    /// Alibaba Cloud region code used in the dashboard path and referer.
    pub fn region_code(self) -> &'static str {
        match self {
            Self::Singapore => "ap-southeast-1",
            Self::UsEast => "us-east-1",
            Self::Germany => "eu-central-1",
            Self::HongKong => "cn-hongkong",
            Self::ChinaMainland => "cn-hangzhou",
        }
    }

    pub fn is_china(self) -> bool {
        matches!(self, Self::ChinaMainland)
    }

    /// Browser cookie domains to try (in priority order) for auto-import.
    pub fn cookie_domains(self) -> &'static [&'static str] {
        match self {
            Self::ChinaMainland => &["bailian.console.alibabacloud.com", "alibabacloud.com"],
            _ => &["modelstudio.console.alibabacloud.com", "alibabacloud.com"],
        }
    }

    /// Primary cookie domain shown in the browser-import hint.
    pub fn primary_cookie_domain(self) -> &'static str {
        self.cookie_domains()[0]
    }

    /// Per-platform gateway request constants.
    pub fn request_profile(self) -> AlibabaRequestProfile {
        match self {
            Self::ChinaMainland => AlibabaRequestProfile {
                gateway: "https://bailian.console.alibabacloud.com",
                console_domain: "bailian.console.alibabacloud.com",
                // The China-mainland console uses a different commodity code /
                // action than the international plan; these inherit the intl
                // values until a real CN request is captured. The gateway and
                // cookie domains above are CN-correct, so CN cookies are never
                // sent to the international endpoint.
                ..INTL_PROFILE
            },
            _ => INTL_PROFILE,
        }
    }

    /// Console data-gateway origin.
    pub fn gateway(self) -> &'static str {
        self.request_profile().gateway
    }

    /// Dashboard URL for the browser-import hint and the "open dashboard" link.
    pub fn dashboard_url(self) -> String {
        match self {
            Self::ChinaMainland => self.gateway().to_string(),
            _ => format!("{}/{}", self.gateway(), self.region_code()),
        }
    }
}

// ─── SEC_TOKEN cache (keyed by region + cookie identity) ─────────────────

#[derive(Clone)]
struct CachedSecToken {
    token: String,
    fetched_at: Instant,
}

fn token_cache() -> &'static RwLock<HashMap<String, CachedSecToken>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CachedSecToken>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Cache key bound to the real auth boundary: the region code plus a hash of
/// the cookie header (account / cookie identity). Changing region, account,
/// or cookies yields a different key, so a stale token is never reused.
fn sec_token_cache_key(region_code: &str, cookies: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cookies.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{region_code}:{hex}")
}

fn cached_sec_token(key: &str) -> Option<String> {
    let cache = token_cache().read().ok()?;
    let entry = cache.get(key)?;
    (entry.fetched_at.elapsed() < SEC_TOKEN_TTL).then(|| entry.token.clone())
}

fn store_sec_token(key: &str, token: &str) {
    if let Ok(mut cache) = token_cache().write() {
        cache.insert(
            key.to_string(),
            CachedSecToken {
                token: token.to_string(),
                fetched_at: Instant::now(),
            },
        );
    }
}

fn invalidate_sec_token(key: &str) {
    if let Ok(mut cache) = token_cache().write() {
        cache.remove(key);
    }
}

// ─── Provider ────────────────────────────────────────────────────────────

pub struct AlibabaProvider {
    metadata: ProviderMetadata,
}

impl AlibabaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Alibaba,
                display_name: "Alibaba",
                session_label: "5-Hour",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://modelstudio.console.alibabacloud.com"),
                status_page_url: None,
            },
        }
    }

    /// Resolve the [`AlibabaRegion`] from a settings value.
    pub fn region_from_settings(value: Option<&str>) -> AlibabaRegion {
        AlibabaRegion::from_settings_value(value)
    }

    /// Cookie domain for the browser import UI, driven by the selected region.
    pub fn cookie_domain_for_region(value: Option<&str>) -> &'static str {
        Self::region_from_settings(value).primary_cookie_domain()
    }

    /// Dashboard URL for the selected region.
    pub fn dashboard_url_for_region(value: Option<&str>) -> String {
        Self::region_from_settings(value).dashboard_url()
    }

    fn resolve_cookies(&self, ctx: &FetchContext) -> Result<String, ProviderError> {
        if let Some(ref manual) = ctx.manual_cookie_header {
            let trimmed = manual.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
        let region = AlibabaRegion::from_settings_value(ctx.api_region.as_deref());
        for domain in region.cookie_domains() {
            match get_cookie_header(domain) {
                Ok(cookies) if !cookies.is_empty() => return Ok(cookies),
                _ => {}
            }
        }
        Err(ProviderError::AuthRequired)
    }

    /// Return the SEC_TOKEN, using a region+account-keyed cache to avoid
    /// fetching the dashboard page on every provider refresh. `force_fresh`
    /// bypasses the cache (used when retrying after an auth failure).
    async fn resolve_sec_token(
        &self,
        client: &reqwest::Client,
        cookies: &str,
        region: AlibabaRegion,
        cache_key: &str,
        force_fresh: bool,
    ) -> Option<String> {
        let cached = if force_fresh {
            None
        } else {
            cached_sec_token(cache_key)
        };
        if let Some(token) = cached {
            return Some(token);
        }
        let token = self.fetch_sec_token(client, cookies, region).await?;
        store_sec_token(cache_key, &token);
        Some(token)
    }

    async fn fetch_sec_token(
        &self,
        client: &reqwest::Client,
        cookies: &str,
        region: AlibabaRegion,
    ) -> Option<String> {
        let dashboard_url = format!("{}/{}?tab=plan", region.gateway(), region.region_code());
        let resp = client
            .get(&dashboard_url)
            .header("Cookie", cookies)
            .header("User-Agent", USER_AGENT)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return extract_cookie_value("sec_token", cookies);
        }
        let html = resp.text().await.ok()?;
        extract_sec_token(&html).or_else(|| extract_cookie_value("sec_token", cookies))
    }

    async fn fetch_via_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let cookies = self.resolve_cookies(ctx)?;
        let region = AlibabaRegion::from_settings_value(ctx.api_region.as_deref());
        let cache_key = sec_token_cache_key(region.region_code(), &cookies);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(15)))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        // Try with a cached token first; on an auth failure the token may be
        // stale or rotated, so invalidate it and retry once with a fresh one.
        let mut force_fresh = false;
        let mut last_err = ProviderError::AuthRequired;
        for _attempt in 0..2 {
            let sec_token = self
                .resolve_sec_token(&client, &cookies, region, &cache_key, force_fresh)
                .await;
            match self
                .request_quota(&client, &cookies, region, sec_token)
                .await
            {
                Ok(usage) => return Ok(usage),
                Err(ProviderError::AuthRequired) => {
                    invalidate_sec_token(&cache_key);
                    force_fresh = true;
                    last_err = ProviderError::AuthRequired;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err)
    }

    async fn request_quota(
        &self,
        client: &reqwest::Client,
        cookies: &str,
        region: AlibabaRegion,
        sec_token: Option<String>,
    ) -> Result<UsageSnapshot, ProviderError> {
        let profile = region.request_profile();
        let cna = extract_cookie_value("cna", cookies).unwrap_or_default();
        let referer = format!("{}/{}?tab=plan", profile.gateway, region.region_code());
        let fe_url = format!("{referer}#/efm/subscription/coding-plan");

        let params = serde_json::json!({
            "Api": profile.api_method,
            "V": "1.0",
            "Data": {
                "queryCodingPlanInstanceInfoRequest": {
                    "commodityCode": profile.commodity_code,
                    "onlyLatestOne": true
                },
                "cornerstoneParam": {
                    "feTraceId": uuid::Uuid::new_v4().to_string(),
                    "feURL": fe_url,
                    "protocol": "V2",
                    "console": "ONE_CONSOLE",
                    "productCode": "p_efm",
                    "switchAgent": profile.switch_agent,
                    "switchUserType": profile.switch_user_type,
                    "domain": profile.console_domain,
                    "consoleSite": profile.console_site,
                    "userNickName": "",
                    "userPrincipalName": "",
                    "xsp_lang": "en-US",
                    "X-Anonymous-Id": cna
                }
            }
        });

        let url = format!(
            "{}/data/api.json?action={}&product={}",
            profile.gateway, profile.api_action, profile.api_product
        );

        let mut form = vec![
            ("action", profile.api_action.to_string()),
            ("product", profile.api_product.to_string()),
            ("api", profile.api_method.to_string()),
            ("_v", "undefined".to_string()),
            ("params", params.to_string()),
        ];
        if let Some(token) = sec_token.filter(|t| !t.is_empty()) {
            form.push(("sec_token", token));
        }

        let resp = client
            .post(&url)
            .header("Cookie", cookies)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "*/*")
            .header("Origin", profile.gateway)
            .header("Referer", &referer)
            .header("User-Agent", USER_AGENT)
            .header("sec-fetch-site", "same-origin")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-dest", "empty")
            .form(&form)
            .send()
            .await?;

        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!("HTTP {status}")));
        }

        let body = resp.bytes().await?;

        // Detect HTML login-redirect pages (may have leading whitespace before '<').
        let first_nonws = body.iter().find(|&&b| !b.is_ascii_whitespace()).copied();
        if first_nonws == Some(b'<') {
            return Err(ProviderError::AuthRequired);
        }

        let json: serde_json::Value =
            serde_json::from_slice(&body).map_err(|e| ProviderError::Parse(e.to_string()))?;

        self.parse_response(&json)
    }

    fn parse_response(&self, json: &serde_json::Value) -> Result<UsageSnapshot, ProviderError> {
        let top_code = json.get("code").and_then(|v| v.as_str()).unwrap_or("");
        if !top_code.is_empty() && top_code != "200" {
            if top_code == "401" || top_code == "403" {
                return Err(ProviderError::AuthRequired);
            }
            let msg = json
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or(top_code);
            return Err(ProviderError::Other(format!("API error: {msg}")));
        }

        // Authorization failures arrive as a 200 with an error ret/code.
        if let Some(ret) = json.pointer("/data/DataV2/ret").and_then(|v| v.as_array()) {
            let joined: String = ret
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(";");
            if joined.contains("No Authority")
                || joined.contains("10032390")
                || joined.contains("NeedLogin")
            {
                return Err(ProviderError::AuthRequired);
            }
        }

        let instances = json
            .pointer("/data/DataV2/data/data/codingPlanInstanceInfos")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ProviderError::Parse("codingPlanInstanceInfos not found in response".into())
            })?;

        let instance = instances
            .iter()
            .find(|i| i.get("status").and_then(|s| s.as_str()) == Some("VALID"))
            .or_else(|| instances.first())
            .ok_or_else(|| ProviderError::Parse("no Coding Plan instance in response".into()))?;

        let quota = instance
            .get("codingPlanQuotaInfo")
            .ok_or_else(|| ProviderError::Parse("codingPlanQuotaInfo missing".into()))?;

        let plan_name = instance
            .get("instanceName")
            .and_then(|v| v.as_str())
            .unwrap_or("Coding Plan");

        let ms_to_dt = |key: &str| -> Option<DateTime<Utc>> {
            quota
                .get(key)
                .and_then(|v| v.as_i64())
                .and_then(|ms| Utc.timestamp_opt(ms / 1000, 0).single())
        };

        let pct = |used_key: &str, total_key: &str| -> f64 {
            let used = quota.get(used_key).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let total = quota.get(total_key).and_then(|v| v.as_f64()).unwrap_or(1.0);
            if total > 0.0 {
                (used / total * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            }
        };

        let detail = |used_key: &str, total_key: &str| -> Option<String> {
            let used = quota.get(used_key).and_then(|v| v.as_f64())?;
            let total = quota.get(total_key).and_then(|v| v.as_f64())?;
            Some(format!(
                "{} / {} tokens",
                fmt_tokens(used as i64),
                fmt_tokens(total as i64)
            ))
        };

        let five_hour = RateWindow::with_details(
            pct("per5HourUsedQuota", "per5HourTotalQuota"),
            Some(300),
            ms_to_dt("per5HourQuotaNextRefreshTime"),
            detail("per5HourUsedQuota", "per5HourTotalQuota"),
        );
        let weekly = RateWindow::with_details(
            pct("perWeekUsedQuota", "perWeekTotalQuota"),
            Some(7 * 24 * 60),
            ms_to_dt("perWeekQuotaNextRefreshTime"),
            detail("perWeekUsedQuota", "perWeekTotalQuota"),
        );
        let monthly = RateWindow::with_details(
            pct("perBillMonthUsedQuota", "perBillMonthTotalQuota"),
            Some(30 * 24 * 60),
            ms_to_dt("perBillMonthQuotaNextRefreshTime"),
            detail("perBillMonthUsedQuota", "perBillMonthTotalQuota"),
        );

        Ok(UsageSnapshot::new(five_hour)
            .with_secondary(weekly)
            .with_tertiary(monthly)
            .with_login_method(plan_name))
    }
}

fn extract_sec_token(html: &str) -> Option<String> {
    let pos = html.find("SEC_TOKEN")?;
    let rest = &html[pos + "SEC_TOKEN".len()..];
    let start = rest.find('"')? + 1;
    let rest = &rest[start..];
    let end = rest.find('"')?;
    let token = rest[..end].trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn extract_cookie_value(name: &str, cookie_header: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

fn fmt_tokens(n: i64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

impl Default for AlibabaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AlibabaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Alibaba
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!(region = ?ctx.api_region, "Fetching Alibaba Coding Plan usage");
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let usage = self.fetch_via_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_response() -> serde_json::Value {
        serde_json::json!({
            "code": "200",
            "data": {
                "DataV2": {
                    "data": {
                        "data": {
                            "codingPlanInstanceInfos": [{
                                "instanceName": "Coding Plan Pro",
                                "status": "VALID",
                                "codingPlanQuotaInfo": {
                                    "per5HourUsedQuota": 0,
                                    "per5HourTotalQuota": 6000,
                                    "per5HourQuotaNextRefreshTime": 1780731422000_i64,
                                    "perWeekUsedQuota": 2019,
                                    "perWeekTotalQuota": 45000,
                                    "perWeekQuotaNextRefreshTime": 1780848000000_i64,
                                    "perBillMonthUsedQuota": 25,
                                    "perBillMonthTotalQuota": 90000,
                                    "perBillMonthQuotaNextRefreshTime": 1783267200000_i64
                                }
                            }]
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn parses_real_response_shape() {
        let provider = AlibabaProvider::new();
        let usage = provider.parse_response(&sample_response()).unwrap();

        assert!((usage.primary.used_percent - 0.0).abs() < 0.01); // 5h: 0/6000
        assert_eq!(usage.primary.window_minutes, Some(300));
        assert!(usage.primary.resets_at.is_some());

        let weekly = usage.secondary.unwrap();
        assert!((weekly.used_percent - 4.487).abs() < 0.01); // 2019/45000

        let monthly = usage.tertiary.unwrap();
        assert!((monthly.used_percent - 0.028).abs() < 0.01); // 25/90000

        assert_eq!(usage.login_method.as_deref(), Some("Coding Plan Pro"));
    }

    #[test]
    fn picks_valid_instance_over_first() {
        let provider = AlibabaProvider::new();
        let json = serde_json::json!({
            "code": "200",
            "data": { "DataV2": { "data": { "data": {
                "codingPlanInstanceInfos": [
                    {
                        "instanceName": "Expired",
                        "status": "EXPIRED",
                        "codingPlanQuotaInfo": {
                            "per5HourUsedQuota": 100, "per5HourTotalQuota": 100,
                            "perWeekUsedQuota": 100, "perWeekTotalQuota": 100,
                            "perBillMonthUsedQuota": 100, "perBillMonthTotalQuota": 100
                        }
                    },
                    {
                        "instanceName": "Coding Plan Pro",
                        "status": "VALID",
                        "codingPlanQuotaInfo": {
                            "per5HourUsedQuota": 0, "per5HourTotalQuota": 6000,
                            "perWeekUsedQuota": 10, "perWeekTotalQuota": 45000,
                            "perBillMonthUsedQuota": 25, "perBillMonthTotalQuota": 90000
                        }
                    }
                ]
            }}}}
        });
        let usage = provider.parse_response(&json).unwrap();
        assert_eq!(usage.login_method.as_deref(), Some("Coding Plan Pro"));
        assert!(usage.primary.used_percent < 1.0);
    }

    #[test]
    fn no_authority_response_maps_to_auth_required() {
        let provider = AlibabaProvider::new();
        let json = serde_json::json!({
            "code": "200",
            "data": { "DataV2": { "ret": ["10032390::No Authority"], "data": {} } }
        });
        assert!(matches!(
            provider.parse_response(&json),
            Err(ProviderError::AuthRequired)
        ));
    }

    #[test]
    fn region_from_settings_value_round_trips() {
        for region in AlibabaRegion::ALL {
            assert_eq!(
                AlibabaRegion::from_settings_value(Some(region.settings_value())),
                *region
            );
        }
        // Defaults and aliases.
        assert_eq!(
            AlibabaRegion::from_settings_value(None),
            AlibabaRegion::Singapore
        );
        assert_eq!(
            AlibabaRegion::from_settings_value(Some("intl")),
            AlibabaRegion::Singapore
        );
        assert_eq!(
            AlibabaRegion::from_settings_value(Some("cn")),
            AlibabaRegion::ChinaMainland
        );
    }

    #[test]
    fn region_codes_are_correct() {
        assert_eq!(AlibabaRegion::Singapore.region_code(), "ap-southeast-1");
        assert_eq!(AlibabaRegion::UsEast.region_code(), "us-east-1");
        assert_eq!(AlibabaRegion::Germany.region_code(), "eu-central-1");
        assert_eq!(AlibabaRegion::HongKong.region_code(), "cn-hongkong");
    }

    #[test]
    fn china_routes_to_its_own_gateway_and_cookies() {
        // Regression for the review: CN must not ride the international gateway.
        let cn = AlibabaRegion::ChinaMainland;
        assert!(cn.is_china());
        assert_eq!(cn.gateway(), "https://bailian.console.alibabacloud.com");
        assert_ne!(cn.gateway(), AlibabaRegion::Singapore.gateway());
        assert_eq!(
            cn.primary_cookie_domain(),
            "bailian.console.alibabacloud.com"
        );
        assert_ne!(
            cn.primary_cookie_domain(),
            AlibabaRegion::Singapore.primary_cookie_domain()
        );
    }

    #[test]
    fn international_regions_share_one_gateway() {
        let intl = "https://modelstudio.console.alibabacloud.com";
        for region in [
            AlibabaRegion::Singapore,
            AlibabaRegion::UsEast,
            AlibabaRegion::Germany,
            AlibabaRegion::HongKong,
        ] {
            assert_eq!(region.gateway(), intl);
            assert!(!region.is_china());
        }
    }

    #[test]
    fn exposed_regions_include_china() {
        assert!(AlibabaRegion::ALL.contains(&AlibabaRegion::ChinaMainland));
        assert_eq!(AlibabaRegion::ALL.len(), 5);
    }

    #[test]
    fn cookie_domain_for_region_mapping() {
        assert_eq!(
            AlibabaProvider::cookie_domain_for_region(None),
            "modelstudio.console.alibabacloud.com"
        );
        assert_eq!(
            AlibabaProvider::cookie_domain_for_region(Some("cn")),
            "bailian.console.alibabacloud.com"
        );
    }

    #[test]
    fn sec_token_cache_key_distinguishes_region_and_cookies() {
        let a = sec_token_cache_key("ap-southeast-1", "cookie=one");
        let b = sec_token_cache_key("us-east-1", "cookie=one");
        let c = sec_token_cache_key("ap-southeast-1", "cookie=two");
        assert_ne!(a, b); // region changes the key
        assert_ne!(a, c); // cookies change the key
        assert_eq!(a, sec_token_cache_key("ap-southeast-1", "cookie=one")); // stable
    }

    #[test]
    fn extract_sec_token_from_html() {
        let html = r#"var config = { SEC_TOKEN: "AvLZTKds7DW5utd3p5xm48", OTHER: "x" };"#;
        assert_eq!(
            extract_sec_token(html).as_deref(),
            Some("AvLZTKds7DW5utd3p5xm48")
        );
    }

    #[test]
    fn fmt_tokens_formats_correctly() {
        assert_eq!(fmt_tokens(6000), "6,000");
        assert_eq!(fmt_tokens(90000), "90,000");
        assert_eq!(fmt_tokens(25), "25");
        assert_eq!(fmt_tokens(1000000), "1,000,000");
    }
}
