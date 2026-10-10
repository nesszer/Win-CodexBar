//! Alibaba Token Plan provider implementation.
//!
//! Fetches Bailian / Model Studio token-plan credits from the same commerce
//! endpoints used by the upstream macOS provider. Authentication uses browser
//! cookies or a manually pasted cookie header.
//!
//! Team path: `GetSubscriptionSummary` / BssOpenAPI-V3 (+ optional sec_token).
//! Personal/Solo path: OneConsole personal token-plan APIs (+ best-effort sec_token).

mod cli;
mod fields;
#[cfg(test)]
mod monthly_tests;
mod personal;
mod region;

pub use region::AlibabaTokenPlanRegion;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex_lite::Regex;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, RateWindow, SourceMode,
    UsageSnapshot,
};
use crate::providers::{browser_cookie_header, strip_cookie_prefix};

use region::AlibabaTokenPlanRegion as Region;

use fields::{
    REMAINING_QUOTA_KEYS, TOTAL_QUOTA_KEYS, USED_QUOTA_KEYS, date_field, deep_find,
    expand_json_strings, find_first_i64, find_first_string, find_object_containing_any_of,
    find_plan_name, find_quota_info, find_reset_date, find_token_plan_instance, first_f64,
    number_field, parse_bool, percentage_points,
};

pub(super) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";
pub(super) const LANGUAGE: &str = "en-US";
pub(super) const PERSONAL_CONSOLE_PRODUCT: &str = "sfm_bailian";
pub(super) const PERSONAL_USAGE_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage";
pub(super) const PERSONAL_SUBSCRIPTION_API: &str =
    "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription";
pub(super) const PERSONAL_QUOTA_CONFIG_API: &str =
    "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/quota-config";

const FIVE_HOUR_MINUTES: u32 = 5 * 60;
const WEEKLY_MINUTES: u32 = 7 * 24 * 60;
const LEGACY_MINUTES: u32 = 30 * 24 * 60;
const MONTHLY_MINUTES: u32 = 30 * 24 * 60;

#[derive(Default)]
pub struct AlibabaTokenPlanProvider;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TokenPlanSnapshot {
    pub(super) plan_name: Option<String>,
    pub(super) used_quota: Option<f64>,
    pub(super) total_quota: Option<f64>,
    pub(super) remaining_quota: Option<f64>,
    pub(super) resets_at: Option<DateTime<Utc>>,
    pub(super) five_hour_used_percent: Option<f64>,
    pub(super) five_hour_total_quota: Option<f64>,
    pub(super) five_hour_resets_at: Option<DateTime<Utc>>,
    pub(super) weekly_used_percent: Option<f64>,
    pub(super) weekly_total_quota: Option<f64>,
    pub(super) weekly_resets_at: Option<DateTime<Utc>>,
    pub(super) monthly_used_percent: Option<f64>,
    pub(super) monthly_total_quota: Option<f64>,
    pub(super) monthly_resets_at: Option<DateTime<Utc>>,
}

impl AlibabaTokenPlanProvider {
    pub fn new() -> Self {
        Self
    }

    fn resolve_region(ctx: &FetchContext) -> Region {
        Region::from_settings_value(ctx.api_region.as_deref())
    }

    async fn fetch_via_cli(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let snapshot = cli::fetch_cli_usage(Self::resolve_region(ctx)).await?;
        Self::snapshot_to_usage(snapshot)
    }

