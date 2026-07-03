//! Sakana AI provider implementation.
//!
//! Scrapes the billing page with browser/manual cookies, matching upstream
//! v0.38.0's UTC interpretation for reset dates shown in billing HTML.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regex_lite::Regex;
use reqwest::Client;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const BILLING_URL: &str = "https://console.sakana.ai/billing";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub struct SakanaProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl SakanaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Sakana,
                display_name: "Sakana AI",
                session_label: "5-hour",
                weekly_label: "Weekly",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(BILLING_URL),
                status_page_url: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_web(&self, cookie_header: &str) -> Result<ProviderFetchResult, ProviderError> {
        let cookie = normalize_cookie_header(cookie_header).ok_or(ProviderError::NoCookies)?;
        let response = self
            .client
            .get(BILLING_URL)
            .header("Cookie", cookie)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Sakana billing returned status {}",
                response.status()
            )));
        }
        let text = response.text().await?;
        if looks_signed_out(&text) {
            return Err(ProviderError::AuthRequired);
        }
        Ok(ProviderFetchResult::new(snapshot_from_html(&text)?, "web"))
    }
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

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let cookie = match ctx.manual_cookie_header.as_deref() {
                    Some(cookie) => cookie.to_string(),
                    None => crate::providers::browser_cookie_header(&["console.sakana.ai"])?,
                };
                self.fetch_web(&cookie).await
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sakana_windows_and_utc_reset() {
        let html = r#"
            <section>5-hour quota <span>42%</span> resets July 3, 2026 at 4:30 PM</section>
            <section>Weekly usage <span>80% used</span> resets July 8, 2026 at 12:00 AM</section>
        "#;
        let snapshot = snapshot_from_html(html).unwrap();
        assert_eq!(snapshot.primary.used_percent, 42.0);
        assert_eq!(snapshot.secondary.unwrap().used_percent, 80.0);
        assert_eq!(
            snapshot.primary.resets_at.unwrap().to_rfc3339(),
            "2026-07-03T16:30:00+00:00"
        );
    }

    #[test]
    fn normalizes_sakana_cookie_header() {
        assert_eq!(
            normalize_cookie_header("Cookie: session=a; empty=; other=b").as_deref(),
            Some("session=a; other=b")
        );
    }
}
