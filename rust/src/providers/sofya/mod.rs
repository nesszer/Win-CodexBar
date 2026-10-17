//! Sofya account credits provider.
//!
//! Fetches account credit balances (available, plan, and purchased) from
//! `GET https://sofya.co/v1/auth/me` with bearer token authentication.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/sofya.ts`).

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Map, Value};

use super::format;
use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::read_bounded_response;

const USAGE_URL: &str = "https://sofya.co/v1/auth/me";
const CREDENTIAL_TARGET: &str = "codexbar-sofya";
pub const API_KEY_ENV: &str = "SOFYA_API_KEY";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const SECTION_TITLE: &str = "Account credits";
const UNRECOGNIZED: &str = "Sofya returned an unrecognized account credit response.";

pub struct SofyaProvider {
    client: Option<Client>,
}

impl SofyaProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .ok(),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure Sofya HTTP client.".into())
        })?;
        let api_key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;

        let response = client
            .get(USAGE_URL)
            .bearer_auth(&api_key)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        let status = response.status().as_u16();
        if status != 200 {
            return Err(ProviderError::Other(status_message(status)));
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|_| unrecognized())?;
        let root: Value = serde_json::from_slice(&body).map_err(|_| unrecognized())?;
        parse_usage(&root)
    }
}

impl Default for SofyaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for SofyaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Sofya
    }

    fn metadata(&self) -> &ProviderMetadata {
        &SOFYA_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_impl(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            // Upstream reports HTTP 401 as `authenticationExpired`; 403 is `permissionDenied`.
            ProviderError::Other(message) if *message == status_message(401) => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

fn status_message(status: u16) -> String {
    format!("Sofya returned HTTP {status}.")
}

fn unrecognized() -> ProviderError {
    ProviderError::Other(UNRECOGNIZED.into())
}

/// Parse a credit value: absent or null is `None`; anything else must be a
/// non-negative finite number.
fn credit_value(data: &Map<String, Value>, key: &str) -> Result<Option<f64>, ProviderError> {
    match data.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) if f.is_finite() && f >= 0.0 => Ok(Some(f)),
            _ => Err(unrecognized()),
        },
        Some(_) => Err(unrecognized()),
    }
}

/// Format credit value with up to 6 decimal places, removing trailing zeros.
fn format_credits(value: f64) -> String {
    format::number(value, 6)
}

/// Validate an RFC 3339 reset timestamp and render it as upstream's
/// `toISOString()` does: UTC, millisecond precision, `.000` dropped.
fn format_reset(reset: &str) -> Result<String, ProviderError> {
    if !regex_iso8601_datetime(reset) {
        return Err(unrecognized());
    }
    let utc = chrono::DateTime::parse_from_rfc3339(reset)
        .map_err(|_| unrecognized())?
        .with_timezone(&chrono::Utc);
    // chrono encodes a :60 leap second as >= 1e9 nanos; JavaScript rejects it.
    if utc.timestamp_subsec_nanos() >= 1_000_000_000 {
        return Err(unrecognized());
    }
    let base = utc.format("%Y-%m-%d %H:%M:%S");
    Ok(match utc.timestamp_subsec_millis() {
        0 => format!("{base} UTC"),
        ms => format!("{base}.{ms:03} UTC"),
    })
}
/// Check if string matches ISO 8601 datetime pattern
fn regex_iso8601_datetime(s: &str) -> bool {
    // Pattern: YYYY-MM-DDTHH:MM:SS[.fff][Z|±HH:MM]
    if s.len() < 19 {
        return false;
    }
    let bytes = s.as_bytes();

    // YYYY-
    if !bytes[..4].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if bytes[4] != b'-' {
        return false;
    }

    // MM-
    if !bytes[5..7].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if bytes[7] != b'-' {
        return false;
    }

    // DD
    if !bytes[8..10].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }

    // T
    if bytes[10] != b'T' {
        return false;
    }

    // HH:MM:SS
    if !bytes[11..13].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if bytes[13] != b':' {
        return false;
    }
    if !bytes[14..16].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if bytes[16] != b':' {
        return false;
    }
    if !bytes[17..19].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }

    // Optional: .fff
    let mut pos = 19;
    if pos < bytes.len() && bytes[pos] == b'.' {
        pos += 1;
        // Consume fractional seconds
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
    }

    // Z or ±HH:MM - REQUIRED
    if pos >= bytes.len() {
        return false; // No timezone
    }

    if bytes[pos] == b'Z' {
        pos += 1;
    } else if bytes[pos] == b'+' || bytes[pos] == b'-' {
        pos += 1;
        if pos + 5 <= bytes.len() {
            // ±HH:MM
            if bytes[pos..pos + 2].iter().all(|b| b.is_ascii_digit())
                && bytes[pos + 2] == b':'
                && bytes[pos + 3..pos + 5].iter().all(|b| b.is_ascii_digit())
            {
                pos += 5;
            } else {
                return false;
            }
        } else {
            return false;
        }
    } else {
        return false;
    }

    pos == bytes.len()
}

fn parse_usage(root: &Value) -> Result<ProviderFetchResult, ProviderError> {
    let data = root.as_object().ok_or_else(unrecognized)?;

    // Extract credit values
    let credits = credit_value(data, "credits")?;
    let plan_credits = credit_value(data, "plan_credits")?;
    let purchased_credits = credit_value(data, "purchased_credits")?;

    // At least one balance must be present
    if credits.is_none() && plan_credits.is_none() && purchased_credits.is_none() {
        return Err(unrecognized());
    }

    let mut rows: Vec<(String, String)> = Vec::new();

    if let Some(val) = credits {
        rows.push(("Available credits".to_string(), format_credits(val)));
    }
    if let Some(val) = plan_credits {
        rows.push(("Plan credits".to_string(), format_credits(val)));
    }
    if let Some(val) = purchased_credits {
        rows.push(("Purchased credits".to_string(), format_credits(val)));
    }

    // Parse eligibility
    let is_free_tier = match data.get("is_free_tier") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(unrecognized()),
    };

    // Parse reset datetime
    let reset_str = match data.get("credits_reset_at") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => return Err(unrecognized()),
    };

    if let Some(reset_str) = reset_str {
        let reset_formatted = format_reset(reset_str)?;
        if is_free_tier == Some(true) {
            rows.push(("Monthly reset".to_string(), reset_formatted));
        }
    }

    let login_method = match is_free_tier {
        Some(true) => "Free tier",
        Some(false) => "Pay as you go",
        None => "API",
    };

    // Upstream's menu bar balance is the "Available credits" row.
    let primary_text = rows
        .iter()
        .find(|(label, _)| label == "Available credits")
        .map_or_else(
            || SECTION_TITLE.to_string(),
            |(label, value)| format!("{label}: {value}"),
        );
    let primary = RateWindow::informational(primary_text).hidden_on_card();
    let usage = UsageSnapshot::new(primary).with_login_method(login_method);

    let mut result = ProviderFetchResult::new(usage, "api");

    // Add rows as display details
    for (label, value) in rows {
        result = result.with_display_detail(
            ProviderDisplayDetail::new(
                format!("sofya-{}", label.to_lowercase().replace(' ', "-")),
                label,
                value,
            )
            .and_then(|d| d.with_section_title(SECTION_TITLE)),
        );
    }

    Ok(result)
}

const SOFYA_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Sofya,
    display_name: "Sofya",
    session_label: "Credits",
    weekly_label: "Credits",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://sofya.co/dashboard"),
    status_page_url: None,
    tertiary_label_key: None,
};

#[cfg(test)]
mod tests;
