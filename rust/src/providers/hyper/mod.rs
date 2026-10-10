//! Charm Hyper Hypercredit balance provider.
//!
//! Port of upstream `hyper.ts` (v0.65.0). Unless browser cookies are off, a
//! hyper.charm.land session is tried first. Auto falls back to the API key
//! when no session cookie exists, the session request cannot connect, or the
//! session is rejected; Web fails closed instead. Every other response is
//! final: malformed balances, rate limits and server errors never fall back.

use async_trait::async_trait;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, COOKIE},
    redirect::Policy,
};
use std::time::Duration;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, format, read_bounded_response};

const CREDITS_URL: &str = "https://hyper.charm.land/v1/credits";
const COOKIE_DOMAIN: &str = "hyper.charm.land";
const CREDENTIAL_TARGET: &str = "codexbar-hyper";
const API_KEY_ENV: &[&str] = &["HYPER_API_KEY"];
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const API_TIMEOUT: Duration = Duration::from_secs(10);

const MISSING_CREDENTIAL: &str = "Sign in to hyper.charm.land or configure a Charm Hyper API key.";
const SESSION_REQUEST_FAILED: &str = "Charm Hyper session request failed.";
const SESSION_EXPIRED: &str =
    "Charm Hyper session expired. Sign in again or paste a fresh Cookie header.";
const API_KEY_REJECTED: &str = "Charm Hyper API key rejected (HTTP 401).";
const API_KEY_FORBIDDEN: &str = "Charm Hyper API key cannot access credits (HTTP 403).";
const RATE_LIMITED: &str = "Charm Hyper credits requests are rate limited.";
const INVALID_JSON: &str = "Charm Hyper credits response is not valid JSON.";
const INVALID_BALANCE: &str = "Charm Hyper balance must be a non-negative number.";

pub struct HyperProvider {
    client: Client,
    credits_url: String,
}

/// Which credential a `/v1/credits` request carries. Upstream never sends
/// the cookie and the bearer key together.
enum Credential<'a> {
    Session(&'a str),
    ApiKey(&'a str),
}

/// A `/v1/credits` reply. Upstream reads the body as part of the request, so
/// an unreadable success body counts as a failed request; other bodies are
/// never shown and are not read.
struct CreditsReply {
    status: StatusCode,
    is_html: bool,
    body: Vec<u8>,
}

impl CreditsReply {
    /// Upstream treats auth failures, redirects and an HTML sign-in page as an
    /// expired session.
    fn is_rejected_session(&self) -> bool {
        self.status == StatusCode::UNAUTHORIZED
            || self.status == StatusCode::FORBIDDEN
            || self.status.is_redirection()
            || (self.status == StatusCode::OK && self.is_html)
    }
}

impl HyperProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .redirect(Policy::none())
            .timeout(API_TIMEOUT)
            .build()
            .expect("Charm Hyper HTTP client configuration is valid");
        Self::with_client(CREDITS_URL, client)
    }

    fn with_client(credits_url: impl Into<String>, client: Client) -> Self {
        Self {
            client,
            credits_url: credits_url.into(),
        }
    }

    async fn request(&self, credential: Credential<'_>) -> Result<CreditsReply, ProviderError> {
        let request = self
            .client
            .get(&self.credits_url)
            .header(ACCEPT, "application/json");
        let request = match credential {
            Credential::Session(cookie) => request.header(COOKIE, cookie).timeout(SESSION_TIMEOUT),
            Credential::ApiKey(key) => request.bearer_auth(key).timeout(API_TIMEOUT),
        };
        let response = request.send().await?;
        let status = response.status();
        let is_html = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"));
        let body = if status == StatusCode::OK && !is_html {
            read_bounded_response(response, MAX_RESPONSE_BYTES)
                .await
                .map_err(|error| match error {
                    BoundedBodyError::Read(error) => ProviderError::Network(error),
                    BoundedBodyError::TooLarge => ProviderError::Parse(format!(
                        "Charm Hyper response exceeded {MAX_RESPONSE_BYTES} bytes."
                    )),
                })?
        } else {
            Vec::new()
        };
        Ok(CreditsReply {
            status,
            is_html,
            body,
        })
    }

    /// Runs upstream's session-then-key flow with an already resolved key.
    async fn fetch_with_key(
        &self,
        ctx: &FetchContext,
        key: Option<&str>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        if ctx.source_mode == SourceMode::Cli {
            return Err(ProviderError::UnsupportedSource(SourceMode::Cli));
        }
        // Web never falls back, so a configured key is irrelevant there.
        let fallback_key = match ctx.source_mode {
            SourceMode::Web => None,
            _ => key.and_then(cleaned_key),
        };

        let mut session_reply = None;
        // Upstream turns browser cookies off for the API source.
        if ctx.source_mode != SourceMode::OAuth {
            match session_cookie(ctx) {
                Some(cookie) => match self.request(Credential::Session(&cookie)).await {
                    Ok(reply) => session_reply = Some(reply),
                    Err(error) if fallback_key.is_none() => {
                        tracing::debug!(%error, "Charm Hyper session request failed");
                        return Err(ProviderError::Other(SESSION_REQUEST_FAILED.into()));
                    }
                    Err(error) => {
                        tracing::debug!(%error, "Charm Hyper session request failed; trying the API key");
                    }
                },
                None if fallback_key.is_none() => return Err(missing_credential()),
                None => {}
            }
            if session_reply
                .as_ref()
                .is_some_and(CreditsReply::is_rejected_session)
            {
                // Upstream also drops its cached browser cookie here. This
                // port keeps no cookie cache, so the next refresh reads the
                // browser again.
                if fallback_key.is_none() {
                    return Err(ProviderError::Other(SESSION_EXPIRED.into()));
                }
                tracing::debug!("Charm Hyper session was rejected; trying the API key");
                session_reply = None;
            }
        }

        let (reply, session) = match session_reply {
            Some(reply) => (reply, true),
            None => {
                let Some(key) = fallback_key else {
                    return Err(missing_credential());
                };
                (self.request(Credential::ApiKey(key)).await?, false)
            }
        };
        if reply.status != StatusCode::OK {
            return Err(status_error(reply.status));
        }
        let balance = parse_balance(&reply.body)?;
        Ok(balance_result(balance, session))
    }
}

