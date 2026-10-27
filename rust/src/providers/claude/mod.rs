//! Claude provider implementation

pub mod accounts;
mod admin_api;
mod auto_precision;
pub mod claude_swap;
mod cli_binary;
mod cli_probe;
mod cli_reset;
mod cli_screen;
mod cli_text;
mod cloud_credits;
pub mod cost_account;
mod desktop_cookies;
mod oauth;
pub mod quota_history;
mod reset_credits;
pub mod reset_observations;
mod scoped_weekly;
mod spend_window;
mod subscription_details;
mod trust_dialog;
mod web_api;

pub(crate) use spend_window::PACE_CAPABILITY;

use async_trait::async_trait;
use chrono::Utc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::core::{
    FetchContext, LastGoodFailurePolicy, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};

use admin_api::ClaudeAdminApiFetcher;
use cli_binary::detect_claude_version;
pub use cli_binary::locate_claude_binary;
use cli_probe::{
    claude_cli_error_from_output, fetch_claude_cli_usage_text, redacted_probe_screen,
    resolve_claude_cli_path,
};
#[cfg(test)]
use cli_reset::parse_claude_reset_date_in_system_zone;
use cli_reset::{extract_cli_scoped_weekly_limits, parse_claude_reset_date, percent_matches};
use cli_text::{
    WEEKLY_LABELS, extract_email, extract_inline_reset_description, extract_login_method,
    extract_percent_near_label, extract_reset_description, has_plan_limit_section,
    is_cli_activity_stats_response, is_exhausted_short_form,
    is_non_interactive_slash_command_response, split_usage_insights,
};

/// Limit reset detection (upstream `UsageStore+LimitResetCelebration.swift`):
/// after a weekly reset, two above-threshold samples must confirm recovery
/// before the lane re-arms, and identity-less samples need a second low sample.
pub(crate) const LIMIT_RESET_POLICY: crate::core::ProviderLimitResetPolicy =
    crate::core::ProviderLimitResetPolicy {
        supported: true,
        weekly: crate::core::WeeklyResetRule::RecoveryObservations { count: 2 },
        missing_identity: crate::core::MissingIdentityRule::Track {
            requires_low_confirmation: true,
        },
    };

// ── Upstream 0.50.1 #2516: CLI usage-result cache ────────────────────────────
//
// When token rotation revokes OAuth access, the auto path falls back to the
// CLI. To avoid hammering the CLI probe on every poll, cache the last
// successful CLI result for 15 minutes. The cache is only consulted when
// OAuth returned `OAuthRevoked` (revoked, not merely expired) so normal
// refresh cycles are unaffected.
const CLI_RESULT_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

struct CachedCliResult {
    result: ProviderFetchResult,
    cached_at: Instant,
}

static CLI_RESULT_CACHE: LazyLock<Mutex<Option<CachedCliResult>>> =
    LazyLock::new(|| Mutex::new(None));

fn clear_account_caches(credential_path: &std::path::Path) {
    clear_cli_result_cache();
    oauth::clear_account_cache(credential_path);
}

/// Store a successful CLI fetch result in the 15-minute cache.
fn cache_cli_result(result: ProviderFetchResult) {
    if let Ok(mut guard) = CLI_RESULT_CACHE.lock() {
        *guard = Some(CachedCliResult {
            result,
            cached_at: Instant::now(),
        });
    }
}

/// Drop the cached CLI result once a live OAuth answer is newer than it.
fn clear_cli_result_cache() {
    if let Ok(mut guard) = CLI_RESULT_CACHE.lock() {
        *guard = None;
    }
}

