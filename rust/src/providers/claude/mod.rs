//! Claude provider implementation

mod admin_api;
mod oauth;
mod web_api;

use async_trait::async_trait;
use regex_lite::Regex;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::{Command as StdCommand, Stdio};

use crate::cli::tty_runner::{TtyCommandOptions, TtyCommandRunner};
use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

use admin_api::ClaudeAdminApiFetcher;
pub use oauth::ClaudeOAuthFetcher;
pub use web_api::ClaudeWebApiFetcher;

/// Claude provider implementation
pub struct ClaudeProvider {
    metadata: ProviderMetadata,
    web_fetcher: ClaudeWebApiFetcher,
    oauth_fetcher: ClaudeOAuthFetcher,
    admin_fetcher: ClaudeAdminApiFetcher,
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Claude,
                display_name: "Claude",
                session_label: "Session (5h)",
                weekly_label: "Weekly",
                supports_opus: true,
                supports_credits: true,
                default_enabled: true,
                is_primary: true,
                dashboard_url: Some("https://claude.ai/settings/usage"),
                status_page_url: Some("https://status.claude.com/"),
            },
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

struct ClaudePtyProbeOptions {
    script: &'static str,
    timeout_secs: f64,
    idle_timeout_secs: Option<f64>,
    initial_delay_secs: f64,
    script_char_delay_secs: f64,
    script_line_delay_secs: f64,
    send_on_substring: Option<(&'static str, &'static str)>,
}

async fn run_claude_usage_pty_probe(
    claude_path: std::path::PathBuf,
    working_directory: std::path::PathBuf,
) -> Result<String, ProviderError> {
    run_claude_pty_probe(
        claude_path,
        working_directory,
        ClaudePtyProbeOptions {
            script: "/usage",
            timeout_secs: 20.0,
            idle_timeout_secs: Some(6.0),
            initial_delay_secs: 3.0,
            script_char_delay_secs: 0.04,
            script_line_delay_secs: 0.0,
            send_on_substring: None,
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
            send_on_substring: Some(("Enter", "\n/exit\n")),
        },
    )
    .await
}

async fn run_claude_pty_probe(
    claude_path: std::path::PathBuf,
    working_directory: std::path::PathBuf,
    probe: ClaudePtyProbeOptions,
) -> Result<String, ProviderError> {
    tokio::task::spawn_blocking(move || {
        let mut env = TtyCommandRunner::enriched_environment();
        env.insert("NO_COLOR".to_string(), "1".to_string());

        let mut options = TtyCommandOptions::new()
            .with_timeout(probe.timeout_secs)
            .with_initial_delay(probe.initial_delay_secs)
            .with_script_char_delay(probe.script_char_delay_secs)
            .with_script_line_delay(probe.script_line_delay_secs)
            .with_working_directory(working_directory)
            .with_extra_args(vec!["--setting-sources".to_string(), "user".to_string()]);
        if let Some(idle) = probe.idle_timeout_secs {
            options = options.with_idle_timeout(idle);
        }
        if let Some((trigger, keys)) = probe.send_on_substring {
            options = options.with_send_on_substring(trigger, keys);
        }
        options.env = env;

        TtyCommandRunner::new()
            .run(&claude_path.to_string_lossy(), probe.script, options)
            .map(|result| result.text)
    })
    .await
    .map_err(|e| ProviderError::Other(format!("Claude CLI probe failed: {}", e)))?
    .map_err(|e| match e {
        crate::cli::tty_runner::TtyCommandError::TimedOut => ProviderError::Timeout,
        other => ProviderError::Other(format!("Claude CLI failed: {}", other)),
    })
}

#[async_trait]
impl Provider for ClaudeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto => {
                let mut failures = Vec::new();

