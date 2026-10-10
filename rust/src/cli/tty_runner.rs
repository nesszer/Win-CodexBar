//! TTY Command Runner
//!
//! Executes interactive CLI commands using the platform pseudo-console.
//! Provides PTY-like functionality for capturing output from interactive TUI programs.

use super::tty_responder::{ResponderState, ScreenResponder};
use crate::process_environment::ProcessEnvironment;
use regex_lite::Regex;
use std::collections::HashMap;
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::{Command, Stdio};
use std::sync::{LazyLock, mpsc};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Result of running a TTY command
#[derive(Debug, Clone)]
pub struct TtyCommandResult {
    /// Captured output text
    pub text: String,
    /// Whether the command was interrupted early
    pub stopped_early: bool,
    /// URLs detected in output (if stop_on_url was set)
    pub detected_urls: Vec<String>,
}

impl TtyCommandResult {
    /// Extract the first URL found in the output
    pub fn first_url(&self) -> Option<&str> {
        self.detected_urls.first().map(|s| s.as_str())
    }
}

/// Options for running TTY commands
#[derive(Debug, Clone)]
pub struct TtyCommandOptions {
    /// Terminal rows (default: 50)
    pub rows: u16,
    /// Terminal columns (default: 160)
    pub cols: u16,
    /// Overall timeout in seconds (default: 20)
    pub timeout_secs: f64,
    /// Idle timeout - stop if no output for this duration (optional)
    pub idle_timeout_secs: Option<f64>,
    /// Working directory
    pub working_directory: Option<PathBuf>,
    /// Extra arguments to pass to the command
    pub extra_args: Vec<String>,
    /// Initial delay before sending script (default: 0.4s)
    pub initial_delay_secs: f64,
    /// Delay between script characters (default: 0s)
    pub script_char_delay_secs: f64,
    /// Delay between script lines (default: 0s)
    pub script_line_delay_secs: f64,
    /// Send enter/return every N seconds (optional)
    pub send_enter_every_secs: Option<f64>,
    /// Map of substrings to keys to send when detected
    pub send_on_substrings: HashMap<String, String>,
    /// Screen-aware responder, consulted on all output since its last answer
    pub screen_responder: Option<ScreenResponder>,
    /// Stop early when a URL is detected
    pub stop_on_url: bool,
    /// Stop early when any of these substrings are detected
    pub stop_on_substrings: Vec<String>,
    /// Settle time after stopping (default: 0.25s)
    pub settle_after_stop_secs: f64,
    /// Re-send the script at these offsets (seconds since launch) while none
    /// of `script_done_substrings` has appeared in the output yet. Interactive
    /// CLIs such as Claude Code drop keystrokes that arrive before their input
    /// widget is mounted, and that readiness delay varies between machines.
    pub script_retry_delays_secs: Vec<f64>,
    /// Case-insensitive markers that show the script was accepted (stops retries).
    pub script_done_substrings: Vec<String>,
    /// Case-insensitive markers that show the script text was at least echoed
    /// into the input widget; a retry is skipped while one is visible so the
    /// same text is not typed twice into a half-processed line.
    pub script_echo_substrings: Vec<String>,
    /// Shorter idle timeout used once a done marker is visible: the answer
    /// is on screen, so only trailing output is awaited (optional).
    pub idle_timeout_after_done_secs: Option<f64>,
    /// Environment variables to set (`Debug` renders only the entry count)
    pub env: ProcessEnvironment<HashMap<String, String>>,
}

impl Default for TtyCommandOptions {
    fn default() -> Self {
        Self {
            rows: 50,
            cols: 160,
            timeout_secs: 20.0,
            idle_timeout_secs: None,
            working_directory: None,
            extra_args: Vec::new(),
            initial_delay_secs: 0.4,
            script_char_delay_secs: 0.0,
            script_line_delay_secs: 0.0,
            send_enter_every_secs: None,
            send_on_substrings: HashMap::new(),
            screen_responder: None,
            stop_on_url: false,
            stop_on_substrings: Vec::new(),
            settle_after_stop_secs: 0.25,
            script_retry_delays_secs: Vec::new(),
            script_done_substrings: Vec::new(),
            script_echo_substrings: Vec::new(),
            idle_timeout_after_done_secs: None,
            env: ProcessEnvironment::default(),
        }
    }
}

