use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const CREDENTIAL_TARGET: &str = "codexbar-chutes";

pub struct ChutesProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ChutesProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Chutes,
                display_name: "Chutes",
                session_label: "4-hour quota",
                weekly_label: "Monthly quota",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://chutes.ai"),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for ChutesProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ChutesProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Chutes
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
                    &["CHUTES_API_KEY"],
                )?;
                let base = std::env::var("CHUTES_API_URL")
                    .unwrap_or_else(|_| "https://api.chutes.ai".into());
                let url = crate::providers::validated_https_url(&base, "Chutes API")?
                    .join("users/me/subscription_usage")
                    .map_err(|e| ProviderError::Other(format!("Invalid Chutes API URL: {e}")))?;
                let response = self.client.get(url).bearer_auth(key).send().await?;
                if response.status() == reqwest::StatusCode::UNAUTHORIZED
                    || response.status() == reqwest::StatusCode::FORBIDDEN
                {
                    return Err(ProviderError::AuthRequired);
                }
                if !response.status().is_success() {
                    return Err(ProviderError::Other(format!(
                        "Chutes usage returned status {}",
                        response.status()
                    )));
                }
                let value: Value = response.json().await.map_err(|e| {
                    ProviderError::Parse(format!("Failed to parse Chutes usage: {e}"))
                })?;
                Ok(ProviderFetchResult::new(snapshot_from_usage(&value), "api"))
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

fn snapshot_from_usage(value: &Value) -> UsageSnapshot {
    let windows = quota_windows(value);
    let primary = windows
        .first()
        .cloned()
        .unwrap_or_else(|| RateWindow::new(0.0));
    let mut snapshot = UsageSnapshot::new(primary);
    if let Some(second) = windows.get(1).cloned() {
        snapshot = snapshot.with_secondary(second);
    }
    snapshot.with_login_method("Chutes API")
}

fn quota_windows(value: &Value) -> Vec<RateWindow> {
    let mut out = Vec::new();
    collect_windows(value, &mut out);
    out
}

fn collect_windows(value: &Value, out: &mut Vec<RateWindow>) {
    match value {
        Value::Object(map) => {
            if let Some(percent) = percent_from_object(map) {
                out.push(RateWindow::new(percent));
            }
            for value in map.values() {
                collect_windows(value, out);
            }
        }
        Value::Array(items) => {
            for value in items {
                collect_windows(value, out);
            }
        }
        _ => {}
    }
}

fn percent_from_object(map: &serde_json::Map<String, Value>) -> Option<f64> {
    for key in [
        "usage_percent",
        "usagePercent",
        "percent_used",
        "percentUsed",
    ] {
        if let Some(v) = map.get(key).and_then(Value::as_f64) {
            return Some(if v <= 1.0 { v * 100.0 } else { v });
        }
    }
    let used = ["used", "usage", "current_usage", "currentUsage"]
        .iter()
        .find_map(|k| map.get(*k).and_then(Value::as_f64));
    let limit = ["limit", "quota", "quota_limit", "quotaLimit", "total"]
        .iter()
        .find_map(|k| map.get(*k).and_then(Value::as_f64));
    match (used, limit) {
        (Some(used), Some(limit)) if limit > 0.0 => Some(used / limit * 100.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ratio_window() {
        let snapshot =
            snapshot_from_usage(&serde_json::json!({"quotas":[{"used":25,"limit":100}]}));
        assert_eq!(snapshot.primary.used_percent, 25.0);
    }
}