    async fn fetch_via(
        &self,
        source: &'static str,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let usage = if source == "web" {
            self.fetch_via_web(ctx).await?
        } else {
            self.fetch_via_cli(ctx).await?
        };
        Ok(ProviderFetchResult::new(usage, source))
    }

    async fn fetch_via_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let region = Self::resolve_region(ctx);
        let cookie_header = Self::resolve_cookie_header(ctx, region)?;
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let snapshot = if region.uses_personal_api() {
            let sec_token = Self::resolve_sec_token(&client, &cookie_header, region, ctx).await;
            personal::fetch_personal_usage(
                &client,
                &cookie_header,
                region,
                sec_token.as_deref(),
                ctx,
            )
            .await?
        } else {
            self.fetch_team_usage(&client, &cookie_header, region, ctx)
                .await?
        };
        Self::snapshot_to_usage(snapshot)
    }

    async fn fetch_team_usage(
        &self,
        client: &reqwest::Client,
        cookie_header: &str,
        region: Region,
        ctx: &FetchContext,
    ) -> Result<TokenPlanSnapshot, ProviderError> {
        let sec_token = Self::resolve_sec_token(client, cookie_header, region, ctx).await;
        let mut form = vec![
            ("product", "BssOpenAPI-V3".to_string()),
            ("action", "GetSubscriptionSummary".to_string()),
            ("params", Self::team_request_params(region)),
            ("region", region.current_region_id().to_string()),
        ];
        push_sec_token(&mut form, sec_token.as_deref());
        let body = send_console_form(
            client.post(Self::team_quota_url(region)),
            cookie_header,
            region,
            "*/*",
            &form,
            "Alibaba Token Plan",
        )
        .await?;
        Self::parse_usage_snapshot(&body)
    }

    fn resolve_cookie_header(ctx: &FetchContext, region: Region) -> Result<String, ProviderError> {
        if let Some(raw) = ctx
            .manual_cookie_header
            .as_deref()
            .and_then(normalize_cookie_header)
        {
            return Ok(raw);
        }
        for env_name in [
            "ALIBABA_TOKEN_PLAN_COOKIE",
            "ALIBABA_TOKEN_PLAN_COOKIE_HEADER",
            "BAILIAN_TOKEN_PLAN_COOKIE",
        ] {
            if let Ok(raw) = std::env::var(env_name)
                && let Some(header) = normalize_cookie_header(&raw)
            {
                return Ok(header);
            }
        }
        browser_cookie_header(region.cookie_domains())
            .and_then(|header| normalize_cookie_header(&header).ok_or(ProviderError::NoCookies))
    }

    async fn resolve_sec_token(
        client: &reqwest::Client,
        cookie_header: &str,
        region: Region,
        ctx: &FetchContext,
    ) -> Option<String> {
        let response = client
            .get(region.dashboard_url())
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.clamp(1, 10)))
            .header("Cookie", cookie_header)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Referer", dashboard_referer(region))
            .header("Sec-Fetch-Site", "same-origin")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Dest", "document")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return cookie_value("sec_token", cookie_header);
        }
        let text = response.text().await.ok()?;
        extract_sec_token(&text).or_else(|| cookie_value("sec_token", cookie_header))
    }

    fn team_quota_url(region: Region) -> String {
        format!(
            "{}/data/api.json?action=GetSubscriptionSummary&product=BssOpenAPI-V3&_tag=",
            region.gateway_base_url()
        )
    }

    fn team_request_params(region: Region) -> String {
        serde_json::json!({
            "ProductCode": region.product_code(),
        })
        .to_string()
    }

    fn parse_usage_snapshot(data: &[u8]) -> Result<TokenPlanSnapshot, ProviderError> {
        let expanded = decode_console_payload(data, "Alibaba Token Plan")?;

        let instance = find_token_plan_instance(&expanded);
        let plan_name = instance
            .as_ref()
            .and_then(find_plan_name)
            .or_else(|| find_plan_name(&expanded));
        let quota_source = instance
            .as_ref()
            .and_then(find_quota_info)
            .or_else(|| find_quota_info(&expanded));
        let used = quota_source
            .as_ref()
            .and_then(|v| first_f64(v, USED_QUOTA_KEYS));
        let total = quota_source
            .as_ref()
            .and_then(|v| first_f64(v, TOTAL_QUOTA_KEYS));
        let remaining = quota_source
            .as_ref()
            .and_then(|v| first_f64(v, REMAINING_QUOTA_KEYS));
        let resets_at = instance
            .as_ref()
            .and_then(find_reset_date)
            .or_else(|| find_reset_date(&expanded));

        if plan_name.is_none() && total.is_none() && used.is_none() && remaining.is_none() {
            return Err(ProviderError::Parse(format!(
                "Missing Alibaba Token Plan data ({})",
                payload_diagnostics(&expanded)
            )));
        }

        Ok(TokenPlanSnapshot {
            plan_name,
            used_quota: used,
            total_quota: total,
            remaining_quota: remaining,
            resets_at,
            five_hour_used_percent: None,
            five_hour_total_quota: None,
            five_hour_resets_at: None,
            weekly_used_percent: None,
            weekly_total_quota: None,
            weekly_resets_at: None,
            monthly_used_percent: None,
            monthly_total_quota: None,
            monthly_resets_at: None,
        })
    }

    pub(super) fn snapshot_to_usage(
        snapshot: TokenPlanSnapshot,
    ) -> Result<UsageSnapshot, ProviderError> {
        let five_hour = snapshot.five_hour_used_percent.map(|percent| {
            RateWindow::with_details(
                percent,
                Some(FIVE_HOUR_MINUTES),
                snapshot.five_hour_resets_at,
                quota_detail_percent(percent, snapshot.five_hour_total_quota),
            )
        });
        let legacy = used_percent(
            snapshot.used_quota,
            snapshot.total_quota,
            snapshot.remaining_quota,
        )
        .map(|percent| {
            RateWindow::with_details(
                percent,
                RateWindow::monthly_window_minutes(snapshot.resets_at).or(Some(LEGACY_MINUTES)),
                snapshot.resets_at,
                quota_detail(
                    snapshot.used_quota,
                    snapshot.total_quota,
                    snapshot.remaining_quota,
                ),
            )
        });
        let weekly = snapshot.weekly_used_percent.map(|percent| {
            RateWindow::with_details(
                percent,
                Some(WEEKLY_MINUTES),
                snapshot.weekly_resets_at,
                quota_detail_percent(percent, snapshot.weekly_total_quota),
            )
        });
        let monthly = snapshot.monthly_used_percent.map(|percent| {
            RateWindow::with_details(
                percent,
                Some(MONTHLY_MINUTES),
                snapshot.monthly_resets_at,
                quota_detail_percent(percent, snapshot.monthly_total_quota),
            )
        });
        // Prefer the 5-hour window, then the Team/legacy credit envelope. Personal/Solo
        // payloads sometimes expose only `per1WeekPercentage`; promote that window to
        // primary instead of failing the whole fetch. A monthly window takes the primary
        // bar only when no other window exists; otherwise it is an extra "Monthly" row.
        let (primary, secondary, primary_label, monthly_extra) =
            match (five_hour.or(legacy), weekly) {
                (Some(primary), secondary) => (primary, secondary, None, monthly),
                (None, Some(weekly)) => (weekly, None, None, monthly),
                (None, None) => match monthly {
                    Some(monthly) => (monthly, None, Some("Monthly"), None),
                    None => {
                        return Err(ProviderError::Parse(
                            "Alibaba Token Plan quota totals missing".into(),
                        ));
                    }
                },
            };
        let mut usage = UsageSnapshot::new(primary);
        if let Some(label) = primary_label {
            usage = usage.with_primary_label(label);
        }
        if let Some(monthly) = monthly_extra {
            usage = usage.with_extra_rate_window("monthly", "Monthly", monthly);
        }
        if let Some(secondary) = secondary {
            usage = usage.with_secondary(secondary);
        }

        if let Some(plan) = snapshot.plan_name.filter(|plan| !plan.trim().is_empty()) {
            usage = usage.with_login_method(plan);
        }
        Ok(usage)
    }
}

