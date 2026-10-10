//! Claude provider implementation

pub mod accounts;
mod admin_api;
mod auto_precision;
pub mod claude_swap;
mod cli_reset;
mod cli_screen;
mod oauth;
pub mod quota_history;
mod reset_credits;
pub mod reset_observations;
mod scoped_weekly;
mod trust_dialog;
mod web_api;

use async_trait::async_trait;
use chrono::Utc;
use regex_lite::Regex;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::{Command as StdCommand, Stdio};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::cli::tty_runner::{TtyCommandOptions, TtyCommandRunner};
use crate::core::{
    FetchContext, LastGoodFailurePolicy, Provider, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};

use admin_api::ClaudeAdminApiFetcher;
#[cfg(test)]
use cli_reset::parse_claude_reset_date_in_system_zone;
use cli_reset::{
    extract_cli_scoped_weekly_limits, label_section, normalized_for_label_search,
    parse_claude_reset_date, parse_percent_line, percent_matches,
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

fn claude_plan_label(tier: &str) -> String {
    let normalized = tier.to_lowercase();
    if normalized.contains("claude_max_5x") || normalized.contains("claude_max_5") {
        "Claude Max 5x".to_string()
    } else if normalized.contains("claude_max_20x") || normalized.contains("claude_max_20") {
        "Claude Max 20x".to_string()
    } else {
        match normalized.as_str() {
            "free" => "Claude Free".to_string(),
            "pro" | "claude_pro" => "Claude Pro".to_string(),
            "max" => "Claude Max".to_string(),
            "team" => "Claude Team".to_string(),
            "enterprise" => "Claude Enterprise".to_string(),
            _ => format!("Claude ({})", tier),
        }
    }
}

const CLAUDE_PROBE_SESSION_ID_FILE: &str = ".codexbar-session-id";
const CLAUDE_PROBE_LOCK_FILE: &str = ".codexbar-probe.lock";
const CLAUDE_PROBE_CACHE_FILE: &str = ".codexbar-usage-cache.json";
/// How long a second codexbar process waits for a running probe to finish.
const CLAUDE_PROBE_LOCK_WAIT: Duration = Duration::from_secs(30);
/// Every codexbar process (the `serve` daemon, one-off `usage` calls from the
/// companion) launches its own Claude CLI for a probe. The interactive
/// `/usage` screen costs 6-10 s of CPU each time, so a recent successful
/// probe output is shared across processes for this long.
const CLAUDE_PROBE_CACHE_TTL: Duration = Duration::from_secs(45);

#[derive(serde::Serialize, serde::Deserialize)]
struct ClaudeProbeCache {
    captured_at_unix: u64,
    /// `claude_login_fingerprint` of the login the screen belongs to.
    #[serde(default)]
    login: String,
    output: String,
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_cached_probe_output(probe_dir: &std::path::Path, login: &str) -> Option<String> {
    let raw = std::fs::read_to_string(probe_dir.join(CLAUDE_PROBE_CACHE_FILE)).ok()?;
    let cache: ClaudeProbeCache = serde_json::from_str(&raw).ok()?;
    let age = unix_now_secs().saturating_sub(cache.captured_at_unix);
    if auto_precision::probe_screen_is_superseded(cache.captured_at_unix)
        || login.is_empty()
        || cache.login != login
        || age > CLAUDE_PROBE_CACHE_TTL.as_secs()
        || cache.output.trim().is_empty()
    {
        return None;
    }
    tracing::debug!(age_secs = age, "Reusing recent Claude CLI probe output");
    Some(cache.output)
}

fn store_cached_probe_output(probe_dir: &std::path::Path, login: &str, output: &str) {
    let cache = ClaudeProbeCache {
        captured_at_unix: unix_now_secs(),
        login: login.to_string(),
        output: output.to_string(),
    };
    // Atomic, because other processes read the cache without the probe lock.
    let stored = serde_json::to_vec(&cache)
        .map_err(anyhow::Error::from)
        .and_then(|json| {
            crate::atomic_file::write_atomic(&probe_dir.join(CLAUDE_PROBE_CACHE_FILE), &json)
        });
    if let Err(err) = stored {
        tracing::debug!(error = %err, "failed to persist Claude probe cache");
    }
}

/// Identifies the Claude login a probe runs under without reading any
/// credential: the location, size and modification time of Claude Code's
/// `.credentials.json`, which every login, token refresh and account switch
/// rewrites. Without a credentials file a probe screen is never shared.
fn claude_login_fingerprint() -> Option<String> {
    let credentials = accounts::config_dir().ok()?.join(".credentials.json");
    login_fingerprint_at(&credentials)
}

fn login_fingerprint_at(credentials: &std::path::Path) -> Option<String> {
    let metadata = std::fs::metadata(credentials).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let identity = format!(
        "{}|{}|{}",
        credentials.display(),
        metadata.len(),
        modified.as_nanos()
    );
    Some(crate::core::sha256_hex(identity.as_bytes()))
}

/// The probe screen as it may appear in a log: secrets and email addresses
/// (the screen can show the signed-in account) are masked.
fn redacted_probe_screen(visible: &str) -> String {
    let redacted = crate::core::SecretRedactor::redact(visible);
    crate::core::PersonalInfoRedactor::redact_emails_in_text(Some(&redacted), true)
        .unwrap_or(redacted)
}

/// Only a parseable usage screen is worth sharing; errors are retried live.
fn claude_cli_output_is_shareable(output: &str) -> bool {
    claude_cli_error_from_output(output).is_none()
        && ClaudeProvider::new().parse_cli_output(output).is_ok()
}

/// Cross-process guard around the Claude PTY probe. Claude Code refuses to
/// start a session whose `--session-id` is already running ("Session ID …
/// is already in use"), so two codexbar processes (for example the
/// `serve` daemon and a one-off `usage` call) must not probe concurrently.
struct ClaudeProbeLock(std::fs::File);

impl ClaudeProbeLock {
    /// Wait for the probe lock. `Ok(None)` means locking is unsupported here
    /// and the probe runs unlocked. A probe still running elsewhere after the
    /// wait is an error: probing alongside it would reuse its session id.
    fn acquire(probe_dir: &std::path::Path) -> Result<Option<Self>, ProviderError> {
        Self::acquire_within(probe_dir, CLAUDE_PROBE_LOCK_WAIT)
    }

    fn acquire_within(
        probe_dir: &std::path::Path,
        wait: Duration,
    ) -> Result<Option<Self>, ProviderError> {
        let path = probe_dir.join(CLAUDE_PROBE_LOCK_FILE);
        let file = match std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
        {
            Ok(file) => file,
            Err(err) => {
                tracing::debug!(error = %err, "Claude probe lock file unavailable; continuing unlocked");
                return Ok(None);
            }
        };
        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Some(Self(file))),
                Err(std::fs::TryLockError::WouldBlock) => {}
                Err(std::fs::TryLockError::Error(err)) => {
                    tracing::debug!(error = %err, "Claude probe lock unavailable; continuing unlocked");
                    return Ok(None);
                }
            }
            if Instant::now() >= deadline {
                return Err(ProviderError::Other(
                    "Timed out waiting for another CodexBar process to finish its Claude CLI \
                     usage probe."
                        .to_string(),
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for ClaudeProbeLock {
    fn drop(&mut self) {
        // Best-effort unlock; closing the handle releases it anyway.
        let _unlocked = self.0.unlock();
    }
}

fn claude_usage_probe_dir() -> Result<std::path::PathBuf, ProviderError> {
    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| {
            ProviderError::Other("Could not resolve a local data directory".to_string())
        })?;
    let dir = base.join("CodexBar").join("claude-usage-probe");
    std::fs::create_dir_all(&dir).map_err(|e| {
        ProviderError::Other(format!(
            "Failed to prepare Claude CLI probe directory: {}",
            e
        ))
    })?;
    Ok(dir)
}

/// Persist and reuse one probe session id so repeated `/usage` PTY launches do
/// not register a fresh empty Claude account session each refresh (upstream #2263).
fn load_or_create_probe_session_id(probe_dir: &std::path::Path) -> String {
    let path = probe_dir.join(CLAUDE_PROBE_SESSION_ID_FILE);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let trimmed = raw.trim();
        if uuid::Uuid::parse_str(trimmed).is_ok() {
            return trimmed.to_ascii_lowercase();
        }
    }
    let id = uuid::Uuid::new_v4().to_string().to_ascii_lowercase();
    if let Err(err) = std::fs::write(&path, &id) {
        tracing::debug!(error = %err, "failed to persist Claude probe session id");
    }
    id
}

/// Claude treats `--session-id` as create-only when a local transcript JSONL
/// already exists for that id. Clear probe-dir jsonl leftovers before reuse.
fn cleanup_probe_session_jsonl(probe_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(probe_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            // Best-effort cleanup: a locked or missing probe file just stays.
            let _removed = std::fs::remove_file(&path);
        }
    }
    cleanup_probe_transcript(probe_dir);
}

/// Claude stores the transcript for a working directory under
/// `<config dir>/projects/<sanitized cwd>/<session-id>.jsonl`, where the
/// config dir is `CLAUDE_CONFIG_DIR` or `~/.claude`. Remove the probe session
/// transcripts there, otherwise the fixed `--session-id` fails with "already
/// in use" on the next run.
fn cleanup_probe_transcript(probe_dir: &std::path::Path) {
    let Ok(config_dir) = accounts::config_dir() else {
        return;
    };
    cleanup_probe_transcripts_in(&config_dir.join("projects"), probe_dir);
}

fn cleanup_probe_transcripts_in(projects_root: &std::path::Path, probe_dir: &std::path::Path) {
    let project_dir = projects_root.join(claude_project_dir_name(probe_dir));
    let Ok(entries) = std::fs::read_dir(&project_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if is_file && path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            // Best-effort cleanup: a locked transcript just stays.
            let _removed = std::fs::remove_file(&path);
        }
    }
    // Succeeds only when nothing else is left in the probe's project dir.
    let _removed = std::fs::remove_dir(&project_dir);
}

/// Longest project directory name Claude Code writes before it truncates the
/// name and appends a hash of the full path.
const CLAUDE_PROJECT_DIR_NAME_MAX: usize = 200;

/// Claude Code's project directory name for a working directory: every UTF-16
/// code unit that is not an ASCII letter or digit becomes `-`
/// (`C:\Users\x` -> `C--Users-x`), and long names are cut to 200 characters
/// plus `-<base36 hash of the path>`. (Claude Code also NFC-normalizes the
/// path first; Windows paths are normally NFC already.)
fn claude_project_dir_name(dir: &std::path::Path) -> String {
    let path = dir.to_string_lossy();
    let sanitized: String = path
        .encode_utf16()
        .map(|unit| match u8::try_from(unit) {
            Ok(byte) if byte.is_ascii_alphanumeric() => char::from(byte),
            _ => '-',
        })
        .collect();
    if sanitized.len() <= CLAUDE_PROJECT_DIR_NAME_MAX {
        return sanitized;
    }
    format!(
        "{}-{}",
        &sanitized[..CLAUDE_PROJECT_DIR_NAME_MAX],
        javascript_hash_base36(&path)
    )
}

/// `Math.abs(hash).toString(36)` of the JavaScript string hash
/// `hash = (hash << 5) - hash + charCode`, kept in 32 bits.
fn javascript_hash_base36(text: &str) -> String {
    let hash = text.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    let mut magnitude = i64::from(hash).unsigned_abs();
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit((magnitude % 36) as u32, 36).unwrap_or('0'));
        magnitude /= 36;
        if magnitude == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

/// Arguments shared by every Claude CLI `/usage` probe.
///
/// The remote-control startup hook can otherwise change the interactive
/// session before the usage command is collected. Keep this override in one
/// helper so future CLI probe paths cannot silently omit it.
fn claude_usage_settings_args() -> [String; 2] {
    [
        "--settings".to_string(),
        // Issue #778 (Claude Code 2.1.293): `tui: default` stops the "Try the new
        // fullscreen renderer?" offer. Older builds ignore unknown settings keys.
        r#"{"remoteControlAtStartup":false,"tui":"default"}"#.to_string(),
    ]
}

fn claude_probe_launch_args(session_id: &str) -> Vec<String> {
    let mut args = vec![
        "--setting-sources".to_string(),
        "user".to_string(),
        "--allowed-tools".to_string(),
        String::new(),
    ];
    args.extend(claude_usage_settings_args());
    args.extend(["--session-id".to_string(), session_id.to_string()]);
    args
}

struct ClaudePtyProbeOptions {
    script: &'static str,
    timeout_secs: f64,
    idle_timeout_secs: Option<f64>,
    initial_delay_secs: f64,
    script_char_delay_secs: f64,
    script_line_delay_secs: f64,
    screen_responder: Option<crate::cli::tty_responder::ScreenResponder>,
    /// Re-type the script at these offsets while no done marker is visible.
    script_retry_delays_secs: &'static [f64],
    script_done_substrings: &'static [&'static str],
    script_echo_substrings: &'static [&'static str],
    /// Idle window after the done marker appeared (trailing output only).
    idle_timeout_after_done_secs: Option<f64>,
    /// Reuse a recent usage screen another process stored, and share this
    /// one (the `/usage` probe only, never the trust preflight).
    share_output: bool,
}