impl Default for HyperProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for HyperProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Hyper
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        // This repository's SourceMode::OAuth is the documented API-key lane
        // for non-OAuth providers; upstream calls this source "api". Web never
        // uses the key, so it is not read there.
        let key = match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => crate::providers::resolve_api_key(
                ctx.api_key.as_deref(),
                CREDENTIAL_TARGET,
                API_KEY_ENV,
            )
            .ok(),
            SourceMode::Web | SourceMode::Cli => None,
        };
        self.fetch_with_key(ctx, key.as_deref()).await
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web, SourceMode::OAuth]
    }

    fn cookie_source_scopes_session_only(&self) -> bool {
        true
    }

    /// The API source must not read browser cookies (upstream turns them off
    /// there), so the provider imports the session only when it will use it.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            // Upstream classifies both as `authenticationExpired`.
            ProviderError::Other(message)
                if message == SESSION_EXPIRED || message == API_KEY_REJECTED =>
            {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}

/// The session cookie upstream's browser broker would hand out: a manual or
/// shell-resolved header first, nothing when the cookie source is off or an
/// empty manual source, otherwise the browser's hyper.charm.land session.
fn session_cookie(ctx: &FetchContext) -> Option<String> {
    if let Some(header) = ctx.manual_cookie_header.as_deref() {
        return crate::providers::normalize_cookie_header(header);
    }
    if ctx.manual_cookie_missing {
        return None;
    }
    match crate::providers::browser_cookie_header(&[COOKIE_DOMAIN]) {
        Ok(header) => crate::providers::normalize_cookie_header(&header),
        Err(error) => {
            tracing::debug!(%error, "Charm Hyper browser session is unavailable");
            None
        }
    }
}

/// Upstream's `SettingsValue.cleaned`: trimmed, one pair of wrapping quotes
/// removed, trimmed again, and blank keys treated as absent.
fn cleaned_key(raw: &str) -> Option<&str> {
    let value = raw.trim();
    let quoted = (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''));
    let value = if quoted {
        value.get(1..value.len() - 1).unwrap_or_default()
    } else {
        value
    };
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn status_error(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::Other(API_KEY_REJECTED.into()),
        StatusCode::FORBIDDEN => ProviderError::Other(API_KEY_FORBIDDEN.into()),
        StatusCode::TOO_MANY_REQUESTS => ProviderError::Other(RATE_LIMITED.into()),
        status => ProviderError::Other(format!("Charm Hyper API error: HTTP {}", status.as_u16())),
    }
}

/// Upstream parses any JSON document and requires a finite, non-negative
/// numeric `balance`. serde_json rejects out-of-range numbers such as
/// `1e400` while parsing, so those report invalid JSON; upstream reaches the
/// balance check with `Infinity`. Both are parse failures without fallback.
fn parse_balance(body: &[u8]) -> Result<f64, ProviderError> {
    let payload: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| ProviderError::Parse(INVALID_JSON.into()))?;
    payload
        .get("balance")
        .and_then(serde_json::Value::as_f64)
        .filter(|balance| balance.is_finite() && *balance >= 0.0)
        .ok_or_else(|| ProviderError::Parse(INVALID_BALANCE.into()))
}

fn balance_result(balance: f64, session: bool) -> ProviderFetchResult {
    let detail = ProviderDisplayDetail::new(
        "hypercredits",
        "Hypercredits",
        format!("{} HC", format::number(balance, 2)),
    );
    let (source, login_method) = if session {
        ("web", "Browser session")
    } else {
        ("api", "API key")
    };
    let usage = UsageSnapshot::new(RateWindow::informational("Hypercredit balance"))
        .with_login_method(login_method);
    ProviderFetchResult::new(usage, source).with_display_detail(detail)
}

fn missing_credential() -> ProviderError {
    ProviderError::NotInstalled(MISSING_CREDENTIAL.into())
}

#[cfg(test)]
mod tests;
