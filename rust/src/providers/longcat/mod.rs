//! LongCat cookie-based quota + fuel-pack provider (upstream 0.44 #1697).
//!
//! Default-disabled. Auth via manual cookie or browser import for longcat.chat.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
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
/// Live token-pack lot summary (upstream 0.49.4 #2670).
const TOKEN_PACKS_SUMMARY: &str = "/api/pay/quota/metering/token-packs/summary";

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
                tertiary_label_key: None,
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

    /// POST variant of [`Self::get_json`] used by the token-packs summary
    /// endpoint (upstream 0.49.4 #2670 posts an empty JSON body).
    async fn post_json(&self, path: &str, cookie: &str) -> Result<Value, ProviderError> {
        let url = format!("{HOST}{path}");
        let resp = self
            .client
            .post(&url)
            .header("Cookie", cookie)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body("{}")
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
                if let Some(code) = envelope_code(&account)?
                    && (code == 401 || code == 403)
                {
                    return Err(ProviderError::AuthRequired);
                }
                // Upstream 0.49.4 #2670: prefer the token-packs summary lot;
                // only fall back to the legacy token-usage endpoint when no
                // active lot is available.
                let token_packs = match self.post_json(TOKEN_PACKS_SUMMARY, &cookie).await {
                    Ok(v) => Some(v),
                    Err(ProviderError::AuthRequired) => return Err(ProviderError::AuthRequired),
                    Err(err) => {
                        tracing::debug!("LongCat token-packs summary probe failed: {err}");
                        None
                    }
                };
                let usage_raw = if token_packs.as_ref().is_some_and(has_active_token_pack_lot) {
                    None
                } else {
                    Some(self.get_json(TOKEN_USAGE, &cookie).await?)
                };
                let fuel = match self.get_json(PENDING_FUEL, &cookie).await {
                    Ok(v) => Some(v),
                    Err(ProviderError::AuthRequired) => return Err(ProviderError::AuthRequired),
                    Err(_) => None,
                };
                let snap = build_snapshot(
                    &account,
                    token_packs.as_ref(),
                    usage_raw.as_ref(),
                    fuel.as_ref(),
                )?;
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

fn envelope_code(value: &Value) -> Result<Option<i64>, ProviderError> {
    value
        .get("code")
        .or_else(|| value.get("status"))
        .map(|raw| {
            json_integer(raw).ok_or_else(|| {
                ProviderError::Parse("LongCat response code was not a valid integer".into())
            })
        })
        .transpose()
}

fn envelope_data(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

fn json_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(json_number)
}

fn json_number(value: &Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_str()?.trim().parse().ok())?;
    number.is_finite().then_some(number)
}

fn json_integer(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().and_then(truncate_to_i64))
        .or_else(|| value.as_str()?.trim().parse().ok())
        .or_else(|| {
            value
                .as_str()?
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(truncate_to_i64)
        })
}

fn truncate_to_i64(value: f64) -> Option<i64> {
    if !value.is_finite() {
        return None;
    }
    let truncated = value.trunc();
    // `i64::MAX as f64` rounds up to 2^63, so keep that boundary exclusive.
    if truncated < i64::MIN as f64 || truncated >= i64::MAX as f64 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the finite value was range-checked before conversion"
    )]
    Some(truncated as i64)
}

fn whole_number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let normalized = if value.trunc() == 0.0 {
        0.0
    } else {
        value.trunc()
    };
    Some(format!("{normalized:.0}"))
}

fn json_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Whether the token-packs summary carries an ACTIVE lot with a positive
/// total (upstream `activeTokenPackLot`).
fn has_active_token_pack_lot(summary: &Value) -> bool {
    active_token_pack_lot(summary).is_some()
}

fn active_token_pack_lot(summary: &Value) -> Option<Value> {
    let lot = envelope_data(summary).get("currentLot")?;
    if json_str(lot, "status")?.to_uppercase() != "ACTIVE" {
        return None;
    }
    json_f64(lot, "totalToken").filter(|total| *total > 0.0)?;
    Some(lot.clone())
}

