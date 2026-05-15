//! Kimi K2 provider implementation
//!
//! Fetches usage data from Kimi K2 API platform
//! Uses API key for credit-based usage totals

use async_trait::async_trait;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const KIMIK2_API_BASE_INTERNATIONAL: &str = "https://api.moonshot.ai";
const KIMIK2_API_BASE_CHINA: &str = "https://api.moonshot.cn";

/// Kimi K2 provider (API-based credits)
pub struct KimiK2Provider {
    metadata: ProviderMetadata,
}

impl KimiK2Provider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::KimiK2,
                display_name: "Moonshot / Kimi API",
                session_label: "Balance",
                weekly_label: "Cash",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://platform.moonshot.ai/console/account"),
                status_page_url: None,
            },
        }
    }

    /// Get API key from environment or config
    fn get_api_key(api_key: Option<&str>) -> Option<String> {
        if let Some(key) = api_key
            && !key.is_empty()
        {
            return Some(key.to_string());
        }

        // Check environment variable first
        if let Ok(key) = std::env::var("MOONSHOT_API_KEY")
            && !key.is_empty()
        {
            return Some(key);
        }

        // Check KIMI_API_KEY
        if let Ok(key) = std::env::var("KIMI_API_KEY")
            && !key.is_empty()
        {
            return Some(key);
        }

        // Check config file
        if let Some(config_dir) = dirs::config_dir() {
            let config_file = config_dir.join("moonshot").join("config.json");
            if config_file.exists()
                && let Ok(content) = std::fs::read_to_string(&config_file)
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                && let Some(key) = json.get("api_key").and_then(|v| v.as_str())
            {
                return Some(key.to_string());
            }
        }

        None
    }

    /// Fetch usage via Moonshot API
    async fn fetch_via_api(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let api_key = Self::get_api_key(ctx.api_key.as_deref()).ok_or_else(|| {
            ProviderError::NotInstalled(
                "Moonshot API key not found. Set it in Preferences → Providers, MOONSHOT_API_KEY, or KIMI_API_KEY."
                    .to_string(),
            )
        })?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let api_base = match std::env::var("MOONSHOT_API_REGION")
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str()
        {
            "cn" | "china" => KIMIK2_API_BASE_CHINA,
            _ => KIMIK2_API_BASE_INTERNATIONAL,
        };

        // Fetch account/billing info
        let resp = client
            .get(format!("{}/v1/users/me/balance", api_base))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!("API error: {}", status)));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        self.parse_usage_response(&json)
    }

    /// Parse Kimi K2 usage response
    fn parse_usage_response(
        &self,
        json: &serde_json::Value,
    ) -> Result<UsageSnapshot, ProviderError> {
        let code_ok = json
            .get("code")
            .and_then(|v| v.as_i64())
            .is_none_or(|code| code == 0);
        let status_ok = json.get("status").and_then(|v| v.as_bool()).unwrap_or(true);
        if !code_ok || !status_ok {
            let code = json
                .get("code")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let scode = json
                .get("scode")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(ProviderError::Other(format!(
                "Moonshot API error: code {code}, scode {scode}"
            )));
        }

        // Extract balance/credit information
        let data = json.get("data").unwrap_or(json);

        // Available balance (credits remaining)
        let available_balance = data
            .get("available_balance")
            .or_else(|| data.get("balance"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Total credits (used + available)
        let total_credits = data
            .get("total_balance")
            .or_else(|| data.get("total"))
            .and_then(|v| v.as_f64())
            .unwrap_or(available_balance.max(0.0));

        // Used credits
        let used_credits = data
            .get("used_balance")
            .or_else(|| data.get("used"))
            .and_then(|v| v.as_f64())
            .unwrap_or(total_credits - available_balance);

        // Calculate percentage used
        let used_percent = if total_credits > 0.0 {
            (used_credits / total_credits) * 100.0
        } else {
            0.0
        };

        // Cash balance (if any)
        let voucher_balance = data.get("voucher_balance").and_then(|v| v.as_f64());
        let cash_balance = data.get("cash_balance").and_then(|v| v.as_f64());

        // Create primary rate window (credits used)
        let mut primary = RateWindow::new(used_percent);
        primary.reset_description = Some(format!("Balance ${available_balance:.2}"));

        let mut login_method = format!("Balance: ${available_balance:.2}");
        if let Some(cash) = cash_balance
            && cash < 0.0
        {
            login_method.push_str(&format!(" · ${:.2} in deficit", cash.abs()));
        }

        let mut usage = UsageSnapshot::new(primary).with_login_method(login_method);

        // Add secondary window for cash balance if available
        if let Some(voucher) = voucher_balance {
            let mut voucher_window = RateWindow::new(0.0);
            voucher_window.reset_description = Some(format!("Voucher ${voucher:.2}"));
            usage = usage.with_extra_rate_window("voucher", "Voucher balance", voucher_window);
        }

        if let Some(cash) = cash_balance {
            let mut cash_window = RateWindow::new(if cash < 0.0 { 100.0 } else { 0.0 });
            cash_window.reset_description = Some(format!("Cash ${cash:.2}"));
            usage = usage.with_extra_rate_window("cash", "Cash balance", cash_window);
        }

        Ok(usage)
    }
}

impl Default for KimiK2Provider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for KimiK2Provider {
    fn id(&self) -> ProviderId {
        ProviderId::KimiK2
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Kimi K2 usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web | SourceMode::OAuth => {
                let usage = self.fetch_via_api(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "api"))
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::KimiK2Provider;

    #[test]
    fn explicit_api_key_overrides_environment_lookup() {
        assert_eq!(
            KimiK2Provider::get_api_key(Some("kimi-direct-key")),
            Some("kimi-direct-key".to_string())
        );
    }
}
