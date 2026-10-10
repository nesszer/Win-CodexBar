//! ZenMux Management API usage provider (upstream 0.44).
//!
//! - `GET https://zenmux.ai/api/v1/management/subscription/detail`
//! - Optional PAYG: `GET .../payg/balance`, requested only when the caller opted
//!   into optional credits (`FetchContext.include_credits`); a 401/403 there is fatal.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, RateWindow, SourceMode, SubscriptionMetadata, UsageSnapshot,
};
use crate::providers::format;

const MANAGEMENT_BASE: &str = "https://zenmux.ai/api/v1/management";
const CREDENTIAL_TARGET: &str = "codexbar-zenmux";
const ENV_KEYS: &[&str] = &["ZENMUX_MANAGEMENT_API_KEY", "ZENMUX_API_KEY"];

#[derive(Debug, Deserialize)]
struct SubscriptionEnvelope {
    success: bool,
    data: SubscriptionData,
}

#[derive(Debug, Deserialize)]
struct SubscriptionData {
    plan: PlanInfo,
    #[serde(default)]
    account_status: String,
    quota_5_hour: QuotaInfo,
    quota_7_day: QuotaInfo,
}

#[derive(Debug, Deserialize)]
struct PlanInfo {
    #[serde(default)]
    tier: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QuotaInfo {
    usage_percentage: f64,
    resets_at: Option<String>,
    max_flows: f64,
    used_flows: f64,
    #[allow(
        dead_code,
        reason = "field mirrors the ZenMux API payload; deserialized for round-trip fidelity but not read yet"
    )]
    remaining_flows: f64,
}

#[derive(Debug, Deserialize)]
struct BalanceEnvelope {
    success: bool,
    data: BalanceData,
}

#[derive(Debug, Deserialize)]
struct BalanceData {
    currency: String,
    total_credits: f64,
}

pub struct ZenMuxProvider {
    client: Client,
}

impl ZenMuxProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn resolve_key(api_key: Option<&str>) -> Result<String, ProviderError> {
        crate::providers::resolve_api_key(api_key, CREDENTIAL_TARGET, ENV_KEYS)
    }

    async fn fetch_json(&self, path: &str, key: &str) -> Result<serde_json::Value, ProviderError> {
        let url = format!("{MANAGEMENT_BASE}/{path}");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(key)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "ZenMux Management API returned HTTP {status}"
            )));
        }
        resp.json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse ZenMux response: {e}")))
    }
}

