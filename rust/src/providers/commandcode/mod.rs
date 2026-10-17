//! Command Code provider implementation.
//!
//! Fetches monthly and purchased credit balances with a Command Code API key
//! (`/alpha/billing/*`) or a browser session cookie (`/internal/billing/*`).

mod plan_cache;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex_lite::Regex;
use reqwest::Client;
use serde_json::Value;
use std::sync::{LazyLock, Mutex};

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use plan_cache::CommandCodePlanCache;

const COMMAND_CODE_API_BASE: &str = "https://api.commandcode.ai";
/// Billing resources. The route prefix comes from [`Credential`]: `/internal`
/// for a web session, `/alpha` for an API key (upstream #4462).
const COMMAND_CODE_CREDITS_PATH: &str = "/billing/credits";
const COMMAND_CODE_SUBSCRIPTIONS_PATH: &str = "/billing/subscriptions";
const COMMAND_CODE_CREDENTIAL_TARGET: &str = "codexbar-commandcode";
const COMMAND_CODE_API_KEY_ENV: &str = "COMMAND_CODE_API_KEY";
/// Shown for a rejected API key. Hyper-style `ExpiredSession` classification
/// uses this exact text, so keep it in sync with `error_state_kind`.
const API_KEY_REJECTED: &str = "Command Code API key is invalid or expired. Replace it in Settings or update COMMAND_CODE_API_KEY.";

/// Credential for one billing request. Browser sessions and API keys use
/// different route prefixes (upstream `CommandCodeUsageFetcher.Credential`).
#[derive(Clone, Copy)]
enum Credential<'a> {
    Cookie(&'a str),
    ApiKey(&'a str),
}

impl<'a> Credential<'a> {
    fn secret(self) -> &'a str {
        match self {
            Self::Cookie(value) | Self::ApiKey(value) => value,
        }
    }

    fn billing_path(self, resource: &str) -> String {
        let prefix = match self {
            Self::Cookie(_) => "/internal",
            Self::ApiKey(_) => "/alpha",
        };
        format!("{prefix}{resource}")
    }

    /// Source label stored on the fetch result (`web` keeps the cookie lane's
    /// existing label).
    fn source_label(self) -> &'static str {
        match self {
            Self::Cookie(_) => "web",
            Self::ApiKey(_) => "api",
        }
    }
}

/// The configured API key, from the explicit provider key, the keyring, or
/// `COMMAND_CODE_API_KEY`. Returns the shared resolver's error when none is set.
fn resolve_command_code_key(explicit: Option<&str>) -> Result<String, ProviderError> {
    crate::providers::resolve_api_key(
        explicit,
        COMMAND_CODE_CREDENTIAL_TARGET,
        &[COMMAND_CODE_API_KEY_ENV],
    )
    .map(|key| key.trim().to_string())
}

/// Rolling-limit window durations reported under `windowLimits` (upstream
/// `CommandCodeUsageFetcher`): five-hour and weekly rolling caps.
const FIVE_HOUR_WINDOW_MINUTES: u32 = 5 * 60;
const WEEKLY_WINDOW_MINUTES: u32 = 7 * 24 * 60;

static PLAN_CACHE: LazyLock<Mutex<CommandCodePlanCache>> =
    LazyLock::new(|| Mutex::new(CommandCodePlanCache::default()));

/// Recognized better-auth session cookie names in upstream priority order
/// (upstream `CommandCodeCookieHeader.supportedSessionCookieNames`). #2706 added
/// the production `commandcode_prod_` family; the legacy `better-auth` names stay
/// as fallback. Name matching against a pasted header is case-insensitive.
const SESSION_COOKIE_NAMES: &[&str] = &[
    "__Secure-commandcode_prod_.session_token",
    "commandcode_prod_.session_token",
    "__Host-commandcode_prod_.session_token",
    "__Host-better-auth.session_token",
    "__Secure-better-auth.session_token",
    "better-auth.session_token",
];
/// Cookie name wrapped around a bare pasted token (upstream #4212). This is the
/// live production cookie name; the legacy `better-auth` default made every
/// pasted bare token look expired.
const BARE_TOKEN_COOKIE_NAME: &str = "__Secure-commandcode_prod_.session_token";

/// Static `planId` → monthly grant catalog mirroring upstream
/// `CommandCodePlanCatalog`. The credits endpoint only reports the *remaining*
/// monthly grant; the plan totals come from the public pricing page (#2706 added
/// `individual-goat` at $70/mo).
struct CommandCodePlan {
    id: &'static str,
    display_name: &'static str,
    monthly_credits_usd: f64,
}