/// Offsets (seconds after launch) at which `/usage` is re-sent when Claude's
/// input widget was not ready for the first attempt. Claude Code needs roughly
/// 1-5 s to mount its prompt on Windows, and keystrokes before that are lost.
const CLAUDE_USAGE_RETRY_DELAYS_SECS: &[f64] = &[6.0, 9.5, 14.0];
/// Output markers that prove `/usage` opened (limits view or activity stats).
const CLAUDE_USAGE_DONE_MARKERS: &[&str] = &[
    "current session",
    "current week",
    "total duration",
    "favorite model:",
    "total tokens:",
];
/// The typed command as Claude echoes it into its prompt line. While this is
/// visible the first attempt is still being processed, so do not type again.
const CLAUDE_USAGE_ECHO_MARKERS: &[&str] = &["❯ /usage", "> /usage", "/usage show session cost"];

async fn run_claude_usage_pty_probe(
    claude_path: std::path::PathBuf,
    working_directory: std::path::PathBuf,
) -> Result<String, ProviderError> {
    run_claude_pty_probe(
        claude_path,
        working_directory,
        ClaudePtyProbeOptions {
            script: "/usage",
            timeout_secs: 24.0,
            idle_timeout_secs: Some(6.0),
            initial_delay_secs: 3.0,
            script_char_delay_secs: 0.04,
            script_line_delay_secs: 0.0,
            screen_responder: None,
            script_retry_delays_secs: CLAUDE_USAGE_RETRY_DELAYS_SECS,
            script_done_substrings: CLAUDE_USAGE_DONE_MARKERS,
            script_echo_substrings: CLAUDE_USAGE_ECHO_MARKERS,
            idle_timeout_after_done_secs: Some(1.5),
            share_output: true,
        },
    )
    .await
}

async fn run_claude_trust_preflight(
    claude_path: std::path::PathBuf,
    working_directory: std::path::PathBuf,
) -> Result<String, ProviderError> {
    run_claude_pty_probe(
        claude_path,
        working_directory,
        ClaudePtyProbeOptions {
            script: "",
            timeout_secs: 15.0,
            idle_timeout_secs: Some(4.0),
            initial_delay_secs: 0.6,
            script_char_delay_secs: 0.0,
            script_line_delay_secs: 0.0,
            screen_responder: Some(trust_dialog::TRUST_RESPONDER),
            script_retry_delays_secs: &[],
            script_done_substrings: &[],
            script_echo_substrings: &[],
            idle_timeout_after_done_secs: None,
            share_output: false,
        },
    )
    .await
}

fn resolve_claude_cli_path() -> Result<std::path::PathBuf, ProviderError> {
    locate_claude_binary().ok_or_else(|| {
        ProviderError::NotInstalled(
            "Claude CLI not found. Install from https://docs.claude.ai/claude-code".to_string(),
        )
    })
}

async fn fetch_claude_cli_usage_text(
    claude_path: std::path::PathBuf,
) -> Result<String, ProviderError> {
    let probe_dir = claude_usage_probe_dir()?;
    if let Some(login) = claude_login_fingerprint()
        && let Some(cached) = load_cached_probe_output(&probe_dir, &login)
    {
        return Ok(cached);
    }
    let combined = run_claude_usage_pty_probe(claude_path.clone(), probe_dir.clone()).await?;
    if !is_workspace_trust_prompt(&cli_screen::render(&combined, true).to_lowercase()) {
        return Ok(combined);
    }

    run_claude_trust_preflight(claude_path.clone(), probe_dir.clone()).await?;
    run_claude_usage_pty_probe(claude_path, probe_dir).await
}

/// Windows launch failures the CLI prints instead of a usage screen, as
/// (lowercase marker, user-facing message).
const CLAUDE_CLI_ENVIRONMENT_ERRORS: &[(&str, &str)] = &[
    (
        "requires git-bash",
        "Claude CLI requires Git Bash on Windows. Install Git for Windows or set \
         CLAUDE_CODE_GIT_BASH_PATH to your bash.exe path.",
    ),
    (
        "running scripts is disabled",
        "Claude CLI could not start because PowerShell script execution is disabled. \
         Use claude.cmd or adjust the execution policy.",
    ),
    (
        "cannot run a document in the middle of a pipeline",
        "Claude CLI resolved to a Unix shell script on Windows. Reinstall Claude Code or \
         ensure claude.cmd is first on PATH.",
    ),
];

/// Auth markers are checked before the environment markers.
fn claude_cli_error_from_output(output: &str) -> Option<ProviderError> {
    let lowered = output.to_lowercase();
    if lowered.contains("not logged in") || lowered.contains("login required") {
        return Some(ProviderError::AuthRequired);
    }
    if lowered.contains("token expired") || lowered.contains("token_expired") {
        return Some(ProviderError::OAuthExpired(
            "Token expired. Run `claude login` to refresh.".to_string(),
        ));
    }
    if lowered.contains("authentication_error") {
        return Some(ProviderError::OAuth(
            "Authentication error. Run `claude login`.".to_string(),
        ));
    }
    CLAUDE_CLI_ENVIRONMENT_ERRORS
        .iter()
        .find(|(marker, _)| lowered.contains(marker))
        .map(|(_, message)| ProviderError::Other((*message).to_string()))
}