/// Return a cached CLI result if it is still within the TTL. Used when
/// revoked OAuth prevents a live fetch and the CLI should not be re-probed.
fn cached_cli_result() -> Option<ProviderFetchResult> {
    let Ok(guard) = CLI_RESULT_CACHE.lock() else {
        return None;
    };
    guard
        .as_ref()
        .filter(|entry| entry.cached_at.elapsed() <= CLI_RESULT_CACHE_TTL)
        .map(|entry| {
            let mut result = entry.result.clone();
            // A retained payload is useful for display, but cannot prove that
            // the current fetch reached Claude CLI successfully.
            result.has_successful_claude_cli_quota = false;
            result
        })
}

/// Whether the OAuth source failed with a revocation (not just expiry).
/// Revoked tokens should reuse the working CLI fallback; expired/missing
/// tokens should NOT block the normal refresh path.
fn is_oauth_revoked_error(error: &ProviderError) -> bool {
    matches!(error, ProviderError::OAuthRevoked(_))
}

/// OAuth failures after which Auto reuses a cached CLI result instead of
/// probing the CLI again: a revocation, or a 429 (the OAuth fetcher then
/// backs off for minutes and every poll would otherwise wait on the probe).
fn oauth_failure_uses_cli_cache(error: &ProviderError) -> bool {
    is_oauth_revoked_error(error) || oauth::is_rate_limited_error(error)
}
pub use oauth::ClaudeOAuthFetcher;
pub use web_api::ClaudeWebApiFetcher;

/// Recovery guidance for a Claude web request blocked by a Cloudflare challenge.
pub const CLOUDFLARE_CHALLENGE_MESSAGE: &str = concat!(
    "claude.ai is behind a Cloudflare challenge, often caused by VPN or datacenter networks. ",
    "Re-authenticating will not help. Switch Claude Usage source to OAuth in Settings ",
    "(Usage credits balance will be unavailable), or try a different network."
);

/// Page that signs the browser in to claude.ai, restoring the session the Web
/// source reads (Issue #640 item 8).
pub const CLAUDE_BROWSER_SIGN_IN_URL: &str = "https://claude.ai/login";

/// Whether the user explicitly consented to reading (and refreshing) Claude
/// Code's own credentials. Upstream #2634/#2745: without consent the
/// file/keyring sources stay closed and refreshed tokens are never rotated
/// into Claude Code's storage; Auto then falls back to labeled
/// reduced-fidelity CLI usage.
pub(crate) fn claude_code_consent() -> bool {
    crate::settings::Settings::load().claude_allow_reading_claude_code_credentials
}

/// Return the identity of the credential that can authorize a Claude CLI
/// resume. The OAuth module applies the same consent boundary as its fetcher.
pub fn auto_resume_identity() -> Option<String> {
    oauth::auto_resume_identity()
}

