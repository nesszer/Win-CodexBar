//! Ollama provider implementation
//!
//! Fetches usage data by scraping the Ollama settings page
//! Uses session cookies from browser or manual input

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex_lite::Regex;
use reqwest::Url;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::settings::ApiKeys;

/// Ollama settings page URL
const OLLAMA_SETTINGS_URL: &str = "https://ollama.com/settings";
const OLLAMA_TAGS_URL: &str = "https://ollama.com/api/tags";
const OLLAMA_COOKIE_DOMAIN: &str = "ollama.com";
const OLLAMA_SESSION_COOKIE_NAME: &str = "__Secure-session";

/// Ollama provider
pub struct OllamaProvider {
    metadata: ProviderMetadata,
}

#[derive(Debug, Clone, PartialEq)]
struct UsageBlock {
    used_percent: f64,
    window_minutes: Option<u32>,
    resets_at: Option<DateTime<Utc>>,
    reset_description: Option<String>,
}

impl OllamaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Ollama,
                display_name: "Ollama",
                session_label: "Session",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://ollama.com/settings"),
                status_page_url: None,
            },
        }
    }

    /// Fetch usage by scraping ollama.com/settings
    async fn fetch_usage_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let cookie_header = self.resolve_cookie_header(ctx)?;

        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let mut current_url =
            Url::parse(OLLAMA_SETTINGS_URL).map_err(|e| ProviderError::Other(e.to_string()))?;
        let mut resp = None;

        for _ in 0..5 {
            let mut request = client
                .get(current_url.clone())
                .header(
                    "Accept",
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                )
                .header(
                    "User-Agent",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36",
                );

            if should_attach_ollama_cookie(&current_url) {
                request = request.header("Cookie", &cookie_header);
            }

            let response = request.send().await?;
            if response.status().is_redirection() {
                let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                    return Err(ProviderError::Other(
                        "Ollama redirect missing Location header".to_string(),
                    ));
                };
                let location = location
                    .to_str()
                    .map_err(|e| ProviderError::Other(e.to_string()))?;
                let next_url = current_url
                    .join(location)
                    .map_err(|e| ProviderError::Other(e.to_string()))?;
                if is_ollama_login_url(&next_url) {
                    return Err(ProviderError::AuthRequired);
                }
                if !should_attach_ollama_cookie(&next_url) {
                    return Err(ProviderError::AuthRequired);
                }
                current_url = next_url;
                continue;
            }
            resp = Some(response);
            break;
        }

        let Some(resp) = resp else {
            return Err(ProviderError::Other(
                "Ollama returned too many redirects".to_string(),
            ));
        };

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }

        // Check for redirect to login page
        if is_ollama_login_url(resp.url()) {
            return Err(ProviderError::AuthRequired);
        }

        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Ollama returned status {}",
                resp.status()
            )));
        }

        let html = resp
            .text()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        self.parse_usage_html(&html)
    }

    async fn fetch_usage_api(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let api_key = Self::resolve_api_key(ctx).ok_or(ProviderError::AuthRequired)?;
        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        let response = client
            .get(OLLAMA_TAGS_URL)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .header("User-Agent", "CodexBar/1.0")
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        match status {
            reqwest::StatusCode::OK => Self::parse_api_tags(&bytes),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                Err(ProviderError::AuthRequired)
            }
            _ => Err(ProviderError::Other(format!(
                "Ollama API returned status {status}"
            ))),
        }
    }

    fn resolve_api_key(ctx: &FetchContext) -> Option<String> {
        ctx.api_key
            .as_deref()
            .and_then(|key| clean_secret(Some(key)))
            .or_else(|| {
                ["OLLAMA_API_KEY", "OLLAMA_KEY"].iter().find_map(|name| {
                    std::env::var(name)
                        .ok()
                        .and_then(|value| clean_secret(Some(&value)))
                })
            })
            .or_else(|| {
                ApiKeys::load()
                    .get("ollama")
                    .and_then(|key| clean_secret(Some(key)))
            })
    }

    fn has_api_key(ctx: &FetchContext) -> bool {
        ctx.api_key
            .as_deref()
            .and_then(|key| clean_secret(Some(key)))
            .is_some()
            || ["OLLAMA_API_KEY", "OLLAMA_KEY"].iter().any(|name| {
                std::env::var(name)
                    .ok()
                    .and_then(|value| clean_secret(Some(&value)))
                    .is_some()
            })
            || ApiKeys::load()
                .get("ollama")
                .and_then(|key| clean_secret(Some(key)))
                .is_some()
    }

    fn parse_api_tags(bytes: &[u8]) -> Result<UsageSnapshot, ProviderError> {
        #[derive(Deserialize)]
        struct TagsResponse {
            models: Vec<serde_json::Value>,
        }

        let response: TagsResponse = serde_json::from_slice(bytes)
            .map_err(|e| ProviderError::Parse(format!("Could not parse Ollama API tags: {e}")))?;
        let mut primary = RateWindow::new(0.0);
        primary.reset_description =
            Some(format!("{} cloud models available", response.models.len()));
        Ok(UsageSnapshot::new(primary).with_login_method("API key"))
    }

    fn normalize_cookie_header(input: &str) -> Option<String> {
        let mut header = input.trim();
        if header.is_empty() {
            return None;
        }

        if header
            .get(.."cookie:".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cookie:"))
        {
            header = header["cookie:".len()..].trim();
        }

        if header.is_empty() {
            return None;
        }

        if header.contains('=') {
            Some(header.to_string())
        } else {
            Some(format!("{OLLAMA_SESSION_COOKIE_NAME}={header}"))
        }
    }

    /// Resolve cookie header from manual cookies, browser import, or context
    fn resolve_cookie_header(&self, ctx: &FetchContext) -> Result<String, ProviderError> {
        // Check manual cookie header first
        if let Some(ref cookie) = ctx.manual_cookie_header
            && let Some(header) = Self::normalize_cookie_header(cookie)
        {
            return Ok(header);
        }

        // Try browser cookie extraction
        match crate::providers::browser_cookie_header(&[OLLAMA_COOKIE_DOMAIN]) {
            Ok(header) if !header.is_empty() => {
                // Validate that we have a recognized session cookie
                const SESSION_COOKIE_NAMES: &[&str] = &[
                    "session",
                    "__Secure-session",
                    "ollama_session",
                    "__Host-ollama_session",
                    "__Secure-next-auth.session-token",
                    "next-auth.session-token",
                ];
                let has_session = SESSION_COOKIE_NAMES
                    .iter()
                    .any(|name| header.contains(name));
                if has_session {
                    Ok(header)
                } else {
                    Err(ProviderError::NoCookies)
                }
            }
            Ok(_) | Err(ProviderError::NoCookies) => Err(ProviderError::NoCookies),
            Err(err) => Err(err),
        }
    }

    /// Parse usage data from the Ollama settings HTML page
    fn parse_usage_html(&self, html: &str) -> Result<UsageSnapshot, ProviderError> {
        // Check if we're signed out
        if html.contains("Sign in")
            && !html.contains("Cloud Usage")
            && !html.contains("Session usage")
        {
            return Err(ProviderError::AuthRequired);
        }

        let session_block =
            self.parse_usage_block(&["Session usage", "Hourly usage"], html, Some(5 * 60));
        let weekly_block = self.parse_usage_block(&["Weekly usage"], html, Some(7 * 24 * 60));

        if session_block.is_none() && weekly_block.is_none() {
            return Err(ProviderError::Parse(
                "Could not find usage data on Ollama settings page".to_string(),
            ));
        }

        let primary = rate_window_from_usage_block(session_block.as_ref());
        let mut usage = UsageSnapshot::new(primary);

        // Parse plan name
        if let Some(plan) = self.parse_plan_name(html) {
            usage = usage.with_login_method(&plan);
        }

        // Parse account email
        if let Some(email) = self.parse_account_email(html) {
            usage = usage.with_login_method(&email);
        }

        if let Some(weekly) = weekly_block.as_ref() {
            usage = usage.with_secondary(rate_window_from_usage_block(Some(weekly)));
        }

        Ok(usage)
    }

    /// Parse a usage block by looking for a label then extracting the percentage
    fn parse_usage_block(
        &self,
        labels: &[&str],
        html: &str,
        window_minutes: Option<u32>,
    ) -> Option<UsageBlock> {
        for label in labels {
            if let Some(pos) = html.find(label) {
                let tail = &html[pos..];
                let end = usage_block_end(tail, label).unwrap_or_else(|| tail.len().min(4000));
                let window = &tail[..end.min(tail.len())];

                // Try "XX% used" pattern
                let used_re = Regex::new(r"(\d+(?:\.\d+)?)\s*%\s*used").ok()?;
                if let Some(caps) = used_re.captures(window)
                    && let Ok(val) = caps[1].parse::<f64>()
                {
                    return Some(UsageBlock {
                        used_percent: val,
                        window_minutes,
                        resets_at: parse_first_datetime(window),
                        reset_description: parse_reset_description(window),
                    });
                }

                // Try "width: XX%" pattern (progress bar CSS)
                let width_re = Regex::new(r"width:\s*(\d+(?:\.\d+)?)%").ok()?;
                if let Some(caps) = width_re.captures(window)
                    && let Ok(val) = caps[1].parse::<f64>()
                {
                    return Some(UsageBlock {
                        used_percent: val,
                        window_minutes,
                        resets_at: parse_first_datetime(window),
                        reset_description: parse_reset_description(window),
                    });
                }
            }
        }
        None
    }

    /// Parse plan name from "Cloud Usage" section
    fn parse_plan_name(&self, html: &str) -> Option<String> {
        let re = Regex::new(r#"Cloud Usage\s*</span>\s*<span[^>]*>([^<]+)</span>"#).ok()?;
        re.captures(html)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().trim().to_string())
    }

    /// Parse account email from the page
    fn parse_account_email(&self, html: &str) -> Option<String> {
        let re = Regex::new(r#"[\w.+-]+@[\w-]+\.[\w.-]+"#).ok()?;
        re.find(html).map(|m| m.as_str().to_string())
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Ollama
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching Ollama usage");

        match ctx.source_mode {
            SourceMode::Auto => {
                if Self::has_api_key(ctx)
                    && let Ok(usage) = self.fetch_usage_api(ctx).await
                {
                    return Ok(ProviderFetchResult::new(usage, "api"));
                }
                let usage = self.fetch_usage_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Web => {
                let usage = self.fetch_usage_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::OAuth | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
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

fn clean_secret(raw: Option<&str>) -> Option<String> {
    let mut value = raw?.trim().to_string();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value.remove(0);
        value.pop();
    }
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn usage_block_end(tail: &str, current_label: &str) -> Option<usize> {
    ["Session usage", "Hourly usage", "Weekly usage"]
        .iter()
        .filter(|label| **label != current_label)
        .filter_map(|label| tail.get(current_label.len()..)?.find(label))
        .map(|idx| idx + current_label.len())
        .min()
        .map(|idx| idx.min(4000))
}

fn rate_window_from_usage_block(block: Option<&UsageBlock>) -> RateWindow {
    block
        .map(|block| {
            RateWindow::with_details(
                block.used_percent,
                block.window_minutes,
                block.resets_at,
                block.reset_description.clone(),
            )
        })
        .unwrap_or_else(|| RateWindow::new(0.0))
}

fn parse_first_datetime(html: &str) -> Option<DateTime<Utc>> {
    let re =
        Regex::new(r#"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})"#).ok()?;
    let raw = re.find(html)?.as_str();
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn parse_reset_description(html: &str) -> Option<String> {
    let re = Regex::new(r"(?i)(resets?\s+in\s+[^<\n\r]+|reset\s+[^<\n\r]+)").ok()?;
    re.find(html)
        .map(|m| strip_html_entities(m.as_str()).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn strip_html_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&#x2F;", "/")
}

fn should_attach_ollama_cookie(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(OLLAMA_COOKIE_DOMAIN))
}

fn is_ollama_login_url(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    path.contains("/login") || path.contains("/signin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_raw_ollama_session_cookie_value() {
        assert_eq!(
            OllamaProvider::normalize_cookie_header("abc123"),
            Some("__Secure-session=abc123".to_string())
        );
    }

    #[test]
    fn preserves_full_cookie_header() {
        assert_eq!(
            OllamaProvider::normalize_cookie_header("__Secure-session=abc123; aid=device"),
            Some("__Secure-session=abc123; aid=device".to_string())
        );
    }

    #[test]
    fn strips_cookie_header_prefix() {
        assert_eq!(
            OllamaProvider::normalize_cookie_header("Cookie: __Secure-session=abc123"),
            Some("__Secure-session=abc123".to_string())
        );
    }

    #[test]
    fn ignores_empty_cookie_input() {
        assert_eq!(OllamaProvider::normalize_cookie_header("   "), None);
        assert_eq!(OllamaProvider::normalize_cookie_header("Cookie:   "), None);
    }

    #[test]
    fn only_attaches_web_cookie_to_https_ollama_urls() {
        assert!(should_attach_ollama_cookie(
            &Url::parse("https://ollama.com/settings").unwrap()
        ));
        assert!(!should_attach_ollama_cookie(
            &Url::parse("http://ollama.com/settings").unwrap()
        ));
        assert!(!should_attach_ollama_cookie(
            &Url::parse("https://example.com/settings").unwrap()
        ));
    }

    #[test]
    fn strips_wrapping_quotes_from_api_key() {
        assert_eq!(
            clean_secret(Some("  'ollama-key'  ")),
            Some("ollama-key".to_string())
        );
        assert_eq!(
            clean_secret(Some("  \"ollama-key\"  ")),
            Some("ollama-key".to_string())
        );
    }

    #[test]
    fn parses_api_tags_model_count() {
        let snapshot =
            OllamaProvider::parse_api_tags(br#"{"models":[{"name":"gpt-oss"},{"name":"qwen3"}]}"#)
                .unwrap();
        assert_eq!(snapshot.primary.used_percent, 0.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("2 cloud models available")
        );
        assert_eq!(snapshot.login_method.as_deref(), Some("API key"));
    }

    #[test]
    fn parses_ollama_usage_blocks_with_window_bounds() {
        let provider = OllamaProvider::new();
        let html = r#"
            <section>Session usage <div style="width: 42%"></div><span>resets in 2h</span></section>
            <section>Weekly usage <span>84% used</span><time>2026-06-01T00:00:00Z</time></section>
        "#;
        let session = provider
            .parse_usage_block(&["Session usage", "Hourly usage"], html, Some(300))
            .unwrap();
        let weekly = provider
            .parse_usage_block(&["Weekly usage"], html, Some(10080))
            .unwrap();
        assert_eq!(session.used_percent, 42.0);
        assert_eq!(session.window_minutes, Some(300));
        assert_eq!(session.reset_description.as_deref(), Some("resets in 2h"));
        assert_eq!(weekly.used_percent, 84.0);
        assert!(weekly.resets_at.is_some());
    }
}
