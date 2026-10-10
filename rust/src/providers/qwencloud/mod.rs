//! Qwen Cloud personal token-plan provider (upstream 0.46.0).
//!
//! Cookie-authenticated OneConsole gateway:
//! `POST https://cs-data.qwencloud.com/data/api.json?...`
//! with `IntlBroadScopeAspnGateway` / `sfm_bailian`.

mod fields;
#[cfg(test)]
mod monthly_tests;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex_lite::Regex;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, RateWindow, SourceMode,
    UsageSnapshot,
};
use crate::providers::{browser_cookie_header, strip_cookie_prefix};
use fields::{
    PLAN_NAME_KEYS, REMAINING_QUOTA_KEYS, RESET_DATE_KEYS, TOTAL_QUOTA_KEYS, USED_QUOTA_KEYS,
    date_field, expand_json_strings, find_first_bool, find_first_date, find_first_f64,
    find_first_i64, find_first_string, find_first_value_for_key, find_object_containing_any_of,
    find_quota_info, find_token_plan_instance, first_date, first_f64, number_field,
    percentage_points,
};

const GATEWAY_BASE_URL: &str = "https://home.qwencloud.com";
const DATA_GATEWAY_BASE_URL: &str = "https://cs-data.qwencloud.com";
pub(crate) const DASHBOARD_URL: &str =
    "https://home.qwencloud.com/billing/subscription/token-plan-individual";
const PRODUCT_CODE: &str = "sfm_tokenplansolo_public_intl";
const CONSOLE_PRODUCT: &str = "sfm_bailian";
const CONSOLE_ACTION: &str = "IntlBroadScopeAspnGateway";
const USAGE_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage";
const SUBSCRIPTION_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/subscription";
const QUOTA_CONFIG_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/quota-config";
const REGION: &str = "ap-southeast-1";
const LANGUAGE: &str = "en-US";
const USER_INFO_PATH: &str = "/tool/user/info.json";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

const COOKIE_DOMAINS: &[&str] = &[
    "qwencloud.com",
    "home.qwencloud.com",
    "account.qwencloud.com",
    "signin.qwencloud.com",
    "www.qwencloud.com",
    "cs-data.qwencloud.com",
    "alibabacloud.com",
    "account.alibabacloud.com",
    "aliyun.com",
    "console.aliyun.com",
];

const FIVE_HOUR_MINUTES: u32 = 5 * 60;
const WEEKLY_MINUTES: u32 = 7 * 24 * 60;
const LEGACY_MINUTES: u32 = 30 * 24 * 60;
const MONTHLY_MINUTES: u32 = 30 * 24 * 60;

#[derive(Default)]
pub struct QwenCloudProvider;

#[derive(Debug, Clone, PartialEq)]
struct QwenCloudSnapshot {
    plan_name: Option<String>,
    used_quota: Option<f64>,
    total_quota: Option<f64>,
    remaining_quota: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
    five_hour_used_percent: Option<f64>,
    five_hour_total_quota: Option<f64>,
    five_hour_resets_at: Option<DateTime<Utc>>,
    weekly_used_percent: Option<f64>,
    weekly_total_quota: Option<f64>,
    weekly_resets_at: Option<DateTime<Utc>>,
    monthly_used_percent: Option<f64>,
    monthly_total_quota: Option<f64>,
    monthly_resets_at: Option<DateTime<Utc>>,
}

/// Per-window credit totals from the quota-config payload.
#[derive(Clone, Copy)]
struct QuotaTotals {
    five_hour: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
}

/// Usage-object keys that mark a payload as carrying token-plan window data.
const USAGE_WINDOW_KEYS: &[&str] = &[
    "per5HourPercentage",
    "per1WeekPercentage",
    "per1MonthPercentage",
];

impl QwenCloudProvider {
    pub fn new() -> Self {
        Self
    }

    async fn fetch_via_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let cookie_header = Self::resolve_cookie_header(ctx)?;
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let sec_token = Self::resolve_sec_token(&client, &cookie_header, ctx)
            .await
            .ok_or(ProviderError::AuthRequired)?;