const PLANS: &[CommandCodePlan] = &[
    CommandCodePlan {
        id: "individual-go",
        display_name: "Go",
        monthly_credits_usd: 10.0,
    },
    CommandCodePlan {
        id: "individual-go-v1",
        display_name: "Go",
        monthly_credits_usd: 10.0,
    },
    CommandCodePlan {
        id: "individual-goat",
        display_name: "GOAT",
        monthly_credits_usd: 70.0,
    },
    CommandCodePlan {
        id: "individual-pro",
        display_name: "Pro",
        monthly_credits_usd: 30.0,
    },
    CommandCodePlan {
        id: "individual-pro-v1",
        display_name: "Pro",
        monthly_credits_usd: 80.0,
    },
    CommandCodePlan {
        id: "individual-max",
        display_name: "Max",
        monthly_credits_usd: 150.0,
    },
    CommandCodePlan {
        id: "individual-ultra",
        display_name: "Ultra",
        monthly_credits_usd: 300.0,
    },
];

/// Outcome of the optional subscription request for one refresh (upstream
/// `subscriptionEnrichmentUnavailable`). It stays `Unavailable` even when a
/// remembered plan stands in for the failed answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubscriptionLookup {
    Answered,
    Unavailable,
}

fn find_plan(plan_id: &str) -> Option<&'static CommandCodePlan> {
    let normalized = plan_id.trim().to_ascii_lowercase();
    PLANS.iter().find(|plan| plan.id == normalized)
}

/// The plan an active subscription reports (upstream `CommandCodePlanCatalog.Plan`).
/// A catalog id gets its display name and grant size. An unknown id is kept as
/// its raw label with no grant size, so the API usage still shows (upstream #4423).
struct ResolvedPlan {
    label: String,
    monthly_credits_usd: Option<f64>,
}

impl ResolvedPlan {
    fn from_plan_id(plan_id: &str) -> Self {
        match find_plan(plan_id) {
            Some(plan) => Self {
                label: plan.display_name.to_string(),
                monthly_credits_usd: Some(plan.monthly_credits_usd),
            },
            None => Self {
                label: plan_id.to_string(),
                monthly_credits_usd: None,
            },
        }
    }
}

/// Reset-window pace (upstream `CommandCodeProviderDescriptor.pace`, v0.70.0):
/// `.calendarMonthResetWindow`.
pub(crate) const PACE_CAPABILITY: crate::core::ProviderPaceCapability =
    crate::core::ProviderPaceCapability::CALENDAR_MONTH_RESET_WINDOW;

pub struct CommandCodeProvider {
    metadata: ProviderMetadata,
    client: Client,
    api_base: String,
}

impl CommandCodeProvider {
    pub fn new() -> Self {
        Self::with_api_base(COMMAND_CODE_API_BASE)
    }