/// Environment overrides for passive Claude CLI PTY probes.
fn claude_passive_probe_env(
    mut base: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    // Passive status/usage probes must not mutate or update the user's Claude CLI installation.
    base.insert("NO_COLOR".to_string(), "1".to_string());
    base.insert("DISABLE_AUTOUPDATER".to_string(), "1".to_string());
    // Issue #778 (Claude Code 2.1.293): stops the "Claude in Chrome extension
    // detected" offer, which would swallow `/usage`. Claude Code reads this
    // variable; older builds ignore it. Probe launch only; config is untouched.
    base.insert("CLAUDE_CODE_ENABLE_CFC".to_string(), "0".to_string());
    base
}

async fn run_claude_pty_probe(
    claude_path: std::path::PathBuf,
    working_directory: std::path::PathBuf,
    probe: ClaudePtyProbeOptions,
) -> Result<String, ProviderError> {
    tokio::task::spawn_blocking(move || {
        // Keep ownership in the worker: cancelling the async refresh does not
        // stop spawn_blocking or its CLI process from rotating credentials.
        let _account_operation = accounts::CREDENTIAL_OPERATION.blocking_lock();
        let login: Option<&dyn Fn() -> Option<String>> =
            probe.share_output.then_some(&claude_login_fingerprint);
        run_locked_probe(&working_directory, login, || {
            cleanup_probe_session_jsonl(&working_directory);
            let session_id = load_or_create_probe_session_id(&working_directory);
            let env = claude_passive_probe_env(TtyCommandRunner::enriched_environment());

            let mut options = TtyCommandOptions::new()
                .with_timeout(probe.timeout_secs)
                .with_initial_delay(probe.initial_delay_secs)
                .with_script_char_delay(probe.script_char_delay_secs)
                .with_script_line_delay(probe.script_line_delay_secs)
                .with_working_directory(working_directory.clone())
                .with_extra_args(claude_probe_launch_args(&session_id));
            if let Some(idle) = probe.idle_timeout_secs {
                options = options.with_idle_timeout(idle);
            }
            if let Some(idle) = probe.idle_timeout_after_done_secs {
                options = options.with_idle_timeout_after_done(idle);
            }
            if let Some(responder) = probe.screen_responder {
                options = options.with_screen_responder(responder);
            }
            if !probe.script_retry_delays_secs.is_empty() {
                options = options.with_script_retries(
                    probe.script_retry_delays_secs.to_vec(),
                    probe
                        .script_done_substrings
                        .iter()
                        .map(|marker| (*marker).to_string())
                        .collect(),
                    probe
                        .script_echo_substrings
                        .iter()
                        .map(|marker| (*marker).to_string())
                        .collect(),
                );
            }
            options.env = env.into();

            TtyCommandRunner::new()
                .run(&claude_path.to_string_lossy(), probe.script, options)
                .map(|result| result.text)
                .map_err(|error| match error {
                    crate::cli::tty_runner::TtyCommandError::TimedOut => ProviderError::Timeout,
                    other => ProviderError::Other(format!("Claude CLI failed: {}", other)),
                })
        })
    })
    .await
    .map_err(|e| ProviderError::Other(format!("Claude CLI probe failed: {}", e)))?
}

/// Run one probe under the cross-process probe lock. `login` is set when the
/// screen may be shared and identifies the Claude login it belongs to: a
/// fresh screen another process stored for that login while this one waited
/// is reused, and a parseable screen is stored for the others unless the
/// login changed while the probe ran.
fn run_locked_probe(
    probe_dir: &std::path::Path,
    login: Option<&dyn Fn() -> Option<String>>,
    probe: impl FnOnce() -> Result<String, ProviderError>,
) -> Result<String, ProviderError> {
    let _probe_lock = ClaudeProbeLock::acquire(probe_dir)?;
    let before = login.and_then(|login| login());
    if let Some(before) = &before
        && let Some(cached) = load_cached_probe_output(probe_dir, before)
    {
        return Ok(cached);
    }
    let output = probe()?;
    if let (Some(before), Some(login)) = (&before, login)
        && login().as_ref() == Some(before)
        && claude_cli_output_is_shareable(&output)
    {
        store_cached_probe_output(probe_dir, before, &output);
    }
    Ok(output)
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

    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn retains_last_good_on_transport_failure(&self) -> bool {
        true
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto => self
                .fetch_via_auto(ctx)
                .await
                .map(auto_precision::finish_auto_result),
            SourceMode::OAuth => self.fetch_via_oauth(ctx).await,
            SourceMode::Web => self.fetch_via_web(ctx).await,
            SourceMode::Cli => match self.fetch_via_cli(ctx).await {
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
            return Ok(result);
        }

        // Upstream 0.50.1 #2516: track whether OAuth failed with a revocation.
        let oauth_result = self.fetch_via_oauth(ctx).await;
        let use_cli_cache = oauth_result
            .as_ref()
            .err()
            .is_some_and(oauth_failure_uses_cli_cache);
        if let Some(result) = record_auto_source(&mut failures, "OAuth", oauth_result)? {
            return Ok(result);
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

        if let Some(mut result) =
            record_auto_source(&mut failures, "CLI", self.fetch_via_cli(ctx).await)?
        {
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

        // Check for manual cookie header first
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
    ) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Attempting CLI probe for Claude");

        let claude_path = resolve_claude_cli_path()?;
        let combined = fetch_claude_cli_usage_text(claude_path).await?;
        // Replay cursor redraws once; rendering is idempotent on rendered text.
        let visible = cli_screen::render(&combined, true);
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
        let clean = cli_screen::render(output, true);
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
            return Err(ProviderError::Parse(
                "Claude CLI did not return usage data".to_string(),
            ));
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

/// Locate the Claude CLI for shell integrations that need to reopen a session.
pub fn locate_claude_binary() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("CLAUDE_BINARY")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }

    #[cfg(windows)]
    {
        let candidates = [
            // Direct install
            dirs::data_local_dir().map(|p| p.join("Programs").join("claude").join("claude.exe")),
            // npm global (AppData\Roaming\npm)
            dirs::data_local_dir().map(|p| p.join("npm").join("claude.cmd")),
            dirs::home_dir().map(|h| {
                h.join("AppData")
                    .join("Roaming")
                    .join("npm")
                    .join("claude.cmd")
            }),
            // npm global alternate (~\.npm-global)
            dirs::home_dir().map(|h| h.join(".npm-global").join("claude.cmd")),
            // Volta managed
            dirs::data_local_dir().map(|p| {
                p.join("Volta")
                    .join("tools")
                    .join("image")
                    .join("packages")
                    .join("@anthropic-ai")
                    .join("claude-code")
                    .join("bin")
                    .join("claude.cmd")
            }),
            // fnm managed (via shim)
            dirs::data_local_dir().map(|p| p.join("fnm_multishells").join("claude.cmd")),
            // PATH lookup
            find_windows_claude_in_path(),
        ];

        candidates.into_iter().flatten().find(|p| p.exists())
    }

    #[cfg(not(windows))]
    {
        which::which("claude").ok()
    }
}

#[cfg(windows)]
fn find_windows_claude_in_path() -> Option<std::path::PathBuf> {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut command = StdCommand::new("where");
    command
        .arg("claude")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let output = command.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let mut matches: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect();

    matches.sort_by_key(|path| {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("cmd") => 0,
            Some("bat") => 1,
            Some("exe") => 2,
            _ => 3,
        }
    });

    matches.into_iter().find(|path| path.exists())
}