impl Default for ZenMuxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ZenMuxProvider {
    fn id(&self) -> ProviderId {
        ProviderId::ZenMux
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = Self::resolve_key(ctx.api_key.as_deref())?;
                let sub_val = self.fetch_json("subscription/detail", &key).await?;
                let snapshot = snapshot_from_subscription(&sub_val)?;

                let result = ProviderFetchResult::new(snapshot, "api");
                if !ctx.include_credits {
                    return Ok(result);
                }
                let balance = self.fetch_json("payg/balance", &key).await;
                attach_payg_balance(result, balance)
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

/// Strict ISO-8601 (`YYYY-MM-DDTHH:MM:SS[.f+](Z|+HH:MM)`), matching the shape
/// upstream `zenmux.js` accepts. Anything else is treated as absent.
fn parse_iso(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw?;
    if !is_strict_iso8601(raw) {
        return None;
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn is_strict_iso8601(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if !(digits(0..4)
        && digits(5..7)
        && digits(8..10)
        && digits(11..13)
        && digits(14..16)
        && digits(17..19)
        && separators
            .iter()
            .all(|&(index, expected)| bytes.get(index) == Some(&expected)))
    {
        return false;
    }
    let mut rest = &raw[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let count = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            return false;
        }
        rest = &fraction[count..];
    }
    match rest.as_bytes() {
        [b'Z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            [h1, h2, m1, m2].iter().all(|digit| digit.is_ascii_digit())
        }
        _ => false,
    }
}

fn quota_window(q: &QuotaInfo, minutes: u32) -> RateWindow {
    let used = (q.usage_percentage * 100.0).clamp(0.0, 100.0);
    let mut w = RateWindow::new(used);
    w.window_minutes = Some(minutes);
    w.resets_at = parse_iso(q.resets_at.as_deref());
    w.reset_description = Some(format!(
        "{} / {} flows",
        format::whole_or_two_decimals(q.used_flows),
        format::whole_or_two_decimals(q.max_flows)
    ));
    w
}

fn snapshot_from_subscription(value: &serde_json::Value) -> Result<UsageSnapshot, ProviderError> {
    let env: SubscriptionEnvelope = serde_json::from_value(value.clone())
        .map_err(|e| ProviderError::Parse(format!("Failed to parse ZenMux subscription: {e}")))?;
    if !env.success {
        return Err(ProviderError::Parse(
            "ZenMux subscription response reported failure".into(),
        ));
    }
    let plan = env.data.plan.tier.trim();
    let status = env.data.account_status.trim();
    let login = if status.eq_ignore_ascii_case("healthy") || status.is_empty() {
        if plan.is_empty() {
            None
        } else {
            Some(format!("{} plan", capitalize(plan)))
        }
    } else if plan.is_empty() {
        Some(capitalize(status))
    } else {
        Some(format!(
            "{} plan · {}",
            capitalize(plan),
            capitalize(status)
        ))
    };

    let mut snap = UsageSnapshot::new(quota_window(&env.data.quota_5_hour, 5 * 60))
        .with_secondary(quota_window(&env.data.quota_7_day, 7 * 24 * 60));
    if let Some(login) = login {
        snap = snap.with_login_method(login);
    }
    if let Some(expires_at) = parse_iso(env.data.plan.expires_at.as_deref()) {
        snap = snap.with_subscription(Some(SubscriptionMetadata::new(
            None,
            Some(expires_at),
            None,
        )));
    }
    Ok(snap)
}

/// Attach the optional PAYG balance. A 401/403 during this optional request is
/// fatal (the key is rejected); every other failure keeps the quota usage.
fn attach_payg_balance(
    result: ProviderFetchResult,
    balance: Result<serde_json::Value, ProviderError>,
) -> Result<ProviderFetchResult, ProviderError> {
    match balance {
        Err(ProviderError::AuthRequired) => Err(ProviderError::AuthRequired),
        Err(_) => Ok(result),
        Ok(value) => Ok(match payg_from_balance(&value) {
            Ok((cost, row)) => result.with_cost(cost).with_display_detail(row),
            Err(_) => result,
        }),
    }
}

/// Zero and negative balances stay visible. `CostSnapshot` clamps negatives to
/// zero, so the typed value keeps only the non-negative part and the signed
/// amount is also returned as a formatted display row.
fn payg_from_balance(
    value: &serde_json::Value,
) -> Result<(CostSnapshot, Option<ProviderDisplayDetail>), ProviderError> {
    let env: BalanceEnvelope = serde_json::from_value(value.clone())
        .map_err(|e| ProviderError::Parse(format!("Failed to parse ZenMux balance: {e}")))?;
    if !env.success {
        return Err(ProviderError::Parse(
            "ZenMux balance response reported failure".into(),
        ));
    }
    if !env.data.currency.trim().eq_ignore_ascii_case("usd") {
        return Err(ProviderError::Parse(
            "ZenMux balance currency is not USD".into(),
        ));
    }
    if !env.data.total_credits.is_finite() {
        return Err(ProviderError::Parse(
            "ZenMux balance is not a finite number".into(),
        ));
    }
    let cost = CostSnapshot::new(env.data.total_credits, "USD", "ZenMux PAYG balance");
    let row = ProviderDisplayDetail::new(
        "payg_balance",
        "Pay-as-you-go balance",
        format::usd_signed(env.data.total_credits),
    );
    Ok((cost, row))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_subscription_windows() {
        let value = json!({
            "success": true,
            "data": {
                "plan": { "tier": "pro", "expires_at": "2026-08-01T00:00:00Z" },
                "account_status": "healthy",
                "quota_5_hour": {
                    "usage_percentage": 0.25,
                    "resets_at": "2026-07-21T10:00:00Z",
                    "max_flows": 100,
                    "used_flows": 25,
                    "remaining_flows": 75
                },
                "quota_7_day": {
                    "usage_percentage": 0.1,
                    "resets_at": "2026-07-28T00:00:00Z",
                    "max_flows": 1000,
                    "used_flows": 100,
                    "remaining_flows": 900
                }
            }
        });
        let snap = snapshot_from_subscription(&value).unwrap();
        assert!((snap.primary.used_percent - 25.0).abs() < 0.01);
        assert_eq!(snap.primary.window_minutes, Some(300));
        let weekly = snap.secondary.unwrap();
        assert!((weekly.used_percent - 10.0).abs() < 0.01);
        assert_eq!(snap.login_method.as_deref(), Some("Pro plan"));
    }

    #[test]
    fn parses_payg_balance() {
        let value = json!({
            "success": true,
            "data": { "currency": "USD", "total_credits": 12.5 }
        });
        let (cost, row) = payg_from_balance(&value).unwrap();
        assert!((cost.used - 12.5).abs() < 0.001);
        assert_eq!(cost.currency_code, "USD");
        assert_eq!(row.unwrap().value(), "$12.50");
    }

    fn subscription_fixture() -> serde_json::Value {
        json!({
            "success": true,
            "data": {
                "plan": {
                    "tier": "ultra",
                    "amount_usd": 200,
                    "interval": "month",
                    "expires_at": "2026-04-12T08:26:56.000Z"
                },
                "currency": "usd",
                "account_status": "healthy",
                "quota_5_hour": {
                    "usage_percentage": 0.0715,
                    "resets_at": "2026-03-24T08:35:09.000Z",
                    "max_flows": 800,
                    "used_flows": 57.2,
                    "remaining_flows": 742.8
                },
                "quota_7_day": {
                    "usage_percentage": 0.0673,
                    "resets_at": "2026-03-26T02:15:05.000Z",
                    "max_flows": 6182,
                    "used_flows": 416.11,
                    "remaining_flows": 5765.89
                },
                "quota_monthly": { "max_flows": 34560, "max_value_usd": 1134.33 }
            }
        })
    }

    fn balance_fixture(total_credits: f64) -> serde_json::Value {
        json!({
            "success": true,
            "data": {
                "currency": "usd",
                "total_credits": total_credits,
                "top_up_credits": 35,
                "bonus_credits": 447.74
            }
        })
    }

    fn base_result() -> ProviderFetchResult {
        let snapshot = snapshot_from_subscription(&subscription_fixture()).unwrap();
        ProviderFetchResult::new(snapshot, "api")
    }

    fn utc(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn plan_expiry_populates_subscription_metadata() {
        let snap = snapshot_from_subscription(&subscription_fixture()).unwrap();
        let subscription = snap.subscription.expect("subscription metadata");
        assert_eq!(subscription.expires_at, Some(utc("2026-04-12T08:26:56Z")));
        assert_eq!(subscription.renews_at, None);
        assert_eq!(subscription.starts_at, None);
        assert_eq!(snap.login_method.as_deref(), Some("Ultra plan"));
        assert_eq!(
            snap.primary.reset_description.as_deref(),
            Some("57.20 / 800 flows")
        );
        assert_eq!(snap.primary.resets_at, Some(utc("2026-03-24T08:35:09Z")));
        assert_eq!(
            snap.secondary.unwrap().reset_description.as_deref(),
            Some("416.11 / 6182 flows")
        );
    }

    #[test]
    fn plan_expiry_requires_strict_iso_8601() {
        for raw in [
            "not-a-date",
            "",
            "2026-04-12",
            "2026-04-12 08:26:56Z",
            "2026-04-12t08:26:56z",
            "2026-04-12T08:26:56",
            "2026-04-12T08:26:56.Z",
            "2026-04-12T08:26:56+0800",
            "2026-13-12T08:26:56Z",
            " 2026-04-12T08:26:56Z",
        ] {
            let mut value = subscription_fixture();
            value["data"]["plan"]["expires_at"] = json!(raw);
            let snap = snapshot_from_subscription(&value).unwrap();
            assert!(snap.subscription.is_none(), "{raw:?} must be omitted");
        }
        for raw in [
            "2026-04-12T08:26:56Z",
            "2026-04-12T08:26:56.5Z",
            "2026-04-12T08:26:56+08:00",
            "2026-04-12T08:26:56.123456-05:30",
        ] {
            let mut value = subscription_fixture();
            value["data"]["plan"]["expires_at"] = json!(raw);
            let snap = snapshot_from_subscription(&value).unwrap();
            assert!(snap.subscription.is_some(), "{raw:?} must be accepted");
        }
    }

    #[test]
    fn missing_or_null_plan_expiry_is_omitted() {
        let mut value = subscription_fixture();
        value["data"]["plan"]["expires_at"] = serde_json::Value::Null;
        assert!(
            snapshot_from_subscription(&value)
                .unwrap()
                .subscription
                .is_none()
        );
        value["data"]["plan"]
            .as_object_mut()
            .unwrap()
            .remove("expires_at");
        assert!(
            snapshot_from_subscription(&value)
                .unwrap()
                .subscription
                .is_none()
        );
    }

    #[test]
    fn non_string_plan_expiry_fails_parsing() {
        let mut value = subscription_fixture();
        value["data"]["plan"]["expires_at"] = json!(12345);
        assert!(matches!(
            snapshot_from_subscription(&value),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn unhealthy_account_status_is_included_in_identity() {
        let mut value = subscription_fixture();
        value["data"]["account_status"] = json!("monitored");
        let snap = snapshot_from_subscription(&value).unwrap();
        assert_eq!(snap.login_method.as_deref(), Some("Ultra plan · Monitored"));
    }

    #[test]
    fn quota_fractions_require_json_numbers() {
        for bad in [json!("0.0715"), json!(null), json!(true)] {
            let mut value = subscription_fixture();
            value["data"]["quota_5_hour"]["usage_percentage"] = bad;
            assert!(matches!(
                snapshot_from_subscription(&value),
                Err(ProviderError::Parse(_))
            ));
        }
    }

    #[test]
    fn malformed_subscription_payload_fails_parsing() {
        let value = json!({"success": true, "data": {"plan": {}}});
        assert!(matches!(
            snapshot_from_subscription(&value),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn flow_labels_use_two_decimals_or_integers() {
        assert_eq!(format::whole_or_two_decimals(57.2), "57.20");
        assert_eq!(format::whole_or_two_decimals(6182.0), "6182");
        assert_eq!(format::whole_or_two_decimals(0.125), "0.12");
        assert_eq!(format::whole_or_two_decimals(1.375), "1.38");
        assert_eq!(format::whole_or_two_decimals(-0.125), "-0.12");
        assert_eq!(format::whole_or_two_decimals(-0.0), "-0");
        assert_eq!(
            format::whole_or_two_decimals(1e21),
            "1000000000000000000000"
        );
    }

    #[test]
    fn payg_balance_attaches_cost_and_signed_row() {
        let result = attach_payg_balance(base_result(), Ok(balance_fixture(482.74))).unwrap();
        let cost = result.cost.as_ref().expect("cost");
        assert_eq!(cost.used, 482.74);
        assert_eq!(cost.currency_code, "USD");
        assert_eq!(cost.period, "ZenMux PAYG balance");
        let rows = result.display_details();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id(), "payg_balance");
        assert_eq!(rows[0].value(), "$482.74");
    }

    #[test]
    fn negative_overdue_payg_balance_remains_visible() {
        let result = attach_payg_balance(base_result(), Ok(balance_fixture(-12.34))).unwrap();
        let cost = result.cost.as_ref().expect("typed cost stays present");
        assert_eq!(cost.used, 0.0, "typed amounts clamp to zero");
        assert_eq!(result.display_details()[0].value(), "-$12.34");
    }

    #[test]
    fn zero_payg_balance_remains_visible() {
        for zero in [0.0, -0.0, -0.001] {
            let result = attach_payg_balance(base_result(), Ok(balance_fixture(zero))).unwrap();
            assert_eq!(result.cost.as_ref().expect("cost").used, 0.0);
            assert_eq!(result.display_details()[0].value(), "$0.00");
        }
    }

    #[test]
    fn payg_auth_failure_is_fatal() {
        assert!(matches!(
            attach_payg_balance(base_result(), Err(ProviderError::AuthRequired)),
            Err(ProviderError::AuthRequired)
        ));
    }

    #[test]
    fn payg_other_failures_keep_quota_usage() {
        for failure in [
            ProviderError::Other("ZenMux Management API returned HTTP 500".into()),
            ProviderError::Parse("bad json".into()),
        ] {
            let result = attach_payg_balance(base_result(), Err(failure)).unwrap();
            assert!(result.cost.is_none());
            assert!(result.display_details().is_empty());
            assert!((result.usage.primary.used_percent - 7.15).abs() < 0.0001);
        }
    }

    #[test]
    fn non_usd_or_unsuccessful_payg_balance_is_ignored() {
        let mut eur = balance_fixture(482.74);
        eur["data"]["currency"] = json!("eur");
        let mut failed = balance_fixture(482.74);
        failed["success"] = json!(false);
        let mut malformed = balance_fixture(482.74);
        malformed["data"]["total_credits"] = json!("482.74");
        for value in [eur, failed, malformed] {
            let result = attach_payg_balance(base_result(), Ok(value)).unwrap();
            assert!(result.cost.is_none());
            assert!(result.display_details().is_empty());
            assert!((result.usage.primary.used_percent - 7.15).abs() < 0.0001);
        }
    }
}