    fn with_api_base(api_base: impl Into<String>) -> Self {
        Self {
            api_base: api_base.into(),
            metadata: ProviderMetadata {
                id: ProviderId::CommandCode,
                display_name: "Command Code",
                session_label: "5-hour",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://commandcode.ai"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_web(&self, cookie_header: &str) -> Result<ProviderFetchResult, ProviderError> {
        let cookie_header =
            normalize_cookie_header(cookie_header).ok_or_else(|| ProviderError::NoCookies)?;
        self.fetch_billing(Credential::Cookie(&cookie_header)).await
    }

    /// API-key lane (upstream #4462): `/alpha/billing/*` with a bearer key.
    async fn fetch_api_key(&self, api_key: &str) -> Result<ProviderFetchResult, ProviderError> {
        self.fetch_billing(Credential::ApiKey(api_key)).await
    }

    async fn fetch_billing(
        &self,
        credential: Credential<'_>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let now = Utc::now();
        let fingerprint = crate::core::sha256_hex(credential.secret().as_bytes());
        let credits = self
            .get_json(
                &self.billing_url(credential, COMMAND_CODE_CREDITS_PATH),
                credential,
            )
            .await?;
        let subscription_result = self
            .get_json(
                &self.billing_url(credential, COMMAND_CODE_SUBSCRIPTIONS_PATH),
                credential,
            )
            .await;
        let lookup = if subscription_result.is_ok() {
            SubscriptionLookup::Answered
        } else {
            SubscriptionLookup::Unavailable
        };
        let subscription = resolve_subscription_payload(subscription_result, &fingerprint, now);
        let mut result = result_from_payloads(&credits, subscription.as_ref(), lookup)?;
        result.source_label = credential.source_label().to_string();
        Ok(result)
    }

    fn billing_url(&self, credential: Credential<'_>, resource: &str) -> String {
        format!("{}{}", self.api_base, credential.billing_path(resource))
    }

    async fn get_json(
        &self,
        url: &str,
        credential: Credential<'_>,
    ) -> Result<Value, ProviderError> {
        let request = self.client.get(url);
        let request = match credential {
            Credential::Cookie(cookie_header) => request
                .header("Cookie", cookie_header)
                .header("Accept", "application/json, text/plain, */*")
                .header("Origin", "https://commandcode.ai")
                .header("Referer", "https://commandcode.ai/"),
            Credential::ApiKey(api_key) => request
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Accept", "application/json"),
        };
        let response = request.send().await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(match credential {
                Credential::Cookie(_) => ProviderError::AuthRequired,
                // A rejected key reports an error; it never falls back to cookies.
                Credential::ApiKey(_) => ProviderError::Other(API_KEY_REJECTED.into()),
            });
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Command Code API returned status {}",
                response.status()
            )));
        }
        response.json::<Value>().await.map_err(|e| {
            ProviderError::Parse(format!("Failed to parse Command Code response: {e}"))
        })
    }

    /// Cookie refresh path (upstream #2564):
    /// 1. Try last validated cached cookie header
    /// 2. On auth failure: clear cache, re-import browser cookies, validate, store
    async fn fetch_with_cookie_refresh(&self) -> Result<ProviderFetchResult, ProviderError> {
        use crate::browser::cookie_cache::CookieHeaderCache;

        if let Some(cached) = CookieHeaderCache::load(ProviderId::CommandCode) {
            match self.fetch_web(&cached.cookie_header).await {
                Ok(result) => return Ok(result),
                Err(ProviderError::AuthRequired) => {
                    CookieHeaderCache::clear(ProviderId::CommandCode);
                }
                Err(err) => return Err(err),
            }
        }

        let cookie_header = crate::providers::browser_cookie_header(&["commandcode.ai"])?;
        let result = self.fetch_web(&cookie_header).await?;
        // Best-effort cookie cache write; a failed write just re-fetches next run.
        let _stored = CookieHeaderCache::store(ProviderId::CommandCode, &cookie_header, "browser");
        Ok(result)
    }
}

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let extracted;
    let mut header = raw.trim();
    if let Some(cookie_header) = cookie_header_from_curl(raw) {
        extracted = cookie_header;
        header = extracted.trim();
    } else if looks_like_curl_capture(header) {
        return None;
    }
    if header
        .get(.."cookie:".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cookie:"))
    {
        header = header["cookie:".len()..].trim();
    }
    if header.is_empty() {
        return None;
    }
    if !header.contains('=') && !header.contains(';') {
        // Bare token — wrap it in the production cookie name (upstream #4212).
        return Some(format!("{BARE_TOKEN_COOKIE_NAME}={header}"));
    }

    // A pasted header narrows to the session cookie only (upstream
    // `CommandCodeCookieHeader.override(from:)`); forwarding unrelated cookies
    // (analytics, stripe ids, session_data sidecars) is unnecessary for auth.
    let (name, token) = extract_session_cookie(header)?;
    Some(format!("{name}={token}"))
}

/// Pick the better-auth session cookie out of a `name=value; …` header by the
/// upstream priority list, case-insensitively, preserving the header's own
/// name casing and token bytes.
fn extract_session_cookie(header: &str) -> Option<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for chunk in header.split(';') {
        let Some((name, value)) = chunk.trim().split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        pairs.push((name.to_string(), value.to_string()));
    }
    for expected in SESSION_COOKIE_NAMES {
        for (name, value) in &pairs {
            if name.eq_ignore_ascii_case(expected) {
                return Some((name.clone(), value.clone()));
            }
        }
    }
    None
}