/// Claude provider implementation
pub struct ClaudeProvider {
    web_fetcher: ClaudeWebApiFetcher,
    oauth_fetcher: ClaudeOAuthFetcher,
    admin_fetcher: ClaudeAdminApiFetcher,
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            web_fetcher: ClaudeWebApiFetcher::new(),
            oauth_fetcher: ClaudeOAuthFetcher::new(),
            admin_fetcher: ClaudeAdminApiFetcher::new(),
        }
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Consumer usage page for recognized Claude subscriptions (upstream #4503).
/// Default and API accounts use the spec's Platform usage link instead.
const CLAUDE_SUBSCRIPTION_DASHBOARD_URL: &str = "https://claude.ai/settings/usage";

/// Upstream `ClaudePlan.isSubscriptionLoginMethod`: the plan is resolved by
/// `ClaudePlan.fromCompatibilityLoginMethod`, which checks the label's words in
/// a fixed priority order (Max, Pro, Team, Enterprise, Ultra); the first word
/// in that order that appears decides. Max, Pro, Team and Ultra count as a
/// subscription; Enterprise alone, API and other labels do not.
fn is_subscription_login_method(login_method: Option<&str>) -> bool {
    let Some(login_method) = login_method else {
        return false;
    };
    let words: Vec<String> = login_method
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    const PLAN_WORDS: [(&str, bool); 5] = [
        ("max", true),
        ("pro", true),
        ("team", true),
        ("enterprise", false),
        ("ultra", true),
    ];
    PLAN_WORDS
        .iter()
        .find(|(plan, _)| words.iter().any(|word| word == plan))
        .is_some_and(|(_, counts_as_subscription)| *counts_as_subscription)
}

/// Branded plan label for a rate-limit tier or saved plan id, following
/// upstream `ClaudePlan.fromRateLimitTier` (substring match, Max multiplier
/// from the word after `max`). `None` for tiers that name no plan, such as
/// `default_claude_ai`, so raw tier ids never reach the UI.
fn claude_plan_label(tier: &str) -> Option<String> {
    let normalized = tier.trim().to_lowercase();
    if normalized == "free" {
        return Some("Claude Free".to_string());
    }
    let plan = if normalized.contains("max") {
        "Claude Max"
    } else if normalized.contains("pro") {
        "Claude Pro"
    } else if normalized.contains("team") {
        "Claude Team"
    } else if normalized.contains("enterprise") {
        "Claude Enterprise"
    } else {
        return None;
    };
    if plan != "Claude Max" {
        return Some(plan.to_string());
    }
    let words: Vec<&str> = normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let multiplier = words
        .iter()
        .position(|word| *word == "max")
        .and_then(|index| words.get(index + 1))
        .filter(|word| {
            word.strip_suffix('x')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        });
    Some(match multiplier {
        Some(multiplier) => format!("{plan} {multiplier}"),
        None => plan.to_string(),
    })
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Initial `/usage` PTY timeout. Kept at 24 s rather than upstream's 12 s
/// auto-probe timeout: the Windows PTY needs ~3 s before the prompt mounts,
/// and the in-probe `/usage` re-sends run up to 14 s after launch.
const CLAUDE_CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(24);
/// Upstream #4155 (`cliRetryProbeTimeout`): a retry after a transient CLI
/// timeout gets the longer window. Capped by the remaining fetch budget.
const CLAUDE_CLI_RETRY_PROBE_TIMEOUT: Duration = Duration::from_secs(60);
/// Shell per-provider budget for Claude (`SLOW_PROVIDER_FETCH_TIMEOUT` in
/// `apps/desktop-tauri/src-tauri/src/commands/providers.rs`). The shell drops
/// the whole fetch future at this point, so a retry must end before it.
const CLAUDE_FETCH_BUDGET: Duration = Duration::from_secs(75);
/// Reserved after the last probe for the summary and stale-cache fallback.
const CLAUDE_FETCH_BUDGET_MARGIN: Duration = Duration::from_secs(5);
/// Below this a retry cannot finish the PTY handshake and `/usage` render.
const CLAUDE_CLI_MIN_RETRY_TIMEOUT: Duration = Duration::from_secs(15);

/// Whether a failed CLI probe is a transient timeout or loading stall that a
/// longer retry can fix (upstream `isRetryableCLIProbeError`). Auth failures
/// are never retried.
fn is_retryable_cli_probe_error(error: &ProviderError) -> bool {
    match error {
        ProviderError::AuthRequired | ProviderError::OAuth(_) | ProviderError::OAuthExpired(_) => {
            false
        }
        ProviderError::Timeout => true,
        ProviderError::Parse(message) => message.to_lowercase().contains("still loading usage"),
        other => {
            let message = other.to_string().to_lowercase();
            message.contains("timed out") || message.contains("timeout")
        }
    }
}

/// Whether the latest `/usage` panel shows only the "Loading usage data…"
/// placeholder (upstream `isUsageStillLoading`). Earlier panels in the PTY
/// capture are ignored, and any percentage means the data has rendered.
fn claude_cli_usage_still_loading(visible: &str) -> bool {
    let lower = visible.to_lowercase();
    let panel = lower
        .rfind("settings:")
        .map(|start| &lower[start..])
        .filter(|tail| tail.contains("usage"))
        .unwrap_or(&lower);
    let normalized: String = panel.chars().filter(|c| !c.is_whitespace()).collect();
    normalized.contains("loadingusage") && !normalized.contains('%')
}

/// Timeout for the outer retry, given the time already spent in this fetch.
/// Returns `None` when the remaining budget is too small for a useful probe.
fn cli_retry_timeout(elapsed: Duration) -> Option<Duration> {
    let remaining = CLAUDE_FETCH_BUDGET
        .checked_sub(elapsed)?
        .checked_sub(CLAUDE_FETCH_BUDGET_MARGIN)?;
    let timeout = remaining.min(CLAUDE_CLI_RETRY_PROBE_TIMEOUT);
    (timeout >= CLAUDE_CLI_MIN_RETRY_TIMEOUT).then_some(timeout)
}

/// Run the CLI probe once with the initial timeout, then retry once with the
/// longer timeout when the failure is retryable and budget remains. Errors
/// from the retry are returned as is.
async fn probe_cli_with_retry<F, Fut>(
    started: Instant,
    mut probe: F,
) -> Result<String, ProviderError>
where
    F: FnMut(Duration) -> Fut,
    Fut: std::future::Future<Output = Result<String, ProviderError>>,
{
    match probe(CLAUDE_CLI_PROBE_TIMEOUT).await {
        Err(error) if is_retryable_cli_probe_error(&error) => {
            let Some(timeout) = cli_retry_timeout(started.elapsed()) else {
                return Err(error);
            };
            tracing::debug!(
                timeout_secs = timeout.as_secs(),
                "Claude CLI probe timed out; retrying with a longer timeout"
            );
            probe(timeout).await
        }
        result => result,
    }
}

fn last_good_failure_policy_for_error(error: &str) -> LastGoodFailurePolicy {
    let lower = error.to_ascii_lowercase();
    if lower.contains("credentials not found")
        || (lower.contains("run") && lower.contains("claude") && lower.contains("authenticate"))
        || (lower.contains("not installed") && lower.contains("claude"))
        || (lower.contains("subscription") && lower.contains("unavailable"))
    {
        return LastGoodFailurePolicy::Replace;
    }
    if lower.contains(&CLOUDFLARE_CHALLENGE_MESSAGE.to_ascii_lowercase()) {
        return LastGoodFailurePolicy::PreserveOnceThenSurface;
    }
    if lower.contains("parse error")
        || lower.contains("empty output")
        || lower.contains("missing current session")
        || lower.contains("treated /usage as a normal prompt")
        || lower.contains("local activity stats")
        || lower.contains("could not parse")
        || error.eq_ignore_ascii_case("timeout")
        || lower.contains("timed out")
    {
        return LastGoodFailurePolicy::Preserve;
    }
    if lower.contains("unauthorized")
        || lower.contains("authentication required")
        || lower.contains("auth required")
    {
        return LastGoodFailurePolicy::PreserveOnce;
    }
    LastGoodFailurePolicy::Replace
}

#[async_trait]
impl Provider for ClaudeProvider {
    fn manual_cookie_precedes_token_account(&self) -> bool {
        true
    }

    fn dashboard_url_for_login_method(&self, login_method: Option<&str>) -> Option<&'static str> {
        if is_subscription_login_method(login_method) {
            Some(CLAUDE_SUBSCRIPTION_DASHBOARD_URL)
        } else {
            self.metadata().dashboard_url
        }
    }

    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    /// Upstream #4126: a scoped-only account shows its model-scoped weekly
    /// quota instead of the empty session placeholder.
    fn automatic_metric_absent_core_window_prefix(&self) -> Option<&'static str> {
        Some(scoped_weekly::SCOPED_WEEKLY_ID_PREFIX)
    }

    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn retains_last_good_on_transport_failure(&self) -> bool {
        true
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        // Budget clock for the shell's per-provider timeout; see `cli_retry_timeout`.
        let started = Instant::now();
        match ctx.source_mode {
            SourceMode::Auto => self
                .fetch_via_auto(ctx, started)
                .await
                .map(auto_precision::finish_auto_result),
            SourceMode::OAuth => self.fetch_via_oauth(ctx).await,
            SourceMode::Web => self.fetch_via_web(ctx).await,
            SourceMode::Cli => match self.fetch_via_cli(ctx, started).await {
                Ok(result) => Ok(result),
                Err(error) if should_fallback_from_claude_cli_error(&error) => {
                    tracing::debug!(
                        error = %error,
                        "Claude CLI usage probe failed with a fallback-safe error; trying OAuth"
                    );
                    self.fetch_via_oauth(ctx).await
                }
                Err(error) => Err(error),
            },
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![
            SourceMode::Auto,
            SourceMode::OAuth,
            SourceMode::Web,
            SourceMode::Cli,
        ]
    }

    fn supports_oauth(&self) -> bool {
        true
    }

    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn last_good_failure_policy(&self, error: &str) -> LastGoodFailurePolicy {
        last_good_failure_policy_for_error(error)
    }

    fn detect_version(&self) -> Option<String> {
        detect_claude_version()
    }
    /// Claude's CLI-presence probe (`resolve_claude_cli_path`) raises
    /// `NotInstalled` when the `claude` binary itself is missing — an
    /// installation gap, not a credential problem — so it surfaces as an
    /// offline local runtime (matching the pre-backend classifier's
    /// treatment of CLI-presence failures). Message-scoped so any future
    /// credential-flavored `NotInstalled` keeps the default mapping.
    fn error_state_kind(&self, error: &ProviderError) -> crate::core::ProviderStateKind {
        match error {
            ProviderError::NotInstalled(msg) if msg.contains("CLI not found") => {
                crate::core::ProviderStateKind::LocalRuntimeOffline
            }
            _ => error.state_kind(),
        }
    }
}

impl ClaudeProvider {
    async fn fetch_via_auto(
        &self,
        ctx: &FetchContext,
        started: Instant,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let mut failures = Vec::new();

        if self.admin_fetcher.has_credentials(ctx) {
            tracing::debug!("Attempting Admin API fetch for Claude");
            let admin = self.admin_fetcher.fetch(ctx).await;
            if let Some(result) = record_auto_source(&mut failures, "Admin API", admin)? {
                return Ok(result);
            }
        }

        if let Some(result) =
            record_auto_source(&mut failures, "Web", self.fetch_via_web(ctx).await)?
        {
            return Ok(live_auto_success(result));
        }

        // Upstream 0.50.1 #2516: track whether OAuth failed with a revocation.
        let oauth_result = self.fetch_via_oauth(ctx).await;
        let use_cli_cache = oauth_result
            .as_ref()
            .err()
            .is_some_and(oauth_failure_uses_cli_cache);
        if let Some(result) = record_auto_source(&mut failures, "OAuth", oauth_result)? {
            return Ok(live_auto_success(result));
        }

        // When OAuth was revoked (not just expired) or is rate limited, reuse a
        // cached CLI result if still within the 15-minute TTL to avoid
        // re-probing the CLI.
        if use_cli_cache && let Some(cached) = cached_cli_result() {
            tracing::debug!(
                "Claude OAuth revoked or rate limited; returning cached CLI result (15-min cache)"
            );
            return Ok(cached);
        }

        let cli_result = self.fetch_via_cli(ctx, started).await;
        if let Some(cached) = stale_cli_result_on_probe_timeout(&cli_result) {
            tracing::debug!("Claude CLI probe timed out; returning stale cached CLI result");
            return Ok(cached);
        }
        if let Some(mut result) = record_auto_source(&mut failures, "CLI", cli_result)? {
            // Without consent for reading Claude Code credentials, label the
            // CLI fallback as reduced fidelity.
            if !claude_code_consent() {
                result.source_label = "cli (reduced fidelity)".to_string();
            }
            // Cache the CLI result when OAuth was revoked or rate limited so
            // subsequent polls within the TTL reuse it without re-probing.
            if use_cli_cache {
                cache_cli_result(result.clone());
            }
            return Ok(result);
        }

        // Upstream 0.50.1 #2516: when all live sources fail, keep the
        // last-known quota visible (stale) instead of blanking the UI.
        if let Some(cached) = cached_cli_result() {
            tracing::debug!("All Claude live sources failed; returning stale cached CLI result");
            return Ok(cached);
        }

        Err(claude_auto_fetch_error(failures))
    }

    async fn fetch_via_oauth(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Attempting OAuth fetch for Claude");
        if let Some(token) = ctx
            .api_key
            .as_deref()
            .filter(|token| !token.trim().is_empty())
        {
            return self.oauth_fetcher.fetch_with_access_token(token).await;
        }
        self.oauth_fetcher.fetch().await
    }

    async fn fetch_via_web(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Attempting Web API fetch for Claude");

        // Selection requires the Automatic cookie source, where the header is
        // the stored cookie (usually an earlier browser import). The desktop
        // session replaces it so a desktop account switch is followed on the
        // next refresh; the Manual source never reaches this branch.
        if desktop_cookies::selected(&crate::settings::Settings::load()) {
            tracing::debug!("Using the Claude desktop app cookie store");
            let cookie_header = desktop_cookies::session_cookie_header()?;
            return self
                .web_fetcher
                .fetch_with_cookie_header(&cookie_header)
                .await;
        }

        if let Some(ref cookie_header) = ctx.manual_cookie_header {
            tracing::debug!("Using manual cookie header");
            return self
                .web_fetcher
                .fetch_with_cookie_header(cookie_header)
                .await;
        }

        // Otherwise, try to extract cookies from browser
        self.web_fetcher.fetch_with_cookies().await
    }

    async fn fetch_via_cli(
        &self,
        _ctx: &FetchContext,
        started: Instant,
    ) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Attempting CLI probe for Claude");

        let claude_path = resolve_claude_cli_path()?;
        let visible = probe_cli_with_retry(started, |timeout| {
            let claude_path = claude_path.clone();
            async move {
                let combined = fetch_claude_cli_usage_text(claude_path, timeout).await?;
                // Replay cursor redraws once; rendering is idempotent on rendered text.
                let visible = cli_screen::render(&combined, true);
                // Checked inside the probe so a loading stall gets the longer retry.
                if claude_cli_usage_still_loading(&visible) {
                    return Err(ProviderError::Parse(
                        "Claude CLI /usage is still loading usage data.".to_string(),
                    ));
                }
                Ok(visible)
            }
        })
        .await?;
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(output = %redacted_probe_screen(&visible), "Claude CLI probe output");
        }

        if let Some(error) = claude_cli_error_from_output(&visible) {
            return Err(error);
        }

        let mut result = self.parse_cli_output(&visible)?;
        if let Some(identity) = auto_resume_identity() {
            result = result.with_account_identity(identity);
        }
        Ok(mark_live_claude_cli_result(result))
    }

    /// Parse Claude CLI /usage output
    fn parse_cli_output(&self, output: &str) -> Result<ProviderFetchResult, ProviderError> {
        let rendered = cli_screen::render(output, true);
        // Insights carry arbitrary tool names and usage shares, never quota or
        // account fields, so nothing below the heading may feed the parsers.
        let (clean, has_insights) = split_usage_insights(rendered);
        let clean_lower = clean.to_lowercase();

        if clean.trim().is_empty() {
            return Err(ProviderError::Parse(
                "Empty output from Claude CLI".to_string(),
            ));
        }

        if is_non_interactive_slash_command_response(&clean_lower) {
            return Err(ProviderError::Other(
                "Claude CLI treated /usage as a normal prompt instead of opening the interactive usage screen. Use Auto, OAuth, or Web mode for Claude usage.".to_string(),
            ));
        }

        // Newer Claude versions print local activity stats (cost, duration,
        // cache tokens) below the plan limits; only stats without any limit
        // section are rejected.
        let activity_stats = is_cli_activity_stats_response(&clean_lower);
        if activity_stats && !has_plan_limit_section(&clean_lower) {
            return Err(ProviderError::Other(
                "Claude CLI /usage opened, but this Claude version returned local activity stats instead of plan limit percentages. Use Auto, OAuth, or Web mode for Claude limits.".to_string(),
            ));
        }

        let mut session_percent = extract_percent_near_label(&clean, "current session");
        let mut weekly_percent = WEEKLY_LABELS
            .iter()
            .find_map(|label| extract_percent_near_label(&clean, label));

        // Fallback: collect all percentages in order. Activity stats carry
        // their own percentages, which must never be read as plan limits.
        if session_percent.is_none() && !activity_stats {
            let all_percents: Vec<f64> = percent_matches(&clean).collect();
            if !all_percents.is_empty() {
                session_percent = Some(all_percents[0]);
            }
            if all_percents.len() > 1 && weekly_percent.is_none() {
                weekly_percent = Some(all_percents[1]);
            }
        }

        if session_percent.is_none()
            && weekly_percent.is_none()
            && !is_exhausted_short_form(&clean_lower)
        {
            let message = if has_insights {
                "Claude CLI /usage returned usage insights without quota data."
            } else {
                "Claude CLI did not return usage data"
            };
            return Err(ProviderError::Parse(message.to_string()));
        }

        // Extract identity info
        let email = extract_email(&clean);
        let login_method = extract_login_method(&clean);

        // Extract reset times
        let session_reset = extract_reset_description(&clean, "current session");
        let weekly_reset = WEEKLY_LABELS
            .iter()
            .find_map(|label| extract_reset_description(&clean, label));
        let short_form_reset = if is_exhausted_short_form(&clean_lower) {
            extract_inline_reset_description(&clean)
        } else {
            None
        };
        let session_reset = session_reset.or(short_form_reset);
        let now = Utc::now();
        let scoped_weekly_limits = extract_cli_scoped_weekly_limits(&clean, now);

        if session_percent.is_none() && is_exhausted_short_form(&clean_lower) {
            session_percent = Some(100.0);
        }

        // Build usage snapshot
        let session_used = session_percent.unwrap_or(0.0);
        let primary = RateWindow::with_details(
            session_used,
            Some(300), // 5 hour session window
            session_reset
                .as_deref()
                .and_then(|reset| parse_claude_reset_date(reset, now, Some(300))),
            session_reset,
        );

        let mut usage = UsageSnapshot::new(primary);

        if let Some(weekly_used) = weekly_percent {
            let secondary = RateWindow::with_details(
                weekly_used,
                Some(10080), // weekly (7 * 24 * 60)
                weekly_reset
                    .as_deref()
                    .and_then(|reset| parse_claude_reset_date(reset, now, Some(10080))),
                weekly_reset,
            );
            usage = usage.with_secondary(secondary);
        }

        for limit in scoped_weekly_limits {
            usage.extra_rate_windows.push(limit);
        }

        if let Some(method) = login_method {
            usage = usage.with_login_method(&method);
        } else {
            usage = usage.with_login_method("Claude (CLI)");
        }

        if let Some(email) = email {
            usage = usage.with_email(&email);
        }

        Ok(ProviderFetchResult::new(usage, "cli"))
    }
}