#[async_trait]
impl Provider for AlibabaTokenPlanProvider {
    fn id(&self) -> ProviderId {
        ProviderId::AlibabaTokenPlan
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let (first, fallback) = match ctx.source_mode {
            SourceMode::Auto if ctx.auto_prefer_web => ("web", Some("cli")),
            SourceMode::Auto => ("cli", Some("web")),
            SourceMode::Cli => ("cli", None),
            SourceMode::Web => ("web", None),
            SourceMode::OAuth => return Err(ProviderError::UnsupportedSource(ctx.source_mode)),
        };
        match (self.fetch_via(first, ctx).await, fallback) {
            (Err(_), Some(fallback)) => self.fetch_via(fallback, ctx).await,
            (result, _) => result,
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Cli, SourceMode::Web]
    }

    fn error_state_kind(&self, error: &ProviderError) -> crate::core::ProviderStateKind {
        match error {
            ProviderError::NotInstalled(message) if message.contains("Bailian CLI 'bl'") => {
                crate::core::ProviderStateKind::LocalRuntimeOffline
            }
            _ => error.state_kind(),
        }
    }
}

pub(super) fn push_sec_token(form: &mut Vec<(&'static str, String)>, sec_token: Option<&str>) {
    if let Some(token) = sec_token.filter(|token| !token.trim().is_empty()) {
        form.push(("sec_token", token.to_string()));
    }
}

/// POST a console gateway form. The CSRF headers go after the form so the
/// header order matches the browser capture.
pub(super) async fn send_console_form(
    request: reqwest::RequestBuilder,
    cookie_header: &str,
    region: Region,
    accept: &str,
    form: &[(&'static str, String)],
    scope: &str,
) -> Result<Vec<u8>, ProviderError> {
    let mut request = request
        .header("Cookie", cookie_header)
        .header("Accept", accept)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Origin", region.gateway_base_url())
        .header("Referer", region.dashboard_url())
        .header("User-Agent", USER_AGENT)
        .header("X-Requested-With", "XMLHttpRequest")
        .form(form);

    if let Some(csrf) = cookie_value("login_aliyunid_csrf", cookie_header)
        .or_else(|| cookie_value("csrf", cookie_header))
    {
        request = request
            .header("x-xsrf-token", csrf.clone())
            .header("x-csrf-token", csrf);
    }

    let response = request.send().await?;
    let status = response.status();
    let body = response.bytes().await?;
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthRequired);
        }
        return Err(ProviderError::Other(format!(
            "{scope} API error: HTTP {status}"
        )));
    }
    Ok(body.to_vec())
}