/// Detect the version of the claude CLI
fn detect_claude_version() -> Option<String> {
    let claude_path = locate_claude_binary()?;

    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new(claude_path);
    cmd.args(["--version"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().ok()?;

    if output.status.success() {
        let version_str = String::from_utf8_lossy(&output.stdout);
        super::extract_semver(&version_str)
    } else {
        None
    }
}

fn is_non_interactive_slash_command_response(text: &str) -> bool {
    let mentions_usage_and_exit = text.contains("/usage") && text.contains("/exit");
    let says_entered_commands =
        text.contains("i see you've entered") || text.contains("you've entered two slash commands");
    let says_no_slash_command = text.contains("available custom slash commands")
        && text.contains("don't see these commands");
    let says_usage_is_cli_only = text
        .contains("token usage and statistics are typically displayed by the cli interface")
        || text.contains("i don't have direct access to those metrics");

    mentions_usage_and_exit
        && (says_entered_commands || says_no_slash_command || says_usage_is_cli_only)
}

fn is_workspace_trust_prompt(text: &str) -> bool {
    text.contains("quick safety check")
        && text.contains("trust this folder")
        && text.contains("yes, i trust this folder")
}

/// Current `/usage` panels show a local session summary above the plan limits,
/// so the presence of a limit section outranks the activity-stats markers.
fn has_plan_limit_section(text: &str) -> bool {
    text.contains("current session") || text.contains("current week")
}

fn is_cli_activity_stats_response(text: &str) -> bool {
    let has_activity_overview = text.contains("favorite model:") || text.contains("total tokens:");
    let has_session_cost_summary =
        text.contains("total duration") && text.contains("usage:") && text.contains("cache read");

    has_activity_overview || has_session_cost_summary
}

/// The weekly heading Claude prints, newest wording first.
const WEEKLY_LABELS: [&str; 2] = ["current week (all models)", "current week"];

/// First match of `find` in any section headed by `label`; each section is
/// scanned for at most `max_lines` lines, label line included.
fn find_near_label<T>(
    text: &str,
    label: &str,
    max_lines: usize,
    mut find: impl FnMut(&str) -> Option<T>,
) -> Option<T> {
    let label_normalized = normalized_for_label_search(label);
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| normalized_for_label_search(line).contains(&label_normalized))
        .find_map(|(idx, _)| {
            label_section(&lines, idx, &label_normalized, max_lines).find_map(&mut find)
        })
}

/// Percentage near a label (e.g. "Current session"), as "used".
fn extract_percent_near_label(text: &str, label: &str) -> Option<f64> {
    find_near_label(text, label, 12, parse_percent_line)
}

fn is_exhausted_short_form(clean_lower: &str) -> bool {
    clean_lower.contains("out of extra usage") || clean_lower.contains("hit your limit")
}

/// Extract email address from text
fn extract_email(text: &str) -> Option<String> {
    // Try explicit patterns first
    let patterns = [
        r"Account:\s*([^\s@]+@[^\s@]+\.[^\s]+)",
        r"Email:\s*([^\s@]+@[^\s@]+\.[^\s]+)",
        r"([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})",
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern)
            && let Some(caps) = re.captures(text)
            && let Some(m) = caps.get(1)
        {
            return Some(m.as_str().trim().to_string());
        }
    }

    None
}

/// Extract login method / plan name from text
fn extract_login_method(text: &str) -> Option<String> {
    // Look for explicit "Login method:" line
    if let Ok(re) = Regex::new(r"(?i)login\s+method:\s*(.+)")
        && let Some(caps) = re.captures(text)
        && let Some(m) = caps.get(1)
    {
        let method = m.as_str().trim();
        if !method.is_empty() {
            return Some(clean_plan_name(method));
        }
    }

    // Look for "Claude <plan>" patterns
    if let Ok(re) = Regex::new(r"(?i)(claude\s+(?:max|pro|ultra|team|free)[a-z0-9\s._-]*)")
        && let Some(caps) = re.captures(text)
        && let Some(m) = caps.get(1)
    {
        let plan = m.as_str().trim();
        if !plan.to_lowercase().contains("code") {
            return Some(clean_plan_name(plan));
        }
    }

    None
}

/// Reset text near a label, from "resets" to the end of its line.
fn extract_reset_description(text: &str, label: &str) -> Option<String> {
    find_near_label(text, label, 14, extract_inline_reset_description)
}

/// Extract a "resets ..." suffix from a short single-line status.
fn extract_inline_reset_description(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let pos = lower.find("resets")?;
    Some(text[pos..].trim().to_string())
}

