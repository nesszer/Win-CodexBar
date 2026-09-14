//! Opt-in reopening of the exact local CLI session that hit a quota limit.
//!
//! The tray process owns the small transition state machine. Providers only
//! report fresh usage; this module decides whether a captured Codex or Claude
//! session may be reopened after its blocking window becomes available.

use codexbar::agent_sessions::{
    AgentSession, AgentSessionProvider, AgentSessionSource, AgentSessionState,
    LocalAgentSessionScanner, SessionFocusResult,
};
use codexbar::core::{ProviderId, TokenAccountStore};
use codexbar::settings::Settings;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::commands::{ProviderUsageSnapshot, RateWindowSnapshot};
use crate::state::AppState;
use tauri::Manager;

const MAX_SESSION_ID_LEN: usize = 256;
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum QuotaSlot {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumeSource {
    CodexPat,
    CodexOAuth,
    ClaudeCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InFlightOp {
    generation: u64,
    token: uuid::Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeTarget {
    pub provider: ProviderId,
    quota_source: ResumeSource,
    pub session_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub token_account_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResumeArm {
    target: ResumeTarget,
    blocked_slots: Vec<QuotaSlot>,
    account_identity: Option<String>,
}

/// In-memory only by design: restarting the tray app discards the arm instead
/// of guessing which account or session should be reopened later.
#[derive(Debug, Default)]
pub(crate) struct AutoResumeState {
    arms: HashMap<ProviderId, ResumeArm>,
    captures_in_progress: HashMap<ProviderId, InFlightOp>,
    resumes_in_progress: HashMap<ProviderId, InFlightOp>,
    capture_generations: HashMap<ProviderId, u64>,
}

impl AutoResumeState {
    pub(crate) fn clear_provider(&mut self, provider: ProviderId) {
        self.arms.remove(&provider);
        self.captures_in_progress.remove(&provider);
        self.resumes_in_progress.remove(&provider);
        let generation = self.capture_generations.entry(provider).or_default();
        *generation = generation.wrapping_add(1);
    }

    pub(crate) fn clear_disabled(&mut self, enabled_ids: &[ProviderId]) {
        for provider in [ProviderId::Codex, ProviderId::Claude] {
            if !enabled_ids.contains(&provider) {
                self.clear_provider(provider);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// Observe one successful, live provider snapshot after it has been committed
/// to the provider cache.
pub(crate) async fn observe_fresh_snapshot(
    app: &tauri::AppHandle,
    provider: ProviderId,
    snapshot: &ProviderUsageSnapshot,
    token_account_id: Option<uuid::Uuid>,
    account_identity: Option<&str>,
) {
    if !supports_auto_resume(provider) {
        return;
    }

    let settings = Settings::load();
    if !settings.auto_resume_after_quota_reset(provider) {
        clear(app, provider);
        return;
    }
    if !auto_resume_lane_is_available(
        provider,
        token_account_id,
        is_auto_resume_available(provider),
    ) {
        // Agent discovery has no account identity, so a managed token-account
        // snapshot can never be correlated to the process that would be
        // reopened. An unreadable store is also unsafe because the active
        // account cannot be established.
        clear(app, provider);
        return;
    }
    let account_identity = normalize_account_identity(account_identity);
    if account_identity.is_none() {
        // A local CLI session must carry the identity published by the same
        // credential source that supplied the quota snapshot. Email and an
        // uncorrelated process list are insufficient substitutes.
        clear(app, provider);
        return;
    }
    let Some(quota_source) = resume_source(provider, snapshot) else {
        // A live snapshot from another source is positive evidence that the
        // captured lane is no longer the lane that would be resumed. Transient
        // failures and offline snapshots carry no such evidence and preserve
        // the arm for the next live refresh.
        if is_live_snapshot(snapshot) {
            clear(app, provider);
        }
        return;
    };

    let blocked_slots = blocking_slots(&snapshot.primary, snapshot.secondary.as_ref());
    if blocked_slots.is_empty() {
        let Some((target, resume_operation)) = begin_resume_attempt(
            app,
            provider,
            snapshot,
            token_account_id,
            quota_source,
            account_identity.as_deref(),
        ) else {
            return;
        };
        resume_captured_session(app, target, resume_operation, account_identity.as_deref()).await;
        return;
    }

    let Some(capture_operation) =
        begin_capture(app, provider, quota_source, account_identity.as_deref())
    else {
        return;
    };

    let target = capture_session(provider, token_account_id, quota_source).await;
    let still_enabled = Settings::load().auto_resume_after_quota_reset(provider);
    finish_capture(
        app,
        provider,
        capture_operation,
        still_enabled,
        target,
        blocked_slots,
        account_identity,
    );
}

pub(crate) fn clear(app: &tauri::AppHandle, provider: ProviderId) {
    let state = app.state::<Mutex<AppState>>();
    if let Ok(mut guard) = state.lock() {
        guard.auto_resume.clear_provider(provider);
    }
}

pub(crate) fn supports_auto_resume(provider: ProviderId) -> bool {
    matches!(provider, ProviderId::Codex | ProviderId::Claude)
}

/// Managed token-account lanes cannot be correlated with the local process
/// list. Treat an unreadable account store as unavailable so the watcher fails
/// closed.
pub(crate) fn is_auto_resume_available(provider: ProviderId) -> bool {
    let unmanaged = TokenAccountStore::new()
        .load_provider(provider)
        .map(|data| data.active_account().is_none())
        .unwrap_or(false);
    unmanaged
        && match provider {
            ProviderId::Claude => codexbar::providers::claude::auto_resume_identity().is_some(),
            _ => true,
        }
}

fn auto_resume_lane_is_available(
    provider: ProviderId,
    token_account_id: Option<uuid::Uuid>,
    account_store_is_unmanaged: bool,
) -> bool {
    supports_auto_resume(provider) && token_account_id.is_none() && account_store_is_unmanaged
}

pub(crate) fn enabled_provider_ids(settings: &Settings) -> Vec<ProviderId> {
    settings
        .get_enabled_provider_ids()
        .into_iter()
        .filter(|provider| {
            auto_resume_lane_is_available(*provider, None, is_auto_resume_available(*provider))
                && settings.auto_resume_after_quota_reset(*provider)
        })
        .collect()
}

fn is_live_snapshot(snapshot: &ProviderUsageSnapshot) -> bool {
    snapshot.error.is_none()
        && snapshot.error_state == codexbar::core::ProviderStateKind::Ready
        && !snapshot.source_label.trim().eq_ignore_ascii_case("offline")
}

fn normalize_account_identity(identity: Option<&str>) -> Option<String> {
    identity
        .map(str::trim)
        .filter(|identity| !identity.is_empty())
        .map(str::to_owned)
}

fn resume_binding_matches(
    arm: &ResumeArm,
    quota_source: ResumeSource,
    account_identity: Option<&str>,
) -> bool {
    arm.target.quota_source == quota_source && arm.account_identity.as_deref() == account_identity
}

fn resume_source(provider: ProviderId, snapshot: &ProviderUsageSnapshot) -> Option<ResumeSource> {
    if !is_live_snapshot(snapshot) {
        return None;
    }
    match provider {
        ProviderId::Codex => match snapshot.source_label.trim().to_ascii_lowercase().as_str() {
            "pat" => Some(ResumeSource::CodexPat),
            "oauth" => Some(ResumeSource::CodexOAuth),
            _ => None,
        },
        ProviderId::Claude
            if snapshot.has_successful_claude_cli_quota
                && snapshot
                    .source_label
                    .trim()
                    .to_ascii_lowercase()
                    .starts_with("cli") =>
        {
            Some(ResumeSource::ClaudeCli)
        }
        _ => None,
    }
}

fn blocking_slots(
    primary: &RateWindowSnapshot,
    secondary: Option<&RateWindowSnapshot>,
) -> Vec<QuotaSlot> {
    let mut slots = Vec::with_capacity(2);
    if is_blocking_window(primary) {
        slots.push(QuotaSlot::Primary);
    }
    if secondary.is_some_and(is_blocking_window) {
        slots.push(QuotaSlot::Secondary);
    }
    slots
}

fn is_blocking_window(window: &RateWindowSnapshot) -> bool {
    !window.is_informational && (window.is_exhausted || window.used_percent >= 100.0)
}

fn is_available_window(window: Option<&RateWindowSnapshot>) -> bool {
    window.is_some_and(|window| {
        !window.is_informational && !window.is_exhausted && window.used_percent < 100.0
    })
}

fn restored(
    arm: &ResumeArm,
    primary: &RateWindowSnapshot,
    secondary: Option<&RateWindowSnapshot>,
) -> bool {
    arm.blocked_slots.iter().all(|slot| match slot {
        QuotaSlot::Primary => is_available_window(Some(primary)),
        QuotaSlot::Secondary => is_available_window(secondary),
    })
}

fn begin_capture(
    app: &tauri::AppHandle,
    provider: ProviderId,
    quota_source: ResumeSource,
    account_identity: Option<&str>,
) -> Option<InFlightOp> {
    let state = app.state::<Mutex<AppState>>();
    let Ok(mut guard) = state.lock() else {
        return None;
    };
    if guard
        .auto_resume
        .arms
        .get(&provider)
        .is_some_and(|arm| resume_binding_matches(arm, quota_source, account_identity))
    {
        return None;
    }
    if guard.auto_resume.arms.contains_key(&provider) {
        guard.auto_resume.clear_provider(provider);
    }
    let capture_generation = *guard
        .auto_resume
        .capture_generations
        .entry(provider)
        .or_default();
    if guard
        .auto_resume
        .captures_in_progress
        .contains_key(&provider)
    {
        return None;
    }
    let operation = InFlightOp {
        generation: capture_generation,
        token: uuid::Uuid::new_v4(),
    };
    guard
        .auto_resume
        .captures_in_progress
        .insert(provider, operation);
    Some(operation)
}

fn finish_capture(
    app: &tauri::AppHandle,
    provider: ProviderId,
    operation: InFlightOp,
    still_enabled: bool,
    target: Option<ResumeTarget>,
    blocked_slots: Vec<QuotaSlot>,
    account_identity: Option<String>,
) {
    let state = app.state::<Mutex<AppState>>();
    let Ok(mut guard) = state.lock() else {
        return;
    };
    finish_capture_state(
        &mut guard.auto_resume,
        provider,
        operation,
        still_enabled,
        target,
        blocked_slots,
        account_identity,
    );
}

fn finish_capture_state(
    state: &mut AutoResumeState,
    provider: ProviderId,
    operation: InFlightOp,
    still_enabled: bool,
    target: Option<ResumeTarget>,
    blocked_slots: Vec<QuotaSlot>,
    account_identity: Option<String>,
) {
    // An older async completion must never release a newer capture's slot.
    if state.captures_in_progress.get(&provider) != Some(&operation) {
        return;
    }
    state.captures_in_progress.remove(&provider);

    if !still_enabled
        || state
            .capture_generations
            .get(&provider)
            .copied()
            .unwrap_or_default()
            != operation.generation
        || state.arms.contains_key(&provider)
    {
        return;
    }
    if let Some(target) = target {
        state.arms.insert(
            provider,
            ResumeArm {
                target,
                blocked_slots,
                account_identity,
            },
        );
    }
}

fn begin_resume_attempt(
    app: &tauri::AppHandle,
    provider: ProviderId,
    snapshot: &ProviderUsageSnapshot,
    token_account_id: Option<uuid::Uuid>,
    quota_source: ResumeSource,
    account_identity: Option<&str>,
) -> Option<(ResumeTarget, InFlightOp)> {
    let state = app.state::<Mutex<AppState>>();
    let Ok(mut guard) = state.lock() else {
        return None;
    };
    let source_changed = guard
        .auto_resume
        .arms
        .get(&provider)
        .is_some_and(|arm| !resume_binding_matches(arm, quota_source, account_identity));
    if source_changed {
        guard.auto_resume.clear_provider(provider);
        return None;
    }
    let arm = guard.auto_resume.arms.get(&provider)?;
    if arm.target.token_account_id != token_account_id {
        return None;
    }
    if !restored(arm, &snapshot.primary, snapshot.secondary.as_ref()) {
        return None;
    }
    let target = arm.target.clone();
    let resume_generation = guard
        .auto_resume
        .capture_generations
        .get(&provider)
        .copied()
        .unwrap_or_default();
    if guard
        .auto_resume
        .resumes_in_progress
        .contains_key(&provider)
    {
        return None;
    }
    let operation = InFlightOp {
        generation: resume_generation,
        token: uuid::Uuid::new_v4(),
    };
    guard
        .auto_resume
        .resumes_in_progress
        .insert(provider, operation);
    Some((target, operation))
}

async fn capture_session(
    provider: ProviderId,
    token_account_id: Option<uuid::Uuid>,
    quota_source: ResumeSource,
) -> Option<ResumeTarget> {
    // AgentSession has no account identity. A managed token account therefore
    // cannot be positively correlated with a discovered process; do not arm
    // a session that could belong to another account.
    if token_account_id.is_some() {
        return None;
    }
    let result = LocalAgentSessionScanner::default().scan().await;
    if result.error.is_some() {
        return None;
    }
    capture_target_from_sessions(provider, &result.sessions, token_account_id, quota_source)
}

fn capture_target_from_sessions(
    provider: ProviderId,
    sessions: &[AgentSession],
    token_account_id: Option<uuid::Uuid>,
    quota_source: ResumeSource,
) -> Option<ResumeTarget> {
    if token_account_id.is_some() {
        return None;
    }
    let mut candidates = sessions
        .iter()
        .filter_map(|session| target_from_active_session(provider, session, None, quota_source));
    let target = candidates.next()?;
    // Scanner ordering is an activity/display concern, not an account
    // correlation. Multiple live candidates are ambiguous, so fail closed.
    candidates.next().is_none().then_some(target)
}

fn target_from_active_session(
    provider: ProviderId,
    session: &AgentSession,
    token_account_id: Option<uuid::Uuid>,
    quota_source: ResumeSource,
) -> Option<ResumeTarget> {
    (session.state == AgentSessionState::Active).then_some(())?;
    session.pid?;
    target_from_session(provider, session, token_account_id, quota_source)
}

fn target_from_session(
    provider: ProviderId,
    session: &AgentSession,
    token_account_id: Option<uuid::Uuid>,
    quota_source: ResumeSource,
) -> Option<ResumeTarget> {
    let expected = match provider {
        ProviderId::Codex => AgentSessionProvider::Codex,
        ProviderId::Claude => AgentSessionProvider::Claude,
        _ => return None,
    };
    if session.provider != expected
        || session.source != AgentSessionSource::Cli
        || !is_local_host(&session.host)
    {
        return None;
    }
    let session_id = valid_session_id(&session.id)?.to_string();
    let cwd = canonical_workspace(session.workspace.cwd.as_deref())?;
    let transcript_path = session
        .transcript_path
        .as_deref()
        .and_then(canonical_transcript);
    Some(ResumeTarget {
        provider,
        quota_source,
        session_id,
        cwd,
        transcript_path,
        token_account_id,
    })
}

fn matching_session<'a>(
    target: &ResumeTarget,
    sessions: &'a [AgentSession],
) -> Option<&'a AgentSession> {
    sessions.iter().find(|session| {
        target_from_session(
            target.provider,
            session,
            target.token_account_id,
            target.quota_source,
        )
        .is_some_and(|candidate| {
            candidate.provider == target.provider
                && candidate.session_id == target.session_id
                && candidate.cwd == target.cwd
        })
    })
}

fn is_local_host(host: &str) -> bool {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    std::env::var("COMPUTERNAME")
        .ok()
        .is_some_and(|local| !local.trim().is_empty() && local.trim().eq_ignore_ascii_case(host))
}

fn canonical_workspace(raw: Option<&str>) -> Option<PathBuf> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return None;
    }
    std::fs::canonicalize(path)
        .ok()
        .filter(|path| path.is_dir())
}

fn canonical_transcript(raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw.trim());
    path.is_file()
        .then(|| std::fs::canonicalize(path).ok())
        .flatten()
}

fn transcript_is_still_present(target: &ResumeTarget) -> bool {
    target.provider == ProviderId::Claude
        && target
            .transcript_path
            .as_ref()
            .is_some_and(|path| path.is_file())
}

fn valid_session_id(raw: &str) -> Option<&str> {
    let id = raw.trim();
    if id.is_empty()
        || id.len() > MAX_SESSION_ID_LEN
        || id.starts_with("pid:")
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return None;
    }
    Some(id)
}

async fn resume_captured_session(
    app: &tauri::AppHandle,
    target: ResumeTarget,
    operation: InFlightOp,
    account_identity: Option<&str>,
) {
    let result = LocalAgentSessionScanner::default().scan().await;
    if result.error.is_some() {
        tracing::debug!(
            provider = target.provider.cli_name(),
            "captured auto-resume session could not be revalidated"
        );
        finish_resume_attempt(app, &target, operation, false);
        return;
    }
    if let Some(session) = matching_session(&target, &result.sessions) {
        // A live process already owns this exact session. Reopening it would
        // create a duplicate terminal, so focus the existing window instead.
        if session.pid.is_some() {
            if !resume_attempt_is_still_valid(app, &target, operation, account_identity) {
                clear_provider_if_resume_owner(app, target.provider, operation);
                return;
            }
            let focus_result = codexbar::agent_sessions::focus_session(session);
            let succeeded = matches!(focus_result, SessionFocusResult::Focused);
            tracing::info!(
                provider = target.provider.cli_name(),
                focus_result = ?focus_result,
                succeeded,
                "captured CLI session is already running; attempting to focus it"
            );
            finish_resume_attempt(app, &target, operation, succeeded);
            return;
        }
    } else if !transcript_is_still_present(&target) {
        tracing::debug!(
            provider = target.provider.cli_name(),
            "captured auto-resume session disappeared or could not be revalidated"
        );
        finish_resume_attempt(app, &target, operation, false);
        return;
    }

    if !resume_attempt_is_still_valid(app, &target, operation, account_identity) {
        clear_provider_if_resume_owner(app, target.provider, operation);
        return;
    }

    let launch_result = launch_resume(&target);
    let succeeded = launch_result.is_ok();
    match launch_result {
        Ok(()) => tracing::info!(
            provider = target.provider.cli_name(),
            "reopened captured CLI session after quota reset"
        ),
        Err(error) => tracing::warn!(
            provider = target.provider.cli_name(),
            error,
            "could not reopen captured CLI session after quota reset"
        ),
    }
    finish_resume_attempt(app, &target, operation, succeeded);
}

fn resume_attempt_is_still_valid(
    app: &tauri::AppHandle,
    target: &ResumeTarget,
    operation: InFlightOp,
    account_identity: Option<&str>,
) -> bool {
    if !Settings::load().auto_resume_after_quota_reset(target.provider)
        || !auto_resume_lane_is_available(
            target.provider,
            target.token_account_id,
            is_auto_resume_available(target.provider),
        )
    {
        return false;
    }
    if target.provider == ProviderId::Claude
        && normalize_account_identity(
            codexbar::providers::claude::auto_resume_identity().as_deref(),
        ) != normalize_account_identity(account_identity)
    {
        return false;
    }

    let state = app.state::<Mutex<AppState>>();
    let Ok(guard) = state.lock() else {
        return false;
    };
    guard.auto_resume.resumes_in_progress.get(&target.provider) == Some(&operation)
        && resume_generation_is_current(&guard, target.provider, operation.generation)
        && guard
            .auto_resume
            .arms
            .get(&target.provider)
            .is_some_and(|arm| {
                arm.target == *target
                    && arm.account_identity.as_deref() == account_identity
                    && resume_binding_matches(arm, target.quota_source, account_identity)
            })
}

fn finish_resume_attempt(
    app: &tauri::AppHandle,
    target: &ResumeTarget,
    operation: InFlightOp,
    succeeded: bool,
) {
    let state = app.state::<Mutex<AppState>>();
    let Ok(mut guard) = state.lock() else {
        return;
    };
    finish_resume_attempt_state(&mut guard.auto_resume, target, operation, succeeded);
}

fn clear_provider_if_resume_owner(
    app: &tauri::AppHandle,
    provider: ProviderId,
    operation: InFlightOp,
) {
    let state = app.state::<Mutex<AppState>>();
    let Ok(mut guard) = state.lock() else {
        return;
    };
    clear_provider_if_resume_owner_state(&mut guard.auto_resume, provider, operation);
}

fn clear_provider_if_resume_owner_state(
    state: &mut AutoResumeState,
    provider: ProviderId,
    operation: InFlightOp,
) {
    if state.resumes_in_progress.get(&provider) == Some(&operation) {
        state.clear_provider(provider);
    }
}

fn finish_resume_attempt_state(
    state: &mut AutoResumeState,
    target: &ResumeTarget,
    operation: InFlightOp,
    succeeded: bool,
) {
    // A stale async completion must not release a newer resume attempt's slot.
    if state.resumes_in_progress.get(&target.provider) != Some(&operation) {
        return;
    }
    state.resumes_in_progress.remove(&target.provider);
    if !succeeded
        || state
            .capture_generations
            .get(&target.provider)
            .copied()
            .unwrap_or_default()
            != operation.generation
    {
        return;
    }
    if state
        .arms
        .get(&target.provider)
        .is_some_and(|arm| arm.target == *target)
    {
        state.arms.remove(&target.provider);
    }
}

fn resume_generation_is_current(state: &AppState, provider: ProviderId, generation: u64) -> bool {
    state
        .auto_resume
        .capture_generations
        .get(&provider)
        .copied()
        .unwrap_or_default()
        == generation
}

fn launch_resume(target: &ResumeTarget) -> Result<(), String> {
    let executable = match target.provider {
        ProviderId::Codex => codexbar::codex_cli::locate_codex_binary(),
        ProviderId::Claude => codexbar::providers::claude::locate_claude_binary(),
        _ => None,
    }
    .ok_or_else(|| format!("{} CLI was not found", target.provider.display_name()))?;

    let command = build_resume_command(target, executable)?;
    let mut process = Command::new(&command.program);
    process.args(&command.args).current_dir(&command.cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP);
    }
    process.spawn().map(|_| ()).map_err(|error| {
        format!(
            "failed to launch {} CLI: {error}",
            target.provider.display_name()
        )
    })
}

fn build_resume_command(
    target: &ResumeTarget,
    executable: PathBuf,
) -> Result<ResumeCommand, String> {
    let session_id = valid_session_id(&target.session_id)
        .ok_or_else(|| "captured session id is invalid".to_string())?;
    if !target.cwd.is_absolute() {
        return Err("captured session workspace must be absolute".to_string());
    }

    let cli_args = match target.provider {
        ProviderId::Codex => vec!["resume".to_string(), session_id.to_string()],
        ProviderId::Claude => vec!["--resume".to_string(), session_id.to_string()],
        _ => return Err("provider does not support auto-resume".to_string()),
    };

    #[cfg(windows)]
    if executable
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "cmd" | "bat"))
    {
        let command_line = std::iter::once(quote_cmd_arg(&executable.to_string_lossy()))
            .chain(cli_args.iter().map(|arg| quote_cmd_arg(arg)))
            .collect::<Vec<_>>()
            .join(" ");
        return Ok(ResumeCommand {
            program: PathBuf::from("cmd.exe"),
            args: vec![
                "/d".to_string(),
                "/s".to_string(),
                "/c".to_string(),
                command_line,
            ],
            cwd: target.cwd.clone(),
        });
    }

    Ok(ResumeCommand {
        program: executable,
        args: cli_args,
        cwd: target.cwd.clone(),
    })
}

#[cfg(windows)]
fn quote_cmd_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(test)]
#[path = "auto_resume_tests.rs"]
mod auto_resume_tests;
