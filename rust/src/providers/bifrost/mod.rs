//! Bifrost self-hosted gateway quotas and model spend.
//!
//! Ported from upstream CodexBar v0.65.0. The configured gateway URL is
//! validated before resolving or attaching the virtual-key credential.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use std::{net::IpAddr, time::Duration};

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const CREDENTIAL_TARGET: &str = "codexbar-bifrost";
const API_KEY_ENV: &str = "BIFROST_API_KEY";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const QUOTA_PATH: &str = "/api/governance/virtual-keys/quota";

pub struct BifrostProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
}

#[derive(Debug, Clone, PartialEq)]
struct Scope {
    id: String,
    title: Option<String>,
    body: Value,
}

#[derive(Debug, Clone, PartialEq)]
struct Budget {
    id: String,
    scope: Scope,
    source: Option<String>,
    used: f64,
    usage_known: bool,
    limit: f64,
    reset: ResetTiming,
    models: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ResetTiming {
    seconds: Option<f64>,
    window_minutes: Option<u32>,
    resets_at: Option<DateTime<Utc>>,
    label: Option<&'static str>,
}

impl BifrostProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Bifrost,
                display_name: "Bifrost",
                session_label: "Budget",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: None,
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                // The virtual-key credential uses a custom header; never
                // follow a gateway redirect that could forward it elsewhere.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok(),
        }
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        // Validate before touching the keyring/environment or constructing a
        // request. A malformed/custom public HTTP endpoint must never receive
        // the virtual-key secret.
        let base = ctx
            .gateway_url
            .clone()
            .or_else(|| std::env::var("BIFROST_BASE_URL").ok())
            .ok_or_else(|| {
                ProviderError::NotInstalled(
                    "Bifrost gateway URL not found. Set it in provider settings or BIFROST_BASE_URL."
                        .into(),
                )
            })?;
        let url = quota_url(&base)?;
        let api_key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure Bifrost HTTP client.".into())
        })?;

        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let response = client
            .get(url)
            .header("x-bf-vk", api_key)
            .header("Accept", "application/json")
            .timeout(timeout)
            .send()
            .await?;
        let status = response.status();
        match status {
            StatusCode::UNAUTHORIZED => return Err(ProviderError::AuthRequired),
            StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(
                    "Bifrost denied access to this virtual key's quota.".into(),
                ));
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(ProviderError::Other(
                    "Bifrost quota request was rate limited (HTTP 429).".into(),
                ));
            }
            status if status.is_server_error() => {
                return Err(ProviderError::Other(format!(
                    "Bifrost quota service is unavailable (HTTP {status})."
                )));
            }
            status if !status.is_success() => {
                return Err(ProviderError::Other(format!(
                    "Bifrost quota request failed (HTTP {status})."
                )));
            }
            _ => {}
        }

        let bytes = crate::providers::read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                crate::providers::BoundedBodyError::TooLarge => {
                    ProviderError::Parse("Bifrost returned an oversized quota response.".into())
                }
                crate::providers::BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|_| ProviderError::Parse("Bifrost returned non-UTF-8 quota data.".into()))?;
        let value: Value = serde_json::from_str(body)
            .map_err(|_| ProviderError::Parse("Bifrost returned invalid quota JSON.".into()))?;
        let parsed = parse_usage(&value, Utc::now())?;
        Ok(result_from_usage(parsed))
    }
}

impl Default for BifrostProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for BifrostProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Bifrost
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx).await,
            source => Err(ProviderError::UnsupportedSource(source)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn quota_url(raw: &str) -> Result<Url, ProviderError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ProviderError::Other("Bifrost gateway URL is empty.".into()));
    }
    let candidate = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    let mut url = Url::parse(&candidate)
        .map_err(|_| ProviderError::Other("Bifrost gateway URL is invalid.".into()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ProviderError::Other("Bifrost gateway URL must include a host.".into()))?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(ProviderError::Other(
            "Bifrost gateway URL cannot contain user information or a fragment.".into(),
        ));
    }
    let allowed = match url.scheme() {
        "https" => true,
        "http" => is_private_http_host(host),
        _ => false,
    };
    if !allowed {
        return Err(ProviderError::Other(
            "Bifrost gateway must use HTTPS; HTTP is allowed only for private or loopback IP addresses.".into(),
        ));
    }

    // Treat the configured value as a base, preserving a configured path and
    // query while replacing any trailing slash with the documented API route.
    let query = url.query().map(str::to_owned);
    let path = format!("{}{}", url.path().trim_end_matches('/'), QUOTA_PATH);
    url.set_path(&path);
    url.set_query(query.as_deref());
    Ok(url)
}

