//! Linkup credit balance provider.
//!
//! Fetches account credit balance from `GET https://api.linkup.so/v1/credits/balance`
//! with the account API key as a bearer token.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/linkup.js`,
//! `Providers/Linkup/LinkupProviderDescriptor.swift`).

#[cfg(test)]
mod tests;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::format;
use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::read_bounded_response;

const BALANCE_URL: &str = "https://api.linkup.so/v1/credits/balance";
const CREDENTIAL_TARGET: &str = "codexbar-linkup";
pub const API_KEY_ENV: &str = "LINKUP_API_KEY";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const SECTION_TITLE: &str = "Account balance";
const ROW_LABEL: &str = "Credit balance";
const UNRECOGNIZED: &str = "Linkup returned an unrecognized credit balance.";

pub struct LinkupProvider {
    client: Option<Client>,
}

impl LinkupProvider {
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
            ProviderError::Other("Could not create a secure Linkup HTTP client.".into())
        })?;
        let api_key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;

        let response = client
            .get(BALANCE_URL)
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
        parse_balance(&root)
    }
}

impl Default for LinkupProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LinkupProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Linkup
    }

    fn metadata(&self) -> &ProviderMetadata {
        &LINKUP_METADATA
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
            // Upstream reports HTTP 401 as `authenticationExpired`.
            ProviderError::Other(message) if *message == status_message(401) => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

fn status_message(status: u16) -> String {
    format!("Linkup returned HTTP {status}.")
}

fn unrecognized() -> ProviderError {
    ProviderError::Other(UNRECOGNIZED.into())
}

/// Parse balance from the API response.
/// Upstream requires: data must be an object, balance must be a finite JSON number.
/// Zero and negative values are preserved.
pub fn parse_balance(root: &Value) -> Result<ProviderFetchResult, ProviderError> {
    // Must be an object, not an array or null
    let obj = root.as_object().ok_or_else(unrecognized)?;

    // Must have a 'balance' field that is a finite number
    let balance_val = obj.get("balance").ok_or_else(unrecognized)?;
    let balance = balance_val.as_f64().ok_or_else(unrecognized)?;

    if !balance.is_finite() {
        return Err(unrecognized());
    }

    let balance_text = format::usd(balance);
    let usage = UsageSnapshot::new(
        RateWindow::informational(format!("{ROW_LABEL}: {balance_text}")).hidden_on_card(),
    )
    .with_login_method("API");

    Ok(ProviderFetchResult::new(usage, "api").with_display_detail(
        ProviderDisplayDetail::new("linkup-balance", ROW_LABEL, balance_text)
            .and_then(|d| d.with_section_title(SECTION_TITLE)),
    ))
}

const LINKUP_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Linkup,
    display_name: "Linkup",
    session_label: "Balance",
    weekly_label: "Balance",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://app.linkup.so/organization/billing"),
    status_page_url: None,
    tertiary_label_key: None,
};
