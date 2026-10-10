//! MiniMax provider implementation
//!
//! Fetches usage data from MiniMax AI API
//! MiniMax stores API keys locally or in environment

mod billing;
mod coding_plan;
mod coding_plan_html;
mod remains_api;
mod token_plan;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::PathBuf;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, RateWindow, SourceMode,
    UsageSnapshot,
};
use billing::{MiniMaxBillingSummary, attach_billing_summary, parse_billing_summary};

const CODING_PLAN_PATH: &str = "/user-center/payment/coding-plan";
const CODING_PLAN_QUERY: &str = "cycle_type=3";
const JSON_ACCEPT: &str = "application/json, text/plain, */*";

fn http_client() -> Result<reqwest::Client, ProviderError> {
    crate::core::credentialed_http_client_builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ProviderError::Other(e.to_string()))
}

fn is_auth_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
}

/// 401/403 read as AuthRequired and other failures as
/// "MiniMax {what} returned status {status}". With `not_found_is_parse`,
/// 404/405 read as Parse so the caller tries its next URL.
fn check_status(
    status: reqwest::StatusCode,
    what: &str,
    not_found_is_parse: bool,
) -> Result<(), ProviderError> {
    if is_auth_status(status) {
        return Err(ProviderError::AuthRequired);
    }
    if status.is_success() {
        return Ok(());
    }
    let message = format!("MiniMax {what} returned status {status}");
    if not_found_is_parse
        && (status == reqwest::StatusCode::NOT_FOUND
            || status == reqwest::StatusCode::METHOD_NOT_ALLOWED)
    {
        return Err(ProviderError::Parse(message));
    }
    Err(ProviderError::Other(message))
}

/// Browser-shaped console GET: cookie, `accept`, the optional XHR marker,
/// then the Chrome UA, language, origin and coding-plan referer.
fn console_get(
    client: &reqwest::Client,
    url: &str,
    cookie_header: &str,
    region: MiniMaxRegion,
    accept: &str,
    xhr: bool,
) -> reqwest::RequestBuilder {
    let base = region.base_url();
    let mut request = client
        .get(url)
        .header("Cookie", cookie_header)
        .header("Accept", accept);
    if xhr {
        request = request.header("X-Requested-With", "XMLHttpRequest");
    }
    request
        .header("User-Agent", MiniMaxProvider::WEB_USER_AGENT)
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Origin", base)
        .header("Referer", format!("{base}/user-center/payment/coding-plan"))
}

/// MiniMax API region
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniMaxRegion {
    Global,
    ChinaMainland,
}

impl MiniMaxRegion {
    pub fn from_settings_value(value: Option<&str>) -> Self {
        match value.unwrap_or_default().trim().to_lowercase().as_str() {
            "cn" | "china" | "china-mainland" | "china_mainland" | "mainland" => {
                Self::ChinaMainland
            }
            _ => Self::Global,
        }
    }

    pub fn settings_value(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::ChinaMainland => "cn",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Global => "Global (platform.minimax.io)",
            Self::ChinaMainland => "China mainland (platform.minimaxi.com)",
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Self::Global => "https://platform.minimax.io",
            Self::ChinaMainland => "https://platform.minimaxi.com",
        }
    }

    pub fn api_base_url(self) -> &'static str {
        match self {
            Self::Global => "https://api.minimax.io",
            Self::ChinaMainland => "https://api.minimaxi.com",
        }
    }

    pub fn cookie_domain(self) -> &'static str {
        match self {
            Self::Global => "platform.minimax.io",
            Self::ChinaMainland => "platform.minimaxi.com",
        }
    }

    /// WWW host for the remains-API fallback (apex domain for cookie search).
    pub fn www_base_url(self) -> &'static str {
        match self {
            Self::Global => "https://www.minimax.io",
            Self::ChinaMainland => "https://www.minimaxi.com",
        }
    }

    /// Cookie search domains — apex so `cookies.rs::domain_matches` suffix-matches
    /// `platform.`/`www.` subdomains and parent-scoped session cookies.
    pub fn cookie_search_domains(self) -> [&'static str; 1] {
        match self {
            Self::Global => ["minimax.io"],
            Self::ChinaMainland => ["minimaxi.com"],
        }
    }

    /// Remains-API URL (platform host) — the coding-plan quota endpoint.
    pub fn coding_plan_remains_url(self) -> String {
        format!(
            "{}/v1/api/openplatform/coding_plan/remains",
            self.base_url()
        )
    }

    pub fn www_remains_url(self) -> String {
        format!(
            "{}/v1/api/openplatform/coding_plan/remains",
            self.www_base_url()
        )
    }

    pub fn coding_plan_url(self) -> String {
        format!(
            "{}{}?{}",
            self.base_url(),
            CODING_PLAN_PATH,
            CODING_PLAN_QUERY
        )
    }

    fn billing_history_url(self) -> String {
        format!("{}/account/amount", self.base_url())
    }
}

