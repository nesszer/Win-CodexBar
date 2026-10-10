use super::cli_binary::locate_claude_binary;
use super::cli_text::is_workspace_trust_prompt;
use super::{ClaudeProvider, accounts, auto_precision, cli_screen, trust_dialog, unix_now_secs};
use crate::cli::tty_runner::{TtyCommandOptions, TtyCommandRunner};
use crate::core::ProviderError;
use std::time::{Duration, Instant};

const CLAUDE_PROBE_SESSION_ID_FILE: &str = ".codexbar-session-id";
const CLAUDE_PROBE_LOCK_FILE: &str = ".codexbar-probe.lock";
pub(super) const CLAUDE_PROBE_CACHE_FILE: &str = ".codexbar-usage-cache.json";
/// How long a second codexbar process waits for a running probe to finish.
const CLAUDE_PROBE_LOCK_WAIT: Duration = Duration::from_secs(30);
/// Every codexbar process (the `serve` daemon, one-off `usage` calls from the
/// companion) launches its own Claude CLI for a probe. The interactive
/// `/usage` screen costs 6-10 s of CPU each time, so a recent successful
/// probe output is shared across processes for this long.
pub(super) const CLAUDE_PROBE_CACHE_TTL: Duration = Duration::from_secs(45);

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct ClaudeProbeCache {
    pub(super) captured_at_unix: u64,
    /// `claude_login_fingerprint` of the login the screen belongs to.
    #[serde(default)]
    pub(super) login: String,
    pub(super) output: String,
}

pub(super) fn load_cached_probe_output(probe_dir: &std::path::Path, login: &str) -> Option<String> {
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

pub(super) fn store_cached_probe_output(probe_dir: &std::path::Path, login: &str, output: &str) {
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

pub(super) fn login_fingerprint_at(credentials: &std::path::Path) -> Option<String> {
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
pub(super) fn redacted_probe_screen(visible: &str) -> String {
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
pub(super) struct ClaudeProbeLock(std::fs::File);

impl ClaudeProbeLock {
    /// Wait for the probe lock. `Ok(None)` means locking is unsupported here
    /// and the probe runs unlocked. A probe still running elsewhere after the
    /// wait is an error: probing alongside it would reuse its session id.
    fn acquire(probe_dir: &std::path::Path) -> Result<Option<Self>, ProviderError> {
        Self::acquire_within(probe_dir, CLAUDE_PROBE_LOCK_WAIT)
    }

    pub(super) fn acquire_within(
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
pub(super) fn load_or_create_probe_session_id(probe_dir: &std::path::Path) -> String {
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
pub(super) fn cleanup_probe_session_jsonl(probe_dir: &std::path::Path) {
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

pub(super) fn cleanup_probe_transcripts_in(
    projects_root: &std::path::Path,
    probe_dir: &std::path::Path,
) {
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
pub(super) fn claude_project_dir_name(dir: &std::path::Path) -> String {
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
pub(super) fn javascript_hash_base36(text: &str) -> String {
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
pub(super) fn claude_usage_settings_args() -> [String; 2] {
    [
        "--settings".to_string(),
        // Issue #778 (Claude Code 2.1.293): `tui: default` stops the "Try the new
        // fullscreen renderer?" offer. Older builds ignore unknown settings keys.
        r#"{"remoteControlAtStartup":false,"tui":"default"}"#.to_string(),
    ]
}

pub(super) fn claude_probe_launch_args(session_id: &str) -> Vec<String> {
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

pub(super) fn resolve_claude_cli_path() -> Result<std::path::PathBuf, ProviderError> {
    locate_claude_binary().ok_or_else(|| {
        ProviderError::NotInstalled(
            "Claude CLI not found. Install from https://docs.claude.ai/claude-code".to_string(),
        )
    })
}

pub(super) async fn fetch_claude_cli_usage_text(
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
pub(super) fn claude_cli_error_from_output(output: &str) -> Option<ProviderError> {
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
pub(super) fn claude_passive_probe_env(
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
pub(super) fn run_locked_probe(
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