/// Validate a configured gateway URL before saving it to settings.
pub fn validate_gateway_url(raw: &str) -> Result<(), ProviderError> {
    quota_url(raw).map(|_| ())
}

fn is_private_http_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let Ok(ip) = host.parse::<IpAddr>() else {
        return false;
    };
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            ip.is_loopback() || ip.is_unique_local() || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn parse_usage(value: &Value, now: DateTime<Utc>) -> Result<ParsedUsage, ProviderError> {
    object(value, "quota response")?;
    let active = optional_bool(value, "is_active")?;
    let mut scopes = vec![Scope {
        id: String::new(),
        title: None,
        body: value.clone(),
    }];
    add_scopes(
        &mut scopes,
        value,
        "provider_configs",
        "provider",
        "provider-",
    )?;
    add_scopes(&mut scopes, value, "model_configs", "model_name", "model-")?;

    let mut budgets = Vec::new();
    let mut limits = Vec::new();
    for scope in scopes {
        if let Some(items) = scope.body.get("budgets") {
            for item in array(items, "budgets")? {
                object(item, "budget")?;
                let id = optional_text(item, "id")?.unwrap_or_default();
                if id.is_empty() {
                    continue;
                }
                let mut limit = optional_number(item, "max_limit")?.unwrap_or(0.0);
                let override_amount = optional_number(item, "override_amount")?.unwrap_or(0.0);
                let override_mode = optional_text(item, "override_mode")?;
                let cycles = optional_number(item, "override_cycles_remaining")?.unwrap_or(0.0);
                if cycles.fract() != 0.0 {
                    return Err(parse_error("invalid override cycles"));
                }
                if override_amount > 0.0
                    && (override_mode.as_deref() == Some("forever")
                        || (override_mode.as_deref() == Some("cycles") && cycles > 0.0))
                {
                    limit += override_amount;
                }
                if !limit.is_finite() || limit < 0.0 {
                    return Err(parse_error("invalid effective budget"));
                }
                let current_usage = optional_number(item, "current_usage")?;
                let used = current_usage.unwrap_or(0.0);
                if used < 0.0 {
                    return Err(parse_error("invalid current usage"));
                }
                let reset = reset_timing(
                    optional_text(item, "reset_duration")?.as_deref(),
                    optional_text(item, "last_reset")?.as_deref(),
                    now,
                );
                let models = item
                    .get("per_model_usage")
                    .map(|v| array(v, "per_model_usage"))
                    .transpose()?
                    .unwrap_or_default()
                    .to_vec();
                budgets.push(Budget {
                    id,
                    scope: scope.clone(),
                    source: optional_text(item, "source_name")?,
                    used,
                    usage_known: current_usage.is_some(),
                    limit,
                    reset,
                    models,
                });
            }
        }

        // The aggregate field is a compatibility merge. It is never counted
        // together with its component rate_limits when components are present.
        let components = scope
            .body
            .get("rate_limits")
            .map(|v| array(v, "rate_limits"))
            .transpose()?
            .unwrap_or_default();
        if components.is_empty() {
            if let Some(aggregate) = scope.body.get("rate_limit")
                && !aggregate.is_null()
            {
                limits.push((scope.clone(), aggregate.clone(), 0usize));
            }
        } else {
            limits.extend(
                components
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, limit)| {
                        object(&limit, "rate limit")?;
                        Ok((scope.clone(), limit, index))
                    })
                    .collect::<Result<Vec<_>, ProviderError>>()?,
            );
        }
    }
    budgets.sort_by(|a, b| {
        a.reset
            .seconds
            .unwrap_or(f64::INFINITY)
            .total_cmp(&b.reset.seconds.unwrap_or(f64::INFINITY))
            .then_with(|| a.id.cmp(&b.id))
    });
    if active == Some(false) && budgets.is_empty() && limits.is_empty() {
        return Err(ProviderError::Other(
            "Bifrost virtual key is inactive and has no budgets or rate limits to display.".into(),
        ));
    }
    Ok(ParsedUsage {
        active,
        budgets,
        limits,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedUsage {
    active: Option<bool>,
    budgets: Vec<Budget>,
    limits: Vec<(Scope, Value, usize)>,
}

fn add_scopes(
    scopes: &mut Vec<Scope>,
    root: &Value,
    collection: &str,
    label_field: &str,
    prefix: &str,
) -> Result<(), ProviderError> {
    let Some(value) = root.get(collection) else {
        return Ok(());
    };
    for (index, body) in array(value, collection)?.iter().enumerate() {
        let provider = optional_text(body, "provider")?;
        let model = optional_text(body, label_field)?;
        let title = if label_field == "model_name" {
            Some(format!(
                "Model {}",
                [provider, model.or_else(|| Some((index + 1).to_string()))]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ")
            ))
        } else {
            Some(format!(
                "Provider {}",
                provider.unwrap_or_else(|| (index + 1).to_string())
            ))
        };
        scopes.push(Scope {
            id: format!("{prefix}{index}-"),
            title,
            body: body.clone(),
        });
    }
    Ok(())
}

fn result_from_usage(usage: ParsedUsage) -> ProviderFetchResult {
    let mut budget_windows = usage
        .budgets
        .iter()
        .filter(|budget| budget.limit > 0.0)
        .map(|budget| {
            let title = bounded(
                &[
                    budget.scope.title.as_deref(),
                    Some(budget.source.as_deref().unwrap_or("Budget")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
            );
            let window = RateWindow::with_details(
                percent(budget.used, budget.limit),
                budget.reset.window_minutes,
                budget.reset.resets_at,
                Some(budget_description(budget)),
            )
            .with_usage_known(budget.usage_known);
            (budget, title, window)
        })
        .collect::<Vec<_>>();
    let root_windows = budget_windows
        .iter()
        .filter(|(budget, _, _)| budget.scope.id.is_empty())
        .collect::<Vec<_>>();
    let primary = root_windows
        .first()
        .map(|(_, _, window)| window.clone())
        .unwrap_or_else(|| RateWindow::informational("No Bifrost budget quota reported"));
    let secondary = root_windows.get(1).map(|(_, _, window)| (*window).clone());
    let mut snapshot = UsageSnapshot::new(primary).with_login_method("API");
    if let Some(secondary) = secondary {
        snapshot = snapshot.with_secondary(secondary);
    }
    let mut extra_windows = root_windows
        .iter()
        .skip(2)
        .map(|(_, title, window)| {
            (
                format!(
                    "bifrost-budget-{}",
                    window.reset_description.as_deref().unwrap_or("extra")
                ),
                title.clone(),
                window.clone(),
            )
        })
        .collect::<Vec<_>>();
    extra_windows.extend(
        budget_windows
            .drain(..)
            .filter(|(budget, _, _)| !budget.scope.id.is_empty())
            .map(|(budget, title, window)| {
                (
                    format!("bifrost-{}budget-{}", budget.scope.id, budget.id),
                    title,
                    window,
                )
            }),
    );

    for (scope, limit, index) in &usage.limits {
        let source = optional_text(limit, "source_name").ok().flatten();
        for (key, title) in [("token", "Tokens"), ("request", "Requests")] {
            let max = optional_number(limit, &format!("{key}_max_limit"))
                .ok()
                .flatten();
            let reset_raw = optional_text(limit, &format!("{key}_reset_duration"))
                .ok()
                .flatten();
            let Some(max_or_reset) = max
                .filter(|max| *max > 0.0)
                .or_else(|| reset_raw.as_ref().map(|_| 0.0))
            else {
                continue;
            };
            let used = optional_number(limit, &format!("{key}_current_usage"))
                .ok()
                .flatten()
                .unwrap_or(0.0);
            let reset = reset_timing(
                reset_raw.as_deref(),
                optional_text(limit, &format!("{key}_last_reset"))
                    .ok()
                    .flatten()
                    .as_deref(),
                Utc::now(),
            );
            let title = bounded(
                &[scope.title.as_deref(), source.as_deref(), Some(title)]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            let window = RateWindow::with_details(
                if max_or_reset > 0.0 {
                    percent(used, max_or_reset)
                } else {
                    0.0
                },
                reset.window_minutes,
                reset.resets_at,
                reset.label.map(str::to_owned),
            );
            extra_windows.push((
                format!("bifrost-{}{}s-{index}", scope.id, key),
                title,
                window,
            ));
        }
    }
    for (id, title, window) in extra_windows {
        snapshot = snapshot.with_extra_rate_window(id, title, window);
    }
    if usage.active == Some(false) {
        snapshot = snapshot.with_extra_rate_window(
            "bifrost-key-inactive",
            "Key inactive",
            RateWindow::informational("Virtual key is inactive"),
        );
    }

    let mut result = ProviderFetchResult::new(snapshot, "api");
    if let Some(first) = usage
        .budgets
        .iter()
        .find(|budget| budget.scope.id.is_empty())
    {
        let mut cost = CostSnapshot::new(
            first.used,
            "USD",
            first
                .reset
                .label
                .unwrap_or(if first.limit > 0.0 { "Budget" } else { "Spend" }),
        );
        if first.limit > 0.0 {
            cost = cost.with_limit(first.limit);
        }
        if let Some(resets_at) = first.reset.resets_at {
            cost = cost.with_resets_at(resets_at);
        }
        result = result.with_cost(cost);
    }

    if usage.budgets.len() > 1 || usage.budgets.iter().any(|b| !b.scope.id.is_empty()) {
        for (index, budget) in usage.budgets.iter().take(24).enumerate() {
            let title = bounded(
                &[
                    budget.scope.title.as_deref(),
                    Some(budget.source.as_deref().unwrap_or("Budget")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
            );
            let value = if budget.limit > 0.0 {
                format!("{} / {}", usd(budget.used), usd(budget.limit))
            } else {
                usd(budget.used)
            };
            let mut detail =
                ProviderDisplayDetail::new(format!("bifrost-budget-detail-{index}"), title, value);
            if budget.limit > 0.0 {
                detail =
                    detail.and_then(|row| row.with_progress(budget.used.max(0.0), budget.limit));
            }
            if let Some(label) = budget.reset.label {
                detail = detail.and_then(|row| row.with_secondary_value(label));
            }
            result = result.with_display_detail(detail);
        }
    }
    if let Some(first) = usage
        .budgets
        .iter()
        .find(|budget| budget.scope.id.is_empty())
    {
        let mut models = first
            .models
            .iter()
            .filter_map(|model| {
                let name = optional_text(model, "model")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "Model".into());
                let provider = optional_text(model, "provider").ok().flatten();
                let cost = optional_number(model, "total_cost").ok().flatten();
                let tokens = optional_number(model, "total_tokens").ok().flatten();
                (cost.unwrap_or(0.0) != 0.0 || tokens.unwrap_or(0.0) != 0.0)
                    .then_some((name, provider, cost, tokens))
            })
            .collect::<Vec<_>>();
        models.sort_by(|a, b| {
            b.2.unwrap_or(0.0)
                .total_cmp(&a.2.unwrap_or(0.0))
                .then_with(|| b.3.unwrap_or(0.0).total_cmp(&a.3.unwrap_or(0.0)))
                .then_with(|| a.0.cmp(&b.0))
        });
        for (index, (name, provider, cost, tokens)) in models.iter().take(5).enumerate() {
            let label = bounded(
                &[provider.as_deref(), Some(name.as_str())]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
            let value = cost.map(usd).unwrap_or_else(|| "—".into());
            let mut detail =
                ProviderDisplayDetail::new(format!("bifrost-model-{index}"), label, value);
            if let Some(tokens) = tokens {
                detail = detail.and_then(|row| {
                    row.with_secondary_value(format!("{} tokens", format_count(*tokens)))
                });
            }
            result = result.with_display_detail(detail);
        }
        if models.len() > 5 {
            result = result.with_display_detail(ProviderDisplayDetail::new(
                "bifrost-model-other",
                "Other models",
                (models.len() - 5).to_string(),
            ));
        }
    }
    result
}

fn budget_description(budget: &Budget) -> String {
    let mut parts = Vec::new();
    if let Some(source) = &budget.source {
        parts.push(bounded(&source.chars().take(24).collect::<String>()));
    }
    if let Some(label) = budget.reset.label {
        parts.push(label.into());
    }
    parts.push(format!("{} / {}", usd(budget.used), usd(budget.limit)));
    bounded(&parts.join(" · "))
}

fn reset_timing(reset: Option<&str>, last_reset: Option<&str>, now: DateTime<Utc>) -> ResetTiming {
    let seconds = reset.and_then(parse_duration);
    let fixed =
        reset.is_some_and(|raw| !matches!(raw.chars().last(), Some('d' | 'w' | 'M' | 'Q' | 'Y')));
    let resets_at = if fixed {
        let start = last_reset
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|v| v.with_timezone(&Utc));
        match (start, seconds) {
            (Some(start), Some(seconds)) => Duration::try_from_secs_f64(seconds)
                .ok()
                .and_then(|period| chrono::TimeDelta::from_std(period).ok())
                .and_then(|period| {
                    let period_ms = period.num_milliseconds();
                    if period_ms <= 0 {
                        return None;
                    }
                    let elapsed_ms = (now - start).num_milliseconds().max(0);
                    let periods = elapsed_ms.checked_div(period_ms)?.checked_add(1)?;
                    let delta_ms = period_ms.checked_mul(periods)?;
                    chrono::TimeDelta::try_milliseconds(delta_ms)
                        .and_then(|delta| start.checked_add_signed(delta))
                }),
            _ => None,
        }
    } else {
        None
    };
    let label = match reset {
        None => None,
        Some(raw) => match raw {
            "1h" => Some("Hourly"),
            "1d" => Some("Daily"),
            "1w" => Some("Weekly"),
            "1M" => Some("Monthly"),
            "1Q" => Some("Quarterly"),
            "1Y" => Some("Yearly"),
            _ => None,
        },
    };
    let minutes = seconds
        .and_then(|value| Duration::try_from_secs_f64(value).ok())
        .map(|duration| duration.as_secs() / 60)
        .and_then(|minutes| u32::try_from(minutes).ok())
        .filter(|minutes| *minutes > 0);
    ResetTiming {
        seconds,
        window_minutes: (fixed).then_some(minutes).flatten(),
        resets_at,
        label,
    }
}

fn parse_duration(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    let extended = raw
        .strip_suffix('Y')
        .map(|n| (n, 31_536_000.0))
        .or_else(|| raw.strip_suffix('Q').map(|n| (n, 7_776_000.0)))
        .or_else(|| raw.strip_suffix('M').map(|n| (n, 2_592_000.0)))
        .or_else(|| raw.strip_suffix('w').map(|n| (n, 604_800.0)))
        .or_else(|| raw.strip_suffix('d').map(|n| (n, 86_400.0)));
    if let Some((amount, multiplier)) = extended {
        let seconds = amount.parse::<f64>().ok()? * multiplier;
        return (seconds.is_finite() && seconds > 0.0).then_some(seconds);
    }

    let mut rest = raw;
    let mut total = 0.0;
    while !rest.is_empty() {
        let number_end = rest
            .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
            .unwrap_or(rest.len());
        if number_end == 0 {
            return None;
        }
        let amount = rest[..number_end].parse::<f64>().ok()?;
        if !amount.is_finite() {
            return None;
        }
        rest = &rest[number_end..];
        let (unit, multiplier) = [
            ("ms", 0.001),
            ("us", 0.000_001),
            ("µs", 0.000_001),
            ("μs", 0.000_001),
            ("ns", 0.000_000_001),
            ("s", 1.0),
            ("m", 60.0),
            ("h", 3_600.0),
        ]
        .into_iter()
        .find(|(unit, _)| rest.starts_with(unit))?;
        total += amount * multiplier;
        if !total.is_finite() {
            return None;
        }
        rest = &rest[unit.len()..];
    }
    (total > 0.0).then_some(total)
}

fn quota_url_for_test(raw: &str) -> Result<Url, ProviderError> {
    quota_url(raw)
}

fn object<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a serde_json::Map<String, Value>, ProviderError> {
    value
        .as_object()
        .ok_or_else(|| parse_error(&format!("{name} must be an object")))
}

fn array<'a>(value: &'a Value, name: &str) -> Result<&'a [Value], ProviderError> {
    match value {
        Value::Null => Ok(&[]),
        Value::Array(values) => Ok(values),
        _ => Err(parse_error(&format!("{name} must be an array"))),
    }
}

fn optional_text(value: &Value, key: &str) -> Result<Option<String>, ProviderError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            Ok((!value.trim().is_empty()).then(|| value.trim().to_owned()))
        }
        _ => Err(parse_error(&format!("{key} must be a string"))),
    }
}

fn optional_number(value: &Value, key: &str) -> Result<Option<f64>, ProviderError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or_else(|| parse_error(&format!("{key} must be a finite number"))),
    }
}

