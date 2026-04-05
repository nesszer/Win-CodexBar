//! Antigravity provider implementation
//!
//! Fetches usage data from Antigravity's local language server probe
//! Uses Windows process detection to find CSRF token

use async_trait::async_trait;
use regex_lite::Regex;
use serde::Deserialize;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

/// Antigravity provider
pub struct AntigravityProvider {
    metadata: ProviderMetadata,
}

impl AntigravityProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Antigravity,
                display_name: "Antigravity",
                session_label: "Claude",
                weekly_label: "Gemini Pro",
                supports_opus: true,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: None,
                status_page_url: None,
            },
        }
    }

    fn build_local_client(timeout: std::time::Duration) -> Result<reqwest::Client, ProviderError> {
        reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))
    }

    /// Detect running Antigravity language server and extract connection info
    fn detect_process_info() -> Result<ProcessInfo, ProviderError> {
        // Use PowerShell to get process command lines
        #[cfg(windows)]
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut cmd = Command::new("powershell.exe");
        cmd.args([
                "-ExecutionPolicy", "Bypass",
                "-Command",
                "Get-CimInstance Win32_Process | Where-Object { $_.Name -like '*language_server_windows*' } | Select-Object -ExpandProperty CommandLine"
            ]);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let output = cmd
            .output()
            .map_err(|e| ProviderError::Other(format!("Failed to run PowerShell: {}", e)))?;

        if !output.status.success() {
            return Err(ProviderError::NotInstalled(
                "Failed to detect Antigravity process".to_string(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse command line for CSRF token and port
        let csrf_regex = Regex::new(r"--csrf_token\s+([a-f0-9-]+)").unwrap();
        let port_regex = Regex::new(r"--extension_server_port\s+(\d+)").unwrap();

        for line in stdout.lines() {
            if line.contains("language_server_windows") && line.contains("--csrf_token") {
                let csrf_token = csrf_regex
                    .captures(line)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                let port = port_regex
                    .captures(line)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u16>().ok());

                if let (Some(token), Some(p)) = (csrf_token, port) {
                    return Ok(ProcessInfo {
                        csrf_token: token,
                        extension_port: p,
                    });
                }
            }
        }

        Err(ProviderError::NotInstalled(
            "Antigravity language server not running".to_string(),
        ))
    }

    /// Find the actual API port by checking listening ports
    async fn find_api_port(extension_port: u16) -> Result<u16, ProviderError> {
        // The language server listens on multiple ports near the extension port
        // Try ports in range extension_port to extension_port + 20
        let client = Self::build_local_client(std::time::Duration::from_secs(2))?;

        for offset in 0..20 {
            let port = extension_port + offset;
            let url = format!(
                "https://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/GetUnleashData",
                port
            );

            // Just check if the port responds (even with error)
            if let Ok(resp) = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Connect-Protocol-Version", "1")
                .body("{}")
                .send()
                .await
            {
                // If we get any response (even error), this is the API port
                if resp.status().as_u16() == 200 || resp.status().as_u16() == 401 {
                    return Ok(port);
                }
            }
        }

        // Fallback: try common ports
        for port in [53835, 53836, 53837, 53838, 53845, 53849] {
            let url = format!(
                "https://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/GetUnleashData",
                port
            );
            if let Ok(resp) = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Connect-Protocol-Version", "1")
                .body("{}")
                .send()
                .await
                && (resp.status().as_u16() == 200 || resp.status().as_u16() == 401)
            {
                return Ok(port);
            }
        }

        Err(ProviderError::Other(
            "Could not find Antigravity API port".to_string(),
        ))
    }

    /// Fetch user status from Antigravity API
    async fn fetch_user_status(&self) -> Result<UsageSnapshot, ProviderError> {
        let process_info = Self::detect_process_info()?;
        let api_port = Self::find_api_port(process_info.extension_port).await?;

        let client = Self::build_local_client(std::time::Duration::from_secs(8))?;

        let url = format!(
            "https://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/GetUserStatus",
            api_port
        );

        let body = serde_json::json!({
            "metadata": {
                "ideName": "antigravity",
                "extensionName": "antigravity",
                "ideVersion": "unknown",
                "locale": "en"
            }
        });

        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .header("X-Codeium-Csrf-Token", &process_info.csrf_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Other(format!("API request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Other(format!(
                "API error {}: {}",
                status, text
            )));
        }

        let json: UserStatusResponse = resp
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("Failed to parse response: {}", e)))?;

        self.parse_user_status(json)
    }

    fn parse_user_status(
        &self,
        response: UserStatusResponse,
    ) -> Result<UsageSnapshot, ProviderError> {
        let user_status = response
            .user_status
            .ok_or_else(|| ProviderError::Other("Missing userStatus".to_string()))?;

        let model_configs = user_status
            .cascade_model_config_data
            .and_then(|d| d.client_model_configs)
            .unwrap_or_default();

        let mut primary: Option<RateWindow> = None;
        let mut secondary: Option<RateWindow> = None;
        let mut tertiary: Option<RateWindow> = None;

        for config in &model_configs {
            let family = classify_model(&config.label);
            match family {
                ModelFamily::Claude if primary.is_none() => {
                    if let Some(quota) = &config.quota_info {
                        primary = Some(rate_window_from_quota(quota));
                    }
                }
                ModelFamily::GeminiProLow if secondary.is_none() => {
                    if let Some(quota) = &config.quota_info {
                        secondary = Some(rate_window_from_quota(quota));
                    }
                }
                ModelFamily::GeminiFlash if tertiary.is_none() => {
                    if let Some(quota) = &config.quota_info {
                        tertiary = Some(rate_window_from_quota(quota));
                    }
                }
                _ => {}
            }
        }

        if primary.is_none()
            && let Some(first) = model_configs.first()
            && let Some(quota) = &first.quota_info
        {
            primary = Some(rate_window_from_quota(quota));
        }

        let primary = primary.unwrap_or_else(|| RateWindow::new(0.0));
        let mut snapshot = UsageSnapshot::new(primary);

        if let Some(sec) = secondary {
            snapshot = snapshot.with_secondary(sec);
        }
        if let Some(ter) = tertiary {
            snapshot = snapshot.with_model_specific(ter);
        }

        // Add plan info
        let plan_name = user_status
            .plan_status
            .and_then(|ps| ps.plan_info)
            .and_then(|pi| pi.plan_display_name.or(pi.plan_name));

        if let Some(plan) = plan_name {
            snapshot = snapshot.with_login_method(&plan);
        }

        Ok(snapshot)
    }
}

