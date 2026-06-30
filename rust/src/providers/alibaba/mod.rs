//! Alibaba Cloud Model Studio – Coding Plan provider.
//!
//! Flow:
//! 1. Resolve cookies for the selected Alibaba region.
//! 2. Fetch a dashboard `SEC_TOKEN`, cached by region and cookie identity.
//! 3. POST to the console data gateway and parse the Coding Plan quota payload.

mod parser;
mod region;
mod sec_token;

use async_trait::async_trait;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    SourceMode, UsageSnapshot,
};
use crate::providers::browser_cookie_header;

use self::parser::parse_response;
pub use self::region::AlibabaRegion;
use self::sec_token::{
    cached_sec_token, extract_cookie_value, extract_sec_token, invalidate_sec_token,
    sec_token_cache_key, store_sec_token,
};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";

pub struct AlibabaProvider {
    metadata: ProviderMetadata,
}

impl AlibabaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Alibaba,
                display_name: "Alibaba",
                session_label: "5-Hour",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://modelstudio.console.alibabacloud.com"),
                status_page_url: None,
            },
        }
    }

    /// Resolve the [`AlibabaRegion`] from a settings value.
    pub fn region_from_settings(value: Option<&str>) -> AlibabaRegion {
        AlibabaRegion::from_settings_value(value)
    }

    /// Cookie domain for the browser import UI, driven by the selected region.
    pub fn cookie_domain_for_region(value: Option<&str>) -> &'static str {
        Self::region_from_settings(value).primary_cookie_domain()
    }

    /// Dashboard URL for the selected region.
    pub fn dashboard_url_for_region(value: Option<&str>) -> String {
        Self::region_from_settings(value).dashboard_url()
    }

    fn resolve_cookies(&self, ctx: &FetchContext) -> Result<String, ProviderError> {
        if let Some(ref manual) = ctx.manual_cookie_header {
            let trimmed = manual.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        let region = AlibabaRegion::from_settings_value(ctx.api_region.as_deref());
        browser_cookie_header(region.cookie_domains())
    }

    async fn resolve_sec_token(
        &self,
        client: &reqwest::Client,
        cookies: &str,
        region: AlibabaRegion,
        cache_key: &str,
        force_fresh: bool,
    ) -> Option<String> {
        let cached = if force_fresh {
            None
        } else {
            cached_sec_token(cache_key)
        };
        if let Some(token) = cached {
            return Some(token);
        }

        let token = self.fetch_sec_token(client, cookies, region).await?;
        store_sec_token(cache_key, &token);
        Some(token)
    }

    async fn fetch_sec_token(
        &self,
        client: &reqwest::Client,
        cookies: &str,
        region: AlibabaRegion,
    ) -> Option<String> {
        let dashboard_url = format!("{}?tab=plan", region.dashboard_url());
        let resp = client
            .get(&dashboard_url)
            .header("Cookie", cookies)
            .header("User-Agent", USER_AGENT)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            tracing::debug!(
                status = %resp.status(),
                region = ?region,
                "Alibaba dashboard fetch returned non-success; not falling back to cookie sec_token",
            );
            return None;
        }

        let html = resp.text().await.ok()?;
        extract_sec_token(&html).or_else(|| extract_cookie_value("sec_token", cookies))
    }

    async fn fetch_via_web(&self, ctx: &FetchContext) -> Result<UsageSnapshot, ProviderError> {
        let region = AlibabaRegion::from_settings_value(ctx.api_region.as_deref());
        let cookies = self.resolve_cookies(ctx)?;
        let cache_key = sec_token_cache_key(region.region_code(), &cookies);

        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(15)))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let mut force_fresh = false;
        let mut last_err = ProviderError::AuthRequired;
        for _attempt in 0..2 {
            let sec_token = self
                .resolve_sec_token(&client, &cookies, region, &cache_key, force_fresh)
                .await;
            match self
                .request_quota(&client, &cookies, region, sec_token)
                .await
            {
                Ok(usage) => return Ok(usage),
                Err(ProviderError::AuthRequired) => {
                    invalidate_sec_token(&cache_key);
                    force_fresh = true;
                    last_err = ProviderError::AuthRequired;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err)
    }

    async fn request_quota(
        &self,
        client: &reqwest::Client,
        cookies: &str,
        region: AlibabaRegion,
        sec_token: Option<String>,
    ) -> Result<UsageSnapshot, ProviderError> {
        let profile = region.request_profile();
        let cna = extract_cookie_value("cna", cookies).unwrap_or_default();
        let referer = format!("{}?tab=plan", region.dashboard_url());
        let fe_url = format!("{referer}#/efm/subscription/coding-plan");

        let params = serde_json::json!({
            "Api": profile.api_method,
            "V": "1.0",
            "Data": {
                "queryCodingPlanInstanceInfoRequest": {
                    "commodityCode": profile.commodity_code,
                    "onlyLatestOne": true
                },
                "cornerstoneParam": {
                    "feTraceId": uuid::Uuid::new_v4().to_string(),
                    "feURL": fe_url,
                    "protocol": "V2",
                    "console": "ONE_CONSOLE",
                    "productCode": "p_efm",
                    "switchAgent": profile.switch_agent,
                    "switchUserType": profile.switch_user_type,
                    "domain": profile.console_domain,
                    "consoleSite": profile.console_site,
                    "userNickName": "",
                    "userPrincipalName": "",
                    "xsp_lang": "en-US",
                    "X-Anonymous-Id": cna
                }
            }
        });

        let url = format!(
            "{}/data/api.json?action={}&product={}&_tag=",
            profile.gateway, profile.api_action, profile.api_product
        );

        let mut form = vec![
            ("action", profile.api_action.to_string()),
            ("product", profile.api_product.to_string()),
            ("api", profile.api_method.to_string()),
            ("_v", "undefined".to_string()),
            ("params", params.to_string()),
            ("region", region.region_code().to_string()),
        ];
        if let Some(token) = sec_token.filter(|t| !t.is_empty()) {
            form.push(("sec_token", token));
        }

        let resp = client
            .post(&url)
            .header("Cookie", cookies)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "*/*")
            .header("Origin", profile.gateway)
            .header("Referer", &referer)
            .header("User-Agent", USER_AGENT)
            .header("sec-fetch-site", "same-origin")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-dest", "empty")
            .form(&form)
            .send()
            .await?;

        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!("HTTP {status}")));
        }

        let body = resp.bytes().await?;
        let first_nonws = body.iter().find(|&&b| !b.is_ascii_whitespace()).copied();
        if first_nonws == Some(b'<') {
            return Err(ProviderError::AuthRequired);
        }

        let json: serde_json::Value =
            serde_json::from_slice(&body).map_err(|e| ProviderError::Parse(e.to_string()))?;
        parse_response(&json)
    }
}

impl Default for AlibabaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AlibabaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Alibaba
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!(region = ?ctx.api_region, "Fetching Alibaba Coding Plan usage");
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let usage = self.fetch_via_web(ctx).await?;
                Ok(ProviderFetchResult::new(usage, "web"))
            }
            SourceMode::Cli => Err(ProviderError::UnsupportedSource(SourceMode::Cli)),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
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
    use super::*;

    #[test]
    fn cookie_domain_for_region_mapping() {
        assert_eq!(
            AlibabaProvider::cookie_domain_for_region(None),
            "modelstudio.console.alibabacloud.com"
        );
        assert_eq!(
            AlibabaProvider::cookie_domain_for_region(Some("cn")),
            "bailian.console.alibabacloud.com"
        );
    }
}
