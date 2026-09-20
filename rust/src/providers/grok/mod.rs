//! Grok provider implementation.
//!
//! Uses the grok.com billing gRPC-web endpoint via either browser cookies or
//! `~/.grok/auth.json` produced by `grok login`.

pub mod accounts;
mod billing;
pub mod local_sessions;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderInventoryItem,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

use self::accounts::{GrokAuthKind, ParsedGrokAuthFile};
use self::billing::GrokBillingSnapshot;

const BILLING_ENDPOINT: &str = "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
const REMAINING_RESETS_ENDPOINT: &str =
    "https://grok.com/prod_mc_billing.ConsumerUiSvc/GetRemainingResets";
const CLI_SETTINGS_ENDPOINT: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const RESET_CREDITS_TIMEOUT: Duration = Duration::from_secs(2);
const RESET_CREDITS_JOIN_GRACE: Duration = Duration::from_millis(250);

pub struct GrokProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl GrokProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Grok,
                display_name: "Grok",
                session_label: "Credits",
                weekly_label: "On-demand",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://grok.com/?_s=usage"),
                status_page_url: Some("https://status.x.ai"),
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn auth_file_path() -> Option<PathBuf> {
        if let Ok(home) = std::env::var("GROK_HOME")
            && !home.trim().is_empty()
        {
            return Some(PathBuf::from(home).join("auth.json"));
        }
        dirs::home_dir().map(|home| home.join(".grok").join("auth.json"))
    }

    #[cfg(test)]
    fn client_for_tests(&self) -> Client {
        self.client.clone()
    }

    fn load_credentials(kind: GrokAuthKind) -> Result<GrokCredentials, ProviderError> {
        let path = Self::auth_file_path()
            .ok_or_else(|| ProviderError::NotInstalled("Grok auth path not found".to_string()))?;
        let text = std::fs::read_to_string(&path).map_err(|_| {
            ProviderError::NotInstalled("Grok auth.json not found. Run `grok login`.".to_string())
        })?;
        GrokCredentials::parse_for_kind(&text, kind)
    }

    pub async fn fetch_usage_from_auth_json(
        &self,
        text: &str,
    ) -> Result<crate::providers::grok::accounts::GrokAccountUsage, ProviderError> {
        let (credentials, kind) =
            if let Ok(credentials) = GrokCredentials::parse_for_kind(text, GrokAuthKind::OAuth) {
                (credentials, GrokAuthKind::OAuth)
            } else {
                (
                    GrokCredentials::parse_for_kind(text, GrokAuthKind::Cli)?,
                    GrokAuthKind::Cli,
                )
            };
        let result = self
            .fetch_with_auth(&credentials, kind, &FetchContext::default())
            .await?;
        Ok(account_usage_from_result(&result))
    }

    async fn fetch_with_auth(
        &self,
        credentials: &GrokCredentials,
        kind: GrokAuthKind,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let reset_lookup = GrokProvider::spawn_remaining_resets(
            ctx,
            Some(credentials.access_token.clone()),
            None,
            self.client.clone(),
        );
        let billing = match self
            .fetch_billing(Some(format!("Bearer {}", credentials.access_token)), None)
            .await
        {
            Ok(billing) => billing,
            Err(error) => {
                reset_lookup.abort();
                return Err(error);
            }
        };
        let plan = if kind == GrokAuthKind::Cli {
            self.fetch_cli_subscription_tier(credentials).await
        } else {
            None
        }
        .or_else(|| credentials.login_method());
        let reset_credits = reset_lookup
            .join(ctx.requires_optional_usage_completeness)
            .await;
        let result = result_from_billing(
            billing,
            if kind == GrokAuthKind::Cli {
                "grok-cli"
            } else {
                "grok-oauth"
            },
            credentials.email.clone(),
            credentials.team_id.clone(),
            plan,
        );
        Ok(match reset_credits {
            Some(credits) => result.with_inventory_item(credits),
            None => result,
        })
    }

    async fn fetch_cli_subscription_tier(&self, credentials: &GrokCredentials) -> Option<String> {
        let response = self
            .client
            .get(CLI_SETTINGS_ENDPOINT)
            .timeout(std::time::Duration::from_secs(2))
            .header(
                "Authorization",
                format!("Bearer {}", credentials.access_token),
            )
            .header("x-xai-token-auth", "xai-grok-cli")
            .header("Accept", "application/json")
            .header("User-Agent", "CodexBar")
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let value: Value = response.json().await.ok()?;
        grok_plan_display_name(
            value
                .get("subscription_tier_display")
                .and_then(Value::as_str),
        )
    }

    async fn fetch_with_cookie(
        &self,
        cookie_header: &str,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let reset_lookup = GrokProvider::spawn_remaining_resets(
            ctx,
            None,
            Some(cookie_header.to_string()),
            self.client.clone(),
        );
        let billing = match self
            .fetch_billing(None, Some(cookie_header.to_string()))
            .await
        {
            Ok(billing) => billing,
            Err(error) => {
                reset_lookup.abort();
                return Err(error);
            }
        };
        let reset_credits = reset_lookup
            .join(ctx.requires_optional_usage_completeness)
            .await;
        // v0.56.0: a browser session is its own principal. Never enrich a
        // successful cookie billing result from ambient auth.json metadata,
        // which may belong to a different account or change during the fetch.
        let result = result_from_cookie_billing(billing);
        Ok(match reset_credits {
            Some(credits) => result.with_inventory_item(credits),
            None => result,
        })
    }

    async fn fetch_auto(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let allow_browser_cookie_fallback = !ctx.auto_prefer_web;
        for step in grok_auto_steps(
            ctx.api_key
                .as_deref()
                .is_some_and(|token| !token.trim().is_empty()),
            ctx.manual_cookie_header
                .as_deref()
                .is_some_and(|cookie| !cookie.trim().is_empty())
                && allow_browser_cookie_fallback,
            allow_browser_cookie_fallback,
        ) {
            match step {
                GrokAutoStep::AmbientOAuth => {
                    if let Some(result) = self.try_ambient(GrokAuthKind::OAuth, ctx).await {
                        return result;
                    }
                }
                GrokAutoStep::AmbientCli => {
                    if let Some(result) = self.try_ambient(GrokAuthKind::Cli, ctx).await {
                        return result;
                    }
                }
                GrokAutoStep::ApiKey => {
                    if let Some(token) = ctx.api_key.as_deref() {
                        let credentials = GrokCredentials::from_bearer(token);
                        return self
                            .fetch_with_auth(&credentials, GrokAuthKind::OAuth, ctx)
                            .await;
                    }
                }
                GrokAutoStep::ManualCookie => {
                    if let Some(cookie_header) = &ctx.manual_cookie_header {
                        return self.fetch_with_cookie(cookie_header, ctx).await;
                    }
                }
                GrokAutoStep::CookieRefresh => {
                    return self.fetch_with_cookie_refresh(ctx).await;
                }
            }
        }
        Err(ProviderError::AuthRequired)
    }

    async fn try_ambient(
        &self,
        kind: GrokAuthKind,
        ctx: &FetchContext,
    ) -> Option<Result<ProviderFetchResult, ProviderError>> {
        let credentials = Self::load_credentials(kind).ok()?;
        match self.fetch_with_auth(&credentials, kind, ctx).await {
            Ok(result) => Some(Ok(result)),
            Err(ProviderError::AuthRequired) => None,
            Err(error) => {
                tracing::debug!("Grok login path failed: {error}");
                None
            }
        }
    }

    /// Cookie refresh path (upstream #2458):
    /// 1. Try last validated cached cookie header (background reuse)
    /// 2. On miss/auth failure: re-import browser cookies, validate, cache
    async fn fetch_with_cookie_refresh(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        use crate::browser::cookie_cache::CookieHeaderCache;

        if let Some(cached) = CookieHeaderCache::load(ProviderId::Grok) {
            match self.fetch_with_cookie(&cached.cookie_header, ctx).await {
                Ok(result) => return Ok(result),
                Err(err) if is_cookie_authentication_failure(&err) => {
                    CookieHeaderCache::clear(ProviderId::Grok);
                }
                Err(err) => return Err(err),
            }
        }

        let cookie_header = crate::providers::browser_cookie_header(&["grok.com"])?;
        let result = self.fetch_with_cookie(&cookie_header, ctx).await?;
        // Best-effort cache write: failing to persist the cookie only costs a
        // re-read from the browser on the next fetch.
        let _cached = CookieHeaderCache::store(ProviderId::Grok, &cookie_header, "browser");
        Ok(result)
    }

    async fn fetch_billing(
        &self,
        authorization: Option<String>,
        cookie_header: Option<String>,
    ) -> Result<GrokBillingSnapshot, ProviderError> {
        let mut request = self
            .client
            .post(BILLING_ENDPOINT)
            .body(vec![0, 0, 0, 0, 0])
            .header("Origin", "https://grok.com")
            .header("Referer", "https://grok.com/?_s=usage")
            .header("Accept", "*/*")
            .header("Content-Type", "application/grpc-web+proto")
            .header("x-grpc-web", "1")
            .header("x-user-agent", "connect-es/2.1.1")
            .header("User-Agent", "CodexBar");
        if let Some(auth) = authorization {
            request = request.header("Authorization", auth);
        }
        if let Some(cookie) = cookie_header {
            request = request.header("Cookie", cookie);
        }

        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                return Err(ProviderError::AuthRequired);
            }
            return Err(ProviderError::Other(format!(
                "Grok web billing returned status {status}"
            )));
        }
        billing::validate_grpc_headers(&headers)?;
        billing::parse_grpc_web_response(&bytes)
    }

    fn spawn_remaining_resets(
        ctx: &FetchContext,
        access_token: Option<String>,
        cookie_header: Option<String>,
        client: Client,
    ) -> ResetLookup {
        if !ctx.include_credits {
            return ResetLookup::idle();
        }
        ResetLookup {
            task: Some(tokio::spawn(async move {
                Self::fetch_remaining_resets(
                    client,
                    access_token.as_deref(),
                    cookie_header.as_deref(),
                )
                .await
            })),
        }
    }

    /// Fetch optional SuperGrok reset-credit inventory using the same principal
    /// that produced the successful billing result. This is deliberately
    /// best-effort: billing remains valid when this secondary endpoint is down,
    /// malformed, unauthorized, or empty.
    async fn fetch_remaining_resets(
        client: Client,
        access_token: Option<&str>,
        cookie_header: Option<&str>,
    ) -> Option<ProviderInventoryItem> {
        let mut request = client
            .post(REMAINING_RESETS_ENDPOINT)
            .body(vec![0, 0, 0, 0, 0])
            .timeout(RESET_CREDITS_TIMEOUT)
            .header("Origin", "https://grok.com")
            .header("Referer", "https://grok.com/?_s=usage")
            .header("Accept", "*/*")
            .header("Content-Type", "application/grpc-web+proto")
            .header("x-grpc-web", "1")
            .header("x-user-agent", "connect-es/2.1.1")
            .header("User-Agent", "CodexBar");
        if let Some(access_token) = access_token {
            request = request.header("Authorization", format!("Bearer {access_token}"));
        }
        if let Some(cookie_header) = cookie_header {
            request = request.header("Cookie", cookie_header);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::debug!("Grok reset-credit lookup failed: {error}");
                return None;
            }
        };
        if !response.status().is_success() {
            tracing::debug!(status = %response.status(), "Grok reset-credit lookup returned a non-success status");
            return None;
        }
        let headers = response.headers().clone();
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::debug!("Grok reset-credit response read failed: {error}");
                return None;
            }
        };
        if let Err(error) = billing::validate_grpc_headers(&headers) {
            tracing::debug!("Grok reset-credit RPC failed: {error}");
            return None;
        }
        let coupons = match billing::parse_grpc_web_reset_coupons(&bytes, Utc::now()) {
            Ok(coupons) => coupons,
            Err(error) => {
                tracing::debug!("Grok reset-credit response was invalid: {error}");
                return None;
            }
        };
        let next_expiry = coupons.first().map(|coupon| coupon.expires_at);
        let available_count = u32::try_from(coupons.len()).ok()?;
        (!coupons.is_empty()).then_some(ProviderInventoryItem {
            id: "reset-credits".to_string(),
            title: "Limit Reset Credits".to_string(),
            available_count,
            next_expires_at: next_expiry,
        })
    }

    fn detect_cli_version() -> Option<String> {
        let mut command = std::process::Command::new("grok");
        command.arg("--version");
        hide_windows_console(&mut command);
        let output = command.output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let trimmed = text
            .lines()
            .next()?
            .trim()
            .strip_prefix("grok ")
            .unwrap_or(text.trim());
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }
}