fn build_snapshot(
    account: &Value,
    token_packs: Option<&Value>,
    usage_raw: Option<&Value>,
    fuel_raw: Option<&Value>,
) -> Result<UsageSnapshot, ProviderError> {
    let account_data = envelope_data(account);

    let (total, used) = if let Some(lot) = token_packs.and_then(active_token_pack_lot) {
        // Active token-pack lot: consumed/total drive the quota directly.
        let total = json_f64(&lot, "totalToken")
            .ok_or_else(|| ProviderError::Parse("token-packs lot missing totalToken".into()))?;
        let used = json_f64(&lot, "consumedToken").unwrap_or(0.0);
        (total, used)
    } else if let Some(usage_raw) = usage_raw {
        // Legacy token quota: data.usage is the canonical aggregate.
        let usage_outer = envelope_data(usage_raw);
        let usage = usage_outer
            .get("usage")
            .filter(|u| u.is_object())
            .unwrap_or(usage_outer);
        let total = json_f64(usage, "totalToken")
            .ok_or_else(|| ProviderError::Parse("tokenUsage data was missing totalToken".into()))?;
        let remaining = json_f64(usage, "availableToken");
        let used = remaining.map(|r| (total - r).max(0.0)).unwrap_or(0.0);
        (total, used)
    } else {
        return Err(ProviderError::Parse(
            "LongCat usage data was missing (no active token-pack lot and no tokenUsage payload)"
                .into(),
        ));
    };

    let primary = if total.is_finite() && total > 0.0 && used.is_finite() {
        let mut w = RateWindow::new(((used / total) * 100.0).clamp(0.0, 100.0));
        if let (Some(used_text), Some(total_text)) = (whole_number(used), whole_number(total)) {
            w.reset_description = Some(format!("{used_text}/{total_text}"));
        }
        w
    } else if total.is_finite() && total <= 0.0 {
        RateWindow::informational("No token quota")
    } else {
        RateWindow::informational("Token quota unavailable")
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
        if let Some((total_fuel, remaining_fuel, expiry)) = parse_fuel(fuel_data)
            && total_fuel.is_finite()
            && remaining_fuel.is_finite()
            && total_fuel > 0.0
        {
            let used_fuel = (total_fuel - remaining_fuel).max(0.0);
            let mut secondary =
                RateWindow::new(((used_fuel / total_fuel) * 100.0).clamp(0.0, 100.0));
            secondary.resets_at = expiry;
            if let (Some(remaining_text), Some(total_text)) =
                (whole_number(remaining_fuel), whole_number(total_fuel))
            {
                secondary.reset_description =
                    Some(format!("Fuel pack: {remaining_text}/{total_text}"));
            }
            snap = snap.with_secondary(secondary);
        }
    }

    Ok(snap)
}

fn parse_fuel(fuel: &Value) -> Option<(f64, f64, Option<DateTime<Utc>>)> {
    // Accept either { packages: [...] } or a bare array.
    let packages = fuel
        .get("packages")
        .or_else(|| fuel.get("list"))
        .and_then(|p| p.as_array())
        .or_else(|| fuel.as_array())?;

    let declared_total = json_f64(fuel, "totalQuota")
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(0.0);
    let mut package_total = 0.0;
    let mut remaining = 0.0;
    let mut saw_remaining = false;
    let mut nearest: Option<DateTime<Utc>> = None;

    for pkg in packages {
        if let Some(t) = json_f64(pkg, "totalToken")
            .or_else(|| json_f64(pkg, "total"))
            .or_else(|| json_f64(pkg, "amount"))
        {
            package_total += t.max(0.0);
        }
        if let Some(r) = json_f64(pkg, "availableToken")
            .or_else(|| json_f64(pkg, "remainingToken"))
            .or_else(|| json_f64(pkg, "remaining"))
        {
            remaining += r.max(0.0);
            saw_remaining = true;
        }
        if let Some(raw) = pkg
            .get("expireTime")
            .or_else(|| pkg.get("expireAt"))
            .or_else(|| pkg.get("expiresAt"))
            && let Some(dt) = parse_fuel_timestamp(raw)
        {
            nearest = Some(match nearest {
                Some(n) if n < dt => n,
                _ => dt,
            });
        }
    }

    let mut total = if declared_total > 0.0 {
        declared_total
    } else {
        package_total
    };
    if total <= 0.0 && !saw_remaining {
        return None;
    }
    if total <= 0.0 {
        total = remaining;
    }
    let remaining = if saw_remaining { remaining } else { total };
    Some((total, remaining, nearest))
}