/// Decode a console response body: reject empty bodies and login HTML,
/// expand JSON-in-string fields, then surface gateway error payloads.
pub(super) fn decode_console_payload(data: &[u8], scope: &str) -> Result<Value, ProviderError> {
    if data.is_empty() {
        return Err(ProviderError::Parse(format!("Empty {scope} response")));
    }
    let value: Value = serde_json::from_slice(data).map_err(|_| {
        if is_likely_login_html(data) {
            ProviderError::AuthRequired
        } else {
            ProviderError::Parse(format!("Invalid {scope} JSON response"))
        }
    })?;
    let expanded = expand_json_strings(value);
    throw_if_error_payload(&expanded)?;
    Ok(expanded)
}

pub(super) fn throw_if_error_payload(value: &Value) -> Result<(), ProviderError> {
    if let Some(status) = find_first_i64(value, &["statusCode", "status_code", "code"])
        && status != 0
        && status != 200
    {
        if status == 401 || status == 403 {
            return Err(ProviderError::AuthRequired);
        }
        let message = find_first_string(value, &["statusMessage", "status_msg", "message", "msg"])
            .unwrap_or_else(|| format!("status code {status}"));
        return Err(ProviderError::Other(format!(
            "Alibaba Token Plan API error: {message}"
        )));
    }

    if let Some(frame) = find_failing_success_frame(value) {
        let code = find_first_string(frame, &["errorCode", "Code", "code"]);
        let message = find_first_string(
            frame,
            &["errorMsg", "message", "msg", "Message", "errorMessage"],
        )
        .or_else(|| code.clone())
        .unwrap_or_else(|| "request failed".to_string());
        let lower = message.to_lowercase();
        if lower.contains("needlogin")
            || lower.contains("login")
            || lower.contains("log in")
            || lower.contains("unauthorized")
        {
            return Err(ProviderError::AuthRequired);
        }
        return Err(ProviderError::Other(format!(
            "Alibaba Token Plan API error: {message}"
        )));
    }

    let code = find_first_string(value, &["code", "status", "statusCode"])
        .unwrap_or_default()
        .to_lowercase();
    let message = find_first_string(value, &["message", "msg", "statusMessage"])
        .unwrap_or_default()
        .to_lowercase();
    if code.contains("needlogin")
        || code.contains("login")
        || message.contains("log in")
        || message.contains("login")
    {
        return Err(ProviderError::AuthRequired);
    }
    Ok(())
}

