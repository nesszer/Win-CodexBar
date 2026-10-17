//! Aerostack monthly AI-token usage provider.
//!
//! Fetches account AI-token usage from `GET https://api.aerostack.dev/api/billing/usage`
//! with the account JWT as a bearer token.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/aerostack.js`).

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Map, Value};

use super::format;
use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::read_bounded_response;

const USAGE_URL: &str = "https://api.aerostack.dev/api/billing/usage";
const CREDENTIAL_TARGET: &str = "codexbar-aerostack";
pub const API_KEY_ENV: &str = "AEROSTACK_TOKEN";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const SECTION_TITLE: &str = "Monthly AI tokens";
const UNRECOGNIZED: &str = "Aerostack returned an unrecognized account AI-token usage response.";

pub struct AerostackProvider {
    client: Client,
}

impl AerostackProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let token = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;

        let response = self
            .client
            .get(USAGE_URL)
            .bearer_auth(&token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        let status = response.status().as_u16();
        match status {
            200 => {}
            401 => {
                return Err(ProviderError::Other(
                    "Aerostack account JWT expired or was rejected.".into(),
                ));
            }
            _ => {
                return Err(ProviderError::Other(format!(
                    "Aerostack returned HTTP {status}."
                )));
            }
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|_| unrecognized())?;
        let root: Value = serde_json::from_slice(&body).map_err(|_| unrecognized())?;
        parse_usage(&root)
    }
}

impl Default for AerostackProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AerostackProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Aerostack
    }

    fn metadata(&self) -> &ProviderMetadata {
        &AEROSTACK_METADATA
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
}

fn unrecognized() -> ProviderError {
    ProviderError::Other(UNRECOGNIZED.into())
}

/// Upstream `counter`: absent or null is `None`; anything else must be a
/// non-negative safe integer.
fn counter(tokens: &Map<String, Value>, key: &str) -> Result<Option<u64>, ProviderError> {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
    match tokens.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            if let Some(v) = n.as_u64() {
                return if v <= MAX_SAFE_INTEGER {
                    Ok(Some(v))
                } else {
                    Err(unrecognized())
                };
            }
            match n.as_f64() {
                // JSON like `5.0` is still a safe integer to JavaScript.
                Some(f) if f.fract() == 0.0 && (0.0..=MAX_SAFE_INTEGER as f64).contains(&f) => {
                    // `+ 0.0` turns -0 into 0; JavaScript accepts -0 as a safe integer.
                    format!("{:.0}", f + 0.0)
                        .parse()
                        .map(Some)
                        .map_err(|_| unrecognized())
                }
                _ => Err(unrecognized()),
            }
        }
        Some(_) => Err(unrecognized()),
    }
}

fn tokens(value: u64) -> String {
    format::number(value as f64, 0)
}

fn is_valid_period(period: &str) -> bool {
    let bytes = period.as_bytes();
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && matches!(
            (bytes[5], bytes[6]),
            (b'0', b'1'..=b'9') | (b'1', b'0'..=b'2')
        )
}

fn parse_usage(root: &Value) -> Result<ProviderFetchResult, ProviderError> {
    let root = root.as_object().ok_or_else(unrecognized)?;
    let tokens_obj = root
        .get("usage")
        .and_then(Value::as_object)
        .and_then(|usage| usage.get("ai_tokens"))
        .and_then(Value::as_object)
        .ok_or_else(unrecognized)?;

    let used = counter(tokens_obj, "used")?;
    let limit = counter(tokens_obj, "limit")?;
    if used.is_none() && limit.is_none() {
        return Err(unrecognized());
    }

    let mut rows: Vec<(&str, &str, String)> = Vec::new();
    if let Some(used) = used {
        rows.push(("tokens-used", "Tokens used", tokens(used)));
    }
    if let Some(limit) = limit {
        rows.push(("allowance", "Allowance", tokens(limit)));
    }
    if let (Some(used), Some(limit)) = (used, limit) {
        rows.push(("remaining", "Remaining", tokens(limit.saturating_sub(used))));
        if used > limit {
            rows.push(("above-allowance", "Above allowance", tokens(used - limit)));
        }
    }
    match root.get("period") {
        None | Some(Value::Null) => {}
        Some(Value::String(period)) if is_valid_period(period) => {
            rows.push(("period", "Period", period.clone()));
        }
        Some(_) => return Err(unrecognized()),
    }

    // JavaScript `tier.length` counts UTF-16 units of the untrimmed string.
    let login_method = match root.get("tier") {
        None | Some(Value::Null) => None,
        Some(Value::String(tier)) if tier.encode_utf16().count() <= 120 => {
            Some(tier.trim().to_string()).filter(|t| !t.is_empty())
        }
        Some(_) => return Err(unrecognized()),
    };

    // Upstream sends no primary window unless both counters exist and the
    // allowance is positive; the card then draws no bar.
    let primary = match (used, limit) {
        (Some(used), Some(limit)) if limit > 0 => {
            RateWindow::new((used as f64 / limit as f64 * 100.0).clamp(0.0, 100.0))
        }
        _ => RateWindow::informational(SECTION_TITLE).hidden_on_card(),
    };
    let mut usage = UsageSnapshot::new(primary);
    if let Some(login_method) = login_method {
        usage = usage.with_login_method(login_method);
    }

    let mut result = ProviderFetchResult::new(usage, "api");
    for (id, label, value) in rows {
        result = result.with_display_detail(
            ProviderDisplayDetail::new(format!("aerostack-{id}"), label, value)
                .and_then(|d| d.with_section_title(SECTION_TITLE)),
        );
    }
    Ok(result)
}

const AEROSTACK_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Aerostack,
    display_name: "Aerostack",
    session_label: "Monthly AI tokens",
    weekly_label: "Weekly",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://app.aerostack.dev/settings/billing"),
    status_page_url: None,
    tertiary_label_key: None,
};

#[cfg(test)]
mod tests;