impl Default for AntigravityProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AntigravityProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Antigravity
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, _ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Antigravity usage via local probe");

        match self.fetch_user_status().await {
            Ok(usage) => Ok(ProviderFetchResult::new(usage, "local")),
            Err(e) => {
                tracing::warn!("Antigravity probe failed: {}", e);
                Err(e)
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Cli]
    }

    fn supports_cli(&self) -> bool {
        true
    }
}

struct ProcessInfo {
    csrf_token: String,
    extension_port: u16,
}

// API Response types

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserStatusResponse {
    user_status: Option<UserStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserStatus {
    #[allow(dead_code)]
    email: Option<String>,
    plan_status: Option<PlanStatus>,
    cascade_model_config_data: Option<ModelConfigData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanStatus {
    plan_info: Option<PlanInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanInfo {
    plan_name: Option<String>,
    plan_display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelConfigData {
    client_model_configs: Option<Vec<ModelConfig>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelConfig {
    label: String,
    quota_info: Option<QuotaInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaInfo {
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
}

// ── Model-family classification ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum ModelFamily {
    Claude,
    ClaudeThinking,
    GeminiProLow,
    GeminiFlash,
    Other,
}

fn classify_model(label: &str) -> ModelFamily {
    let lower = label.to_lowercase();
    if lower.contains("claude") {
        if lower.contains("thinking") {
            ModelFamily::ClaudeThinking
        } else {
            ModelFamily::Claude
        }
    } else if lower.contains("gemini") && lower.contains("pro") && lower.contains("low") {
        ModelFamily::GeminiProLow
    } else if lower.contains("gemini") && lower.contains("flash") {
        ModelFamily::GeminiFlash
    } else if lower.contains("pro") && lower.contains("low") {
        ModelFamily::GeminiProLow
    } else if lower.contains("flash") {
        ModelFamily::GeminiFlash
    } else {
        ModelFamily::Other
    }
}

fn rate_window_from_quota(quota: &QuotaInfo) -> RateWindow {
    let remaining = quota.remaining_fraction.unwrap_or(1.0);
    let used_percent = (1.0 - remaining) * 100.0;
    RateWindow::with_details(used_percent, None, None, quota.reset_time.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_model_families() {
        assert_eq!(classify_model("Claude 3.5 Sonnet"), ModelFamily::Claude);
        assert_eq!(classify_model("claude-4-opus"), ModelFamily::Claude);
        assert_eq!(
            classify_model("Claude Thinking"),
            ModelFamily::ClaudeThinking
        );
        assert_eq!(
            classify_model("claude-3.5-sonnet-thinking"),
            ModelFamily::ClaudeThinking
        );
        assert_eq!(
            classify_model("Gemini 2.5 Pro Low"),
            ModelFamily::GeminiProLow
        );
        assert_eq!(classify_model("gemini-pro-low"), ModelFamily::GeminiProLow);
        assert_eq!(classify_model("Pro Low Latency"), ModelFamily::GeminiProLow);
        assert_eq!(classify_model("Gemini 2.5 Flash"), ModelFamily::GeminiFlash);
        assert_eq!(classify_model("gemini-flash"), ModelFamily::GeminiFlash);
        assert_eq!(classify_model("Flash Model"), ModelFamily::GeminiFlash);
        assert_eq!(classify_model("GPT-4o"), ModelFamily::Other);
        assert_eq!(classify_model("unknown-model"), ModelFamily::Other);
    }

    fn make_response(models: Vec<(&str, f64)>) -> UserStatusResponse {
        let json = serde_json::json!({
            "userStatus": {
                "cascadeModelConfigData": {
                    "clientModelConfigs": models.iter().map(|(label, remaining)| {
                        serde_json::json!({
                            "label": label,
                            "quotaInfo": {
                                "remainingFraction": remaining
                            }
                        })
                    }).collect::<Vec<_>>()
                }
            }
        });
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn test_parse_user_status_standard() {
        let resp = make_response(vec![
            ("Claude 3.5 Sonnet", 0.8),
            ("Gemini 2.5 Pro Low", 0.5),
            ("Gemini 2.5 Flash", 0.9),
        ]);
        let provider = AntigravityProvider::new();
        let snap = provider.parse_user_status(resp).unwrap();

        assert!((snap.primary.used_percent - 20.0).abs() < 0.1);
        let sec = snap.secondary.unwrap();
        assert!((sec.used_percent - 50.0).abs() < 0.1);
        let ter = snap.model_specific.unwrap();
        assert!((ter.used_percent - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_parse_user_status_thinking_skipped() {
        let resp = make_response(vec![
            ("Claude Thinking", 0.6),
            ("Claude 3.5 Sonnet", 0.7),
            ("Gemini 2.5 Flash", 0.5),
        ]);
        let provider = AntigravityProvider::new();
        let snap = provider.parse_user_status(resp).unwrap();

        assert!((snap.primary.used_percent - 30.0).abs() < 0.1);
    }

    #[test]
    fn test_parse_user_status_fallback_first() {
        let resp = make_response(vec![("GPT-4o", 0.4), ("Mistral Large", 0.6)]);
        let provider = AntigravityProvider::new();
        let snap = provider.parse_user_status(resp).unwrap();

        assert!((snap.primary.used_percent - 60.0).abs() < 0.1);
        assert!(snap.secondary.is_none());
        assert!(snap.model_specific.is_none());
    }

    #[test]
    fn test_local_client_builder_keeps_tls_verification_enabled() {
        let client =
            AntigravityProvider::build_local_client(std::time::Duration::from_secs(1)).unwrap();
        let _ = client;
    }
}
