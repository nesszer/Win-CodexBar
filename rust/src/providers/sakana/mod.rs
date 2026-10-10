//! Sakana AI provider implementation.
//!
//! Scrapes the billing page with browser/manual cookies, matching upstream
//! v0.38.0's UTC interpretation for reset dates shown in billing HTML. A
//! best-effort second GET adds the pay-as-you-go credit balance (v0.67.0).

mod payg;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regex_lite::Regex;
use reqwest::{Client, RequestBuilder, Url};

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, RateWindow, SourceMode,
    UsageSnapshot,
};

pub(crate) const BILLING_URL: &str = "https://console.sakana.ai/billing";
const PAYG_QUERY: &str = "tab=payAsYouGo";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub struct SakanaProvider {
    client: Client,
    billing_url: Url,
}

impl SakanaProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
            billing_url: Url::parse(BILLING_URL).expect("billing URL is valid"),
        }
    }

    async fn fetch_web(
        &self,
        cookie_header: &str,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let cookie = normalize_cookie_header(cookie_header).ok_or(ProviderError::NoCookies)?;
        // The optional lookup runs alongside the required request. Dropping
        // the handle on any early return cancels it.
        let payg = ctx.include_credits.then(|| {
            let mut payg_url = self.billing_url.clone();
            payg_url.set_query(Some(PAYG_QUERY));
            payg::PaygLookup::spawn(
                self.client.clone(),
                payg_url,
                cookie.clone(),
                self.billing_url.clone(),
            )
        });
        let response = billing_request(&self.client, self.billing_url.clone(), &cookie)
            .send()
            .await?;
        let status = response.status();
        // Cross-origin redirects are stopped by the client policy and surface
        // as a redirect status; same-origin ones are followed, so the final
        // URL must also stay on the billing origin.
        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || status.is_redirection()
            || !crate::core::is_same_origin(response.url(), &self.billing_url)
        {
            return Err(ProviderError::AuthRequired);
        }
        if status != reqwest::StatusCode::OK {
            return Err(ProviderError::Other(format!(
                "Sakana billing returned status {status}"
            )));
        }
        let text = response.text().await?;
        if looks_signed_out(&text) {
            return Err(ProviderError::AuthRequired);
        }
        let mut result = ProviderFetchResult::new(snapshot_from_html(&text)?, "web");
        if let Some(payg) = payg
            && let Some(balance) = payg.join(ctx.requires_optional_usage_completeness).await
        {
            for row in balance.display_details() {
                result = result.with_display_detail(Some(row));
            }
        }
        Ok(result)
    }

    #[cfg(test)]
    fn with_billing_url(mut self, client: Client, billing_url: Url) -> Self {
        self.client = client;
        self.billing_url = billing_url;
        self
    }
}

fn billing_request(client: &Client, url: Url, cookie: &str) -> RequestBuilder {
    client
        .get(url)
        .header("Cookie", cookie)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("User-Agent", USER_AGENT)
}

impl Default for SakanaProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let mut header = raw.trim();
    if header
        .get(.."cookie:".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cookie:"))
    {
        header = header["cookie:".len()..].trim();
    }
    let pairs = header
        .split(';')
        .filter_map(|chunk| {
            let (name, value) = chunk.trim().split_once('=')?;
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty() && !value.is_empty()).then(|| format!("{name}={value}"))
        })
        .collect::<Vec<_>>();
    (!pairs.is_empty()).then(|| pairs.join("; "))
}

fn looks_signed_out(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("sign in") || lower.contains("log in") || lower.contains("/auth/")
}

fn snapshot_from_html(text: &str) -> Result<UsageSnapshot, ProviderError> {
    let primary = extract_window(text, &["5-hour", "5 hour", "five-hour", "session"])
        .ok_or_else(|| ProviderError::Parse("Missing Sakana 5-hour quota".into()))?;
    let mut snapshot = UsageSnapshot::new(primary).with_login_method("Sakana Console");
    if let Some(weekly) = extract_window(text, &["weekly", "week"]) {
        snapshot = snapshot.with_secondary(weekly);
    }
    Ok(snapshot)
}

fn extract_window(text: &str, labels: &[&str]) -> Option<RateWindow> {
    let lower = text.to_ascii_lowercase();
    let anchor = labels
        .iter()
        .find_map(|label| lower.find(label).map(|idx| (idx, *label)))?;
    let end = (anchor.0 + 1400).min(text.len());
    let segment = &text[anchor.0..end];
    let percent = extract_percent(segment)?;
    let reset = extract_reset(segment);
    let mut window = RateWindow::with_details(
        percent,
        if anchor.1.contains("week") {
            Some(7 * 24 * 60)
        } else {
            Some(5 * 60)
        },
        reset,
        None,
    );
    if let Some(reset_text) = extract_reset_text(segment) {
        window.reset_description = Some(reset_text);
    }
    Some(window)
}

fn extract_percent(segment: &str) -> Option<f64> {
    let patterns = [
        r#"(?i)([0-9]+(?:\.[0-9]+)?)\s*%\s*(?:used|usage)?"#,
        r#"(?i)(?:used|usage)[^0-9]{0,40}([0-9]+(?:\.[0-9]+)?)\s*%"#,
        r#"(?i)"(?:usedPercent|used_percent|percent)"\s*:\s*([0-9]+(?:\.[0-9]+)?)"#,
    ];
    patterns.iter().find_map(|pattern| {
        Regex::new(pattern)
            .ok()?
            .captures(segment)?
            .get(1)?
            .as_str()
            .parse::<f64>()
            .ok()
            .map(|value| if value <= 1.0 { value * 100.0 } else { value })
    })
}

fn extract_reset(segment: &str) -> Option<DateTime<Utc>> {
    extract_reset_text(segment).and_then(parse_reset_date_utc)
}

fn extract_reset_text(segment: &str) -> Option<String> {
    let patterns = [
        r#"(?i)([A-Z][a-z]+ \d{1,2}, \d{4} at \d{1,2}:\d{2} [AP]M)"#,
        r#"(?i)(?:reset|renews?)[^A-Z]{0,80}([A-Z][a-z]+ \d{1,2}, \d{4} at \d{1,2}:\d{2} [AP]M)"#,
    ];
    patterns.iter().find_map(|pattern| {
        Regex::new(pattern)
            .ok()?
            .captures(segment)?
            .get(1)
            .map(|m| m.as_str().to_string())
    })
}

fn parse_reset_date_utc(raw: String) -> Option<DateTime<Utc>> {
    let formats = ["%B %e, %Y at %I:%M %p", "%B %-d, %Y at %-I:%M %p"];
    formats.iter().find_map(|format| {
        NaiveDateTime::parse_from_str(&raw, format)
            .ok()
            .map(|dt| Utc.from_utc_datetime(&dt))
    })
}

#[async_trait]
impl Provider for SakanaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Sakana
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let cookie = match ctx.manual_cookie_header.as_deref() {
                    Some(cookie) => cookie.to_string(),
                    None => crate::providers::browser_cookie_header(&["console.sakana.ai"])?,
                };
                self.fetch_web(&cookie, ctx).await
            }
            SourceMode::OAuth | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }
}

#[cfg(test)]
mod tests;