fn optional_bool(value: &Value, key: &str) -> Result<Option<bool>, ProviderError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| parse_error(&format!("{key} must be a boolean"))),
    }
}

fn percent(used: f64, limit: f64) -> f64 {
    if used.is_finite() && limit.is_finite() && limit > 0.0 {
        (used / limit * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn usd(value: f64) -> String {
    format!("${value:.2}")
}
fn bounded(value: &str) -> String {
    value.chars().take(120).collect()
}
fn format_count(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() <= i64::MAX as f64 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}
fn parse_error(reason: &str) -> ProviderError {
    ProviderError::Parse(format!(
        "Bifrost returned an unrecognized quota response ({reason})."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn component_rate_limits_take_precedence_over_aggregate_compatibility_field() {
        let value = json!({
            "rate_limit": { "token_max_limit": 900, "token_current_usage": 450, "token_reset_duration": "1h" },
            "rate_limits": [
                { "source_name": "provider-a", "token_max_limit": 100, "token_current_usage": 25, "token_reset_duration": "1h" },
                { "source_name": "provider-b", "request_max_limit": 20, "request_current_usage": 5, "request_reset_duration": "1d" }
            ]
        });
        let parsed = parse_usage(&value, now()).unwrap();
        assert_eq!(parsed.limits.len(), 2);
        assert_eq!(parsed.limits[0].1["token_max_limit"], 100);
        assert_eq!(parsed.limits[1].1["request_max_limit"], 20);
        assert!(
            !parsed
                .limits
                .iter()
                .any(|(_, limit, _)| limit["token_max_limit"] == 900)
        );
    }

    #[test]
    fn aggregate_rate_limit_is_fallback_when_components_are_absent() {
        let parsed = parse_usage(&json!({
            "rate_limit": { "token_max_limit": 50, "token_current_usage": 10, "token_reset_duration": "1h" }
        }), now()).unwrap();
        assert_eq!(parsed.limits.len(), 1);
        assert_eq!(parsed.limits[0].1["token_max_limit"], 50);
    }

    #[test]
    fn missing_current_usage_at_a_positive_rate_limit_is_known_zero() {
        for (usage_field, id) in [
            ("token_current_usage", "bifrost-tokens-0"),
            ("request_current_usage", "bifrost-requests-0"),
        ] {
            let mut limit = json!({
                "token_max_limit": 100,
                "token_current_usage": 25,
                "request_max_limit": 20,
                "request_current_usage": 5
            });
            limit.as_object_mut().unwrap().remove(usage_field);
            let result =
                result_from_usage(parse_usage(&json!({ "rate_limit": limit }), now()).unwrap());
            let named = result
                .usage
                .extra_rate_windows
                .iter()
                .find(|window| window.id == id)
                .unwrap_or_else(|| panic!("missing named rate window {id}"));

            assert_eq!(named.window.used_percent, 0.0, "{usage_field}");
            assert!(named.usage_known, "{usage_field}");
            assert!(named.window.usage_known(), "{usage_field}");
        }
    }

    #[test]
    fn validates_gateway_before_request_url_is_built() {
        assert!(
            quota_url_for_test("https://bifrost.example.com/base/")
                .unwrap()
                .as_str()
                .starts_with("https://bifrost.example.com/base/api/governance/virtual-keys/quota")
        );
        assert!(quota_url_for_test("http://10.1.2.3:8080").is_ok());
        assert!(quota_url_for_test("http://bifrost.example.com").is_err());
        assert!(quota_url_for_test("https://user:secret@bifrost.example.com").is_err());
        assert!(quota_url_for_test("ftp://10.1.2.3").is_err());
    }

    #[tokio::test]
    async fn rejects_public_http_before_resolving_any_credential() {
        let provider = BifrostProvider::new();
        let ctx = FetchContext {
            gateway_url: Some("http://public.example.com".into()),
            ..FetchContext::default()
        };
        let error = provider.fetch_api(&ctx).await.unwrap_err();
        assert!(error.to_string().contains("must use HTTPS"));
    }

    #[tokio::test]
    async fn does_not_forward_virtual_key_through_gateway_redirects() {
        let redirect_target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect_target_addr = redirect_target.local_addr().unwrap();
        let gateway = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway_addr = gateway.local_addr().unwrap();
        let gateway_task = tokio::spawn(async move {
            let (mut stream, _) = gateway.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let read = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
            assert!(request.contains("x-bf-vk: test-virtual-key"));
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{redirect_target_addr}/redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let redirect_target_task = tokio::spawn(async move {
            timeout(Duration::from_millis(250), redirect_target.accept()).await
        });

        let provider = BifrostProvider::new();
        let ctx = FetchContext {
            gateway_url: Some(format!("http://{gateway_addr}")),
            api_key: Some("test-virtual-key".into()),
            ..FetchContext::default()
        };
        let error = provider.fetch_api(&ctx).await.unwrap_err();
        assert!(error.to_string().contains("HTTP 302"));
        gateway_task.await.unwrap();
        assert!(redirect_target_task.await.unwrap().is_err());
    }

    #[test]
    fn parses_fixed_reset_and_calendar_label_without_inventing_calendar_time() {
        let fixed = reset_timing(Some("1h"), Some("2025-12-31T23:30:00Z"), now());
        assert_eq!(fixed.window_minutes, Some(60));
        assert_eq!(
            fixed.resets_at.unwrap().to_rfc3339(),
            "2026-01-01T00:30:00+00:00"
        );
        let calendar = reset_timing(Some("1M"), Some("2025-12-01T00:00:00Z"), now());
        assert_eq!(calendar.label, Some("Monthly"));
        assert_eq!(calendar.resets_at, None);
        assert_eq!(calendar.window_minutes, None);
        assert_eq!(parse_duration("1h30m"), Some(5_400.0));
    }

    #[test]
    fn budgets_and_overrides_are_mapped_to_percent_and_spend() {
        let parsed = parse_usage(
            &json!({
                "virtual_key_name": "Build key",
                "budgets": [{ "id": "b1", "max_limit": 10, "current_usage": 5,
                    "override_amount": 5, "override_mode": "forever", "source_name": "Team" }]
            }),
            now(),
        )
        .unwrap();
        let result = result_from_usage(parsed);
        assert!((result.usage.primary.used_percent - (100.0 / 3.0)).abs() < 0.001);
        assert_eq!(result.cost.unwrap().limit, Some(15.0));
    }

    #[test]
    fn shortest_root_budget_is_primary_and_cost_without_summing_budgets() {
        let parsed = parse_usage(
            &json!({
                "budgets": [
                    { "id": "monthly", "max_limit": 1_000, "current_usage": 200,
                        "reset_duration": "1M" },
                    { "id": "daily", "max_limit": 100, "current_usage": 10,
                        "reset_duration": "1d" }
                ]
            }),
            now(),
        )
        .unwrap();
        let result = result_from_usage(parsed);

        assert_eq!(result.usage.primary.used_percent, 10.0);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Daily · $10.00 / $100.00")
        );
        let secondary = result.usage.secondary.as_ref().unwrap();
        assert_eq!(secondary.used_percent, 20.0);
        assert_eq!(
            secondary.reset_description.as_deref(),
            Some("Monthly · $200.00 / $1000.00")
        );

        let cost = result.cost.unwrap();
        assert_eq!(cost.used, 10.0);
        assert_eq!(cost.limit, Some(100.0));
        assert_eq!(cost.period, "Daily");
    }

    #[test]
    fn malformed_optional_scope_collection_fails_closed() {
        assert!(parse_usage(&json!({ "provider_configs": {} }), now()).is_err());
    }
}