impl TtyCommandOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(mut self, secs: f64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn with_idle_timeout(mut self, secs: f64) -> Self {
        self.idle_timeout_secs = Some(secs);
        self
    }

    pub fn with_initial_delay(mut self, secs: f64) -> Self {
        self.initial_delay_secs = secs;
        self
    }

    pub fn with_script_char_delay(mut self, secs: f64) -> Self {
        self.script_char_delay_secs = secs;
        self
    }

    pub fn with_script_line_delay(mut self, secs: f64) -> Self {
        self.script_line_delay_secs = secs;
        self
    }

    pub fn with_working_directory(mut self, dir: PathBuf) -> Self {
        self.working_directory = Some(dir);
        self
    }

    pub fn with_extra_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    pub fn with_stop_on_url(mut self, stop: bool) -> Self {
        self.stop_on_url = stop;
        self
    }

    pub fn with_stop_on_substring(mut self, substring: impl Into<String>) -> Self {
        self.stop_on_substrings.push(substring.into());
        self
    }

    pub fn with_script_retries(
        mut self,
        delays_secs: Vec<f64>,
        done_substrings: Vec<String>,
        echo_substrings: Vec<String>,
    ) -> Self {
        self.script_retry_delays_secs = delays_secs;
        self.script_done_substrings = done_substrings
            .into_iter()
            .map(|marker| marker.to_lowercase())
            .collect();
        self.script_echo_substrings = echo_substrings
            .into_iter()
            .map(|marker| marker.to_lowercase())
            .collect();
        self
    }

    pub fn with_idle_timeout_after_done(mut self, secs: f64) -> Self {
        self.idle_timeout_after_done_secs = Some(secs);
        self
    }

    pub fn with_screen_responder(mut self, responder: ScreenResponder) -> Self {
        self.screen_responder = Some(responder);
        self
    }

    pub fn with_send_on_substring(
        mut self,
        trigger: impl Into<String>,
        keys: impl Into<String>,
    ) -> Self {
        self.send_on_substrings.insert(trigger.into(), keys.into());
        self
    }
}

/// Errors from TTY command execution
#[derive(Debug, Error)]
pub enum TtyCommandError {
    #[error("Binary not found: {0}. Install it or add it to PATH.")]
    BinaryNotFound(String),

    #[error("Failed to launch process: {0}")]
    LaunchFailed(String),

    #[error("Command timed out")]
    TimedOut,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Process error: {0}")]
    ProcessError(String),
}

/// TTY Command Runner
///
/// On Windows, this uses standard process I/O with some heuristics for
/// interactive programs. For true PTY support, consider using the
/// `conpty` crate or Windows ConPTY APIs directly.
pub struct TtyCommandRunner;

impl TtyCommandRunner {
    /// Create a new runner instance
    pub fn new() -> Self {
        Self
    }

    /// Locate a binary using the system PATH
    pub fn which(tool: &str) -> Option<PathBuf> {
        // Check for specific tool overrides
        if tool == "codex"
            && let Some(path) = crate::codex_cli::locate_codex_binary()
        {
            return Some(path);
        }
        if tool == "claude"
            && let Some(path) = crate::providers::claude::locate_claude_binary()
        {
            return Some(path);
        }

        // Use `where` on Windows (equivalent to `which` on Unix)
        Self::run_where(tool)
    }

    /// Find a binary in PATH.
    fn run_where(tool: &str) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;

