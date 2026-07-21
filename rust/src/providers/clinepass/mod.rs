//! ClinePass API-key usage provider (upstream 0.44 #2219).
//!
//! `GET https://api.cline.bot/api/v1/users/me/plan/usage-limits`

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const USAGE_URL: &str = "https://api.cline.bot/api/v1/users/me/plan/usage-limits";
const CREDENTIAL_TARGET: &str = "codexbar-clinepass";
const ENV_KEYS: &[&str] = &["CLINEPASS_API_KEY", "CLINE_API_KEY"];

#[derive(Debug, Deserialize)]
struct LimitsResponse {
    success: bool,
    data: LimitsData,
}

#[derive(Debug, Deserialize)]
struct LimitsData {
    limits: Vec<LimitEntry>,
}

#[derive(Debug, Deserialize)]
struct LimitEntry {
    #[serde(rename = "type")]
    limit_type: String,
    #[serde(rename = "percentUsed")]
    percent_used: f64,
    #[serde(rename = "resetsAt")]
    resets_at: Option<String>,
}

pub struct ClinePassProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ClinePassProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::ClinePass,
                display_name: "ClinePass",
                session_label: "5-hour",
                weekly_label: "Weekly",
                supports_opus: true,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(
                    "https://app.cline.bot/dashboard/subscription?personal=true",
                ),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for ClinePassProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ClinePassProvider {
    fn id(&self) -> ProviderId {
        ProviderId::ClinePass
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    ENV_KEYS,
                )?;
                let resp = self
                    .client
                    .get(USAGE_URL)
                    .bearer_auth(key)
                    .header("Accept", "application/json")
                    .send()
                    .await?;
                let status = resp.status();
                if status == reqwest::StatusCode::UNAUTHORIZED
                    || status == reqwest::StatusCode::FORBIDDEN
                {
                    return Err(ProviderError::AuthRequired);
                }
                if !status.is_success() {
                    return Err(ProviderError::Other(format!(
                        "ClinePass API error: HTTP {status}"
                    )));
                }
                let body: LimitsResponse = resp.json().await.map_err(|e| {
                    ProviderError::Parse(format!("Failed to parse ClinePass usage: {e}"))
                })?;
                let snap = snapshot_from_limits(&body)?;
                Ok(ProviderFetchResult::new(snap, "api"))
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

fn parse_iso(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn window_for(entry: &LimitEntry) -> Option<(RateWindow, &'static str)> {
    let minutes = match entry.limit_type.as_str() {
        "five_hour" => Some(5 * 60),
        "weekly" => Some(7 * 24 * 60),
        "monthly" => Some(30 * 24 * 60),
        _ => None,
    }?;
    let mut w = RateWindow::new(entry.percent_used.clamp(0.0, 100.0));
    w.window_minutes = Some(minutes);
    w.resets_at = parse_iso(entry.resets_at.as_deref());
    let slot = match entry.limit_type.as_str() {
        "five_hour" => "primary",
        "weekly" => "secondary",
        "monthly" => "tertiary",
        _ => return None,
    };
    Some((w, slot))
}

fn snapshot_from_limits(body: &LimitsResponse) -> Result<UsageSnapshot, ProviderError> {
    if !body.success {
        return Err(ProviderError::Parse(
            "ClinePass response success was false".into(),
        ));
    }
    let mut primary = None;
    let mut secondary = None;
    let mut tertiary = None;
    for limit in &body.data.limits {
        if let Some((w, slot)) = window_for(limit) {
            match slot {
                "primary" => primary = Some(w),
                "secondary" => secondary = Some(w),
                "tertiary" => tertiary = Some(w),
                _ => {}
            }
        }
    }
    let primary = primary.ok_or_else(|| {
        ProviderError::Parse("ClinePass response missing five_hour window".into())
    })?;
    let mut snap = UsageSnapshot::new(primary).with_login_method("API key");
    if let Some(s) = secondary {
        snap = snap.with_secondary(s);
    }
    if let Some(t) = tertiary {
        snap = snap.with_tertiary(t);
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_unknown_limit_types() {
        let body: LimitsResponse = serde_json::from_str(
            r#"{
          "success": true,
          "data": {
            "limits": [
              { "type": "five_hour", "percentUsed": 12.5, "resetsAt": "2026-07-16T15:00:00Z" },
              { "type": "experimental_pool", "percentUsed": 77, "resetsAt": "2026-07-16T15:00:00Z" },
              { "type": "weekly", "percentUsed": 25, "resetsAt": "2026-07-20T00:00:00Z" },
              { "type": "monthly", "percentUsed": 40, "resetsAt": null }
            ]
          }
        }"#,
        )
        .unwrap();
        let snap = snapshot_from_limits(&body).unwrap();
        assert!((snap.primary.used_percent - 12.5).abs() < 0.01);
        assert_eq!(snap.primary.window_minutes, Some(300));
        assert!((snap.secondary.as_ref().unwrap().used_percent - 25.0).abs() < 0.01);
        assert!((snap.tertiary.as_ref().unwrap().used_percent - 40.0).abs() < 0.01);
    }
}