                if self.admin_fetcher.has_credentials(ctx) {
                    match self.fetch_via_admin_api(ctx).await {
                        Ok(result) => return Ok(result),
                        Err(error) => failures.push(("Admin API", error)),
                    }
                }
                match self.fetch_via_web(ctx).await {
                    Ok(result) => return Ok(result),
                    Err(error) => failures.push(("Web", error)),
                }
                match self.fetch_via_oauth(ctx).await {
                    Ok(result) => return Ok(result),
                    Err(error) => failures.push(("OAuth", error)),
                }
                match self.fetch_via_cli(ctx).await {
                    Ok(result) => Ok(result),
                    Err(error) => {
                        failures.push(("CLI", error));
                        Err(claude_auto_fetch_error(failures))
                    }
                }
            }
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

    fn supports_web(&self) -> bool {
        true
    }

    fn supports_cli(&self) -> bool {
        true
    }

    fn detect_version(&self) -> Option<String> {
        detect_claude_version()
    }
}

impl ClaudeProvider {
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

    async fn fetch_via_admin_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Attempting Admin API fetch for Claude");
        self.admin_fetcher.fetch(ctx).await
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

        // Check if claude CLI exists
        let claude_path = which_claude().ok_or_else(|| {
            ProviderError::NotInstalled(
                "Claude CLI not found. Install from https://docs.claude.ai/claude-code".to_string(),
            )
        })?;

        let probe_dir = claude_usage_probe_dir()?;
        let mut combined =
            run_claude_usage_pty_probe(claude_path.clone(), probe_dir.clone()).await?;

        if is_workspace_trust_prompt(&strip_ansi(&combined).to_lowercase()) {
            run_claude_trust_preflight(claude_path.clone(), probe_dir.clone()).await?;
            combined = run_claude_usage_pty_probe(claude_path, probe_dir).await?;
        }

        // Check for common error conditions
        let lowered = combined.to_lowercase();
        if lowered.contains("not logged in") || lowered.contains("login required") {
            return Err(ProviderError::AuthRequired);
        }
        if lowered.contains("token expired") || lowered.contains("token_expired") {
            return Err(ProviderError::OAuth(
                "Token expired. Run `claude login` to refresh.".to_string(),
            ));
        }
        if lowered.contains("authentication_error") {
            return Err(ProviderError::OAuth(
                "Authentication error. Run `claude login`.".to_string(),
            ));
        }
        if lowered.contains("requires git-bash") {
            return Err(ProviderError::Other(
                "Claude CLI requires Git Bash on Windows. Install Git for Windows or set \
                 CLAUDE_CODE_GIT_BASH_PATH to your bash.exe path."
                    .to_string(),
            ));
        }
        if lowered.contains("running scripts is disabled") {
            return Err(ProviderError::Other(
                "Claude CLI could not start because PowerShell script execution is disabled. \
                 Use claude.cmd or adjust the execution policy."
                    .to_string(),
            ));
        }
        if lowered.contains("cannot run a document in the middle of a pipeline") {
            return Err(ProviderError::Other(
                "Claude CLI resolved to a Unix shell script on Windows. Reinstall Claude Code or \
                 ensure claude.cmd is first on PATH."
                    .to_string(),
            ));
        }

