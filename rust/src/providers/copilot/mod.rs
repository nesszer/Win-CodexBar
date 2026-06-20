//! GitHub Copilot provider implementation
//!
//! Fetches usage data from GitHub's Copilot API using stored OAuth token

mod api;
pub mod device_flow;

use async_trait::async_trait;

use crate::core::{
    FetchContext, NamedRateWindow, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode,
};

pub use api::CopilotApi;

/// GitHub Copilot provider for fetching AI usage limits
pub struct CopilotProvider {
    metadata: ProviderMetadata,
    api: CopilotApi,
}

impl CopilotProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Copilot,
                display_name: "GitHub Copilot",
                session_label: "Premium",
                weekly_label: "Chat",
                supports_opus: false,
                supports_credits: false,
                default_enabled: true,
                is_primary: false,
                dashboard_url: Some("https://github.com/settings/copilot"),
                status_page_url: Some("https://www.githubstatus.com/"),
            },
            api: CopilotApi::new(),
        }
    }
}

impl Default for CopilotProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Copilot
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching GitHub Copilot usage via GitHub OAuth");

        match self.api.fetch_usage(ctx.api_key.as_deref()).await {
            Ok(mut usage) => {
                // ponytail: full GitHub web budget scraping needs nonce/session churn; env JSON keeps the optional path testable.
                if let Ok(raw) = std::env::var("CODEXBAR_COPILOT_BUDGET_JSON") {
                    usage.extra_rate_windows.extend(parse_budget_windows(&raw));
                }
                Ok(ProviderFetchResult::new(usage, "oauth"))
            }
            Err(e) => {
                tracing::warn!("Copilot API fetch failed: {}", e);
                Err(e)
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn supports_oauth(&self) -> bool {
        true
    }
}

fn parse_budget_windows(raw: &str) -> Vec<NamedRateWindow> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    value
        .get("budgets")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|budget| {
            let limit = number(
                budget,
                &["budgetAmount", "budget_amount", "amount", "limit"],
            )?;
            if limit <= 0.0 || !is_copilot_budget(budget) {
                return None;
            }
            let used = number(
                budget,
                &["currentAmount", "current_amount", "spent", "used"],
            )
            .unwrap_or(0.0);
            let title = string(budget, &["name", "budgetEntityName", "budget_entity_name"])
                .unwrap_or_else(|| "Copilot budget".to_string());
            Some(NamedRateWindow::new(
                format!(
                    "copilot-budget-{}",
                    title.to_ascii_lowercase().replace(' ', "-")
                ),
                title,
                RateWindow::new(used / limit * 100.0),
            ))
        })
        .collect()
}

fn is_copilot_budget(value: &serde_json::Value) -> bool {
    [
        "budgetProductSkus",
        "budget_product_skus",
        "budgetProductSku",
        "budget_product_sku",
        "budgetType",
        "budget_type",
        "name",
    ]
    .iter()
    .filter_map(|key| value.get(*key))
    .any(|v| v.to_string().to_ascii_lowercase().contains("copilot"))
}

fn number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_f64))
}

fn string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_budget_windows() {
        let windows = parse_budget_windows(
            r#"{"budgets":[{"name":"Copilot org","budgetAmount":100,"currentAmount":25,"budgetProductSkus":["copilot"]}]}"#,
        );
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window.used_percent, 25.0);
    }
}