fn parse_fuel_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(number) = json_number(value) {
        if !number.is_finite() {
            return None;
        }
        let millis = if number > 1_000_000_000_000.0 {
            number
        } else {
            number * 1000.0
        };
        if millis <= 1_000_000_000_000.0 || millis >= i64::MAX as f64 {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "validated Unix timestamp is within i64 millisecond range"
        )]
        let millis = millis as i64;
        return Utc.timestamp_millis_opt(millis).single();
    }

    let raw = value.as_str()?.trim();
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|date| date.with_timezone(&Utc))
        .or_else(|| {
            NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|date| Utc.from_utc_datetime(&date))
        })
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
        let snap = build_snapshot(&account, None, Some(&usage), Some(&fuel)).unwrap();
        assert!((snap.primary.used_percent - 75.0).abs() < 0.01);
        assert_eq!(snap.account_organization.as_deref(), Some("cat"));
        let fuel_w = snap.secondary.unwrap();
        assert!((fuel_w.used_percent - 75.0).abs() < 0.01);
        assert_eq!(
            fuel_w.resets_at.map(|value| value.to_rfc3339()),
            Some("2026-08-01T00:00:00+00:00".to_string())
        );
    }

    #[test]
    fn parses_list_fuel_with_numeric_expiry() {
        let expiry = Utc.with_ymd_and_hms(2026, 9, 20, 12, 34, 56).unwrap();
        let fuel = json!({
            "totalQuota": 200,
            "list": [{ "availableToken": 50, "expireTime": expiry.timestamp_millis() }]
        });

        let (total, remaining, nearest) = parse_fuel(&fuel).expect("fuel package");
        assert_eq!(total, 200.0);
        assert_eq!(remaining, 50.0);
        assert_eq!(nearest, Some(expiry));
    }

    #[test]
    fn active_token_pack_lot_wins_over_legacy_usage() {
        // Upstream 0.49.4 #2670: the token-packs summary lot is the live
        // usage source; the legacy endpoint is only a fallback.
        let account = json!({ "code": 0, "data": { "name": "cat" } });
        let summary = json!({
            "code": 0,
            "data": {
                "currentLot": {
                    "status": "active",
                    "totalToken": 5000,
                    "consumedToken": 1250
                }
            }
        });
        let snap = build_snapshot(&account, Some(&summary), None, None).unwrap();
        assert!((snap.primary.used_percent - 25.0).abs() < 0.01);
        assert_eq!(snap.primary.reset_description.as_deref(), Some("1250/5000"));
    }

    #[test]
    fn inactive_or_empty_lot_falls_back_to_legacy_usage() {
        let account = json!({ "code": 0, "data": { "name": "cat" } });
        let inactive = json!({
            "code": 0,
            "data": {
                "currentLot": { "status": "EXPIRED", "totalToken": 5000, "consumedToken": 10 }
            }
        });
        let usage = json!({
            "code": 0,
            "data": { "usage": { "totalToken": 1000, "availableToken": 900 } }
        });
        let snap = build_snapshot(&account, Some(&inactive), Some(&usage), None).unwrap();
        assert!((snap.primary.used_percent - 10.0).abs() < 0.01);

        let no_total = json!({ "code": 0, "data": { "currentLot": { "status": "ACTIVE" } } });
        assert!(!has_active_token_pack_lot(&no_total));
        assert!(has_active_token_pack_lot(&json!({
            "code": 0,
            "data": { "currentLot": { "status": "ACTIVE", "totalToken": 5 } }
        })));
    }

    #[test]
    fn normalizes_cookie() {
        assert_eq!(
            normalize_cookie_header("Cookie: a=1; b=2").as_deref(),
            Some("a=1; b=2")
        );
    }

    #[test]
    fn rejects_unrepresentable_response_codes() {
        let error = envelope_code(&json!({ "code": "Infinity" })).unwrap_err();
        assert!(matches!(error, ProviderError::Parse(_)));
        assert_eq!(envelope_code(&json!({ "code": 200.9 })).unwrap(), Some(200));
    }

    #[test]
    fn formats_large_counts_without_i64_saturation() {
        let account = json!({ "code": 0, "data": { "name": "cat" } });
        let usage = json!({
            "code": 0,
            "data": {
                "usage": {
                    "totalToken": 2e20,
                    "availableToken": 1e20
                }
            }
        });
        let snapshot = build_snapshot(&account, None, Some(&usage), None).unwrap();
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("100000000000000000000/200000000000000000000")
        );
        assert!(snapshot.primary.used_percent.is_finite());
    }

    #[test]
    fn omits_fuel_window_when_counts_overflow() {
        let account = json!({ "code": 0 });
        let usage = json!({
            "data": { "usage": { "totalToken": 100, "availableToken": 50 } }
        });
        let fuel = json!({
            "totalQuota": 1e308,
            "list": [
                { "availableToken": 1e308 },
                { "availableToken": 1e308 }
            ]
        });
        let snapshot = build_snapshot(&account, None, Some(&usage), Some(&fuel)).unwrap();
        assert!(snapshot.secondary.is_none());
        assert!(snapshot.primary.used_percent.is_finite());
    }
}