        // Parse the usage output
        self.parse_cli_output(&combined)
    }

    /// Parse Claude CLI /usage output
    fn parse_cli_output(&self, output: &str) -> Result<ProviderFetchResult, ProviderError> {
        let clean = strip_ansi(output);
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

        if is_cli_activity_stats_response(&clean_lower) {
            return Err(ProviderError::Other(
                "Claude CLI /usage opened, but this Claude version returned local activity stats instead of plan limit percentages. Use Auto, OAuth, or Web mode for Claude limits.".to_string(),
            ));
        }

        // Parse session percent: "X% used" or "X% left"
        let mut session_percent: Option<f64> = None;
        let mut weekly_percent: Option<f64> = None;
        let mut opus_percent: Option<f64> = None;

        // Look for "Current session" section
        if let Some(session_pct) = extract_percent_near_label(&clean, "current session") {
            session_percent = Some(session_pct);
        }

        // Look for "Current week" section
        if let Some(weekly_pct) = extract_percent_near_label(&clean, "current week (all models)")
            .or_else(|| extract_percent_near_label(&clean, "current week"))
        {
            weekly_percent = Some(weekly_pct);
        }

        // Look for Opus/Sonnet specific
        if let Some(opus_pct) = extract_percent_near_label(&clean, "opus") {
            opus_percent = Some(opus_pct);
        } else if let Some(sonnet_pct) = extract_percent_near_label(&clean, "sonnet") {
            opus_percent = Some(sonnet_pct);
        }

        // Fallback: collect all percentages in order
        if session_percent.is_none() {
            let all_percents = extract_all_percents(&clean);
            if !all_percents.is_empty() {
                session_percent = Some(all_percents[0]);
            }
            if all_percents.len() > 1 && weekly_percent.is_none() {
                weekly_percent = Some(all_percents[1]);
            }
            if all_percents.len() > 2 && opus_percent.is_none() {
                opus_percent = Some(all_percents[2]);
            }
        }

        if session_percent.is_none()
            && weekly_percent.is_none()
            && opus_percent.is_none()
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
        let weekly_reset = extract_reset_description(&clean, "current week (all models)")
            .or_else(|| extract_reset_description(&clean, "current week"));
        let short_form_reset = if is_exhausted_short_form(&clean_lower) {
            extract_inline_reset_description(&clean)
        } else {
            None
        };
        let session_reset = session_reset.or(short_form_reset);

        if session_percent.is_none() && is_exhausted_short_form(&clean_lower) {
            session_percent = Some(100.0);
        }

        // Build usage snapshot
        let session_used = session_percent.unwrap_or(0.0);
        let primary = RateWindow::with_details(
            session_used,
            Some(300), // 5 hour session window
            None,      // Could parse reset time
            session_reset,
        );

        let mut usage = UsageSnapshot::new(primary);

        if let Some(weekly_used) = weekly_percent {
            let secondary = RateWindow::with_details(
                weekly_used,
                Some(10080), // weekly (7 * 24 * 60)
                None,
                weekly_reset,
            );
            usage = usage.with_secondary(secondary);
        }

        if let Some(opus_used) = opus_percent {
            let model_specific = RateWindow::with_details(opus_used, Some(10080), None, None);
            usage = usage.with_model_specific(model_specific);
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

fn claude_auto_fetch_error(failures: Vec<(&'static str, ProviderError)>) -> ProviderError {
    let summary = failures
        .into_iter()
        .map(|(source, error)| format!("{source}: {error}"))
        .collect::<Vec<_>>()
        .join("; ");

    ProviderError::Other(format!(
        "Claude usage failed from all configured sources. {summary}"
    ))
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

/// Try to find the claude CLI binary
fn which_claude() -> Option<std::path::PathBuf> {
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
    let claude_path = which_claude()?;

    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new(claude_path);
    cmd.args(["--version"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().ok()?;

    if output.status.success() {
        let version_str = String::from_utf8_lossy(&output.stdout);
        extract_version(&version_str)
    } else {
        None
    }
}

/// Extract version number from a string like "claude 1.2.3"
fn extract_version(s: &str) -> Option<String> {
    let re = regex_lite::Regex::new(r"(\d+(?:\.\d+)+)").ok()?;
    re.find(s).map(|m| m.as_str().to_string())
}

/// Strip ANSI escape codes from text
fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1B' {
            // Skip CSI sequences: ESC[...letter
            if chars.peek() == Some(&'[') {
                chars.next();
                let mut final_char = None;
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        final_char = Some(next);
                        break;
                    }
                }
                if final_char == Some('C') {
                    result.push(' ');
                }
            // Skip OSC sequences: ESC]...BEL
            } else if chars.peek() == Some(&']') {
                for next in chars.by_ref() {
                    if next == '\x07' || next == '\\' {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
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

fn is_cli_activity_stats_response(text: &str) -> bool {
    let has_activity_overview = text.contains("favorite model:") || text.contains("total tokens:");
    let has_session_cost_summary =
        text.contains("total duration") && text.contains("usage:") && text.contains("cache read");

    has_activity_overview || has_session_cost_summary
}

/// Extract percentage near a label (e.g., "Current session")
/// Returns the percentage as "used" (not remaining)
fn extract_percent_near_label(text: &str, label: &str) -> Option<f64> {
    let label_normalized = normalized_for_label_search(label);
    let lines: Vec<&str> = text.lines().collect();

    // Find the line containing the label
    for (idx, line) in lines.iter().enumerate() {
        if normalized_for_label_search(line).contains(&label_normalized) {
            // Look in the next few lines for a percentage
            for (offset, next_line) in lines.iter().skip(idx).take(12).enumerate() {
                if offset > 0 && starts_next_usage_section(next_line, &label_normalized) {
                    break;
                }
                if let Some(pct) = parse_percent_line(next_line) {
                    return Some(pct);
                }
            }
        }
    }

    None
}

/// Parse a line containing "X% used", "X% left", "X% remaining", etc.
/// Returns the percentage as used (converts "left" to used)
fn parse_percent_line(line: &str) -> Option<f64> {
    // Match patterns like "45% used", "55% left", "55% remaining", or "12.5% available".
    let re =
        Regex::new(r"(\d{1,3}(?:\.\d+)?)\s*%\s*(used|spent|consumed|left|remaining|available)")
            .ok()?;

    if let Some(caps) = re.captures(&line.to_lowercase())
        && let Some(value_match) = caps.get(1)
        && let Some(kind_match) = caps.get(2)
    {
        let value: f64 = value_match.as_str().parse().ok()?;
        let kind = kind_match.as_str();

        // Convert to "used" percentage
        if matches!(kind, "left" | "remaining" | "available") {
            Some((100.0 - value).max(0.0))
        } else {
            Some(value.min(100.0))
        }
    } else {
        None
    }
}

/// Extract all percentages from text in order
fn extract_all_percents(text: &str) -> Vec<f64> {
    let re = match Regex::new(
        r"(\d{1,3}(?:\.\d+)?)\s*%\s*(used|spent|consumed|left|remaining|available)",
    ) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut results = Vec::new();
    let lower = text.to_lowercase();

    for caps in re.captures_iter(&lower) {
        if let (Some(val_match), Some(kind_match)) = (caps.get(1), caps.get(2))
            && let Ok(val) = val_match.as_str().parse::<f64>()
        {
            let kind = kind_match.as_str();
            let used = if matches!(kind, "left" | "remaining" | "available") {
                (100.0 - val).max(0.0)
            } else {
                val.min(100.0)
            };
            results.push(used);
        }
    }

    results
}

fn normalized_for_label_search(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn starts_next_usage_section(line: &str, current_label: &str) -> bool {
    let normalized = normalized_for_label_search(line);
    normalized.starts_with("current") && !normalized.contains(current_label)
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

/// Extract reset description near a label
fn extract_reset_description(text: &str, label: &str) -> Option<String> {
    let label_normalized = normalized_for_label_search(label);
    let lines: Vec<&str> = text.lines().collect();

    for (idx, line) in lines.iter().enumerate() {
        if normalized_for_label_search(line).contains(&label_normalized) {
            // Look in the next few lines for "Resets"
            for (offset, next_line) in lines.iter().skip(idx).take(14).enumerate() {
                if offset > 0 && starts_next_usage_section(next_line, &label_normalized) {
                    break;
                }
                let lower = next_line.to_lowercase();
                if lower.contains("resets") {
                    // Extract the reset info
                    if let Some(pos) = lower.find("resets") {
                        let reset_part = &next_line[pos..];
                        return Some(reset_part.trim().to_string());
                    }
                }
            }
        }
    }

    None
}

/// Extract a "resets ..." suffix from a short single-line status.
fn extract_inline_reset_description(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let pos = lower.find("resets")?;
    Some(text[pos..].trim().to_string())
}

/// Clean up a plan name by removing ANSI codes and extra whitespace
fn clean_plan_name(text: &str) -> String {
    let cleaned = strip_ansi(text);
    // Remove bracketed codes like [22m
    let re = Regex::new(r"\[\d+m").unwrap_or_else(|_| Regex::new(".^").unwrap());
    let result = re.replace_all(&cleaned, "");
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .model_specific
            .expect("sonnet usage should be present");
        assert_eq!(sonnet.used_percent, 1.0);
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
        assert_eq!(
            result
                .usage
                .model_specific
                .expect("sonnet usage should be present")
                .used_percent,
            1.0
        );
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
}
