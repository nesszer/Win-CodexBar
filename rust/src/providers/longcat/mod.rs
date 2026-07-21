//! LongCat cookie-based quota + fuel-pack provider (upstream 0.44 #1697).
//!
//! Default-disabled. Auth via manual cookie or browser import for longcat.chat.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const HOST: &str = "https://longcat.chat";
const USER_CURRENT: &str = "/api/v1/user-current";
const TOKEN_USAGE: &str = "/api/lc-platform/v1/tokenUsage";
const PENDING_FUEL: &str = "/api/lc-platform/v1/pending-fuel-packages";

pub struct LongCatProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl LongCatProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::LongCat,
                display_name: "LongCat",
                session_label: "Quota",
                weekly_label: "Fuel Pack",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://longcat.chat/platform/"),
                status_page_url: None,
            },
            // Isolated cookie-free client — auth is only the explicit Cookie header.
            client: crate::core::credentialed_http_client_builder()
                .cookie_store(false)
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn get_json(&self, path: &str, cookie: &str) -> Result<Value, ProviderError> {
        let url = format!("{HOST}{path}");
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookie)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "LongCat API {path} returned HTTP {status}"
            )));
        }
        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse LongCat {path}: {e}")))
    }
}

impl Default for LongCatProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LongCatProvider {
    fn id(&self) -> ProviderId {
        ProviderId::LongCat
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let cookie = match ctx.manual_cookie_header.as_deref() {
                    Some(c) => normalize_cookie_header(c).ok_or(ProviderError::NoCookies)?,
                    None => crate::providers::browser_cookie_header(&["longcat.chat"])?,
                };
                let account = self.get_json(USER_CURRENT, &cookie).await?;
                // Meituan-style envelope may return HTTP 200 with business 401.
                if let Some(code) = envelope_code(&account) {
                    if code == 401 || code == 403 {
                        return Err(ProviderError::AuthRequired);
                    }
                }
                let usage_raw = self.get_json(TOKEN_USAGE, &cookie).await?;
                let fuel = match self.get_json(PENDING_FUEL, &cookie).await {
                    Ok(v) => Some(v),
                    Err(ProviderError::AuthRequired) => return Err(ProviderError::AuthRequired),
                    Err(_) => None,
                };
                let snap = build_snapshot(&account, &usage_raw, fuel.as_ref())?;
                Ok(ProviderFetchResult::new(snap, "web"))
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

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let mut header = raw.trim().to_string();
    if header
        .get(.."cookie:".len())
        .is_some_and(|p| p.eq_ignore_ascii_case("cookie:"))
    {
        header = header["cookie:".len()..].trim().to_string();
    }
    (!header.is_empty()).then_some(header)
}

fn envelope_code(value: &Value) -> Option<i64> {
    value
        .get("code")
        .and_then(|c| c.as_i64())
        .or_else(|| value.get("status").and_then(|c| c.as_i64()))
}

fn envelope_data(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

fn json_f64(value: &Value, key: &str) -> Option<f64> {
    let v = value.get(key)?;
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .or_else(|| v.as_str()?.parse().ok())
}

fn json_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn build_snapshot(
    account: &Value,
    usage_raw: &Value,
    fuel_raw: Option<&Value>,
) -> Result<UsageSnapshot, ProviderError> {
    let account_data = envelope_data(account);
    let usage_outer = envelope_data(usage_raw);
    let usage = usage_outer
        .get("usage")
        .filter(|u| u.is_object())
        .unwrap_or(usage_outer);

    let total = json_f64(usage, "totalToken").ok_or_else(|| {
        ProviderError::Parse("tokenUsage data was missing totalToken".into())
    })?;
    let remaining = json_f64(usage, "availableToken");
    let used = remaining.map(|r| (total - r).max(0.0)).unwrap_or(0.0);

    let primary = if total > 0.0 {
        let mut w = RateWindow::new(((used / total) * 100.0).clamp(0.0, 100.0));
        w.reset_description = Some(format!("{}/{}", used as i64, total as i64));
        w
    } else {
        RateWindow::informational("No token quota")
    };

    let account_name = json_str(account_data, "name")
        .or_else(|| json_str(account_data, "nickname"))
        .or_else(|| json_str(account_data, "userName"));

    let mut snap = UsageSnapshot::new(primary);
    if let Some(name) = account_name {
        snap.account_organization = Some(name);
    }

    if let Some(fuel_raw) = fuel_raw {
        let fuel_data = envelope_data(fuel_raw);
        if let Some((total_fuel, remaining_fuel, expiry)) = parse_fuel(fuel_data) {
            if total_fuel > 0.0 {
                let used_fuel = (total_fuel - remaining_fuel).max(0.0);
                let mut secondary =
                    RateWindow::new(((used_fuel / total_fuel) * 100.0).clamp(0.0, 100.0));
                secondary.resets_at = expiry;
                secondary.reset_description = Some(format!(
                    "Fuel pack: {}/{}",
                    remaining_fuel as i64, total_fuel as i64
                ));
                snap = snap.with_secondary(secondary);
            }
        }
    }

    Ok(snap)
}

fn parse_fuel(fuel: &Value) -> Option<(f64, f64, Option<DateTime<Utc>>)> {
    // Accept either { packages: [...] } or a bare array.
    let packages = fuel
        .get("packages")
        .and_then(|p| p.as_array())
        .or_else(|| fuel.as_array())?;

    let mut total = 0.0;
    let mut remaining = 0.0;
    let mut saw_remaining = false;
    let mut nearest: Option<DateTime<Utc>> = None;

    for pkg in packages {
        if let Some(t) = json_f64(pkg, "totalToken")
            .or_else(|| json_f64(pkg, "total"))
            .or_else(|| json_f64(pkg, "amount"))
        {
            total += t.max(0.0);
        }
        if let Some(r) = json_f64(pkg, "availableToken")
            .or_else(|| json_f64(pkg, "remainingToken"))
            .or_else(|| json_f64(pkg, "remaining"))
        {
            remaining += r.max(0.0);
            saw_remaining = true;
        }
        if let Some(raw) = json_str(pkg, "expireTime")
            .or_else(|| json_str(pkg, "expireAt"))
            .or_else(|| json_str(pkg, "expiresAt"))
        {
            if let Ok(dt) = DateTime::parse_from_rfc3339(&raw) {
                let utc = dt.with_timezone(&Utc);
                nearest = Some(match nearest {
                    Some(n) if n < utc => n,
                    _ => utc,
                });
            }
        }
    }

    if total <= 0.0 && !saw_remaining {
        return None;
    }
    if total <= 0.0 {
        total = remaining;
    }
    let remaining = if saw_remaining { remaining } else { total };
    Some((total, remaining, nearest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_quota_and_fuel() {
        let account = json!({ "code": 0, "data": { "name": "cat" } });
        let usage = json!({
            "code": 0,
            "data": {
                "usage": { "totalToken": 1000, "availableToken": 250 }
            }
        });
        let fuel = json!({
            "code": 0,
            "data": {
                "packages": [
                    { "totalToken": 200, "availableToken": 50, "expireTime": "2026-08-01T00:00:00Z" }
                ]
            }
        });
        let snap = build_snapshot(&account, &usage, Some(&fuel)).unwrap();
        assert!((snap.primary.used_percent - 75.0).abs() < 0.01);
        assert_eq!(snap.account_organization.as_deref(), Some("cat"));
        let fuel_w = snap.secondary.unwrap();
        assert!((fuel_w.used_percent - 75.0).abs() < 0.01);
    }

    #[test]
    fn normalizes_cookie() {
        assert_eq!(
            normalize_cookie_header("Cookie: a=1; b=2").as_deref(),
            Some("a=1; b=2")
        );
    }
}
