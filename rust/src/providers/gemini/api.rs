//! Gemini API client for fetching quota information
//!
//! Uses Google Cloud Code Private API with OAuth tokens from ~/.gemini/oauth_creds.json

use crate::core::{FetchContext, ProviderError, RateWindow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const QUOTA_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";
const CODE_ASSIST_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
const TOKEN_REFRESH_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Gemini API client
pub struct GeminiApi {
    client: reqwest::Client,
    home_dir: PathBuf,
}

impl GeminiApi {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            home_dir: dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    /// Fetch quota information from the Gemini API
    /// Returns (primary RateWindow, optional model-specific RateWindow, optional email, optional plan)
    /// Note: Gemini quota API requires OAuth tokens, not API keys
    pub async fn fetch_quota(
        &self,
        _ctx: &FetchContext,
    ) -> Result<
        (
            RateWindow,
            Option<RateWindow>,
            Option<String>,
            Option<String>,
        ),
        ProviderError,
    > {
        // Gemini quota endpoint requires OAuth credentials (not API keys)
        // Always load OAuth credentials from ~/.gemini/oauth_creds.json
        let mut creds = self.load_credentials()?;

        // Check if token needs refresh
        if creds.is_expired() {
            tracing::debug!("Gemini token expired, refreshing...");
            creds = self.refresh_token(&creds).await?;
        }

        let access_token = creds
            .access_token
            .clone()
            .ok_or_else(|| ProviderError::AuthRequired)?;

        let hosted_domain = creds
            .id_token
            .as_deref()
            .and_then(extract_hosted_domain_from_jwt);
        let code_assist = self.load_code_assist_status(&access_token).await;
        if is_consumer_client_unsupported(&code_assist, hosted_domain.as_deref()) {
            return Err(ProviderError::Other(
                "Gemini consumer-tier access has been retired by Google. Enable the Antigravity provider or switch to a supported Gemini Code Assist account."
                    .to_string(),
            ));
        }

        // Fetch quota
        let response = self
            .client
            .post(QUOTA_ENDPOINT)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .body("{}")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if response.status() == 401 {
            return Err(ProviderError::AuthRequired);
        }

        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Gemini API returned {}",
                response.status()
            )));
        }

        let quota_response: QuotaResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        // Since we use OAuth, we can use the credentials we already loaded for email extraction
        let (primary, model_specific, email) =
            self.parse_quota_response(quota_response, Some(&creds))?;
        let plan = resolve_account_plan(&code_assist, hosted_domain.as_deref());

        Ok((primary, model_specific, email, plan))
    }

    async fn load_code_assist_status(&self, access_token: &str) -> CodeAssistStatus {
        let response = self
            .client
            .post(CODE_ASSIST_ENDPOINT)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .body(r#"{"metadata":{"ideType":"GEMINI_CLI","pluginType":"GEMINI"}}"#)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;

        let response = match response {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                tracing::warn!(status = %response.status(), "Gemini loadCodeAssist request failed");
                return CodeAssistStatus::default();
            }
            Err(error) => {
                tracing::warn!(%error, "Gemini loadCodeAssist request failed");
                return CodeAssistStatus::default();
            }
        };

        match response.text().await {
            Ok(body) => parse_code_assist_status(&body),
            Err(error) => {
                tracing::warn!(%error, "Gemini loadCodeAssist response was invalid");
                CodeAssistStatus::default()
            }
        }
    }

    fn load_credentials(&self) -> Result<OAuthCredentials, ProviderError> {
        let creds_path = self.home_dir.join(".gemini").join("oauth_creds.json");

        if !creds_path.exists() {
            return Err(ProviderError::NotInstalled(
                "Not logged in to Gemini. Run 'gemini' in Terminal to authenticate.".to_string(),
            ));
        }

        let content = std::fs::read_to_string(&creds_path).map_err(|e| {
            ProviderError::Other(format!("Failed to read Gemini credentials: {}", e))
        })?;

        serde_json::from_str(&content)
            .map_err(|e| ProviderError::Parse(format!("Invalid Gemini credentials: {}", e)))
    }

    async fn refresh_token(
        &self,
        creds: &OAuthCredentials,
    ) -> Result<OAuthCredentials, ProviderError> {
        let refresh_token = creds
            .refresh_token
            .as_ref()
            .ok_or_else(|| ProviderError::AuthRequired)?;

        // Get OAuth client credentials from Gemini CLI
        let client_creds = self.extract_oauth_client_credentials()?;

        let params = [
            ("client_id", client_creds.client_id.as_str()),
            ("client_secret", client_creds.client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .client
            .post(TOKEN_REFRESH_ENDPOINT)
            .form(&params)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ProviderError::AuthRequired);
        }

        let refresh_response: TokenRefreshResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        // Update stored credentials
        let mut new_creds = creds.clone();
        new_creds.access_token = Some(refresh_response.access_token.clone());
        if let Some(id_token) = &refresh_response.id_token {
            new_creds.id_token = Some(id_token.clone());
        }
        if let Some(expires_in) = refresh_response.expires_in {
            let expiry_ms = (chrono::Utc::now().timestamp() as f64 + expires_in) * 1000.0;
            new_creds.expiry_date = Some(expiry_ms);
        }

        // Save updated credentials
        self.save_credentials(&new_creds)?;

        tracing::info!("Gemini token refreshed successfully");
        Ok(new_creds)
    }

    fn save_credentials(&self, creds: &OAuthCredentials) -> Result<(), ProviderError> {
        let creds_path = self.home_dir.join(".gemini").join("oauth_creds.json");
        let content =
            serde_json::to_string_pretty(creds).map_err(|e| ProviderError::Parse(e.to_string()))?;
        std::fs::write(&creds_path, content)
            .map_err(|e| ProviderError::Other(format!("Failed to save credentials: {}", e)))?;
        Ok(())
    }

    fn extract_oauth_client_credentials(&self) -> Result<OAuthClientCredentials, ProviderError> {
        self.user_client_config_credentials()
            .or_else(|| self.gemini_binary_oauth_credentials())
            .or_else(Self::platform_oauth_credentials)
            .or_else(Self::fnm_oauth_credentials)
            .map(Ok)
            .unwrap_or_else(Self::oauth_credentials_from_env)
    }

    fn user_client_config_credentials(&self) -> Option<OAuthClientCredentials> {
        let cli_config = dirs::home_dir()?.join(".gemini").join("client_config.json");
        Self::try_read_client_config(&cli_config)
    }

    fn gemini_binary_oauth_credentials(&self) -> Option<OAuthClientCredentials> {
        let gemini_path = which::which("gemini").ok()?;
        let resolved = std::fs::canonicalize(&gemini_path).unwrap_or(gemini_path);
        let base_dir = resolved.parent()?;

        Self::oauth_credentials_from_candidates(Self::binary_oauth_candidates(base_dir))
            .or_else(|| Self::bundled_cli_oauth_credentials(base_dir))
    }

    /// Recent Gemini CLI releases ship as a single esbuild bundle
    /// (`node_modules/@google/gemini-cli/bundle/*.js`) without the separate
    /// `gemini-cli-core/dist` tree, so scan the bundle chunks for the OAuth
    /// client constants instead.
    fn bundled_cli_oauth_credentials(base_dir: &Path) -> Option<OAuthClientCredentials> {
        Self::bundle_dir_candidates(base_dir)
            .into_iter()
            .find_map(|bundle_dir| Self::oauth_credentials_from_bundle_dir(&bundle_dir))
    }

    /// Bundle directories that belong to the `gemini` binary in `base_dir`.
    fn bundle_dir_candidates(base_dir: &Path) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        // Unix installs symlink `bin/gemini` to `.../@google/gemini-cli/bundle/gemini.js`,
        // so the canonical binary already sits inside the bundle directory.
        if Self::is_gemini_cli_bundle_dir(base_dir) {
            dirs.push(base_dir.to_path_buf());
        }
        // Unix npm or Bun prefix: {bin}/../node_modules
        dirs.push(Self::gemini_cli_bundle_dir(
            &base_dir.join("..").join("node_modules"),
        ));
        // Windows npm: %APPDATA%\npm\gemini.cmd next to node_modules
        dirs.push(Self::gemini_cli_bundle_dir(&base_dir.join("node_modules")));
        // Unix npm prefix: {prefix}/bin -> {prefix}/lib/node_modules
        dirs.push(Self::gemini_cli_bundle_dir(
            &base_dir.join("..").join("lib").join("node_modules"),
        ));
        // Homebrew: {bin}/../libexec/lib/node_modules
        dirs.push(Self::gemini_cli_bundle_dir(
            &base_dir
                .join("..")
                .join("libexec")
                .join("lib")
                .join("node_modules"),
        ));
        dirs
    }

    fn gemini_cli_bundle_dir(node_modules: &Path) -> PathBuf {
        node_modules
            .join("@google")
            .join("gemini-cli")
            .join("bundle")
    }

    /// Only a `bundle` directory of the `@google/gemini-cli` package is
    /// scanned, never JavaScript next to an unrelated `gemini` binary.
    fn is_gemini_cli_bundle_dir(dir: &Path) -> bool {
        let names: Vec<_> = dir
            .components()
            .rev()
            .take(3)
            .map(|component| component.as_os_str().to_string_lossy().to_ascii_lowercase())
            .collect();
        names == ["bundle", "gemini-cli", "@google"]
    }

    fn oauth_credentials_from_bundle_dir(bundle_dir: &Path) -> Option<OAuthClientCredentials> {
        let entries = std::fs::read_dir(bundle_dir).ok()?;
        let mut chunks: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "js"))
            .collect();
        chunks.sort();
        Self::oauth_credentials_from_candidates(chunks)
    }

    /// Legacy `gemini-cli-core/dist` file or current bundle under one global
    /// `node_modules` directory.
    fn node_modules_oauth_credentials(node_modules: &Path) -> Option<OAuthClientCredentials> {
        Self::try_extract_oauth_from_js(&node_modules.join(Self::oauth_subpath())).or_else(|| {
            Self::oauth_credentials_from_bundle_dir(&Self::gemini_cli_bundle_dir(node_modules))
        })
    }

    fn oauth_credentials_from_candidates<I>(candidates: I) -> Option<OAuthClientCredentials>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        candidates
            .into_iter()
            .find_map(|candidate| Self::try_extract_oauth_from_js(&candidate))
    }

    fn binary_oauth_candidates(base_dir: &Path) -> Vec<PathBuf> {
        let oauth_subpath = Self::oauth_subpath();
        vec![
            // npm global: {bin}/../node_modules/@google/gemini-cli-core/...
            base_dir
                .join("..")
                .join("node_modules")
                .join(&oauth_subpath),
            // Homebrew: {bin}/../libexec/lib/node_modules/@google/gemini-cli/node_modules/...
            base_dir
                .join("..")
                .join("libexec")
                .join("lib")
                .join("node_modules")
                .join("@google")
                .join("gemini-cli")
                .join("node_modules")
                .join(&oauth_subpath),
            // Nix: {bin}/../share/gemini-cli/node_modules/...
            base_dir
                .join("..")
                .join("share")
                .join("gemini-cli")
                .join("node_modules")
                .join(&oauth_subpath),
            // Bun sibling
            base_dir
                .join("..")
                .join("gemini-cli-core")
                .join("dist")
                .join("src")
                .join("code_assist")
                .join("oauth2.js"),
        ]
    }

    fn oauth_subpath() -> PathBuf {
        Path::new("@google")
            .join("gemini-cli-core")
            .join("dist")
            .join("src")
            .join("code_assist")
            .join("oauth2.js")
    }

    #[cfg(windows)]
    fn platform_oauth_credentials() -> Option<OAuthClientCredentials> {
        // %APPDATA%\npm\node_modules, used when `gemini` is not on PATH.
        let appdata = dirs::data_dir()?;
        Self::node_modules_oauth_credentials(&appdata.join("npm").join("node_modules"))
    }

    #[cfg(not(windows))]
    fn platform_oauth_credentials() -> Option<OAuthClientCredentials> {
        None
    }

    #[cfg(windows)]
    fn fnm_oauth_credentials() -> Option<OAuthClientCredentials> {
        #[cfg(windows)]
        if let Some(local_appdata) = dirs::data_local_dir() {
            let fnm_versions = local_appdata.join("fnm").join("node-versions");
            return Self::fnm_oauth_credentials_from(&fnm_versions);
        }

        None
    }

    #[cfg(not(windows))]
    fn fnm_oauth_credentials() -> Option<OAuthClientCredentials> {
        #[cfg(not(windows))]
        if let Some(data_dir) = dirs::data_dir() {
            let fnm_versions = data_dir.join("fnm").join("node-versions");
            return Self::fnm_oauth_credentials_from(&fnm_versions);
        }

        None
    }

    fn fnm_oauth_credentials_from(fnm_versions: &Path) -> Option<OAuthClientCredentials> {
        if !fnm_versions.is_dir() {
            return None;
        }

        let entries = std::fs::read_dir(fnm_versions).ok()?;
        let mut installations: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path().join("installation"))
            .collect();
        installations.sort();

        // Global packages live in `lib/node_modules` on Unix and directly in
        // `node_modules` on Windows.
        installations.iter().find_map(|installation| {
            Self::node_modules_oauth_credentials(&installation.join("lib").join("node_modules"))
                .or_else(|| {
                    Self::node_modules_oauth_credentials(&installation.join("node_modules"))
                })
        })
    }

    fn oauth_credentials_from_env() -> Result<OAuthClientCredentials, ProviderError> {
        let client_id = std::env::var("GEMINI_CLIENT_ID")
            .map_err(|_| ProviderError::NotInstalled("GEMINI_CLIENT_ID not set. Install Gemini CLI or set GEMINI_CLIENT_ID/GEMINI_CLIENT_SECRET.".to_string()))?;
        let client_secret = std::env::var("GEMINI_CLIENT_SECRET")
            .map_err(|_| ProviderError::NotInstalled("GEMINI_CLIENT_SECRET not set".to_string()))?;

        Ok(OAuthClientCredentials {
            client_id,
            client_secret,
        })
    }

    fn try_read_client_config(path: &std::path::Path) -> Option<OAuthClientCredentials> {
        let content = std::fs::read_to_string(path).ok()?;
        let config: serde_json::Value = serde_json::from_str(&content).ok()?;
        let id = config.get("client_id")?.as_str()?;
        let secret = config.get("client_secret")?.as_str()?;
        Some(OAuthClientCredentials {
            client_id: id.to_string(),
            client_secret: secret.to_string(),
        })
    }

    fn try_extract_oauth_from_js(path: &std::path::Path) -> Option<OAuthClientCredentials> {
        let content = std::fs::read_to_string(path).ok()?;
        let id_re = regex_lite::Regex::new(r#"OAUTH_CLIENT_ID\s*=\s*['"](.*?)['"]"#).ok()?;
        let secret_re =
            regex_lite::Regex::new(r#"OAUTH_CLIENT_SECRET\s*=\s*['"](.*?)['"]"#).ok()?;
        let id = id_re.captures(&content)?.get(1)?.as_str().to_string();
        let secret = secret_re.captures(&content)?.get(1)?.as_str().to_string();
        if id.is_empty() || secret.is_empty() {
            return None;
        }
        Some(OAuthClientCredentials {
            client_id: id,
            client_secret: secret,
        })
    }

    fn parse_quota_response(
        &self,
        response: QuotaResponse,
        creds: Option<&OAuthCredentials>,
    ) -> Result<(RateWindow, Option<RateWindow>, Option<String>), ProviderError> {
        let buckets = response
            .buckets
            .ok_or_else(|| ProviderError::Parse("No quota buckets in response".to_string()))?;

        if buckets.is_empty() {
            return Err(ProviderError::Parse("Empty quota buckets".to_string()));
        }

        // Group quotas by model, keeping lowest per model
        let mut model_quotas: std::collections::HashMap<String, (f64, Option<String>)> =
            std::collections::HashMap::new();

        for bucket in buckets {
            if let (Some(model_id), Some(fraction)) = (bucket.model_id, bucket.remaining_fraction) {
                let entry = model_quotas.entry(model_id).or_insert((1.0, None));
                if fraction < entry.0 {
                    *entry = (fraction, bucket.reset_time);
                }
            }
        }

        // Find Flash and Pro quotas
        let flash_quota = model_quotas
            .iter()
            .filter(|(k, _)| k.to_lowercase().contains("flash"))
            .min_by(|a, b| {
                a.1.0
                    .partial_cmp(&b.1.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        let pro_quota = model_quotas
            .iter()
            .filter(|(k, _)| k.to_lowercase().contains("pro"))
            .min_by(|a, b| {
                a.1.0
                    .partial_cmp(&b.1.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        // Build primary RateWindow from the most constrained quota
        let (primary_fraction, primary_reset) = if let Some((_, (frac, reset))) = pro_quota {
            (*frac, reset.clone())
        } else if let Some((_, (frac, reset))) = flash_quota {
            (*frac, reset.clone())
        } else if let Some((_, (frac, reset))) = model_quotas.iter().next() {
            (*frac, reset.clone())
        } else {
            (1.0, None)
        };

        let primary_percent_used = (1.0 - primary_fraction) * 100.0;
        let primary_reset_at = primary_reset.as_ref().and_then(|s| parse_iso_date(s));

        let primary = RateWindow::with_details(
            primary_percent_used,
            Some(1440), // 24 hours
            primary_reset_at,
            None,
        );

        // Model-specific window for Flash if Pro is primary
        let model_specific = if pro_quota.is_some() {
            flash_quota.map(|(_, (frac, reset))| {
                let percent_used = (1.0 - frac) * 100.0;
                let reset_at = reset.as_ref().and_then(|s| parse_iso_date(s));
                RateWindow::with_details(percent_used, Some(1440), reset_at, None)
            })
        } else {
            None
        };

        // Extract email from ID token
        let email = creds
            .and_then(|c| c.id_token.as_ref())
            .and_then(|token| extract_email_from_jwt(token));

        Ok((primary, model_specific, email))
    }
}

impl Default for GeminiApi {
    fn default() -> Self {
        Self::new()
    }
}

// --- Data structures ---

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OAuthCredentials {
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
    expiry_date: Option<f64>, // milliseconds since epoch
}

impl OAuthCredentials {
    fn is_expired(&self) -> bool {
        if let Some(expiry_ms) = self.expiry_date {
            let expiry_secs = expiry_ms / 1000.0;
            let now_secs = chrono::Utc::now().timestamp() as f64;
            now_secs > expiry_secs
        } else {
            false
        }
    }
}

#[derive(Debug)]
struct OAuthClientCredentials {
    client_id: String,
    client_secret: String,
}

#[derive(Debug, Deserialize)]
struct TokenRefreshResponse {
    access_token: String,
    id_token: Option<String>,
    expires_in: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct QuotaResponse {
    buckets: Option<Vec<QuotaBucket>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaBucket {
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
    model_id: Option<String>,
    token_type: Option<String>,
}

#[derive(Default)]
struct CodeAssistStatus {
    tier: Option<GeminiUserTier>,
    paid_tier_name: Option<String>,
    consumer_client_unsupported: bool,
}

#[derive(Clone, Copy)]
enum GeminiUserTier {
    Free,
    Legacy,
    Standard,
}

fn parse_code_assist_status(body: &str) -> CodeAssistStatus {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return CodeAssistStatus::default();
    };

    let tier = json
        .get("currentTier")
        .and_then(|tier| tier.get("id"))
        .and_then(serde_json::Value::as_str)
        .and_then(|tier| match tier {
            "free-tier" => Some(GeminiUserTier::Free),
            "legacy-tier" => Some(GeminiUserTier::Legacy),
            "standard-tier" => Some(GeminiUserTier::Standard),
            _ => None,
        });
    let paid_tier_name = json
        .get("paidTier")
        .and_then(|tier| tier.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .map(str::to_owned)
        .filter(|name| !name.is_empty());

    let consumer_client_unsupported = json
        .get("ineligibleTiers")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tiers| {
            tiers.iter().any(|entry| {
                let tier_id = entry
                    .get("tier")
                    .or_else(|| entry.get("id"))
                    .and_then(|tier| tier.get("id").or(Some(tier)))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let reason = entry
                    .get("reason")
                    .or_else(|| entry.get("ineligibilityReason"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                tier_id.eq_ignore_ascii_case("free-tier")
                    && reason.eq_ignore_ascii_case("UNSUPPORTED_CLIENT")
            })
        });

    CodeAssistStatus {
        tier,
        paid_tier_name,
        consumer_client_unsupported,
    }
}

fn is_consumer_client_unsupported(status: &CodeAssistStatus, hosted_domain: Option<&str>) -> bool {
    status.consumer_client_unsupported
        && status.paid_tier_name.is_none()
        && hosted_domain.is_none()
        && !matches!(status.tier, Some(GeminiUserTier::Standard))
}

fn resolve_account_plan(status: &CodeAssistStatus, hosted_domain: Option<&str>) -> Option<String> {
    if let Some(plan) = &status.paid_tier_name {
        return Some(plan.clone());
    }

    match status.tier {
        Some(GeminiUserTier::Standard) => Some("Paid".to_string()),
        Some(GeminiUserTier::Free) if hosted_domain.is_some() => Some("Workspace".to_string()),
        Some(GeminiUserTier::Free) => Some("Free".to_string()),
        Some(GeminiUserTier::Legacy) => Some("Legacy".to_string()),
        None => None,
    }
}

// --- Helper functions ---

fn parse_iso_date(s: &str) -> Option<DateTime<Utc>> {
    // Try with fractional seconds first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try without fractional seconds
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(dt.with_timezone(&Utc));
    }

    None
}

fn extract_email_from_jwt(token: &str) -> Option<String> {
    jwt_payload(token)?
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn extract_hosted_domain_from_jwt(token: &str) -> Option<String> {
    jwt_payload(token)?
        .get("hd")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn jwt_payload(token: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }

    // Decode base64url payload
    let mut payload = parts[1].replace('-', "+").replace('_', "/");

    // Add padding if needed
    let remainder = payload.len() % 4;
    if remainder > 0 {
        payload.push_str(&"=".repeat(4 - remainder));
    }

    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &payload).ok()?;

    serde_json::from_slice(&decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(model: Option<&str>, fraction: Option<f64>, reset: Option<&str>) -> QuotaBucket {
        QuotaBucket {
            remaining_fraction: fraction,
            reset_time: reset.map(str::to_string),
            model_id: model.map(str::to_string),
            token_type: None,
        }
    }

    fn parse_buckets(
        buckets: Vec<QuotaBucket>,
        creds: Option<&OAuthCredentials>,
    ) -> Result<(RateWindow, Option<RateWindow>, Option<String>), ProviderError> {
        GeminiApi::new().parse_quota_response(
            QuotaResponse {
                buckets: Some(buckets),
            },
            creds,
        )
    }

    fn at(rfc3339: &str) -> Option<DateTime<Utc>> {
        Some(
            DateTime::parse_from_rfc3339(rfc3339)
                .unwrap()
                .with_timezone(&Utc),
        )
    }

    #[test]
    fn quota_pro_is_primary_and_flash_is_model_specific() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"email":"user@example.com"}"#);
        let creds = OAuthCredentials {
            access_token: None,
            id_token: Some(format!("header.{payload}.sig")),
            refresh_token: None,
            expiry_date: None,
        };
        let (primary, model_specific, email) = parse_buckets(
            vec![
                bucket(
                    Some("gemini-2.5-pro"),
                    Some(0.6),
                    Some("2026-01-14T00:00:00Z"),
                ),
                bucket(
                    Some("gemini-2.5-pro"),
                    Some(0.4),
                    Some("2026-01-15T00:00:00Z"),
                ),
                bucket(
                    Some("gemini-2.5-flash"),
                    Some(0.9),
                    Some("2026-01-16T00:00:00.5Z"),
                ),
                bucket(Some("gemini-2.0-flash"), Some(0.95), None),
                bucket(None, Some(0.0), None),
                bucket(Some("gemini-2.5-pro"), None, None),
            ],
            Some(&creds),
        )
        .unwrap();
        assert_eq!(primary.used_percent, (1.0 - 0.4) * 100.0);
        assert_eq!(primary.window_minutes, Some(1440));
        assert_eq!(primary.resets_at, at("2026-01-15T00:00:00Z"));
        assert_eq!(primary.reset_description, None);
        let flash = model_specific.expect("flash window when pro is primary");
        assert_eq!(flash.used_percent, (1.0 - 0.9) * 100.0);
        assert_eq!(flash.window_minutes, Some(1440));
        assert_eq!(flash.resets_at, at("2026-01-16T00:00:00.5Z"));
        assert_eq!(email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn quota_falls_back_to_flash_then_any_model() {
        let (primary, model_specific, email) = parse_buckets(
            vec![
                bucket(Some("gemini-2.5-flash"), Some(0.3), Some("not a date")),
                bucket(Some("other-model"), Some(0.1), None),
            ],
            None,
        )
        .unwrap();
        assert_eq!(primary.used_percent, (1.0 - 0.3) * 100.0);
        assert_eq!(primary.resets_at, None);
        assert!(model_specific.is_none());
        assert_eq!(email, None);

        let (primary, model_specific, _) = parse_buckets(
            vec![bucket(
                Some("other-model"),
                Some(0.25),
                Some("2026-01-15T00:00:00+02:00"),
            )],
            None,
        )
        .unwrap();
        assert_eq!(primary.used_percent, (1.0 - 0.25) * 100.0);
        assert_eq!(primary.resets_at, at("2026-01-14T22:00:00Z"));
        assert!(model_specific.is_none());
    }

    #[test]
    fn quota_without_usable_model_buckets_reports_an_unused_window() {
        // A fraction of 1.0 never beats the per-model starting value, so its
        // reset time is dropped.
        let (primary, model_specific, _) = parse_buckets(
            vec![
                bucket(
                    Some("gemini-2.5-pro"),
                    Some(1.0),
                    Some("2026-01-15T00:00:00Z"),
                ),
                bucket(None, Some(0.2), Some("2026-01-15T00:00:00Z")),
            ],
            None,
        )
        .unwrap();
        assert_eq!(primary.used_percent, 0.0);
        assert_eq!(primary.resets_at, None);
        assert!(model_specific.is_none());

        let (primary, model_specific, _) =
            parse_buckets(vec![bucket(None, Some(0.2), None)], None).unwrap();
        assert_eq!(primary.used_percent, 0.0);
        assert_eq!(primary.window_minutes, Some(1440));
        assert_eq!(primary.resets_at, None);
        assert!(model_specific.is_none());
    }

    #[test]
    fn quota_without_buckets_is_a_parse_error() {
        let empty = parse_buckets(Vec::new(), None).unwrap_err();
        assert!(matches!(empty, ProviderError::Parse(msg) if msg == "Empty quota buckets"));
        let missing = GeminiApi::new()
            .parse_quota_response(QuotaResponse { buckets: None }, None)
            .unwrap_err();
        assert!(
            matches!(missing, ProviderError::Parse(msg) if msg == "No quota buckets in response")
        );
    }

    #[test]
    fn bundled_cli_layout_yields_oauth_client_credentials() {
        // npm global layout on Windows: %APPDATA%\npm\gemini.cmd next to
        // node_modules\@google\gemini-cli\bundle\chunk-*.js (no gemini-cli-core/dist).
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path();
        let bundle = bin_dir
            .join("node_modules")
            .join("@google")
            .join("gemini-cli")
            .join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("chunk-AAA.js"), "var x = 1;").unwrap();
        std::fs::write(
            bundle.join("chunk-BBB.js"),
            r#"var OAUTH_CLIENT_ID = "id-123.apps.googleusercontent.com"; var OAUTH_CLIENT_SECRET = 'secret-xyz';"#,
        )
        .unwrap();

        assert!(
            GeminiApi::oauth_credentials_from_candidates(GeminiApi::binary_oauth_candidates(
                bin_dir
            ))
            .is_none(),
            "legacy dist layout must not match"
        );
        let creds = GeminiApi::bundled_cli_oauth_credentials(bin_dir)
            .expect("bundle chunks should be scanned");
        assert_eq!(creds.client_id, "id-123.apps.googleusercontent.com");
        assert_eq!(creds.client_secret, "secret-xyz");
    }

    const BUNDLE_CHUNK_WITH_CONSTANTS: &str = r#"var OAUTH_CLIENT_ID = "id-456.apps.googleusercontent.com"; var OAUTH_CLIENT_SECRET = "secret-abc";"#;

    fn write_gemini_bundle(node_modules: &Path) -> PathBuf {
        let bundle = node_modules
            .join("@google")
            .join("gemini-cli")
            .join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("gemini.js"), "import './chunk-A.js';").unwrap();
        std::fs::write(bundle.join("chunk-A.js"), BUNDLE_CHUNK_WITH_CONSTANTS).unwrap();
        bundle
    }

    #[test]
    fn symlinked_binary_inside_bundle_yields_oauth_client_credentials() {
        // Unix npm/Homebrew: bin/gemini canonicalizes to .../gemini-cli/bundle/gemini.js.
        let dir = tempfile::tempdir().unwrap();
        let bundle = write_gemini_bundle(&dir.path().join("lib").join("node_modules"));

        let creds = GeminiApi::bundled_cli_oauth_credentials(&bundle)
            .expect("the bundle that holds the binary should be scanned");
        assert_eq!(creds.client_id, "id-456.apps.googleusercontent.com");
        assert_eq!(creds.client_secret, "secret-abc");
    }

    #[test]
    fn unrelated_bundle_directory_is_not_scanned() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("other-tool").join("bundle");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("chunk.js"), BUNDLE_CHUNK_WITH_CONSTANTS).unwrap();

        assert!(GeminiApi::bundled_cli_oauth_credentials(&other).is_none());
    }

    #[test]
    fn npm_global_node_modules_bundle_yields_oauth_client_credentials() {
        // %APPDATA%\npm\node_modules fallback when `gemini` is not on PATH.
        let dir = tempfile::tempdir().unwrap();
        let node_modules = dir.path().join("npm").join("node_modules");
        write_gemini_bundle(&node_modules);

        let creds = GeminiApi::node_modules_oauth_credentials(&node_modules)
            .expect("bundle under the npm global node_modules should be scanned");
        assert_eq!(creds.client_secret, "secret-abc");
    }

    #[test]
    fn fnm_windows_and_unix_layouts_yield_bundle_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let versions = dir.path().join("node-versions");
        // Windows fnm keeps global packages directly under installation\node_modules.
        write_gemini_bundle(
            &versions
                .join("v22.0.0")
                .join("installation")
                .join("node_modules"),
        );
        let creds = GeminiApi::fnm_oauth_credentials_from(&versions)
            .expect("Windows fnm layout should be scanned");
        assert_eq!(creds.client_id, "id-456.apps.googleusercontent.com");

        let unix_dir = tempfile::tempdir().unwrap();
        let unix_versions = unix_dir.path().join("node-versions");
        write_gemini_bundle(
            &unix_versions
                .join("v22.0.0")
                .join("installation")
                .join("lib")
                .join("node_modules"),
        );
        assert!(GeminiApi::fnm_oauth_credentials_from(&unix_versions).is_some());
    }

    #[test]
    fn paid_tier_name_overrides_generic_tier_fallbacks() {
        let status = parse_code_assist_status(
            r#"{
                "currentTier": { "id": "free-tier" },
                "paidTier": { "name": "Gemini Code Assist in Google One AI Pro" }
            }"#,
        );

        assert_eq!(
            resolve_account_plan(&status, Some("example.com")),
            Some("Gemini Code Assist in Google One AI Pro".to_string())
        );

        let standard = parse_code_assist_status(
            r#"{
                "currentTier": { "id": "standard-tier" },
                "paidTier": { "name": "Plus" }
            }"#,
        );

        assert_eq!(
            resolve_account_plan(&standard, None),
            Some("Plus".to_string())
        );
    }

    #[test]
    fn consumer_shutdown_signal_excludes_paid_and_workspace_accounts() {
        let shutdown = parse_code_assist_status(
            r#"{
                "ineligibleTiers": [
                    {"tier":{"id":"free-tier"},"reason":"UNSUPPORTED_CLIENT"}
                ]
            }"#,
        );
        assert!(is_consumer_client_unsupported(&shutdown, None));
        assert!(!is_consumer_client_unsupported(
            &shutdown,
            Some("example.com")
        ));

        let paid = parse_code_assist_status(
            r#"{
                "paidTier":{"name":"Gemini Code Assist Standard"},
                "ineligibleTiers":[
                    {"tier":{"id":"free-tier"},"reason":"UNSUPPORTED_CLIENT"}
                ]
            }"#,
        );
        assert!(!is_consumer_client_unsupported(&paid, None));

        let standard = parse_code_assist_status(
            r#"{
                "currentTier":{"id":"standard-tier"},
                "ineligibleTiers":[
                    {"tier":{"id":"free-tier"},"reason":"UNSUPPORTED_CLIENT"}
                ]
            }"#,
        );
        assert!(!is_consumer_client_unsupported(&standard, None));
    }

    #[test]
    fn generic_tier_fallbacks_remain_when_paid_tier_is_absent() {
        let free_tier = parse_code_assist_status(r#"{"currentTier":{"id":"free-tier"}}"#);
        let paid = parse_code_assist_status(r#"{"currentTier":{"id":"standard-tier"}}"#);

        assert_eq!(
            resolve_account_plan(&free_tier, Some("example.com")),
            Some("Workspace".to_string())
        );
        assert_eq!(
            resolve_account_plan(&free_tier, None),
            Some("Free".to_string())
        );
        assert_eq!(resolve_account_plan(&paid, None), Some("Paid".to_string()));
    }

    #[test]
    fn invalid_code_assist_response_does_not_create_a_generic_plan() {
        let status = parse_code_assist_status("not json");

        assert_eq!(resolve_account_plan(&status, Some("example.com")), None);
    }

    #[test]
    fn malformed_paid_tier_preserves_current_tier_fallback() {
        let status =
            parse_code_assist_status(r#"{"currentTier":{"id":"free-tier"},"paidTier":[]}"#);

        assert_eq!(
            resolve_account_plan(&status, Some("example.com")),
            Some("Workspace".to_string())
        );
    }
}