fn cookie_header_from_curl(raw: &str) -> Option<String> {
    let re =
        Regex::new(r#"(?s)(?:^|\s)(?:-H|--header)(?:\s+|=)(?:'([^']*)'|"([^"]*)"|(\S+))"#).ok()?;
    re.captures_iter(raw).find_map(|caps| {
        let field = caps
            .get(1)
            .or_else(|| caps.get(2))
            .or_else(|| caps.get(3))?
            .as_str();
        let field = unescape_shell_segment(field);
        let (name, value) = split_header(&field)?;
        name.eq_ignore_ascii_case("cookie")
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn looks_like_curl_capture(raw: &str) -> bool {
    let lower = raw.trim_start().to_ascii_lowercase();
    lower.starts_with("curl ") || lower.starts_with("curl.exe ")
}

fn split_header(field: &str) -> Option<(&str, &str)> {
    let colon = field.find(':')?;
    Some((field[..colon].trim(), field[colon + 1..].trim()))
}

fn unescape_shell_segment(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                output.push(next);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn resolve_subscription_payload(
    result: Result<Value, ProviderError>,
    fingerprint: &str,
    now: DateTime<Utc>,
) -> Option<Value> {
    resolve_subscription_payload_with_cache(result, fingerprint, now, &PLAN_CACHE)
}

fn resolve_subscription_payload_with_cache(
    result: Result<Value, ProviderError>,
    fingerprint: &str,
    now: DateTime<Utc>,
    cache: &Mutex<CommandCodePlanCache>,
) -> Option<Value> {
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    match result {
        Ok(payload) => {
            let plan_id = payload
                .get("data")
                .and_then(|data| data.get("planId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|plan_id| !plan_id.is_empty());
            let period_end = payload
                .get("data")
                .and_then(|data| data.get("currentPeriodEnd"))
                .and_then(Value::as_str)
                .and_then(|value| parse_datetime(value.trim()));

            if let Some(plan) = plan_id.and_then(find_plan) {
                cache.store(plan.id, period_end, fingerprint, now);
            } else {
                cache.clear(fingerprint, now);
            }
            Some(payload)
        }
        Err(_optional_subscription_error) => {
            let entry = cache
                .entry(fingerprint, now)
                .map(|entry| (entry.plan_id.clone(), entry.period_end));
            let (plan_id, period_end) = entry?;
            if find_plan(&plan_id).is_none() {
                cache.clear(fingerprint, now);
                return None;
            }
            Some(serde_json::json!({
                "data": {
                    "planId": plan_id,
                    "currentPeriodEnd": period_end.to_rfc3339(),
                }
            }))
        }
    }
}

#[cfg(test)]
#[path = "plan_cache_integration_tests.rs"]
mod plan_cache_integration_tests;

#[cfg(test)]
#[path = "granted_credits_tests.rs"]
mod granted_credits_tests;

#[cfg(test)]
#[path = "api_key_tests.rs"]
mod api_key_tests;

fn result_from_payloads(
    credits_payload: &Value,
    subscription_payload: Option<&Value>,
    lookup: SubscriptionLookup,
) -> Result<ProviderFetchResult, ProviderError> {
    let credits = credits_payload
        .get("credits")
        .ok_or_else(|| ProviderError::Parse("Command Code credits object missing".into()))?;
    let monthly_credits = number(credits.get("monthlyCredits"))
        .ok_or_else(|| ProviderError::Parse("Command Code monthlyCredits missing".into()))?;
    let purchased = number(credits.get("purchasedCredits")).unwrap_or(0.0);
    let premium = number(credits.get("premiumMonthlyCredits")).unwrap_or(0.0);
    let open_source = number(credits.get("opensourceMonthlyCredits")).unwrap_or(0.0);
    let granted = number(credits.get("monthlyCreditsGranted"));

    // Upstream 0.48.0 F12: rolling 5-hour/weekly limits ride alongside the
    // monthly credits, under `windowLimits` at the root or inside `credits`.
    let five_hour = limit_window(
        window_limits(credits_payload).and_then(|limits| limits.get("fiveHour")),
        FIVE_HOUR_WINDOW_MINUTES,
    );
    let weekly = limit_window(
        window_limits(credits_payload).and_then(|limits| limits.get("weekly")),
        WEEKLY_WINDOW_MINUTES,
    );

    let period_end = subscription_payload
        .and_then(|root| root.get("data"))
        .and_then(|data| data.get("currentPeriodEnd"))
        .and_then(|value| value.as_str())
        .and_then(parse_datetime);
    let plan = subscription_payload
        .and_then(|root| root.get("data"))
        .and_then(|data| data.get("planId"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ResolvedPlan::from_plan_id);
    let total = monthly_credits_total(granted, plan.as_ref());
    let monthly_window = monthly_window(
        monthly_credits,
        purchased,
        total,
        lookup,
        period_end,
        plan.is_some(),
    );

    // Slot ordering mirrors upstream `toUsageSnapshot` (5-hour → weekly →
    // monthly); the local snapshot requires a primary, so an API without
    // `windowLimits` keeps the pre-F12 monthly-primary layout.
    let mut snapshot = if let Some(five) = five_hour {
        let mut snapshot = UsageSnapshot::new(five);
        if let Some(weekly) = weekly {
            snapshot = snapshot.with_secondary(weekly);
        }
        if let Some(monthly) = monthly_window {
            snapshot = snapshot.with_tertiary(monthly);
        }
        snapshot
    } else if let Some(weekly) = weekly {
        let mut snapshot = UsageSnapshot::new(weekly);
        if let Some(monthly) = monthly_window {
            snapshot = snapshot.with_tertiary(monthly);
        }
        snapshot
    } else if let Some(monthly) = monthly_window {
        UsageSnapshot::new(monthly)
    } else {
        // The snapshot needs a primary. With no rolling limit and no sized
        // monthly grant there is no quota to draw (upstream leaves every lane
        // nil), so state the remaining balance without a percentage.
        UsageSnapshot::new(RateWindow::informational(format!(
            "{monthly_credits:.2} monthly credits remaining"
        )))
    };

    if let Some(method) = login_method(monthly_credits, purchased, plan.as_ref(), total) {
        snapshot = snapshot.with_login_method(method);
    }

    let (used, limit) = match total {
        Some(total) => (monthly_credits_used(total, monthly_credits), total),
        None => {
            let total = premium + open_source;
            ((total - monthly_credits).max(0.0), total.max(0.0))
        }
    };
    let cost = CostSnapshot::new(used, "USD", "monthly credits").with_limit(limit);
    Ok(ProviderFetchResult::new(snapshot, "web").with_cost(cost))
}

fn window_limits(credits_payload: &Value) -> Option<&Value> {
    credits_payload.get("windowLimits").or_else(|| {
        credits_payload
            .get("credits")
            .and_then(|credits| credits.get("windowLimits"))
    })
}

/// A rolling `windowLimits.fiveHour|weekly` entry: `{cap, used, resetAt}`.
/// `cap` must be positive (number-or-string coercion) and `usedPercent` is the
/// clamped `used / cap` ratio, matching upstream `UsagePercent.displayClamped`.
fn limit_window(value: Option<&Value>, window_minutes: u32) -> Option<RateWindow> {
    let limit = value?;
    let cap = number(limit.get("cap"))?;
    if cap <= 0.0 {
        return None;
    }
    let used = number(limit.get("used")).unwrap_or(0.0);
    let used_percent = (used / cap * 100.0).clamp(0.0, 100.0);
    Some(RateWindow::with_details(
        used_percent,
        Some(window_minutes),
        coerce_reset_at(limit.get("resetAt")),
        None,
    ))
}

/// `resetAt` arrives as epoch seconds, epoch milliseconds, or an ISO-8601
/// string (upstream `CommandCodeUsageFetcher.date(from:)`).
fn coerce_reset_at(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = value?;
    if let Some(timestamp) = number(Some(value))
        && timestamp > 0.0
    {
        let seconds = if timestamp > 10_000_000_000.0 {
            timestamp / 1000.0
        } else {
            timestamp
        };
        // Epoch seconds; the sub-second fraction is below timestamp resolution.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "epoch seconds; sub-second fraction below timestamp resolution"
        )]
        let whole_seconds = seconds as i64;
        return DateTime::from_timestamp(whole_seconds, 0);
    }
    value.as_str().and_then(|text| parse_datetime(text.trim()))
}

/// USD size of the monthly grant (upstream `monthlyCreditsTotal`): the
/// `monthlyCreditsGranted` the credits response reports when it is finite and
/// positive, otherwise the plan catalog entry. Zero, negative and non-finite
/// grants fall back to the plan.
fn monthly_credits_total(granted: Option<f64>, plan: Option<&ResolvedPlan>) -> Option<f64> {
    granted
        .filter(|granted| granted.is_finite() && *granted > 0.0)
        .or_else(|| plan.and_then(|plan| plan.monthly_credits_usd))
}

/// USD spent in the monthly grant: `total - remaining`, clamped to [0, total].
fn monthly_credits_used(total: f64, monthly_remaining: f64) -> f64 {
    (total - monthly_remaining).min(total).max(0.0)
}

/// Monthly grant window (upstream `makeMonthlyWindow`): usage is the grant
/// total minus the fresh remaining credits, clamped to [0, total]. Without a
/// grant size the free-tier reading keeps a visible-but-empty bar when credits
/// remain and no window when the account holds nothing, but a failed
/// subscription lookup must not borrow that reading: an unknown size leaves the
/// row unavailable. The reset time only comes from the subscription. A reported
/// plan, even an unknown one, is not the free tier, so with no grant size it has
/// no monthly percentage (upstream #4423).
fn monthly_window(
    monthly_remaining: f64,
    purchased: f64,
    total: Option<f64>,
    lookup: SubscriptionLookup,
    period_end: Option<DateTime<Utc>>,
    has_plan: bool,
) -> Option<RateWindow> {
    let used_percent = if let Some(total) = total.filter(|total| *total > 0.0) {
        (monthly_credits_used(total, monthly_remaining) / total * 100.0).clamp(0.0, 100.0)
    } else if has_plan
        || lookup == SubscriptionLookup::Unavailable
        || (monthly_remaining <= 0.0 && purchased <= 0.0)
    {
        return None;
    } else {
        0.0
    };
    Some(RateWindow::with_details(
        used_percent,
        RateWindow::monthly_window_minutes(period_end),
        period_end,
        None,
    ))
}

/// Plan summary line, upstream shape: `GOAT · $61.50 of $70.00 · + $2.00 credits`.
/// The size comes from the grant total, so a reported grant names the size
/// even when the plan is unknown. An unknown plan with no grant still shows its
/// label and the remaining balance (upstream #4423).
fn login_method(
    monthly_remaining: f64,
    purchased: f64,
    plan: Option<&ResolvedPlan>,
    total: Option<f64>,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(plan) = plan {
        parts.push(plan.label.clone());
    }
    if let Some(total) = total {
        parts.push(format!(
            "{} of {}",
            format_usd(monthly_credits_used(total, monthly_remaining)),
            format_usd(total)
        ));
    } else if plan.is_some() || monthly_remaining > 0.0 {
        parts.push(format!("{} remaining", format_usd(monthly_remaining)));
    }
    if purchased > 0.0 {
        parts.push(format!("+ {} credits", format_usd(purchased)));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Upstream `formatUSD`: under $100 keeps two decimals, $100+ rounds to whole.
fn format_usd(value: f64) -> String {
    if value.abs() < 100.0 {
        format!("${value:.2}")
    } else {
        format!("${value:.0}")
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

impl Default for CommandCodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CommandCodeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::CommandCode
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            // A configured key is taken before any cookie is read (upstream
            // `CommandCodeProviderDescriptor` Auto strategy resolution).
            SourceMode::Auto => match resolve_command_code_key(ctx.api_key.as_deref()) {
                Ok(api_key) if !api_key.is_empty() => self.fetch_api_key(&api_key).await,
                _ => self.fetch_cookie_lane(ctx).await,
            },
            // The explicit API source is the key lane and never reads cookies.
            SourceMode::OAuth => {
                let api_key = resolve_command_code_key(ctx.api_key.as_deref())?;
                self.fetch_api_key(&api_key).await
            }
            SourceMode::Web => self.fetch_cookie_lane(ctx).await,
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(ctx.source_mode)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn owns_credential_lane_routing(&self) -> bool {
        true
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == API_KEY_REJECTED => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

impl CommandCodeProvider {
    /// Cookie lane for Auto without a key and for Web. A manual header is used
    /// as-is. `manual_cookie_missing` (cookie source `off`) never reads a
    /// browser; otherwise the browser lookup runs lazily with its cache.
    async fn fetch_cookie_lane(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        if let Some(cookie) = ctx.manual_cookie_header.as_deref() {
            return self.fetch_web(cookie).await;
        }
        if ctx.manual_cookie_missing {
            return Err(ProviderError::NoCookies);
        }
        self.fetch_with_cookie_refresh().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WINDOW_LIMITS_ROOT: &str =
        include_str!("../fixtures/commandcode/window-limits-root.json");
    const WINDOW_LIMITS_NESTED: &str =
        include_str!("../fixtures/commandcode/window-limits-nested.json");

    #[test]
    fn command_code_accepts_bare_session_token() {
        // Upstream #4212: bare tokens use the live production cookie name.
        assert_eq!(
            normalize_cookie_header("abc123").as_deref(),
            Some("__Secure-commandcode_prod_.session_token=abc123")
        );
    }

    #[test]
    fn command_code_narrows_header_to_production_session_cookie() {
        assert_eq!(
            normalize_cookie_header(
                "__Secure-commandcode_prod_.session_token=token; __Secure-commandcode_prod_.session_data=data; stripe=ignored"
            )
            .as_deref(),
            Some("__Secure-commandcode_prod_.session_token=token")
        );
    }

    #[test]
    fn command_code_rejects_header_without_session_cookie() {
        assert_eq!(
            normalize_cookie_header("Cookie: sidebar=value; stripe_mid=mid"),
            None
        );
        assert_eq!(normalize_cookie_header("not-a-cookie; also-bad"), None);
    }

    #[test]
    fn command_code_prefers_new_production_cookie_family() {
        // Upstream priority order: commandcode_prod_ names win over the legacy
        // better-auth family when a pasted header carries both.
        assert_eq!(
            normalize_cookie_header(
                "__Secure-better-auth.session_token=legacy; commandcode_prod_.session_token=prod"
            )
            .as_deref(),
            Some("commandcode_prod_.session_token=prod")
        );
    }

    #[test]
    fn command_code_matches_session_cookie_case_insensitively() {
        assert_eq!(
            normalize_cookie_header("__SECURE-COMMANDCODE_PROD_.SESSION_TOKEN=token").as_deref(),
            Some("__SECURE-COMMANDCODE_PROD_.SESSION_TOKEN=token")
        );
        assert_eq!(
            normalize_cookie_header("__Host-better-auth.session_token=legacy").as_deref(),
            Some("__Host-better-auth.session_token=legacy")
        );
    }

    #[test]
    fn command_code_extracts_cookie_header_from_curl() {
        let curl = r#"curl 'https://commandcode.ai' -H 'User-Agent: Browser' -H 'Cookie: __Secure-commandcode_prod_.session_token=token; __Secure-commandcode_prod_.session_data=data' "#;
        assert_eq!(
            normalize_cookie_header(curl).as_deref(),
            Some("__Secure-commandcode_prod_.session_token=token")
        );
    }

    #[test]
    fn command_code_rejects_curl_without_cookie_header() {
        let curl = r#"curl 'https://commandcode.ai' -H 'User-Agent: Browser'"#;
        assert_eq!(normalize_cookie_header(curl), None);
    }

    #[test]
    fn command_code_rejects_empty_or_malformed_cookie_header() {
        assert_eq!(normalize_cookie_header("Cookie:   "), None);
    }

    #[test]
    fn command_code_result_without_windows_keeps_monthly_layout() {
        let result = result_from_payloads(
            &json!({"credits":{"monthlyCredits":25,"purchasedCredits":2,"premiumMonthlyCredits":100}}),
            None,
            SubscriptionLookup::Answered,
        )
        .unwrap();
        // No windowLimits and no recognized plan → free/unknown-plan fallback:
        // a visible-but-empty monthly bar (upstream makeMonthlyWindow fallback).
        assert_eq!(result.usage.primary.used_percent, 0.0);
        assert!(result.usage.secondary.is_none());
        assert_eq!(
            result.usage.login_method.as_deref(),
            Some("$25.00 remaining · + $2.00 credits")
        );
        let cost = result.cost.expect("monthly credits cost");
        assert_eq!(cost.used, 75.0);
        assert_eq!(cost.limit, Some(100.0));
    }

    // ── F12: windowLimits parsing (upstream fixtures, copied verbatim) ───

    #[test]
    fn window_limits_root_fixture_maps_5h_weekly_monthly_slots() {
        let credits: Value = serde_json::from_str(WINDOW_LIMITS_ROOT).unwrap();
        let result = result_from_payloads(&credits, None, SubscriptionLookup::Answered).unwrap();
        let usage = result.usage;

        // fiveHour: cap 3, used 0.75 → 25%, 5×60 minutes, ms-epoch reset.
        assert_eq!(usage.primary.used_percent, 25.0);
        assert_eq!(usage.primary.window_minutes, Some(300));
        assert_eq!(
            usage.primary.resets_at.map(|ts| ts.timestamp()),
            Some(1_780_000_000)
        );

        // weekly: cap 15, used 1.5 → 10%, 7×24×60 minutes.
        let weekly = usage.secondary.expect("weekly window");
        assert_eq!(weekly.used_percent, 10.0);
        assert_eq!(weekly.window_minutes, Some(10080));
        assert_eq!(
            weekly.resets_at.map(|ts| ts.timestamp()),
            Some(1_780_100_000)
        );

        // No subscription payload → unknown plan → monthly fallback bar at 0%.
        let monthly = usage.tertiary.expect("monthly window");
        assert_eq!(monthly.used_percent, 0.0);
        assert_eq!(usage.login_method.as_deref(), Some("$8.50 remaining"));
    }

    #[test]
    fn window_limits_nested_fixture_coerces_string_numbers_and_epoch_seconds() {
        let credits: Value = serde_json::from_str(WINDOW_LIMITS_NESTED).unwrap();
        let result = result_from_payloads(&credits, None, SubscriptionLookup::Answered).unwrap();
        let usage = result.usage;

        // "4"/"1" strings coerce; resetAt "1780200000" reads as seconds.
        assert_eq!(usage.primary.used_percent, 25.0);
        assert_eq!(
            usage.primary.resets_at.map(|ts| ts.timestamp()),
            Some(1_780_200_000)
        );
        // weekly numeric cap 20/used 4 → 20%, ms-epoch reset.
        let weekly = usage.secondary.expect("weekly window");
        assert_eq!(weekly.used_percent, 20.0);
        assert_eq!(
            weekly.resets_at.map(|ts| ts.timestamp()),
            Some(1_780_300_000)
        );
        assert_eq!(
            usage.login_method.as_deref(),
            Some("$7.25 remaining · + $2.00 credits")
        );
    }

    #[test]
    fn window_limits_ignore_nonpositive_caps() {
        let result = result_from_payloads(
            &json!({
                "credits": {"monthlyCredits": 5},
                "windowLimits": {
                    "fiveHour": {"cap": 0, "used": 1, "resetAt": 1780000000},
                    "weekly": {"cap": -3, "used": 1}
                }
            }),
            None,
            SubscriptionLookup::Answered,
        )
        .unwrap();
        // No usable rolling windows: monthly fallback becomes primary.
        assert_eq!(result.usage.primary.used_percent, 0.0);
        assert!(result.usage.secondary.is_none());
        assert!(result.usage.tertiary.is_none());
    }

    // ── F13: plan catalog (upstream #2706) ───

    #[test]
    fn plan_catalog_recognizes_all_tiers_case_insensitively() {
        assert_eq!(find_plan("individual-goat").unwrap().display_name, "GOAT");
        assert_eq!(
            find_plan("individual-goat").unwrap().monthly_credits_usd,
            70.0
        );
        assert_eq!(
            find_plan("individual-pro-v1").unwrap().monthly_credits_usd,
            80.0
        );
        assert_eq!(find_plan("Individual-ULTRA").unwrap().display_name, "Ultra");
        // Upstream #4423: the versioned Go plan is a $10 grant labelled "Go".
        assert_eq!(find_plan("individual-go-v1").unwrap().display_name, "Go");
        assert_eq!(
            find_plan("individual-go-v1").unwrap().monthly_credits_usd,
            10.0
        );
        assert!(find_plan("team").is_none());
        assert!(find_plan("").is_none());
    }

    #[test]
    fn usd_labels_keep_currency_precision_below_one_hundred() {
        assert_eq!(format_usd(8.7784), "$8.78");
        assert_eq!(format_usd(150.75), "$151");
    }

    #[test]
    fn goat_plan_drives_monthly_window_and_login_method() {
        let subscription = json!({
            "data": {
                "planId": "individual-goat",
                "currentPeriodEnd": "2026-06-16T04:26:40.371Z"
            }
        });
        let credits: Value = serde_json::from_str(WINDOW_LIMITS_ROOT).unwrap();
        let result =
            result_from_payloads(&credits, Some(&subscription), SubscriptionLookup::Answered)
                .unwrap();

        // $70 grant, $8.50 remaining → 61.5 used → 87.857…%.
        let monthly = result.usage.tertiary.expect("monthly window");
        assert!((monthly.used_percent - 87.857).abs() < 0.01, "{monthly:?}");
        assert!(monthly.window_minutes.is_some(), "calendar-month minutes");
        assert_eq!(
            monthly.resets_at.map(|ts| ts.to_rfc3339()),
            Some("2026-06-16T04:26:40.371+00:00".to_string())
        );
        assert_eq!(
            result.usage.login_method.as_deref(),
            Some("GOAT · $61.50 of $70.00")
        );
        let cost = result.cost.expect("monthly credits cost");
        assert_eq!(cost.used, 61.5);
        assert_eq!(cost.limit, Some(70.0));
    }

    #[test]
    fn empty_account_has_no_monthly_window() {
        let result = result_from_payloads(
            &json!({"credits":{"monthlyCredits":0,"purchasedCredits":0}}),
            None,
            SubscriptionLookup::Answered,
        )
        .unwrap();
        assert_eq!(result.usage.primary.used_percent, 0.0);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("0.00 monthly credits remaining")
        );
        assert!(result.usage.tertiary.is_none());
        assert!(result.usage.login_method.is_none());
    }
}