fn has_real_claude_quota_window(usage: &UsageSnapshot) -> bool {
    let is_real = |window: &RateWindow| !window.is_informational && window.used_percent.is_finite();
    is_real(&usage.primary) || usage.secondary.as_ref().is_some_and(is_real)
}

fn mark_live_claude_cli_result(mut result: ProviderFetchResult) -> ProviderFetchResult {
    if has_real_claude_quota_window(&result.usage) {
        result.has_successful_claude_cli_quota = true;
    }
    result
}

fn record_auto_source(
    failures: &mut Vec<(&'static str, ProviderError)>,
    source: &'static str,
    result: Result<ProviderFetchResult, ProviderError>,
) -> Result<Option<ProviderFetchResult>, ProviderError> {
    match result {
        Ok(result) => Ok(Some(result)),
        Err(error) if error.is_transport_failure() => Err(error),
        Err(error) => {
            failures.push((source, error));
            Ok(None)
        }
    }
}

/// A live OAuth or Web answer is newer than any retained CLI result. Dropping
/// the retained result keeps a later CLI timeout from serving usage older than
/// what the user has already seen.
fn live_auto_success(result: ProviderFetchResult) -> ProviderFetchResult {
    clear_cli_result_cache();
    result
}

/// `record_auto_source` propagates transport failures immediately, so a CLI
/// probe timeout would never reach the auto path's stale-cache fallback. The
/// CLI probe has no network leg, so a transport-class error here is a probe
/// timeout; prefer the retained CLI result when one is still within the TTL.
fn stale_cli_result_on_probe_timeout(
    result: &Result<ProviderFetchResult, ProviderError>,
) -> Option<ProviderFetchResult> {
    match result {
        Err(error) if error.is_transport_failure() => cached_cli_result(),
        _ => None,
    }
}

fn claude_auto_fetch_error(failures: Vec<(&'static str, ProviderError)>) -> ProviderError {
    let browser_sign_in = needs_browser_sign_in(&failures);
    let summary = failures
        .into_iter()
        .map(|(source, error)| format!("{source}: {error}"))
        .collect::<Vec<_>>()
        .join("; ");
    let message = format!("Claude usage failed from all configured sources. {summary}");
    if browser_sign_in {
        return ProviderError::BrowserSignInRequired {
            message: format!("{message} {}", browser_sign_in_hint()),
            sign_in_url: CLAUDE_BROWSER_SIGN_IN_URL.to_string(),
        };
    }
    ProviderError::Other(message)
}

/// Issue #640 item 8: the OAuth usage endpoint refused with 429 (Claude Code
/// stays signed in), no claude.ai browser cookies were readable, and the CLI
/// probe failed as well. Until the rate limit lifts only a browser sign-in
/// brings usage back, so callers get a typed signal instead of English text.
fn needs_browser_sign_in(failures: &[(&'static str, ProviderError)]) -> bool {
    let failed = |source: &str, matches: fn(&ProviderError) -> bool| {
        failures
            .iter()
            .any(|(failed_source, error)| *failed_source == source && matches(error))
    };
    failed("OAuth", oauth::is_rate_limited_error)
        && failed("Web", |error| matches!(error, ProviderError::NoCookies))
        && failed("CLI", |_| true)
}

/// Appended to the Auto summary for [`needs_browser_sign_in`]. It must avoid
/// the phrases [`last_good_failure_policy_for_error`] reacts to, so the
/// desktop keeps the retention policy of the plain summary.
fn browser_sign_in_hint() -> String {
    format!(
        "The OAuth usage endpoint is rate limited and no claude.ai browser session was found. Sign in at {CLAUDE_BROWSER_SIGN_IN_URL} in your browser, then refresh."
    )
}

fn should_fallback_from_claude_cli_error(error: &ProviderError) -> bool {
    match error {
        ProviderError::Parse(message) => {
            matches!(
                message.as_str(),
                "Claude CLI did not return usage data" | "Empty output from Claude CLI"
            )
        }
        ProviderError::Other(message) => {
            message.contains("returned local activity stats")
                || message.contains("treated /usage as a normal prompt")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