/// MiniMax provider
#[derive(Default)]
pub struct MiniMaxProvider;

impl MiniMaxProvider {
    pub fn new() -> Self {
        Self
    }

    pub fn region_from_settings(value: Option<&str>) -> MiniMaxRegion {
        MiniMaxRegion::from_settings_value(value)
    }

    pub fn dashboard_url_for_region(value: Option<&str>) -> String {
        Self::region_from_settings(value).coding_plan_url()
    }

    pub fn cookie_domain_for_region(value: Option<&str>) -> &'static str {
        Self::region_from_settings(value).cookie_domain()
    }

    /// Get MiniMax config directory
    fn get_minimax_config_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            dirs::config_dir().map(|p| p.join("minimax"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            dirs::home_dir().map(|p| p.join(".minimax"))
        }
    }

    /// Read MiniMax API key
    async fn read_api_key(&self) -> Result<(String, String), ProviderError> {
        // Check environment variables first
        if let (Ok(group_id), Ok(api_key)) = (
            std::env::var("MINIMAX_GROUP_ID"),
            std::env::var("MINIMAX_API_KEY"),
        ) {
            return Ok((group_id, api_key));
        }

        // Check config file
        let config_path = Self::get_minimax_config_path()
            .ok_or_else(|| ProviderError::NotInstalled("MiniMax config not found".to_string()))?;

        let config_file = config_path.join("config.json");
        if config_file.exists() {
            let content = tokio::fs::read_to_string(&config_file)
                .await
                .map_err(|e| ProviderError::Other(e.to_string()))?;

            let json: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| ProviderError::Parse(e.to_string()))?;

            let group_id = json
                .get("group_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let api_key = json
                .get("api_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let (Some(gid), Some(key)) = (group_id, api_key) {
                return Ok((gid, key));
            }
        }

        Err(ProviderError::AuthRequired)
    }

    /// Fetch usage via MiniMax API with region fallback
    async fn fetch_via_web(
        &self,
        ctx: &FetchContext,
        region: MiniMaxRegion,
    ) -> Result<ProviderFetchResult, ProviderError> {
        // Prefer the coding-plan remains endpoint (Win-CodexBar #425): the
        // console's usage/plan pages are client-rendered (Next.js `ssr:false`),
        // so no server HTML ever contains real numbers, even with a valid
        // cookie. The underlying `coding_plan/remains` endpoint instead
        // accepts a plain `Authorization: Bearer <api_key>` with no cookie at
        // all (no group_id needed), and returns the same `model_remains`
        // shape the cookie-based parser already understands. This key can
        // come from Settings (GUI-stored) or the environment, independent of
        // the dual group_id+api_key credential the legacy billing endpoint
        // below requires.
        if let Some(key) = Self::read_plain_api_key(ctx) {
            match remains_api::fetch_remains_via_api_key(&key, region).await {
                Ok(result) => return Ok(result),
                // Endpoint/shape incompatibility may still use the legacy
                // group-id path. Auth and transport failures are authoritative.
                Err(ProviderError::Parse(_)) => {}
                Err(error) => return Err(error),
            }
        }

        let (group_id, api_key) = self.read_api_key().await?;
        match self.fetch_from_region(&group_id, &api_key, region).await {
            Ok(result) => Ok(result),
            Err(ProviderError::AuthRequired) if region == MiniMaxRegion::Global => {
                self.fetch_from_region(&group_id, &api_key, MiniMaxRegion::ChinaMainland)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    /// A plain MiniMax API key from Settings or `MINIMAX_API_KEY`.
    fn read_plain_api_key(ctx: &FetchContext) -> Option<String> {
        remains_api::read_plain_api_key(ctx)
    }

    /// Fetch from a specific region endpoint
    async fn fetch_from_region(
        &self,
        group_id: &str,
        api_key: &str,
        region: MiniMaxRegion,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let client = http_client()?;

        let base_url = region.api_base_url();
        let resp = client
            .get(format!(
                "{}/v1/billing/usage?group_id={}",
                base_url, group_id
            ))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("MM-API-Source", "CodexBar")
            .send()
            .await?;
        check_status(resp.status(), "API", false)?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        let usage = self.parse_usage_response(&json)?;
        Ok(self.result_with_optional_billing(usage, "api", &json))
    }

    fn parse_usage_response(
        &self,
        json: &serde_json::Value,
    ) -> Result<UsageSnapshot, ProviderError> {
        // Parse MiniMax billing response
        let base_resp = json.get("base_resp");
        if let Some(base) = base_resp {
            let status_code = base
                .get("status_code")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1);
            if status_code != 0 {
                return Err(ProviderError::Parse(
                    base.get("status_msg")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string(),
                ));
            }
        }

        let used_credits = json
            .get("used_amount")
            .or_else(|| json.get("total_amount"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let credit_limit = json
            .get("total_quota")
            .or_else(|| json.get("quota"))
            .and_then(|v| v.as_f64())
            .unwrap_or(100.0);

        let used_percent = if credit_limit > 0.0 {
            (used_credits / credit_limit) * 100.0
        } else {
            0.0
        };

        let plan = json
            .get("plan_name")
            .or_else(|| json.get("current_plan_title"))
            .or_else(|| json.get("current_subscribe_title"))
            .or_else(|| json.get("combo_title"))
            .or_else(|| json.pointer("/current_combo_card/title"))
            .or_else(|| json.get("plan_type"))
            .or_else(|| json.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("MiniMax");

        let usage = UsageSnapshot::new(RateWindow::new(used_percent)).with_login_method(plan);

        Ok(usage)
    }

    /// Fetch the billing history summary (best-effort enrichment; never fatal).
    async fn fetch_billing_summary(
        &self,
        cookie_header: &str,
        region: MiniMaxRegion,
    ) -> Result<MiniMaxBillingSummary, ProviderError> {
        let client = http_client()?;

        let response = client
            .get(region.billing_history_url())
            .query(&[("page", "1"), ("limit", "100"), ("aggregate", "false")])
            .header("Cookie", cookie_header)
            .header("Accept", JSON_ACCEPT)
            .header("X-Requested-With", "XMLHttpRequest")
            .send()
            .await?;
        check_status(response.status(), "billing", false)?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse MiniMax billing: {e}")))?;
        parse_billing_summary(&json)
    }

    /// User-Agent string for coding-plan web requests (matches the upstream
    /// Chrome UA; kept local to minimax so we don't couple to opencodego).
    const WEB_USER_AGENT: &'static str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

    /// Fetch quota from the coding-plan page with a remains-API fallback.
    /// Mirrors upstream `MiniMaxUsageFetcher` cookie/web flow (issue #246).
    async fn fetch_coding_plan_with_cookie(
        &self,
        cookie_header: &str,
        region: MiniMaxRegion,
    ) -> Result<UsageSnapshot, ProviderError> {
        let client = http_client()?;

        let response = console_get(
            &client,
            &region.coding_plan_url(),
            cookie_header,
            region,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            false,
        )
        .send()
        .await?;
        check_status(response.status(), "coding plan", false)?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let now = Utc::now();

        // 200 with JSON content-type → parse_coding_plan_value
        if content_type.contains("application/json") {
            let json: serde_json::Value = response.json().await.map_err(|e| {
                ProviderError::Parse(format!("Failed to parse coding plan JSON: {e}"))
            })?;
            match coding_plan::parse_coding_plan_value(&json, now) {
                Ok(snapshot) => return coding_plan_html::to_usage_snapshot(&snapshot, now),
                Err(ProviderError::Parse(msg)) => {
                    tracing::debug!(
                        "MiniMax coding plan JSON parse failed: {msg}; trying remains API"
                    );
                }
                Err(e) => return Err(e),
            }
            // JSON Parse-error fall-through → remains fallback (response consumed)
            return self
                .fetch_coding_plan_remains_fallback(cookie_header, region, now)
                .await;
        }

        // 200 other → read text → parse_coding_plan_html
        let html = response
            .text()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to read coding plan HTML: {e}")))?;
        match coding_plan_html::parse_coding_plan_html(&html, now) {
            Ok(snapshot) => match &snapshot {
                // Empty-services → remains fallback
                coding_plan::MiniMaxCodingPlanSnapshot::Services(rows) if rows.is_empty() => {}
                _ => return coding_plan_html::to_usage_snapshot(&snapshot, now),
            },
            Err(ProviderError::Parse(msg)) => {
                tracing::debug!("MiniMax coding plan HTML parse failed: {msg}; trying remains API");
            }
            Err(e) => return Err(e),
        }
        // remains fallback (response already consumed by .text())
        self.fetch_coding_plan_remains_fallback(cookie_header, region, now)
            .await
    }

    /// Remains-API fallback: try the platform-host remains URL, then the www-host
    /// remains URL. Move to the second URL only on HTTP 404/405, network error, or
    /// parse failure; AuthRequired and other statuses stop the chain.
    async fn fetch_coding_plan_remains_fallback(
        &self,
        cookie_header: &str,
        region: MiniMaxRegion,
        now: DateTime<Utc>,
    ) -> Result<UsageSnapshot, ProviderError> {
        let urls = [region.coding_plan_remains_url(), region.www_remains_url()];
        let mut last_err: Option<ProviderError> = None;
        for url in &urls {
            match self
                .fetch_remains_once(cookie_header, url, region, now)
                .await
            {
                Ok(snapshot) => return coding_plan_html::to_usage_snapshot(&snapshot, now),
                Err(err) => {
                    let should_try_next =
                        matches!(err, ProviderError::Parse(_) | ProviderError::Network(_));
                    last_err = Some(err);
                    if !should_try_next {
                        break;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| ProviderError::Parse("Missing MiniMax remains URL.".into())))
    }

    /// One remains-API request, returning the parsed snapshot.
    async fn fetch_remains_once(
        &self,
        cookie_header: &str,
        url: &str,
        region: MiniMaxRegion,
        now: DateTime<Utc>,
    ) -> Result<coding_plan::MiniMaxCodingPlanSnapshot, ProviderError> {
        let client = http_client()?;

        let response = console_get(&client, url, cookie_header, region, JSON_ACCEPT, true)
            .send()
            .await?;
        check_status(response.status(), "remains", true)?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        if content_type.contains("application/json") {
            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| ProviderError::Parse(format!("Failed to parse remains JSON: {e}")))?;
            return coding_plan::parse_coding_plan_value(&json, now);
        }

        // Non-JSON remains response → treat as HTML
        let html = response
            .text()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to read remains text: {e}")))?;
        coding_plan_html::parse_coding_plan_html(&html, now)
    }

    /// Full cookie/web fetch: quota from coding plan + best-effort billing.
    async fn fetch_with_cookie(
        &self,
        cookie_header: &str,
        region: MiniMaxRegion,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let snapshot = match self
            .fetch_coding_plan_with_cookie(cookie_header, region)
            .await
        {
            Ok(snapshot) => snapshot,
            Err(err) if coding_plan::is_token_plan_without_coding_plan(&err) => {
                // Token Plan accounts have no coding-plan subscription: the
                // legacy remains endpoint answers base_resp 2062 ("no active
                // token plan subscription"). Fall back to the console
                // token-plan endpoints (issue #254); when they carry nothing,
                // surface the original coding-plan error unchanged.
                match token_plan::fetch_token_plan_with_cookie(self, cookie_header, region).await {
                    Ok(result) => return Ok(result),
                    Err(token_plan_err @ ProviderError::AuthRequired) => {
                        return Err(token_plan_err);
                    }
                    Err(token_plan_err) => {
                        tracing::debug!(
                            "MiniMax token-plan fallback unavailable: {token_plan_err}"
                        );
                        return Err(err);
                    }
                }
            }
            Err(err) => return Err(err),
        };
        let mut result = ProviderFetchResult::new(snapshot, "web");

        // Best-effort billing enrichment — never kills the fetch.
        if let Ok(summary) = self.fetch_billing_summary(cookie_header, region).await {
            result = attach_billing_summary(result, summary);
        } else {
            tracing::warn!("MiniMax billing history unavailable; quota from coding plan only");
        }
        Ok(result)
    }

    /// Resolve the web cookie: manual cookie takes priority, then browser cookies.
    fn resolve_web_cookie(
        &self,
        ctx: &FetchContext,
        region: MiniMaxRegion,
    ) -> Result<Option<String>, ProviderError> {
        if let Some(cookie) = ctx.manual_cookie_header.as_deref()
            && !cookie.trim().is_empty()
        {
            return Ok(Some(cookie.to_string()));
        }
        match crate::providers::browser_cookie_header(&region.cookie_search_domains()) {
            Ok(header) => Ok(Some(header)),
            Err(ProviderError::NoCookies) => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn result_with_optional_billing(
        &self,
        usage: UsageSnapshot,
        source_label: &str,
        json: &serde_json::Value,
    ) -> ProviderFetchResult {
        let Ok(summary) = parse_billing_summary(json) else {
            return ProviderFetchResult::new(usage, source_label);
        };
        attach_billing_summary(ProviderFetchResult::new(usage, source_label), summary)
    }

    /// Probe for MiniMax installation (credentials check)
    async fn probe_cli(&self) -> Result<UsageSnapshot, ProviderError> {
        // Check if API key is configured
        let has_env_vars = std::env::var("MINIMAX_API_KEY").is_ok();
        let has_config = Self::get_minimax_config_path()
            .map(|p| p.join("config.json").exists())
            .unwrap_or(false);

        if has_env_vars || has_config {
            let usage =
                UsageSnapshot::new(RateWindow::new(0.0)).with_login_method("MiniMax (configured)");
            Ok(usage)
        } else {
            Err(ProviderError::NotInstalled(
                "MiniMax API not configured. Set MINIMAX_API_KEY and MINIMAX_GROUP_ID environment variables".to_string()
            ))
        }
    }
}

fn value_i64(value: Option<&serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(text) => text.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

fn scalar_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn format_count(value: i64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (idx, ch) in raw.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[async_trait]
impl Provider for MiniMaxProvider {
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    fn id(&self) -> ProviderId {
        ProviderId::MiniMax
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching MiniMax usage");
        let region = MiniMaxRegion::from_settings_value(ctx.api_region.as_deref());

        match ctx.source_mode {
            SourceMode::Auto => {
                // Manual cookie → try fetch_with_cookie first.
                if let Some(cookie_header) = ctx.manual_cookie_header.as_deref()
                    && !cookie_header.trim().is_empty()
                    && let Ok(result) = self.fetch_with_cookie(cookie_header, region).await
                {
                    return Ok(result);
                }
                // Browser cookie (no manual) → same fetch_with_cookie.
                if let Ok(Some(cookie)) = self.resolve_web_cookie(ctx, region)
                    && let Ok(result) = self.fetch_with_cookie(&cookie, region).await
                {
                    return Ok(result);
                }
                // Fall through to API keys.
                if let Ok(result) = self.fetch_via_web(ctx, region).await {
                    return Ok(result);
                }
                let usage = self.probe_cli().await?;
                Ok(ProviderFetchResult::new(usage, "cli"))
            }
            SourceMode::Web => match self.resolve_web_cookie(ctx, region)? {
                Some(cookie) => self.fetch_with_cookie(&cookie, region).await,
                None => self.fetch_via_web(ctx, region).await,
            },
            SourceMode::Cli => {
                let usage = self.probe_cli().await?;
                Ok(ProviderFetchResult::new(usage, "cli"))
            }
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::Cli]
    }
}

#[cfg(test)]
mod http_tests;

#[cfg(test)]
mod tests;
