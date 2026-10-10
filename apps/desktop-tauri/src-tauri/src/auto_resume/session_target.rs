use super::*;

pub(super) async fn capture_session(
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

pub(super) fn capture_target_from_sessions(
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

pub(super) fn matching_session<'a>(
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

pub(super) fn transcript_is_still_present(target: &ResumeTarget) -> bool {
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

pub(super) fn launch_resume(target: &ResumeTarget) -> Result<(), String> {
    let executable = match target.provider {
        ProviderId::Codex => codexbar::codex_cli::locate_codex_binary(),
        ProviderId::Claude => codexbar::providers::claude::locate_claude_binary(),
        _ => None,
    }
    .ok_or_else(|| format!("{} CLI was not found", target.provider.display_name()))?;

    let command = build_resume_command(target, executable)?;
    // This runs after a background refresh, not a user action: the console
    // starts minimized and inactive, so it waits on the taskbar instead of
    // taking focus from the app the user is working in.
    codexbar::host::console_launch::spawn_minimized_console(
        &command.program,
        &command.args,
        &command.cwd,
    )
    .map_err(|error| {
        format!(
            "failed to launch {} CLI: {error}",
            target.provider.display_name()
        )
    })
}

pub(super) fn build_resume_command(
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

    // Batch shims such as npm's `claude.cmd` stay the program here;
    // `spawn_minimized_console` runs them through cmd.exe with quoting cmd.exe
    // parses.
    Ok(ResumeCommand {
        program: executable,
        args: cli_args,
        cwd: target.cwd.clone(),
    })
}