/// Best-effort reset-credit lookup running alongside the billing request.
/// `abort` and `join` own the handle state; `JoinHandle::abort` is already
/// idempotent on finished tasks.
struct ResetLookup {
    task: Option<tokio::task::JoinHandle<Option<ProviderInventoryItem>>>,
}

impl ResetLookup {
    fn idle() -> Self {
        Self { task: None }
    }

    fn abort(self) {
        if let Some(task) = self.task {
            task.abort();
        }
    }

    /// Wait for the lookup with the policy budget: under
    /// `requires_optional_usage_completeness` the full timeout remains;
    /// otherwise a short grace joins the already-running request.
    async fn join(
        self,
        requires_optional_usage_completeness: bool,
    ) -> Option<ProviderInventoryItem> {
        let mut task = self.task?;
        let budget = if requires_optional_usage_completeness {
            RESET_CREDITS_TIMEOUT
        } else {
            RESET_CREDITS_JOIN_GRACE
        };
        match tokio::time::timeout(budget, &mut task).await {
            Ok(Ok(inventory)) => inventory,
            Ok(Err(_)) | Err(_) => {
                task.abort();
                None
            }
        }
    }
}

#[cfg(windows)]
fn hide_windows_console(command: &mut std::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_windows_console(_command: &mut std::process::Command) {}

impl Default for GrokProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for GrokProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Grok
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto => self.fetch_auto(ctx).await,
            SourceMode::Web => {
                if let Some(cookie_header) = &ctx.manual_cookie_header {
                    return self.fetch_with_cookie(cookie_header, ctx).await;
                }
                self.fetch_with_cookie_refresh(ctx).await
            }
            SourceMode::Cli => {
                let credentials = Self::load_credentials(GrokAuthKind::Cli)?;
                self.fetch_with_auth(&credentials, GrokAuthKind::Cli, ctx)
                    .await
            }
            SourceMode::OAuth => {
                // Prefer the switched ~/.grok/auth.json over a leftover token
                // account so Weekly/notifications follow Grok account Switch.
                let credentials = match Self::load_credentials(GrokAuthKind::OAuth) {
                    Ok(credentials) => credentials,
                    Err(error) => {
                        let Some(token) = ctx.api_key.as_deref() else {
                            return Err(error);
                        };
                        GrokCredentials::from_bearer(token)
                    }
                };
                match self
                    .fetch_with_auth(&credentials, GrokAuthKind::OAuth, ctx)
                    .await
                {
                    Ok(result) => Ok(result),
                    Err(ProviderError::AuthRequired) => {
                        if let Some(token) = ctx.api_key.as_deref() {
                            let fallback = GrokCredentials::from_bearer(token);
                            self.fetch_with_auth(&fallback, GrokAuthKind::OAuth, ctx)
                                .await
                        } else {
                            Err(ProviderError::AuthRequired)
                        }
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![
            SourceMode::Auto,
            SourceMode::Cli,
            SourceMode::OAuth,
            SourceMode::Web,
        ]
    }

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        true
    }

    fn detect_version(&self) -> Option<String> {
        Self::detect_cli_version()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrokAutoStep {
    AmbientOAuth,
    AmbientCli,
    ApiKey,
    ManualCookie,
    CookieRefresh,
}

/// Auto must try the switched ~/.grok/auth.json before leftover cookies or
/// token keys. Account rows already fetch per login; Weekly, pace, and
/// notifications use this provider snapshot.
fn grok_auto_steps(
    has_api_key: bool,
    has_manual_cookie: bool,
    allow_browser_cookie_fallback: bool,
) -> Vec<GrokAutoStep> {
    let mut steps = vec![GrokAutoStep::AmbientOAuth, GrokAutoStep::AmbientCli];
    if has_api_key {
        steps.push(GrokAutoStep::ApiKey);
    }
    if has_manual_cookie && allow_browser_cookie_fallback {
        steps.push(GrokAutoStep::ManualCookie);
    }
    if allow_browser_cookie_fallback {
        steps.push(GrokAutoStep::CookieRefresh);
    }
    steps
}

#[derive(Debug, Clone)]
struct GrokCredentials {
    access_token: String,
    auth_mode: Option<String>,
    email: Option<String>,
    team_id: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

impl GrokCredentials {
    fn from_bearer(token: &str) -> Self {
        Self {
            access_token: token.trim().to_string(),
            auth_mode: Some("oidc".into()),
            email: None,
            team_id: None,
            expires_at: None,
        }
    }

    fn parse_for_kind(text: &str, kind: GrokAuthKind) -> Result<Self, ProviderError> {
        let parsed = ParsedGrokAuthFile::parse(text)
            .map_err(|error| ProviderError::Parse(error.to_string()))?;
        let entry = parsed
            .view()
            .map_err(|error| ProviderError::Parse(error.to_string()))?
            .select(kind)
            .ok_or(ProviderError::AuthRequired)?;
        let access_token = entry.key().ok_or(ProviderError::AuthRequired)?.to_owned();
        let expires_at = entry
            .value()
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
            .map(|dt| dt.with_timezone(&Utc));
        if expires_at.is_some_and(|dt| dt <= Utc::now()) {
            return Err(ProviderError::AuthRequired);
        }
        Ok(Self {
            access_token,
            auth_mode: text_field(entry.value(), "auth_mode"),
            email: text_field(entry.value(), "email"),
            team_id: text_field(entry.value(), "team_id"),
            expires_at,
        })
    }

    fn login_method(&self) -> Option<String> {
        match self.auth_mode.as_deref().map(str::to_lowercase).as_deref() {
            Some("oidc") => Some("SuperGrok".to_string()),
            Some("session") => Some("session".to_string()),
            Some(other) => Some(other.to_string()),
            None if self.expires_at.is_some() => Some("Grok".to_string()),
            None => None,
        }
    }
}

fn grok_plan_display_name(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let compact: String = trimmed
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .collect();
    Some(match compact.as_str() {
        "supergrokheavy" | "heavy" => "SuperGrok Heavy".to_string(),
        "supergrok" => "SuperGrok".to_string(),
        _ => trimmed.to_string(),
    })
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// Classify Grok from the full billing-cycle duration, not time remaining.
/// This preserves the upstream #2431/#2566 invariant that a monthly plan near
/// its reset must not become a weekly plan.
fn primary_label_for_cycle_minutes(minutes: u32) -> Option<&'static str> {
    const DAY_MINUTES: u32 = 24 * 60;
    if minutes <= 60 {
        return None;
    }
    let days = (minutes + DAY_MINUTES / 2) / DAY_MINUTES;
    if (4..=12).contains(&days) {
        Some("Weekly")
    } else if (20..=45).contains(&days) {
        Some("Monthly")
    } else {
        None
    }
}

fn result_from_cookie_billing(billing: GrokBillingSnapshot) -> ProviderFetchResult {
    result_from_billing(billing, "grok-browser", None, None, None)
}

fn account_usage_from_result(
    result: &ProviderFetchResult,
) -> crate::providers::grok::accounts::GrokAccountUsage {
    let primary = &result.usage.primary;
    let usage_available = !primary.is_informational;
    crate::providers::grok::accounts::GrokAccountUsage {
        usage_available,
        used_percent: usage_available.then_some(primary.used_percent),
        plan: result
            .usage
            .login_method
            .clone()
            .or(result.usage.primary_label.clone()),
        window_minutes: primary.window_minutes,
        resets_at: primary.resets_at,
    }
}

fn result_from_billing(
    billing: GrokBillingSnapshot,
    source_label: &str,
    email: Option<String>,
    team_id: Option<String>,
    login_method: Option<String>,
) -> ProviderFetchResult {
    // Dynamic cadence is provider-owned and comes only from a complete billing
    // cycle. A reset timestamp alone is insufficient because monthly quotas can
    // have only a few days remaining.
    let primary_label = billing
        .window_minutes
        .and_then(primary_label_for_cycle_minutes);
    let published_percent = billing.used_percent.filter(|_| {
        billing.used_percent_is_wire_published || billing.used_percent_is_implicit_zero
    });
    let primary = match published_percent {
        Some(used_percent) => RateWindow::with_details(
            used_percent,
            billing.window_minutes,
            billing.resets_at,
            None,
        ),
        None => {
            let mut window = RateWindow::informational("Usage unavailable");
            window.resets_at = billing.resets_at;
            window.window_minutes = billing.window_minutes;
            window
        }
    };
    let mut usage = UsageSnapshot::new(primary);
    if let Some(label) = primary_label {
        usage = usage.with_primary_label(label);
    }
    usage.account_email = email;
    usage.account_organization = team_id;
    usage.login_method = login_method;
    ProviderFetchResult::new(usage, source_label)
}

/// Whether a cookie-path error should invalidate the cached browser session.
fn is_cookie_authentication_failure(err: &ProviderError) -> bool {
    matches!(err, ProviderError::AuthRequired)
}

/// Decide the next cookie-refresh step given cache presence and last error.
/// Pure helper for unit tests of the #2458 refresh flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CookieRefreshAction {
    UseCached,
    ReimportBrowser,
    GiveUp,
}

fn cookie_refresh_action(
    has_cached_header: bool,
    last_error: Option<&ProviderError>,
) -> CookieRefreshAction {
    match last_error {
        None if has_cached_header => CookieRefreshAction::UseCached,
        None => CookieRefreshAction::ReimportBrowser,
        Some(err) if is_cookie_authentication_failure(err) => CookieRefreshAction::ReimportBrowser,
        Some(ProviderError::NoCookies) => CookieRefreshAction::ReimportBrowser,
        Some(_) => CookieRefreshAction::GiveUp,
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
