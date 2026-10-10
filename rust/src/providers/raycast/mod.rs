//! Raycast AI credits provider.
//!
//! Raycast has no public credits API for a website session, so this provider
//! calls the unofficial `frontend_api/current_user/ai_credits` route with the
//! `__raycast_session` and `csrf_token` cookies only. Automatic import reads
//! Chrome alone to avoid unrelated browser prompts; a Manual Cookie header is
//! pinned and never falls back to a browser; Off makes no request. Cookie
//! material stays in memory for the current fetch and is never logged.

mod cookies;
mod parse;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::time::Duration;

use crate::browser::detection::BrowserType;
use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderError, ProviderFetchResult,
    ProviderId, ProviderStateKind, SourceMode,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const ORIGIN: &str = "https://www.raycast.com";
pub(crate) const SETTINGS_URL: &str = "https://www.raycast.com/settings";
const CREDITS_PATH: &str = "/frontend_api/current_user/ai_credits";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MIN_TIMEOUT_SECONDS: u64 = 1;
const MAX_TIMEOUT_SECONDS: u64 = 30;
const BROWSER: BrowserType = BrowserType::Chrome;

const MISSING_SESSION: &str = "No Raycast session cookies found. Sign in at www.raycast.com/settings or paste a Cookie header.";
const SESSION_EXPIRED: &str = "Raycast website session expired. Sign in at www.raycast.com/settings or paste a fresh Cookie header.";
const COOKIES_DISABLED: &str =
    "Raycast cookies are disabled. Set the cookie source to Auto or Manual to read credits.";

pub struct RaycastProvider {
    client: Client,
    origin: String,
}

impl RaycastProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .build()
                .unwrap_or_else(|_| Client::new()),
            origin: ORIGIN.to_string(),
        }
    }

    #[cfg(test)]
    fn with_origin(origin: &str) -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .no_proxy()
                .build()
                .expect("the test client should build"),
            origin: origin.to_string(),
        }
    }

    /// Try each cookie header in order. A candidate rejected with HTTP 401
    /// advances to the next one within the same refresh; any other outcome
    /// (success or a non-authentication failure) ends the search so a
    /// permission, rate-limit, or service error never rejects a session.
    async fn fetch_candidates(
        &self,
        headers: &[String],
        source_label: &str,
        timeout: Duration,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let mut rejected = false;
        for header in headers {
            let response = self
                .client
                .get(format!("{}{CREDITS_PATH}", self.origin))
                .timeout(timeout)
                .header("Cookie", header)
                .header("Accept", "application/json")
                .header("Origin", ORIGIN)
                .header("Referer", SETTINGS_URL)
                .header("User-Agent", USER_AGENT)
                .send()
                .await?;
            let status = response.status();
            if status == StatusCode::UNAUTHORIZED {
                rejected = true;
                continue;
            }
            check_status(status)?;
            let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
                .await
                .map_err(|error| match error {
                    BoundedBodyError::TooLarge => {
                        ProviderError::Parse("Raycast returned an oversized response.".into())
                    }
                    BoundedBodyError::Read(error) => ProviderError::Network(error),
                })?;
            let body = String::from_utf8(body).map_err(|_| {
                ProviderError::Parse("Raycast returned a response that was not valid UTF-8.".into())
            })?;
            return Ok(parse::parse_credits(&body)?.into_result(source_label));
        }
        Err(ProviderError::Other(
            if rejected {
                SESSION_EXPIRED
            } else {
                MISSING_SESSION
            }
            .to_string(),
        ))
    }

    async fn fetch_web(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let timeout = request_timeout(ctx.web_timeout);
        if let Some(raw) = ctx.manual_cookie_header.as_deref() {
            let headers: Vec<String> = cookies::manual_header(raw).into_iter().collect();
            return self.fetch_candidates(&headers, "manual", timeout).await;
        }
        // Manual is selected but nothing is stored: fail closed rather than
        // importing a browser account the user did not choose.
        if ctx.manual_cookie_missing {
            return Err(ProviderError::Other(MISSING_SESSION.to_string()));
        }
        let extracted = tokio::task::spawn_blocking(|| {
            crate::providers::browser_cookies_from_browser(BROWSER, cookies::EXTRACT_DOMAIN)
        })
        .await
        .map_err(|_| ProviderError::Other("Raycast cookie import was interrupted.".into()))?;
        let headers = match extracted {
            Ok(found) => cookies::browser_candidates(&found),
            Err(ProviderError::NoCookies) => Vec::new(),
            Err(error) => return Err(error),
        };
        self.fetch_candidates(&headers, BROWSER.display_name(), timeout)
            .await
    }
}

impl Default for RaycastProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn request_timeout(web_timeout_seconds: u64) -> Duration {
    Duration::from_secs(web_timeout_seconds.clamp(MIN_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS))
}

fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    let message = match status {
        StatusCode::OK => return Ok(()),
        StatusCode::FORBIDDEN => "Raycast denied access to AI credits for this account.".into(),
        StatusCode::TOO_MANY_REQUESTS => "Raycast credits requests are rate limited.".into(),
        status if status.is_server_error() => {
            format!("Raycast credits service is unavailable (HTTP {status}).")
        }
        status => format!("Raycast credits API returned HTTP {status}."),
    };
    Err(ProviderError::Other(message))
}

#[async_trait]
impl Provider for RaycastProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Raycast
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => self.fetch_web(ctx).await,
            // The shell maps the Off cookie source to Cli: no cookie access and
            // no request.
            SourceMode::Cli => Err(ProviderError::Other(COOKIES_DISABLED.to_string())),
            source => Err(ProviderError::UnsupportedSource(source)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn manual_cookie_precedes_token_account(&self) -> bool {
        true
    }

    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }

    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == MISSING_SESSION => {
                ProviderStateKind::NeedsAuthentication
            }
            ProviderError::Other(message) if message == SESSION_EXPIRED => {
                ProviderStateKind::ExpiredSession
            }
            _ => error.state_kind(),
        }
    }
}