/// Clean up a plan name from rendered (escape-free) text: drop bracketed
/// codes like `[22m` and trim.
fn clean_plan_name(text: &str) -> String {
    let re = Regex::new(r"\[\d+m").unwrap_or_else(|_| Regex::new(".^").unwrap());
    let result = re.replace_all(text, "");
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use std::collections::HashMap;

    use super::*;

    const LOGIN_A: &str = "login-a";

    fn login_a() -> Option<String> {
        Some(LOGIN_A.to_string())
    }

    #[test]
    fn probe_cache_roundtrip_and_expiry() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_cached_probe_output(dir.path(), LOGIN_A).is_none());
        store_cached_probe_output(dir.path(), LOGIN_A, "Current session 12% used");
        assert_eq!(
            load_cached_probe_output(dir.path(), LOGIN_A).as_deref(),
            Some("Current session 12% used")
        );
        let stale = ClaudeProbeCache {
            captured_at_unix: unix_now_secs() - CLAUDE_PROBE_CACHE_TTL.as_secs() - 5,
            login: LOGIN_A.to_string(),
            output: "Current session 12% used".to_string(),
        };
        std::fs::write(
            dir.path().join(CLAUDE_PROBE_CACHE_FILE),
            serde_json::to_string(&stale).unwrap(),
        )
        .unwrap();
        assert!(load_cached_probe_output(dir.path(), LOGIN_A).is_none());
    }

    #[test]
    fn probe_cache_is_never_shared_with_another_login() {
        let dir = tempfile::tempdir().unwrap();
        store_cached_probe_output(dir.path(), LOGIN_A, "Current session 12% used");
        assert!(load_cached_probe_output(dir.path(), "login-b").is_none());

        // Written before screens were scoped to a login.
        let unscoped = format!(
            r#"{{"captured_at_unix":{},"output":"Current session 12% used"}}"#,
            unix_now_secs()
        );
        std::fs::write(dir.path().join(CLAUDE_PROBE_CACHE_FILE), unscoped).unwrap();
        assert!(load_cached_probe_output(dir.path(), LOGIN_A).is_none());
        assert!(load_cached_probe_output(dir.path(), "").is_none());
    }

    #[test]
    fn login_fingerprint_follows_credential_rewrites_without_reading_them() {
        let dir = tempfile::tempdir().unwrap();
        let credentials = dir.path().join(".credentials.json");
        assert_eq!(login_fingerprint_at(&credentials), None);

        std::fs::write(&credentials, "{}").unwrap();
        let first = login_fingerprint_at(&credentials).expect("fingerprint");
        assert_eq!(login_fingerprint_at(&credentials).as_ref(), Some(&first));
        assert!(!first.contains(".credentials"), "only a digest is stored");

        std::fs::write(&credentials, r#"{"another":"login"}"#).unwrap();
        assert_ne!(login_fingerprint_at(&credentials), Some(first));
    }

    const SHAREABLE_USAGE_SCREEN: &str = "Current session\n\
        ████████▌ 17% used\n\
        Resets 12pm (America/Bogota)\n";

    #[test]
    fn locked_probe_reuses_a_screen_stored_while_it_waited() {
        let dir = tempfile::tempdir().unwrap();
        store_cached_probe_output(dir.path(), LOGIN_A, SHAREABLE_USAGE_SCREEN);

        let output = run_locked_probe(dir.path(), Some(&login_a), || {
            panic!("a fresh shared screen must not launch another probe")
        })
        .unwrap();
        assert_eq!(output, SHAREABLE_USAGE_SCREEN);
    }

    #[test]
    fn locked_probe_shares_only_parseable_usage_screens() {
        let dir = tempfile::tempdir().unwrap();
        let output = run_locked_probe(dir.path(), Some(&login_a), || {
            Ok("Not logged in".to_string())
        });
        assert_eq!(output.unwrap(), "Not logged in");
        assert!(load_cached_probe_output(dir.path(), LOGIN_A).is_none());

        let output = run_locked_probe(dir.path(), Some(&login_a), || {
            Ok(SHAREABLE_USAGE_SCREEN.into())
        });
        assert_eq!(output.unwrap(), SHAREABLE_USAGE_SCREEN);
        assert_eq!(
            load_cached_probe_output(dir.path(), LOGIN_A).as_deref(),
            Some(SHAREABLE_USAGE_SCREEN)
        );
        assert!(
            !dir.path()
                .read_dir()
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().contains(".tmp-")),
            "the atomic write left no staging file behind"
        );
    }

    #[test]
    fn locked_probe_keeps_a_screen_private_when_the_login_changed_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let calls = std::cell::Cell::new(0);
        let switching_login = || {
            calls.set(calls.get() + 1);
            Some(format!("login-{}", calls.get()))
        };
        let output = run_locked_probe(dir.path(), Some(&switching_login), || {
            Ok(SHAREABLE_USAGE_SCREEN.into())
        });
        assert_eq!(output.unwrap(), SHAREABLE_USAGE_SCREEN);
        assert_eq!(calls.get(), 2, "the login is read before and after");
        assert!(load_cached_probe_output(dir.path(), "login-1").is_none());
        assert!(load_cached_probe_output(dir.path(), "login-2").is_none());

        let no_login = || None;
        run_locked_probe(dir.path(), Some(&no_login), || {
            Ok(SHAREABLE_USAGE_SCREEN.into())
        })
        .unwrap();
        assert!(!dir.path().join(CLAUDE_PROBE_CACHE_FILE).exists());
    }

    #[test]
    fn unshared_probe_neither_reuses_nor_stores_screens() {
        let dir = tempfile::tempdir().unwrap();
        store_cached_probe_output(dir.path(), LOGIN_A, SHAREABLE_USAGE_SCREEN);
        let output = run_locked_probe(dir.path(), None, || Ok("trust preflight".into()));
        assert_eq!(output.unwrap(), "trust preflight");

        let other = tempfile::tempdir().unwrap();
        run_locked_probe(other.path(), None, || Ok(SHAREABLE_USAGE_SCREEN.into())).unwrap();
        assert!(!other.path().join(CLAUDE_PROBE_CACHE_FILE).exists());
    }

    #[test]
    fn logged_probe_screen_masks_account_email_and_secrets() {
        let screen = "Login: someone@example.com (Claude Max)\n\
                      access_token=abcdef0123456789 sk-ant-abcdefgh12345678\n\
                      Current session 12% used";
        let logged = redacted_probe_screen(screen);
        assert!(!logged.contains("someone@example.com"), "{logged}");
        assert!(!logged.contains("abcdef0123456789"), "{logged}");
        assert!(!logged.contains("sk-ant-abcdefgh12345678"), "{logged}");
        assert!(logged.contains("Current session 12% used"));
    }

    #[test]
    fn probe_lock_wait_expiry_fails_instead_of_probing_alongside() {
        let dir = tempfile::tempdir().unwrap();
        let held = ClaudeProbeLock::acquire_within(dir.path(), Duration::ZERO)
            .expect("first lock")
            .expect("file locking is supported");

        let error = match ClaudeProbeLock::acquire_within(dir.path(), Duration::from_millis(300)) {
            Ok(lock) => panic!("second lock acquired while held: {}", lock.is_some()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("Timed out waiting"), "{error}");
        assert_eq!(
            last_good_failure_policy_for_error(&error.to_string()),
            LastGoodFailurePolicy::Preserve
        );

        drop(held);
        assert!(
            ClaudeProbeLock::acquire_within(dir.path(), Duration::ZERO)
                .expect("lock after release")
                .is_some()
        );
    }

    #[test]
    fn passive_probe_env_disables_autoupdater_and_color() {
        let env = claude_passive_probe_env(HashMap::new());
        assert_eq!(
            env.get("DISABLE_AUTOUPDATER").map(String::as_str),
            Some("1")
        );
        assert_eq!(env.get("NO_COLOR").map(String::as_str), Some("1"));
    }

    #[test]
    fn probe_avoids_chrome_and_fullscreen_startup_dialogs() {
        let env = claude_passive_probe_env(HashMap::new());
        assert_eq!(
            env.get("CLAUDE_CODE_ENABLE_CFC").map(String::as_str),
            Some("0")
        );
        let settings: serde_json::Value =
            serde_json::from_str(&claude_usage_settings_args()[1]).unwrap();
        assert_eq!(settings["tui"], "default");
        assert_eq!(settings["remoteControlAtStartup"], false);
    }

    #[test]
    fn probe_session_id_is_reused_from_probe_directory() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create_probe_session_id(dir.path());
        let second = load_or_create_probe_session_id(dir.path());
        assert_eq!(first, second);
        assert!(uuid::Uuid::parse_str(&first).is_ok());
        let args = claude_probe_launch_args(&first);
        // Positional structure only: the settings pair is pinned once by
        // `claude_usage_settings_args` being the sole composer.
        assert_eq!(
            args[..4],
            ["--setting-sources", "user", "--allowed-tools", ""]
        );
        assert_eq!(args[4], claude_usage_settings_args()[0]);
        assert_eq!(args[5], claude_usage_settings_args()[1]);
        assert_eq!(args[6], "--session-id");
        assert_eq!(args[7], first);
    }

    #[test]
    fn usage_probe_settings_disable_remote_control_startup() {
        assert_eq!(
            claude_usage_settings_args(),
            [
                "--settings".to_string(),
                r#"{"remoteControlAtStartup":false,"tui":"default"}"#.to_string(),
            ]
        );
    }

    #[test]
    fn probe_session_jsonl_cleanup_removes_transcript_files() {
        let dir = tempfile::tempdir().unwrap();
        let jsonl = dir.path().join("session.jsonl");
        std::fs::write(&jsonl, "{}").unwrap();
        std::fs::write(dir.path().join("keep.txt"), "x").unwrap();
        cleanup_probe_session_jsonl(dir.path());
        assert!(!jsonl.exists());
        assert!(dir.path().join("keep.txt").exists());
    }

    #[test]
    fn probe_project_dir_name_matches_claude_code() {
        use std::path::Path;
        assert_eq!(
            claude_project_dir_name(Path::new(
                r"C:\Users\user\AppData\Local\CodexBar\claude-usage-probe"
            )),
            "C--Users-user-AppData-Local-CodexBar-claude-usage-probe"
        );
        assert_eq!(
            claude_project_dir_name(Path::new("/Users/me/Library/Application Support/x")),
            "-Users-me-Library-Application-Support-x"
        );
        // One dash per UTF-16 code unit, so two for a character outside the BMP.
        assert_eq!(
            claude_project_dir_name(Path::new("C:\\Users\\J\u{f6}rg\u{1F600}\\probe")),
            "C--Users-J-rg---probe"
        );
        // Reference values from Claude Code's JavaScript implementation.
        let long = format!(
            r"C:\Users\user\AppData\Local\{}claude-usage-probe",
            r"deep\".repeat(40)
        );
        assert_eq!(
            claude_project_dir_name(Path::new(&long)),
            format!(
                "C--Users-user-AppData-Local-{}de-ttzy4x",
                "deep-".repeat(34)
            )
        );
        assert_eq!(javascript_hash_base36("hello"), "1n1e4y");
        assert_eq!(javascript_hash_base36(""), "0");
    }

    #[test]
    fn probe_transcript_cleanup_stays_inside_the_probe_project() {
        use std::path::Path;
        let projects = tempfile::tempdir().unwrap();
        let other = projects.path().join("C--work-repo");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("session.jsonl"), "{}").unwrap();

        let busy_probe = Path::new(r"C:\Users\user\AppData\Local\CodexBar\busy-probe");
        let busy = projects.path().join(claude_project_dir_name(busy_probe));
        std::fs::create_dir_all(busy.join("folder.jsonl")).unwrap();
        std::fs::write(busy.join("session.jsonl"), "{}").unwrap();
        std::fs::write(busy.join("notes.txt"), "x").unwrap();
        cleanup_probe_transcripts_in(projects.path(), busy_probe);
        assert!(!busy.join("session.jsonl").exists());
        assert!(busy.join("notes.txt").exists());
        assert!(busy.join("folder.jsonl").is_dir(), "only files are removed");

        let probe = Path::new(r"C:\Users\user\AppData\Local\CodexBar\claude-usage-probe");
        let project = projects.path().join(claude_project_dir_name(probe));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("a.jsonl"), "{}").unwrap();
        std::fs::write(project.join("b.jsonl"), "{}").unwrap();
        cleanup_probe_transcripts_in(projects.path(), probe);
        assert!(!project.exists(), "an emptied probe project dir is removed");

        assert!(other.join("session.jsonl").exists(), "other projects stay");
    }

    #[test]
    fn parses_current_cli_usage_screen() {
        let provider = ClaudeProvider::new();
        let output = r#"
Status   Config   Usage

  Current session
  ██████████████████████████████████████████████████ 100% used
  Resets 12pm (America/Bogota)

  Current week (all models)
  ████████████████████████▌                          49% used
  Resets Apr 3, 2pm (America/Bogota)

  Extra usage
  ██▍                                                4% used
  $3.31 / $70.00 spent · Resets Apr 1 (America/Bogota)
"#;

        let result = provider.parse_cli_output(output).expect("should parse");

        assert_eq!(result.source_label, "cli");
        assert_eq!(result.usage.primary.used_percent, 100.0);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Resets 12pm (America/Bogota)")
        );

        let weekly = result
            .usage
            .secondary
            .expect("weekly usage should be present");
        assert_eq!(weekly.used_percent, 49.0);
        assert_eq!(
            weekly.reset_description.as_deref(),
            Some("Resets Apr 3, 2pm (America/Bogota)")
        );
    }

    #[test]
    fn parses_exhausted_short_form_as_full_session_usage() {
        let provider = ClaudeProvider::new();
        let output = "You're out of extra usage · resets 12pm (America/Bogota)";

        let result = provider.parse_cli_output(output).expect("should parse");

        assert_eq!(result.usage.primary.used_percent, 100.0);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("resets 12pm (America/Bogota)")
        );
    }

    #[test]
    fn parses_hit_limit_short_form_as_full_session_usage() {
        let provider = ClaudeProvider::new();
        let output = "You've hit your limit \u{00b7} resets 3:20pm (Asia/Shanghai)";

        let result = provider.parse_cli_output(output).expect("should parse");

        assert_eq!(result.usage.primary.used_percent, 100.0);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("resets 3:20pm (Asia/Shanghai)")
        );
    }

    #[test]
    fn parses_remaining_available_and_decimal_percentages() {
        let provider = ClaudeProvider::new();
        let output = r#"
Status   Config   Usage

  Current session
  12.5% remaining
  Resets 8pm

  Current week (all models)
  4% available
  Resets Apr 4, 2pm

  Current week (Sonnet only)
  1% consumed
"#;

        let result = provider.parse_cli_output(output).expect("should parse");

        assert_eq!(result.usage.primary.used_percent, 87.5);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Resets 8pm")
        );

        let weekly = result
            .usage
            .secondary
            .expect("weekly usage should be present");
        assert_eq!(weekly.used_percent, 96.0);
        assert_eq!(
            weekly.reset_description.as_deref(),
            Some("Resets Apr 4, 2pm")
        );

        let sonnet = result
            .usage
            .extra_rate_windows
            .iter()
            .find(|window| window.id == "claude-weekly-scoped-sonnet")
            .expect("sonnet usage should be present");
        assert_eq!(sonnet.window.used_percent, 1.0);
    }

    #[test]
    fn parses_all_cli_model_scoped_weekly_limits() {
        let provider = ClaudeProvider::new();
        let output = r#"
Current session
10% used
Resets 12pm (America/Bogota)

Current week (all models)
20% used
Resets Apr 3, 2pm (America/Bogota)

Current week (Sonnet only)
30% used
Resets Apr 4, 2pm (America/Bogota)

Current week (Opus only)
40% used
Resets Apr 5, 2pm (America/Bogota)
"#;

        let result = provider.parse_cli_output(output).expect("should parse");

        assert_eq!(result.usage.extra_rate_windows.len(), 2);
        assert_eq!(
            result.usage.extra_rate_windows[0].id,
            "claude-weekly-scoped-sonnet"
        );
        assert_eq!(result.usage.extra_rate_windows[0].title, "Sonnet only");
        assert_eq!(result.usage.extra_rate_windows[0].window.used_percent, 30.0);
        assert_eq!(
            result.usage.extra_rate_windows[1].id,
            "claude-weekly-scoped-opus"
        );
        assert!(result.usage.model_specific.is_none());
    }

    #[test]
    fn scoped_weekly_parser_handles_non_ascii_labels_and_reset_prefixes() {
        let now = "2026-04-02T18:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let limits = extract_cli_scoped_weekly_limits(
            "Current week (A€€)\n10% used\nİResets Apr 3 at 2pm (America/Bogota)",
            now,
        );

        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].title, "A€€");
        assert_eq!(
            limits[0].window.resets_at,
            Some("2026-04-03T19:00:00Z".parse().unwrap())
        );
    }

    #[test]
    fn resolves_cli_reset_occurrences_in_the_reported_timezone() {
        let now = "2026-04-02T18:00:00Z".parse::<DateTime<Utc>>().unwrap();

        assert_eq!(
            parse_claude_reset_date("Resets Apr 3, 2027, 2pm (America/Bogota)", now, None),
            Some("2027-04-03T19:00:00Z".parse().unwrap())
        );
        assert_eq!(
            parse_claude_reset_date("Resets Apr 3, 2pm (America/Bogota)", now, None),
            Some("2026-04-03T19:00:00Z".parse().unwrap())
        );
        assert_eq!(
            parse_claude_reset_date("Resets 12pm (America/Bogota)", now, None),
            Some("2026-04-03T17:00:00Z".parse().unwrap())
        );
        assert_eq!(
            parse_claude_reset_date("ResetsApr3at2pm(America/Bogota)", now, None),
            Some("2026-04-03T19:00:00Z".parse().unwrap())
        );
    }

    #[test]
    fn timezone_less_resets_use_the_supplied_system_zone() {
        let now = "2026-03-07T18:00:00Z".parse::<DateTime<Utc>>().unwrap();

        assert_eq!(
            parse_claude_reset_date_in_system_zone(
                "Resets Mar 8 at 3:30am",
                now,
                None,
                "America/New_York".parse().unwrap(),
            ),
            Some("2026-03-08T07:30:00Z".parse().unwrap())
        );
        assert_eq!(
            parse_claude_reset_date_in_system_zone(
                "Resets Mar 8 at 3:30am (America/Los_Angeles)",
                now,
                None,
                "America/New_York".parse().unwrap(),
            ),
            Some("2026-03-08T10:30:00Z".parse().unwrap())
        );
    }

    #[test]
    fn reset_dates_resolve_every_month_and_form() {
        let now = "2026-09-24T12:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let at = |text: &str, window: Option<u32>| {
            parse_claude_reset_date(text, now, window).map(|date| date.to_rfc3339())
        };
        let months = [
            "Jan", "FEB", "mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "dEc",
        ];
        for (index, month) in months.iter().enumerate() {
            assert_eq!(
                at(&format!("Resets {month} 5, 2027 at 3pm (UTC)"), None),
                Some(format!("2027-{:02}-05T15:00:00+00:00", index + 1)),
                "{month}"
            );
        }
        let rows = [
            ("Resets Foo 5, 2027 at 3pm (UTC)", None, None),
            ("Resets Feb 30, 2027 at 3pm (UTC)", None, None),
            (
                "Resets Sep 23 at 3pm (UTC)",
                None,
                Some("2027-09-23T15:00:00+00:00"),
            ),
            (
                "Resets Sep 23 at 3pm (UTC)",
                Some(10_080),
                Some("2026-09-23T15:00:00+00:00"),
            ),
            (
                "Resets Feb 29 at 3pm (UTC)",
                None,
                Some("2028-02-29T15:00:00+00:00"),
            ),
            ("Resets 3pm (UTC)", None, Some("2026-09-24T15:00:00+00:00")),
            ("Resets 11am (UTC)", None, Some("2026-09-25T11:00:00+00:00")),
            (
                "Resets 11am (UTC)",
                Some(300),
                Some("2026-09-24T11:00:00+00:00"),
            ),
            (
                "Resets Nov 1, 2026 at 1:30am (America/New_York)",
                None,
                Some("2026-11-01T05:30:00+00:00"),
            ),
            (
                "Resets Mar 8, 2026 at 2:30am (America/New_York)",
                None,
                None,
            ),
        ];
        for (text, window, expected) in rows {
            assert_eq!(at(text, window).as_deref(), expected, "{text} {window:?}");
        }
    }

    #[test]
    fn parses_compact_usage_screen() {
        let provider = ClaudeProvider::new();
        let output = r#"
Settings:StatusConfigUsage(tabtocycle)
Loadingusagedata...
Currentsession
6%used
Resets4:29am(Asia/Calcutta)
Currentweek(allmodels)
4%used
ResetsFeb12at1:29pm(Asia/Calcutta)
Currentweek(Sonnetonly)
1%used
ResetsFeb12at1:29pm(Asia/Calcutta)
"#;

        let result = provider.parse_cli_output(output).expect("should parse");

        assert_eq!(result.usage.primary.used_percent, 6.0);
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("Resets4:29am(Asia/Calcutta)")
        );
        assert_eq!(
            result
                .usage
                .secondary
                .expect("weekly usage should be present")
                .used_percent,
            4.0
        );
        let sonnet = result
            .usage
            .extra_rate_windows
            .iter()
            .find(|window| window.id == "claude-weekly-scoped-sonnet")
            .expect("sonnet usage should be present");
        assert_eq!(result.usage.extra_rate_windows.len(), 1);
        assert_eq!(sonnet.title, "Sonnet only");
        assert_eq!(sonnet.window.used_percent, 1.0);
    }

    #[test]
    fn does_not_promote_weekly_reset_to_session() {
        let provider = ClaudeProvider::new();
        let output = r#"
Current session
17% used
Current week (all models)
4% used
Resets Dec 24 at 3:59pm (Europe/Paris)
"#;

        let result = provider.parse_cli_output(output).expect("should parse");

        assert_eq!(result.usage.primary.used_percent, 17.0);
        assert_eq!(result.usage.primary.reset_description, None);
        assert_eq!(
            result
                .usage
                .secondary
                .expect("weekly usage should be present")
                .reset_description
                .as_deref(),
            Some("Resets Dec 24 at 3:59pm (Europe/Paris)")
        );
    }

    #[test]
    fn cli_error_markers_map_to_fixed_errors() {
        let git_bash = "Other(\"Claude CLI requires Git Bash on Windows. Install Git for Windows or set CLAUDE_CODE_GIT_BASH_PATH to your bash.exe path.\")";
        let cases = [
            ("Error: Not Logged In", "AuthRequired"),
            ("login required to continue", "AuthRequired"),
            (
                "TOKEN EXPIRED",
                "OAuthExpired(\"Token expired. Run `claude login` to refresh.\")",
            ),
            (
                "{\"type\":\"token_expired\"}",
                "OAuthExpired(\"Token expired. Run `claude login` to refresh.\")",
            ),
            (
                "authentication_error",
                "OAuth(\"Authentication error. Run `claude login`.\")",
            ),
            ("Claude Code on Windows requires git-bash.", git_bash),
            (
                "Running scripts is disabled on this system",
                "Other(\"Claude CLI could not start because PowerShell script execution is disabled. Use claude.cmd or adjust the execution policy.\")",
            ),
            (
                "Cannot run a document in the middle of a pipeline",
                "Other(\"Claude CLI resolved to a Unix shell script on Windows. Reinstall Claude Code or ensure claude.cmd is first on PATH.\")",
            ),
            // Auth markers win over environment markers.
            ("requires git-bash; not logged in", "AuthRequired"),
            (
                "requires git-bash; token expired",
                "OAuthExpired(\"Token expired. Run `claude login` to refresh.\")",
            ),
            // Login wins over the other auth markers.
            ("token expired; not logged in", "AuthRequired"),
            (
                "authentication_error; token_expired",
                "OAuthExpired(\"Token expired. Run `claude login` to refresh.\")",
            ),
            ("running scripts is disabled; requires git-bash", git_bash),
        ];
        for (output, expected) in cases {
            let error = claude_cli_error_from_output(output).expect(output);
            assert_eq!(format!("{error:?}"), expected, "{output}");
        }
        assert!(claude_cli_error_from_output("Current session 5% used").is_none());
    }

    #[test]
    fn all_percents_fold_case_and_clamp() {
        let text = "50% USED\n20 % Left\n101% used\n150% left\n5.5% remaining\n1000% used\n7%Spent 8% available";
        assert_eq!(
            percent_matches(text).collect::<Vec<_>>(),
            vec![50.0, 80.0, 100.0, 0.0, 94.5, 0.0, 7.0, 92.0]
        );
        assert!(percent_matches("no numbers here").next().is_none());
    }

    #[test]
    fn label_sections_stop_at_their_window_and_the_next_section() {
        let filler = |count: usize| vec!["filler"; count].join("\n");
        // Percent on the label line itself, and on the last line of the
        // twelve-line window (label + 11).
        assert_eq!(
            extract_percent_near_label("Current session 30% used", "current session"),
            Some(30.0)
        );
        let at_last = format!("Current session\n{}\n40% used", filler(10));
        assert_eq!(
            extract_percent_near_label(&at_last, "current session"),
            Some(40.0)
        );
        let past_window = format!("Current session\n{}\n40% used", filler(11));
        assert_eq!(
            extract_percent_near_label(&past_window, "current session"),
            None
        );
        // The next "Current ..." heading ends the section, but the same
        // heading does not.
        let next_section = "Current session\nCurrent week\n40% used";
        assert_eq!(
            extract_percent_near_label(next_section, "current session"),
            None
        );
        let same_label = "Current session\nCURRENT SESSION again\n40% used";
        assert_eq!(
            extract_percent_near_label(same_label, "current session"),
            Some(40.0)
        );
        // A section without a value falls through to a later label line.
        let later = "Current session\nCurrent week\n10% used\nCurrent session\n60% left";
        assert_eq!(
            extract_percent_near_label(later, "current session"),
            Some(40.0)
        );
        assert_eq!(
            extract_percent_near_label("Current week (all models)\n10% used", "current week"),
            Some(10.0)
        );

        // Reset text uses a fourteen-line window (label + 13).
        let reset_last = format!("Current week\n{}\nResets Mon 9am", filler(12));
        assert_eq!(
            extract_reset_description(&reset_last, "current week").as_deref(),
            Some("Resets Mon 9am")
        );
        let reset_past = format!("Current week\n{}\nResets Mon 9am", filler(13));
        assert_eq!(extract_reset_description(&reset_past, "current week"), None);
        assert_eq!(
            extract_reset_description("Current week  5% used · resets Fri 1pm  ", "current week")
                .as_deref(),
            Some("resets Fri 1pm")
        );
        assert_eq!(
            extract_reset_description(
                "Current session\nCurrent week\nResets Mon",
                "current session"
            ),
            None
        );
        let later_reset = "Current session\nCurrent week\nCurrent session\nResets 5pm";
        assert_eq!(
            extract_reset_description(later_reset, "current session").as_deref(),
            Some("Resets 5pm")
        );
    }

    #[test]
    fn scoped_weekly_sections_use_a_fourteen_line_window() {
        let now = Utc::now();
        let filler = |count: usize| vec!["filler"; count].join("\n");
        let inside = format!("Current week (Opus)\n{}\n25% used", filler(12));
        let limits = extract_cli_scoped_weekly_limits(&inside, now);
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].window.used_percent, 25.0);
        let outside = format!("Current week (Opus)\n{}\n25% used", filler(13));
        assert!(extract_cli_scoped_weekly_limits(&outside, now).is_empty());
        let next = "Current week (Opus)\nCurrent session\n25% used";
        assert!(extract_cli_scoped_weekly_limits(next, now).is_empty());
    }

    #[test]
    fn rejects_cli_output_without_usage_markers() {
        let provider = ClaudeProvider::new();
        let output = "Claude Code on Windows requires git-bash.";

        let err = provider
            .parse_cli_output(output)
            .expect_err("should reject non-usage output");

        assert!(matches!(err, ProviderError::Parse(_)));
        assert_eq!(
            err.to_string(),
            "Parse error: Claude CLI did not return usage data"
        );
    }

    #[test]
    fn cli_parse_usage_error_can_fallback_to_oauth() {
        let err = ProviderError::Parse("Claude CLI did not return usage data".to_string());

        assert!(should_fallback_from_claude_cli_error(&err));
    }

    #[test]
    fn cli_auth_error_does_not_fallback_to_oauth() {
        assert!(!should_fallback_from_claude_cli_error(
            &ProviderError::AuthRequired
        ));
    }

    #[test]
    fn auto_fetch_error_keeps_all_source_failures() {
        let err = claude_auto_fetch_error(vec![
            ("OAuth", ProviderError::OAuth("token expired".to_string())),
            ("Web", ProviderError::NoCookies),
            (
                "CLI",
                ProviderError::Parse("Empty output from Claude CLI".to_string()),
            ),
        ]);

        assert_eq!(
            err.to_string(),
            "Claude usage failed from all configured sources. OAuth: OAuth error: token expired; Web: No cookies available for web API; CLI: Parse error: Empty output from Claude CLI"
        );
    }

    fn oauth_rate_limited() -> ProviderError {
        ClaudeOAuthFetcher::rate_limited_error(Duration::from_secs(30))
    }

    #[test]
    fn auto_fetch_error_asks_for_a_browser_sign_in_when_only_the_browser_can_help() {
        // (CLI failure, retention policy of the plain summary)
        let cases = [
            (
                ProviderError::Parse("Claude CLI did not return usage data".to_string()),
                LastGoodFailurePolicy::Preserve,
            ),
            (
                ProviderError::Other("Claude CLI failed: exit status 1".to_string()),
                LastGoodFailurePolicy::Replace,
            ),
        ];
        for (cli_failure, policy) in cases {
            let err = claude_auto_fetch_error(vec![
                ("Web", ProviderError::NoCookies),
                ("OAuth", oauth_rate_limited()),
                ("CLI", cli_failure),
            ]);
            let ProviderError::BrowserSignInRequired {
                message,
                sign_in_url,
            } = &err
            else {
                panic!("expected a browser sign-in signal, got {err:?}");
            };
            assert_eq!(sign_in_url, CLAUDE_BROWSER_SIGN_IN_URL);
            assert_eq!(err.to_string(), *message);
            assert!(
                message.starts_with(
                    "Claude usage failed from all configured sources. Web: No cookies available for web API; OAuth: Transient OAuth error: Claude OAuth usage endpoint is rate limited."
                ),
                "{message}"
            );
            assert!(
                message
                    .ends_with("Sign in at https://claude.ai/login in your browser, then refresh."),
                "{message}"
            );
            // ClaudeProvider::error_state_kind defers to this for every
            // variant except a missing CLI.
            assert_eq!(
                err.state_kind(),
                crate::core::ProviderStateKind::NeedsAuthentication
            );
            // The hint leaves the desktop retention policy unchanged.
            let plain = message
                .strip_suffix(browser_sign_in_hint().as_str())
                .map(str::trim_end)
                .expect("hint is appended");
            assert_eq!(last_good_failure_policy_for_error(plain), policy);
            assert_eq!(last_good_failure_policy_for_error(message), policy);
        }
    }

    #[test]
    fn auto_fetch_error_keeps_other_failure_mixes_untyped() {
        let cli_failure =
            || ProviderError::Parse("Claude CLI did not return usage data".to_string());
        let mixes = [
            // A browser session was there; the Web source failed differently.
            vec![
                ("Web", ProviderError::AuthRequired),
                ("OAuth", oauth_rate_limited()),
                ("CLI", cli_failure()),
            ],
            // Signed out of Claude Code, not rate limited.
            vec![
                ("Web", ProviderError::NoCookies),
                (
                    "OAuth",
                    ProviderError::OAuth(
                        "Claude OAuth credentials not found. Run `claude` to authenticate."
                            .to_string(),
                    ),
                ),
                ("CLI", cli_failure()),
            ],
            // Another transient OAuth failure.
            vec![
                ("Web", ProviderError::NoCookies),
                (
                    "OAuth",
                    ProviderError::OAuthTransient(
                        "Claude OAuth token expired and token refresh is cooling down after a failed attempt."
                            .to_string(),
                    ),
                ),
                ("CLI", cli_failure()),
            ],
            // The CLI was not tried.
            vec![
                ("Web", ProviderError::NoCookies),
                ("OAuth", oauth_rate_limited()),
            ],
        ];
        for failures in mixes {
            let err = claude_auto_fetch_error(failures);
            assert!(matches!(err, ProviderError::Other(_)), "{err:?}");
            assert!(
                !err.to_string().contains(CLAUDE_BROWSER_SIGN_IN_URL),
                "{err}"
            );
        }
    }

    #[test]
    fn transient_transport_failure_stops_auto_fallback_and_preserves_last_good() {
        let provider = ClaudeProvider::new();
        assert!(provider.retains_last_good_on_transport_failure());
        assert_eq!(
            provider.last_good_failure_policy_for_error(&ProviderError::Timeout),
            LastGoodFailurePolicy::Preserve
        );

        let mut failures = Vec::new();
        let result = record_auto_source(&mut failures, "Web", Err(ProviderError::Timeout));
        assert!(matches!(result, Err(ProviderError::Timeout)));
        assert!(failures.is_empty());
    }

    #[test]
    fn rejects_claude_2_1_non_interactive_slash_response() {
        let provider = ClaudeProvider::new();
        let output = r#"
I see you've entered `/usage` and `/exit`.

**Usage**: Token usage and statistics are typically displayed by the CLI interface itself. I don't have direct access to those metrics through my available tools.

**Exit**: I'll end the session here. Goodbye!
"#;

        let err = provider
            .parse_cli_output(output)
            .expect_err("should reject non-interactive slash command response");

        assert!(matches!(err, ProviderError::Other(_)));
        assert_eq!(
            err.to_string(),
            "Claude CLI treated /usage as a normal prompt instead of opening the interactive usage screen. Use Auto, OAuth, or Web mode for Claude usage."
        );
    }

    #[test]
    fn rejects_legacy_non_interactive_slash_response() {
        let provider = ClaudeProvider::new();
        let output = r#"
I see you've entered two slash commands:

1. `/usage` - This appears to be a request to check usage information
2. `/exit` - This appears to be a request to exit

However, looking at the available custom slash commands, I don't see these commands defined.
"#;

        let err = provider
            .parse_cli_output(output)
            .expect_err("should reject non-interactive slash command response");

        assert!(matches!(err, ProviderError::Other(_)));
    }

    #[test]
    fn rejects_cli_activity_stats_without_plan_limits() {
        let provider = ClaudeProvider::new();
        let output = r#"
❯ /usage

Status   Config   Usage   Stats

Overview  Models

Favorite model: glm-4.6        Total tokens: 263.3k
Sessions: 6                    Longest session: 18s
Active days: 2/10              Longest streak: 1 day
"#;

        let err = provider
            .parse_cli_output(output)
            .expect_err("should reject local activity stats");

        assert!(matches!(err, ProviderError::Other(_)));
        assert_eq!(
            err.to_string(),
            "Claude CLI /usage opened, but this Claude version returned local activity stats instead of plan limit percentages. Use Auto, OAuth, or Web mode for Claude limits."
        );
    }

    #[test]
    fn rejects_ansi_spaced_cli_activity_stats_without_plan_limits() {
        let provider = ClaudeProvider::new();
        let output = "\x1b[2CTotal\x1b[1Ccost:\x1b[12C$0.0000\n\
                      \x1b[2CTotal\x1b[1Cduration\x1b[1C(API):\x1b[2C0s\n\
                      \x1b[2CUsage:\x1b[17C0\x1b[1Cinput,\x1b[1C0\x1b[1Coutput,\x1b[1C0\x1b[1Ccache\x1b[1Cread";

        let err = provider
            .parse_cli_output(output)
            .expect_err("should reject ANSI-spaced local activity stats");

        assert!(matches!(err, ProviderError::Other(_)));
    }

    #[test]
    fn accepts_plan_limits_followed_by_activity_stats() {
        // Claude Code 2.1.27x on Windows prints the exit summary (cost,
        // duration, cache tokens) after the /usage view when the probe ends.
        let provider = ClaudeProvider::new();
        let output = r#"
❯ /usage

Status   Config   Usage   Stats

Current session
███████░░░░░░░░░░░░░░░░░░░░░░ 19% used
Resets 3pm (Europe/Berlin)

Current week (all models)
█████████░░░░░░░░░░░░░░░░░░░░ 31% used
Resets Sep 19, 4pm (Europe/Berlin)

Total cost:            $0.0000
Total duration (API):  0s
Usage:                 0 input, 0 output, 0 cache read
"#;

        let result = provider
            .parse_cli_output(output)
            .expect("plan limits should win over trailing activity stats");

        assert_eq!(result.usage.primary.used_percent, 19.0);
        assert_eq!(
            result
                .usage
                .secondary
                .as_ref()
                .map(|window| window.used_percent),
            Some(31.0)
        );
    }

    // ── Upstream 0.50.1 #2516: revoked vs missing OAuth ────────────────────────

    #[test]
    fn oauth_revoked_error_is_detected() {
        assert!(is_oauth_revoked_error(&ProviderError::OAuthRevoked(
            "revoked".to_string()
        )));
        assert!(!is_oauth_revoked_error(&ProviderError::OAuth(
            "expired".to_string()
        )));
        assert!(!is_oauth_revoked_error(&ProviderError::AuthRequired));
    }

    #[test]
    fn rate_limited_and_revoked_oauth_reuse_the_cli_cache() {
        let rate_limited = ProviderError::OAuthTransient(
            "Claude OAuth usage endpoint is rate limited. Retrying in about 5m; credentials were preserved."
                .to_string(),
        );
        assert!(oauth::is_rate_limited_error(&rate_limited));
        assert!(oauth_failure_uses_cli_cache(&rate_limited));
        assert!(oauth_failure_uses_cli_cache(&ProviderError::OAuthRevoked(
            "revoked".to_string()
        )));
        // Other transient failures and plain expiry still probe the CLI.
        assert!(!oauth_failure_uses_cli_cache(
            &ProviderError::OAuthTransient("connection reset".to_string())
        ));
        assert!(!oauth_failure_uses_cli_cache(&ProviderError::OAuth(
            "expired".to_string()
        )));
        assert!(!oauth_failure_uses_cli_cache(&ProviderError::AuthRequired));
    }

    #[test]
    fn cli_result_cache_round_trips() {
        let mut result = ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(42.0)), "cli");
        result.has_successful_claude_cli_quota = true;
        cache_cli_result(result.clone());
        let cached = cached_cli_result().expect("cached result within TTL");
        assert!((cached.usage.primary.used_percent - 42.0).abs() < 0.01);
        assert_eq!(cached.source_label, "cli");
        assert!(!cached.has_successful_claude_cli_quota);

        // A live non-CLI success clears the cache. Same test, because the
        // global is shared and tests run in parallel without a lock.
        clear_cli_result_cache();
        assert!(cached_cli_result().is_none());
    }

    #[test]
    fn cli_quota_without_credential_identity_cannot_prove_account_action() {
        let provider = ClaudeProvider::new();
        let result = provider
            .parse_cli_output("Current session\n25% used\nCurrent week (all models)\n40% used")
            .expect("CLI quota should parse");
        let result = mark_live_claude_cli_result(result);

        assert!(result.usage.account_email.is_none());
        assert!(result.has_successful_claude_cli_quota);
    }

    #[test]
    fn non_cli_fetch_result_does_not_prove_account_action() {
        let result = ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(42.0)), "oauth");

        assert!(!result.has_successful_claude_cli_quota);
    }
    #[test]
    fn cli_presence_maps_to_local_runtime_offline() {
        assert_eq!(
            ClaudeProvider::new().error_state_kind(&ProviderError::NotInstalled(
                "Claude CLI not found. Install from https://docs.claude.ai/claude-code".to_string(),
            )),
            crate::core::ProviderStateKind::LocalRuntimeOffline
        );
        // Other error kinds keep their default classification.
        assert_eq!(
            ClaudeProvider::new().error_state_kind(&ProviderError::AuthRequired),
            crate::core::ProviderStateKind::NeedsAuthentication
        );
    }

    #[test]
    fn oauth_rate_limit_is_not_sign_in_required() {
        let error = ProviderError::OAuthTransient(
            "OAuth error: Claude OAuth usage endpoint is rate limited. Retrying in about 1s; credentials were preserved."
                .to_string(),
        );
        assert_eq!(
            ClaudeProvider::new().error_state_kind(&error),
            crate::core::ProviderStateKind::Unknown
        );
        assert_eq!(
            ClaudeProvider::new().last_good_failure_policy_for_error(&error),
            LastGoodFailurePolicy::Preserve
        );
    }

    #[test]
    fn oauth_refresh_cooldown_is_not_sign_in_required() {
        let error = ProviderError::OAuthTransient(
            "Claude OAuth token expired and token refresh is cooling down after a failed attempt. Please retry shortly, or run `claude login`."
                .to_string(),
        );
        assert_eq!(
            ClaudeProvider::new().error_state_kind(&error),
            crate::core::ProviderStateKind::Unknown
        );
        assert_eq!(
            ClaudeProvider::new().last_good_failure_policy_for_error(&error),
            LastGoodFailurePolicy::Preserve
        );
    }

    #[test]
    fn missing_oauth_credentials_still_require_sign_in() {
        let error = ProviderError::OAuth(
            "Claude OAuth credentials not found. Run `claude` to authenticate.".to_string(),
        );
        assert_eq!(
            ClaudeProvider::new().error_state_kind(&error),
            crate::core::ProviderStateKind::NeedsAuthentication
        );
        assert_eq!(
            last_good_failure_policy_for_error(&error.to_string()),
            LastGoodFailurePolicy::Replace
        );
    }

    #[test]
    fn untyped_oauth_rate_limit_text_is_not_transient() {
        let error = ProviderError::OAuth("OAuth API returned rate limited".to_string());
        assert_eq!(
            ClaudeProvider::new().error_state_kind(&error),
            crate::core::ProviderStateKind::NeedsAuthentication
        );
        assert_eq!(
            ClaudeProvider::new().last_good_failure_policy_for_error(&error),
            LastGoodFailurePolicy::Replace
        );
    }
}