            let mut command = Command::new("where");
            command
                .arg(tool)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW);
            let output = command.output().ok()?;

            if !output.status.success() {
                return None;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let first_line = stdout.lines().next()?.trim();
            if first_line.is_empty() {
                return None;
            }

            Some(PathBuf::from(first_line))
        }

        #[cfg(not(windows))]
        {
            which::which(tool).ok()
        }
    }

    fn is_explicit_binary_path(binary: &str) -> bool {
        let path = std::path::Path::new(binary);
        path.is_absolute() || path.components().count() > 1
    }

    /// Run a command and capture its output
    ///
    /// Uses the platform pseudo-terminal implementation, including Windows
    /// ConPTY, so interactive programs see a real terminal.
    ///
    /// run() may block up to `settle_after_stop_secs` after child exit to
    /// drain remaining output.
    pub fn run(
        &self,
        binary: &str,
        script: &str,
        options: TtyCommandOptions,
    ) -> Result<TtyCommandResult, TtyCommandError> {
        // Resolve the binary path
        let resolved = if Self::is_explicit_binary_path(binary) {
            let path = PathBuf::from(binary);
            if path.exists() {
                path
            } else {
                return Err(TtyCommandError::BinaryNotFound(binary.to_string()));
            }
        } else if let Some(path) = Self::which(binary) {
            path
        } else {
            return Err(TtyCommandError::BinaryNotFound(binary.to_string()));
        };

        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(portable_pty::PtySize {
                rows: options.rows,
                cols: options.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TtyCommandError::LaunchFailed(e.to_string()))?;

        let mut cmd = portable_pty::CommandBuilder::new(resolved.as_os_str());
        cmd.args(&options.extra_args);

        if let Some(ref dir) = options.working_directory {
            cmd.cwd(dir.as_os_str());
        }

        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (key, value) in &options.env {
            cmd.env(key, value);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TtyCommandError::LaunchFailed(e.to_string()))?;
        drop(pair.slave);
        let session_tree = SessionTree::contain(child.as_ref());

        self.run_pty_session(pair.master, &mut child, session_tree, script, &options)
    }

    /// Run an interactive session with the child process
    fn run_pty_session(
        &self,
        master: Box<dyn portable_pty::MasterPty + Send>,
        child: &mut Box<dyn portable_pty::Child + Send + Sync>,
        session_tree: SessionTree,
        script: &str,
        options: &TtyCommandOptions,
    ) -> Result<TtyCommandResult, TtyCommandError> {
        let start = Instant::now();
        let timeout = Duration::from_secs_f64(options.timeout_secs);
        let idle_timeout = options.idle_timeout_secs.map(Duration::from_secs_f64);
        let settle = Duration::from_secs_f64(options.settle_after_stop_secs);

        let mut buffer = String::new();
        let mut stopped_early = false;
        let mut detected_urls = Vec::new();
        let mut last_output_time;
        let mut triggered_sends = std::collections::HashSet::new();

        // URL detection regex
        let url_regex = Regex::new(r"https?://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+").ok();

        // Set up non-blocking readers using channels
        let (tx, rx) = mpsc::channel::<String>();
        let mut writer = master
            .take_writer()
            .map_err(|e| TtyCommandError::LaunchFailed(e.to_string()))?;
        let mut reader = master
            .try_clone_reader()
            .map_err(|e| TtyCommandError::LaunchFailed(e.to_string()))?;

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(s) = String::from_utf8(buf[..n].to_vec()) {
                            // Best-effort send; the channel is dropped once the main loop exits.
                            let _sent = tx.send(s);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Initial delay. Keep draining output while waiting so a terminal
        // capability query (ConPTY Device Status Report) sent during startup
        // is answered instead of buffered silently; some TUIs block their
        // input widget until the cursor position reply arrives.
        let initial_deadline = start + Duration::from_secs_f64(options.initial_delay_secs);
        loop {
            let remaining = initial_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
                Ok(chunk) => {
                    tracing::trace!(
                        elapsed_ms = elapsed_ms(start),
                        bytes = chunk.len(),
                        "tty session: startup chunk"
                    );
                    buffer.push_str(&chunk);
                    answer_cursor_query(&chunk, &mut writer);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        tracing::trace!(
            elapsed_ms = elapsed_ms(start),
            buffered = buffer.len(),
            "tty session: sending script"
        );

        // Output that finished rendering inside the initial delay produces no
        // later chunk, so triggers must also be checked against it here.
        fire_substring_triggers(options, &buffer, &mut triggered_sends, &mut writer);
        let mut responder = ResponderState::default();
        if let Some(screen_responder) = &options.screen_responder {
            responder.answer(screen_responder, &buffer, &mut writer);
        }

        // Send the script if provided. PTYs expect carriage-return line endings
        // for interactive programs to treat writes like pressing Enter.
        let script_lines: Vec<&str> = script
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.trim().is_empty())
            .collect();
        write_script_lines(&mut writer, &script_lines, options);
        // The idle timer measures silence after our input, not startup time.
        last_output_time = Instant::now();
        let mut pending_script_retries: Vec<Duration> = options
            .script_retry_delays_secs
            .iter()
            .map(|secs| Duration::from_secs_f64(*secs))
            .collect();
        pending_script_retries.sort();
        let mut next_script_retry = pending_script_retries.into_iter();
        let mut upcoming_script_retry = next_script_retry.next();
        // The buffer only grows, so a done marker stays visible once it
        // appeared: rescan only after new output, and never after a match.
        let mut accepted = script_accepted(&buffer, options);

        let mut last_enter = Instant::now();

        // Main read loop
        loop {
            // Check timeout
            if start.elapsed() > timeout {
                break;
            }

            // Check idle timeout (shorter once the done marker is on screen)
            if !buffer.is_empty() {
                let done_idle = options
                    .idle_timeout_after_done_secs
                    .filter(|_| accepted)
                    .map(Duration::from_secs_f64);
                let effective_idle = match (idle_timeout, done_idle) {
                    (Some(idle), Some(done)) => Some(idle.min(done)),
                    (idle, done) => idle.or(done),
                };
                if let Some(idle) = effective_idle
                    && last_output_time.elapsed() > idle
                {
                    stopped_early = true;
                    break;
                }
            }

            // Check if process has exited
            if let Ok(Some(_)) = child.try_wait() {
                // Process exited; the reader thread may still be forwarding
                // the final bytes, so drain with bounded blocking receives
                // instead of one nonblocking sweep that can race the reader
                // and lose them. Disconnected ends the drain immediately;
                // the overall deadline bounds it so a reader held open by
                // inherited handles cannot hang.
                let drain_deadline = Instant::now() + settle;
                loop {
                    let remaining = drain_deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
                        Ok(chunk) => buffer.push_str(&chunk),
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        // Quiet slice: keep waiting until the deadline so
                        // slowly forwarded tail output is not lost.
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                break;
            }

            // Read available output
            let mut received_output = false;
            while let Ok(chunk) = rx.try_recv() {
                received_output = true;
                tracing::trace!(
                    elapsed_ms = elapsed_ms(start),
                    bytes = chunk.len(),
                    "tty session: output chunk"
                );
                buffer.push_str(&chunk);
                last_output_time = Instant::now();
                answer_cursor_query(&chunk, &mut writer);

                // Check for URLs
                if let Some(ref regex) = url_regex {
                    for mat in regex.find_iter(&chunk) {
                        let mut url = mat.as_str().to_string();
                        // Trim trailing punctuation
                        while url.ends_with(|c| {
                            matches!(
                                c,
                                '.' | ',' | ';' | ':' | ')' | ']' | '}' | '>' | '"' | '\''
                            )
                        }) {
                            url.pop();
                        }
                        if !detected_urls.contains(&url) {
                            detected_urls.push(url);
                        }
                    }

                    if options.stop_on_url && !detected_urls.is_empty() {
                        stopped_early = true;
                        break;
                    }
                }

                // Check for stop substrings
                for stop_str in &options.stop_on_substrings {
                    if buffer.contains(stop_str) {
                        stopped_early = true;
                        break;
                    }
                }

                // Check for send triggers
                fire_substring_triggers(options, &buffer, &mut triggered_sends, &mut writer);
            }

            if received_output && let Some(screen_responder) = &options.screen_responder {
                responder.answer(screen_responder, &buffer, &mut writer);
            }

            if stopped_early {
                break;
            }

            if received_output && !accepted {
                accepted = script_accepted(&buffer, options);
            }

            // Re-send the script while the target program has not shown that it
            // accepted the first attempt (input widget mounted late).
            if let Some(retry_at) = upcoming_script_retry
                && start.elapsed() >= retry_at
            {
                upcoming_script_retry = next_script_retry.next();
                if !script_lines.is_empty() && !accepted {
                    if script_echoed(&buffer, options) {
                        // Text arrived but Enter was swallowed: only confirm.
                        let _enter_written = write!(writer, "\r\n");
                        let _enter_flushed = writer.flush();
                    } else {
                        write_script_lines(&mut writer, &script_lines, options);
                    }
                    // As after the first attempt, the idle timer measures
                    // silence after this input: a quiet screen must not end
                    // the run before the remaining retries had their turn.
                    last_output_time = Instant::now();
                }
            }

            // Send periodic enters if configured
            if let Some(interval) = options.send_enter_every_secs
                && last_enter.elapsed() >= Duration::from_secs_f64(interval)
            {
                // Best-effort periodic Enter keepalive.
                let _enter_written = write!(writer, "\r\n");
                // Best-effort flush after the periodic Enter.
                let _enter_flushed = writer.flush();
                last_enter = Instant::now();
            }

            // Small sleep to avoid busy loop
            std::thread::sleep(Duration::from_millis(50));
        }

        // Settle period - collect remaining output
        if stopped_early {
            let settle_start = Instant::now();
            while settle_start.elapsed() < settle {
                while let Ok(chunk) = rx.try_recv() {
                    buffer.push_str(&chunk);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        if child.try_wait().ok().flatten().is_none() {
            tracing::trace!(
                elapsed_ms = elapsed_ms(start),
                "tty session: killing child tree"
            );
            // On Windows several CLIs (claude.exe, npm shims) are launchers
            // whose real process is a child; killing only the launcher leaves
            // that child alive, holding the PTY session and, for Claude, the
            // fixed --session-id ("already in use" on the next probe). Tear
            // down the whole tree first.
            session_tree.terminate();
            // Best-effort kill; the process may have exited on its own.
            let _killed = child.kill();
            // Best-effort reap; the exit status is intentionally discarded.
            let _reaped = child.wait();
            tracing::trace!(elapsed_ms = elapsed_ms(start), "tty session: child reaped");
        }

        if buffer.is_empty() && !stopped_early {
            return Err(TtyCommandError::TimedOut);
        }

        Ok(TtyCommandResult {
            text: buffer,
            stopped_early,
            detected_urls,
        })
    }

    /// Get enriched PATH for finding CLI tools
    pub fn enriched_path() -> String {
        let mut paths = Vec::new();

        if let Some(home) = dirs::home_dir() {
            #[cfg(windows)]
            paths.push(home.join("AppData").join("Roaming").join("npm"));
            #[cfg(not(windows))]
            paths.push(home.join(".local").join("bin"));
            paths.push(home.join(".bun").join("bin"));
            paths.push(home.join(".deno").join("bin"));
            paths.push(home.join(".cargo").join("bin"));
        }

        #[cfg(windows)]
        if let Some(local) = dirs::data_local_dir() {
            paths.push(local.join("npm"));
        }

        let mut all_paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists()).collect();

        if let Some(current_path) = std::env::var_os("PATH") {
            all_paths.extend(std::env::split_paths(&current_path));
        }

        std::env::join_paths(all_paths)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|_| std::env::var("PATH").unwrap_or_default())
    }

    /// Get enriched environment for CLI commands
    pub fn enriched_environment() -> HashMap<String, String> {
        let mut env: HashMap<String, String> = std::env::vars().collect();

        env.insert("PATH".to_string(), Self::enriched_path());
        env.entry("TERM".to_string())
            .or_insert_with(|| "xterm-256color".to_string());
        env.entry("COLORTERM".to_string())
            .or_insert_with(|| "truecolor".to_string());

        if let Some(home) = dirs::home_dir() {
            env.entry("HOME".to_string())
                .or_insert_with(|| home.to_string_lossy().to_string());
            env.entry("USERPROFILE".to_string())
                .or_insert_with(|| home.to_string_lossy().to_string());
        }

        env
    }
}

impl Default for TtyCommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Milliseconds since `start`, saturating, for trace fields.
fn elapsed_ms(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Some Windows ConPTY-backed shells issue an ANSI Device Status Report
/// request and wait for a cursor position response before processing input;
/// answer it best-effort (a dead PTY ignores the write).
fn answer_cursor_query(chunk: &str, writer: &mut impl Write) {
    if chunk.contains("\x1b[6n") {
        let _cursor_reply = write!(writer, "\x1b[1;1R");
        let _cursor_flushed = writer.flush();
    }
}

/// Type the script lines into the PTY, honouring the configured delays.
/// Send every not-yet-fired `send_on_substrings` entry whose trigger is in `buffer`.
fn fire_substring_triggers(
    options: &TtyCommandOptions,
    buffer: &str,
    triggered: &mut std::collections::HashSet<String>,
    writer: &mut impl Write,
) {
    for (trigger, keys) in &options.send_on_substrings {
        if !triggered.contains(trigger) && buffer.contains(trigger) {
            let normalized = keys.replace('\n', "\r\n");
            // Best-effort send-trigger input; a closed PTY drops the write.
            let _trigger_written = write!(writer, "{}", normalized);
            // Best-effort flush after a send-trigger write.
            let _trigger_flushed = writer.flush();
            triggered.insert(trigger.clone());
        }
    }
}

fn write_script_lines(
    writer: &mut Box<dyn Write + Send>,
    script_lines: &[&str],
    options: &TtyCommandOptions,
) {
    write_script_lines_impl(writer, script_lines, options)
}

/// The PTY child and every process it starts, held in a kill-on-close Job
/// Object on Windows. The job closes when the session ends, so nothing the
/// session started outlives it. Closing the job reaches only processes this
/// session created; a parent-PID walk (`taskkill /T`) can also reach unrelated
/// processes whose recorded parent PID was reused. Elsewhere, and when the
/// child could not be contained, only the direct child is killed.
struct SessionTree {
    #[cfg(windows)]
    job: Option<crate::managed_process::ProcessJob>,
}

impl SessionTree {
    /// Contain a freshly spawned child; descendants it starts from now on
    /// join the job automatically.
    #[cfg(windows)]
    fn contain(child: &(dyn portable_pty::Child + Send + Sync)) -> Self {
        let job = child.as_raw_handle().and_then(|handle| {
            crate::managed_process::ProcessJob::create("tty-session")
                .and_then(|job| job.contain(handle).map(|()| job))
                .inspect_err(|error| {
                    tracing::debug!(%error, "tty session child could not be job-contained");
                })
                .ok()
        });
        Self { job }
    }

    #[cfg(not(windows))]
    fn contain(_child: &(dyn portable_pty::Child + Send + Sync)) -> Self {
        Self {}
    }

    /// Terminate every contained process (closing the kill-on-close job).
    fn terminate(self) {
        #[cfg(windows)]
        drop(self.job);
    }
}

fn write_script_lines_impl(
    writer: &mut Box<dyn Write + Send>,
    script_lines: &[&str],
    options: &TtyCommandOptions,
) {
    for (idx, line) in script_lines.iter().enumerate() {
        if options.script_char_delay_secs > 0.0 {
            for ch in line.chars() {
                // Best-effort scripted input; a closed PTY just drops the write.
                let _typed = write!(writer, "{}", ch);
                // Best-effort flush; failures surface only as missing script output.
                let _flushed = writer.flush();
                std::thread::sleep(Duration::from_secs_f64(options.script_char_delay_secs));
            }
            // Best-effort line terminator; interactive programs expect CRLF.
            let _newline_written = write!(writer, "\r\n");
        } else {
            // Best-effort whole-line write for the no-delay path.
            let _line_written = write!(writer, "{}\r\n", line);
        }
        // Best-effort flush per script line.
        let _line_flushed = writer.flush();
        if idx + 1 < script_lines.len() && options.script_line_delay_secs > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(options.script_line_delay_secs));
        }
    }
}

/// True once any configured done marker is visible in the (ANSI-stripped) output.
fn script_accepted(buffer: &str, options: &TtyCommandOptions) -> bool {
    contains_any_marker(buffer, &options.script_done_substrings)
}

/// True while the typed script text is visible in the output (echoed input).
fn script_echoed(buffer: &str, options: &TtyCommandOptions) -> bool {
    contains_any_marker(buffer, &options.script_echo_substrings)
}

/// CSI sequences (colors, cursor moves) dropped before marker matching.
static ANSI_CSI: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").ok());

fn contains_any_marker(buffer: &str, markers: &[String]) -> bool {
    if markers.is_empty() {
        return false;
    }
    let clean = ANSI_CSI
        .as_ref()
        .map(|re| re.replace_all(buffer, "").to_string())
        .unwrap_or_else(|| buffer.to_string())
        .to_lowercase();
    markers.iter().any(|marker| clean.contains(marker))
}

#[cfg(test)]
mod tests;