        let (client, sec_token, cookie_header) =
            (&client, sec_token.as_str(), cookie_header.as_str());
        let post = move |api: &'static str, data| {
            Self::post_api(client, api, data, sec_token, cookie_header, ctx)
        };
        let usage_body = post(USAGE_API, Map::new()).await?;
        let mut subscription_params = Map::new();
        subscription_params.insert(
            "commodityCode".into(),
            Value::String(PRODUCT_CODE.to_string()),
        );
        let subscription_body = post(SUBSCRIPTION_API, subscription_params).await.ok();
        let quota_config_body = post(QUOTA_CONFIG_API, Map::new()).await.ok();

        let snapshot = Self::parse(
            &usage_body,
            subscription_body.as_deref(),
            quota_config_body.as_deref(),
        )?;
        self.snapshot_to_usage(snapshot)
    }

    fn resolve_cookie_header(ctx: &FetchContext) -> Result<String, ProviderError> {
        if let Some(raw) = ctx
            .manual_cookie_header
            .as_deref()
            .and_then(normalize_cookie_header)
        {
            return Ok(raw);
        }
        for env_name in ["QWEN_CLOUD_COOKIE", "QWEN_CLOUD_COOKIE_HEADER"] {
            if let Ok(raw) = std::env::var(env_name)
                && let Some(header) = normalize_cookie_header(&raw)
            {
                return Ok(header);
            }
        }
        browser_cookie_header(COOKIE_DOMAINS)
            .and_then(|header| normalize_cookie_header(&header).ok_or(ProviderError::NoCookies))
    }

    async fn resolve_sec_token(
        client: &reqwest::Client,
        cookie_header: &str,
        ctx: &FetchContext,
    ) -> Option<String> {
        let timeout = std::time::Duration::from_secs(ctx.web_timeout.clamp(1, 20));

        // 1. Dashboard HTML (freshest OneConsole inject).
        if let Ok(response) = client
            .get(DASHBOARD_URL)
            .timeout(timeout)
            .header("Cookie", cookie_header)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            && response.status().is_success()
            && let Ok(text) = response.text().await
        {
            if looks_like_login_page(&text) {
                return None;
            }
            if let Some(token) = extract_sec_token(&text) {
                return Some(token);
            }
        }

        // 2. Cookie fallback.
        if let Some(token) = cookie_value("sec_token", cookie_header) {
            return Some(token);
        }

        // 3. User-info JSON endpoint.
        let user_info_url = format!("{GATEWAY_BASE_URL}{USER_INFO_PATH}");
        if let Ok(response) = client
            .get(&user_info_url)
            .timeout(timeout)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json, text/plain, */*")
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            && response.status().is_success()
            && let Ok(bytes) = response.bytes().await
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        {
            let expanded = expand_json_strings(value);
            if let Some(token) =
                find_first_string(&expanded, &["secToken", "sec_token", "csrfToken", "token"])
            {
                return Some(token);
            }
        }

        None
    }

    async fn post_api(
        client: &reqwest::Client,
        api: &str,
        data_parameters: Map<String, Value>,
        sec_token: &str,
        cookie_header: &str,
        ctx: &FetchContext,
    ) -> Result<Vec<u8>, ProviderError> {
        let url = api_url(api);
        let params_json = build_params_json(api, data_parameters, cookie_header);
        let form = [
            ("product", CONSOLE_PRODUCT.to_string()),
            ("action", CONSOLE_ACTION.to_string()),
            ("sec_token", sec_token.to_string()),
            ("region", REGION.to_string()),
            ("language", LANGUAGE.to_string()),
            ("params", params_json),
        ];

        let mut request = client
            .post(&url)
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
            .header("Cookie", cookie_header)
            .header("Accept", "application/json, text/plain, */*")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Origin", GATEWAY_BASE_URL)
            .header("Referer", DASHBOARD_URL)
            .header("User-Agent", USER_AGENT)
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form);

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
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!(
                "Qwen Cloud API error: HTTP {status}"
            )));
        }
        Ok(body.to_vec())
    }

    fn parse(
        usage_data: &[u8],
        subscription_data: Option<&[u8]>,
        quota_config_data: Option<&[u8]>,
    ) -> Result<QwenCloudSnapshot, ProviderError> {
        if usage_data.is_empty() {
            return Err(ProviderError::Parse("Empty Qwen Cloud response".into()));
        }

        let value: Value = serde_json::from_slice(usage_data).map_err(|_| {
            if is_likely_login_html(usage_data) {
                ProviderError::AuthRequired
            } else {
                ProviderError::Parse("Invalid Qwen Cloud JSON response".into())
            }
        })?;
        let expanded = expand_json_strings(value);
        throw_if_error_payload(&expanded)?;

        if let Some(snapshot) =
            parse_current_token_plan(&expanded, subscription_data, quota_config_data)
        {
            return Ok(snapshot);
        }

        parse_legacy_token_plan(&expanded)
    }

    fn snapshot_to_usage(
        &self,
        snapshot: QwenCloudSnapshot,
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
                Some(LEGACY_MINUTES),
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

        // Prefer the 5-hour window, then the legacy 30-day envelope. Individual
        // Qwen Cloud plans expose only the weekly window (`per1WeekPercentage`);
        // promote it to primary in that case instead of failing the whole fetch.
        // A monthly window takes the primary bar only when no other window exists;
        // otherwise it is an extra "Monthly" row.
        let (primary, secondary, mut primary_label, monthly_extra) =
            match (five_hour.or(legacy), weekly) {
                (Some(primary), secondary) => (primary, secondary, None, monthly),
                (None, Some(weekly)) => (weekly, None, Some(self.metadata().weekly_label), monthly),
                (None, None) => match monthly {
                    Some(monthly) => (monthly, None, None, None),
                    None => {
                        return Err(ProviderError::Parse(
                            "Qwen Cloud usage windows missing".into(),
                        ));
                    }
                },
            };
        if primary.window_minutes == Some(MONTHLY_MINUTES) {
            primary_label = Some("Monthly");
        }
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
impl Provider for QwenCloudProvider {
    fn id(&self) -> ProviderId {
        ProviderId::QwenCloud
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let usage = self.fetch_via_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli | SourceMode::OAuth => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }
}

fn api_url(api: &str) -> String {
    format!(
        "{DATA_GATEWAY_BASE_URL}/data/api.json?action={CONSOLE_ACTION}&product={CONSOLE_PRODUCT}&api={api}&_v=undefined"
    )
}

fn build_params_json(
    api: &str,
    mut data_parameters: Map<String, Value>,
    cookie_header: &str,
) -> String {
    let mut cornerstone = json!({
        "feTraceId": Uuid::new_v4().to_string().to_lowercase(),
        "feURL": DASHBOARD_URL,
        "protocol": "V2",
        "console": "ONE_CONSOLE",
        "productCode": "p_efm",
        "domain": "home.qwencloud.com",
        "consoleSite": "QWENCLOUD",
        "userNickName": "",
        "userPrincipalName": "",
        "xsp_lang": LANGUAGE,
    });
    if let Some(cna) = cookie_value("cna", cookie_header) {
        cornerstone["X-Anonymous-Id"] = Value::String(cna);
    }

    data_parameters.insert("cornerstoneParam".into(), cornerstone);

    json!({
        "Api": api,
        "V": "1.0",
        "Data": Value::Object(data_parameters),
    })
    .to_string()
}

fn parse_current_token_plan(
    expanded: &Value,
    subscription_data: Option<&[u8]>,
    quota_config_data: Option<&[u8]>,
) -> Option<QwenCloudSnapshot> {
    let usage = find_object_containing_any_of(expanded, USAGE_WINDOW_KEYS)?;
    let five_hour = percentage_points(number_field(&usage, "per5HourPercentage"));
    let weekly = percentage_points(number_field(&usage, "per1WeekPercentage"));
    let monthly = percentage_points(number_field(&usage, "per1MonthPercentage"));
    if five_hour.is_none() && weekly.is_none() && monthly.is_none() {
        return None;
    }

    let plan_code = subscription_data.and_then(plan_code_from_bytes);
    let plan_name = plan_code.as_deref().map(display_plan_name);
    let quota = quota_config_data
        .zip(plan_code.as_ref())
        .and_then(|(data, code)| quota_totals_from_bytes(data, code));

    Some(QwenCloudSnapshot {
        plan_name,
        used_quota: None,
        total_quota: None,
        remaining_quota: None,
        resets_at: None,
        five_hour_used_percent: five_hour,
        five_hour_total_quota: quota.and_then(|q| q.five_hour),
        five_hour_resets_at: date_field(&usage, "per5HourResetTime"),
        weekly_used_percent: weekly,
        weekly_total_quota: quota.and_then(|q| q.weekly),
        weekly_resets_at: date_field(&usage, "per1WeekResetTime"),
        monthly_used_percent: monthly,
        monthly_total_quota: quota.and_then(|q| q.monthly),
        monthly_resets_at: date_field(&usage, "per1MonthResetTime"),
    })
}

fn parse_legacy_token_plan(expanded: &Value) -> Result<QwenCloudSnapshot, ProviderError> {
    let instance = find_token_plan_instance(expanded);
    let plan_name = instance
        .as_ref()
        .and_then(|v| find_first_string(v, PLAN_NAME_KEYS))
        .or_else(|| find_first_string(expanded, PLAN_NAME_KEYS));
    let quota_source = instance
        .as_ref()
        .and_then(find_quota_info)
        .or_else(|| find_quota_info(expanded))
        .unwrap_or_else(|| expanded.clone());
    let used = first_f64(&quota_source, USED_QUOTA_KEYS)
        .or_else(|| find_first_f64(&quota_source, USED_QUOTA_KEYS));
    let total = first_f64(&quota_source, TOTAL_QUOTA_KEYS)
        .or_else(|| find_first_f64(&quota_source, TOTAL_QUOTA_KEYS));
    let remaining = first_f64(&quota_source, REMAINING_QUOTA_KEYS)
        .or_else(|| find_first_f64(&quota_source, REMAINING_QUOTA_KEYS));
    let resets_at = instance
        .as_ref()
        .and_then(|v| {
            first_date(v, RESET_DATE_KEYS).or_else(|| find_first_date(v, RESET_DATE_KEYS))
        })
        .or_else(|| {
            first_date(expanded, RESET_DATE_KEYS)
                .or_else(|| find_first_date(expanded, RESET_DATE_KEYS))
        });

    if used_percent(used, total, remaining).is_none() {
        return Err(ProviderError::Parse(
            "Qwen Cloud has no active token-plan subscription".into(),
        ));
    }

    Ok(QwenCloudSnapshot {
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

fn plan_code_from_bytes(data: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(data).ok()?;
    let expanded = expand_json_strings(value);
    let plan = find_object_containing_any_of(
        &expanded,
        &["specCode", "spec_code", "planName", "plan_name"],
    )?;
    for key in ["specCode", "spec_code", "planName", "plan_name"] {
        if let Some(raw) = plan.get(key).and_then(Value::as_str) {
            let normalized = raw.trim().to_lowercase();
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
}

fn display_plan_name(plan_code: &str) -> String {
    match plan_code {
        "lite" => "Lite".into(),
        "standard" => "Standard".into(),
        "pro" => "Pro".into(),
        "max" => "Max".into(),
        other => other.to_string(),
    }
}

fn quota_totals_from_bytes(data: &[u8], plan_code: &str) -> Option<QuotaTotals> {
    let value: Value = serde_json::from_slice(data).ok()?;
    let expanded = expand_json_strings(value);
    let quota = find_first_value_for_key(&expanded, plan_code)?;
    if !quota.is_object() {
        return None;
    }
    let totals = QuotaTotals {
        five_hour: number_field(&quota, "five_hour").or_else(|| number_field(&quota, "fiveHour")),
        weekly: number_field(&quota, "weekly"),
        monthly: number_field(&quota, "monthly"),
    };
    (totals.five_hour.is_some() || totals.weekly.is_some() || totals.monthly.is_some())
        .then_some(totals)
}

fn throw_if_error_payload(value: &Value) -> Result<(), ProviderError> {
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
            "Qwen Cloud API error: {message}"
        )));
    }

    if let Some(success) = find_first_bool(value, &["success", "Success", "successResponse"])
        && !success
    {
        let message = find_first_string(value, &["message", "msg", "Message", "errorMessage"])
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
            "Qwen Cloud API error: {message}"
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
    if code.contains("forbidden") || code == "403" || message.contains("forbidden") {
        return Err(ProviderError::AuthRequired);
    }
    Ok(())
}

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let mut header = strip_cookie_prefix(raw.trim());
    if (header.starts_with('"') && header.ends_with('"'))
        || (header.starts_with('\'') && header.ends_with('\''))
    {
        header = header[1..header.len().saturating_sub(1)].trim();
    }
    (!header.is_empty() && header.contains('=')).then(|| header.to_string())
}

fn cookie_value(name: &str, cookie_header: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key.trim().eq_ignore_ascii_case(name))
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn extract_sec_token(html: &str) -> Option<String> {
    for pattern in [
        r#""secToken"\s*:\s*"([^"]+)""#,
        r#""sec_token"\s*:\s*"([^"]+)""#,
        r#"secToken['"]?\s*[:=]\s*['"]([^'"]+)['"]"#,
        r#"sec_token['"]?\s*[:=]\s*['"]([^'"]+)['"]"#,
        r#"csrfToken['"]?\s*[:=]\s*['"]([^'"]+)['"]"#,
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

fn looks_like_login_page(html: &str) -> bool {
    let lowered = html.to_lowercase();
    lowered.contains("passport.alibabacloud.com")
        || lowered.contains("signin.aliyun.com")
        || lowered.contains("account.alibabacloud.com/login")
        || lowered.contains("login.qwencloud.com")
        || (lowered.contains("login")
            && lowered.contains("password")
            && lowered.contains("sign in"))
}

fn is_likely_login_html(data: &[u8]) -> bool {
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
        // Whole-number quotas are far below i64::MAX; rounding is intentional.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "whole-number quota fits i64"
        )]
        let rounded = value.round() as i64;
        format_count(rounded)
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

#[cfg(test)]
mod tests;