fn find_failing_success_frame(value: &Value) -> Option<&Value> {
    deep_find(value, &|node| {
        let map = node.as_object()?;
        ["success", "Success"]
            .iter()
            .filter_map(|key| map.get(*key))
            .filter_map(|value| parse_bool(Some(value)))
            .any(|success| !success)
            .then_some(node)
    })
}

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let header = strip_cookie_prefix(raw.trim());
    (!header.is_empty() && header.contains('=')).then(|| header.to_string())
}

pub(super) fn cookie_value(name: &str, cookie_header: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key.trim() == name)
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn dashboard_referer(region: Region) -> String {
    format!("{}/", region.gateway_base_url().trim_end_matches('/'))
}

fn extract_sec_token(html: &str) -> Option<String> {
    for pattern in [
        r#""secToken"\s*:\s*"([^"]+)""#,
        r#""sec_token"\s*:\s*"([^"]+)""#,
        r#"secToken['"]?\s*[:=]\s*['"]([^'"]+)['"]"#,
        r#"sec_token['"]?\s*[:=]\s*['"]([^'"]+)['"]"#,
        r#"SEC_TOKEN['"]?\s*[:=]\s*['"]([^'"]+)['"]"#,
    ] {
        let Ok(regex) = Regex::new(pattern) else {
            continue;
        };
        if let Some(value) = regex
            .captures(html)
            .and_then(|captures| captures.get(1))
            .map(|m| m.as_str().trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return Some(value);
        }
    }
    None
}

pub(super) fn is_likely_login_html(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data).to_lowercase();
    text.contains("<html")
        && (text.contains("login") || text.contains("sign in") || text.contains("signin"))
}

fn used_percent(used: Option<f64>, total: Option<f64>, remaining: Option<f64>) -> Option<f64> {
    let total = total.filter(|total| *total > 0.0)?;
    let used = used.or_else(|| remaining.map(|remaining| total - remaining))?;
    Some((used.clamp(0.0, total) / total * 100.0).clamp(0.0, 100.0))
}

fn quota_detail(used: Option<f64>, total: Option<f64>, remaining: Option<f64>) -> Option<String> {
    if let (Some(used), Some(total)) = (used, total.filter(|total| *total > 0.0)) {
        return Some(format!(
            "{} / {} credits used",
            format_quota(used),
            format_quota(total)
        ));
    }
    if let (Some(remaining), Some(total)) = (remaining, total.filter(|total| *total > 0.0)) {
        return Some(format!(
            "{} / {} credits left",
            format_quota(remaining),
            format_quota(total)
        ));
    }
    remaining.map(|remaining| format!("{} credits left", format_quota(remaining)))
}

fn quota_detail_percent(used_percent: f64, total: Option<f64>) -> Option<String> {
    let total = total.filter(|total| *total > 0.0)?;
    let used = total * used_percent / 100.0;
    Some(format!(
        "{} / {} credits used",
        format_quota(used),
        format_quota(total)
    ))
}

fn format_quota(value: f64) -> String {
    if (value.round() - value).abs() < f64::EPSILON {
        // Guarded above: value is within EPSILON of a whole number, so the
        // fractional part is zero.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "whole-number guard above; fractional part is zero"
        )]
        let whole = value.round() as i64;
        format_count(whole)
    } else {
        let formatted = format!("{value:.2}");
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        format_count_decimal(trimmed)
    }
}

fn format_count(value: i64) -> String {
    let raw = value.to_string();
    let mut output = String::with_capacity(raw.len() + raw.len() / 3);
    for (idx, ch) in raw.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            output.push(',');
        }
        output.push(ch);
    }
    output.chars().rev().collect()
}

fn format_count_decimal(raw: &str) -> String {
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    if fraction.is_empty() {
        format_count(whole.parse().unwrap_or(0))
    } else {
        format!("{}.{}", format_count(whole.parse().unwrap_or(0)), fraction)
    }
}

fn payload_diagnostics(value: &Value) -> String {
    let top_keys = value
        .as_object()
        .map(|map| {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            keys.join(",")
        })
        .unwrap_or_default();
    format!("topKeys={top_keys}")
}

#[cfg(test)]
mod tests;
