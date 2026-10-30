//! Login flow runners for various providers
//!
//! Runs CLI login commands and captures output/URLs

use regex_lite::Regex;
use std::io::{BufRead, BufReader, Read};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the watchdog and the exit wait look at the child.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Result of a login attempt
#[derive(Debug, Clone)]
pub struct LoginResult {
    pub outcome: LoginOutcome,
    pub output: String,
    pub auth_link: Option<String>,
}

/// Outcome of login attempt
#[derive(Debug, Clone)]
pub enum LoginOutcome {
    Success,
    TimedOut,
    Failed {
        status: i32,
    },
    MissingBinary,
    LaunchFailed(String),
    /// The cancel flag was set before the CLI finished. The CLI was stopped.
    Cancelled,
}

/// Phase of the login process
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoginPhase {
    Idle,
    Requesting,
    WaitingBrowser,
    Complete,
}

/// Run Kiro CLI login
pub async fn run_kiro_login<F>(timeout_secs: u64, on_phase: F) -> LoginResult
where
    F: Fn(LoginPhase) + Send + 'static,
{
    run_kiro_login_with(
        timeout_secs,
        Arc::new(AtomicBool::new(false)),
        on_phase,
        |_| {},
    )
}

/// Run Kiro CLI login with a cancel flag and a callback for the sign-in link.
///
/// This blocks until the CLI finishes. Callers that need to race a Ctrl-C run
/// it on a blocking thread. Setting `cancel` stops the CLI and returns
/// [`LoginOutcome::Cancelled`]. The CLI is also stopped after `timeout_secs`,
/// even when it prints nothing. `on_link` runs as soon as the CLI prints a URL,
/// before the login finishes.
pub fn run_kiro_login_with<F, L>(
    timeout_secs: u64,
    cancel: Arc<AtomicBool>,
    on_phase: F,
    on_link: L,
) -> LoginResult
where
    F: Fn(LoginPhase) + Send + 'static,
    L: Fn(&str) + Send + 'static,
{
    // Use Kiro's own binary resolver which checks well-known Windows install
    // locations in addition to PATH.
    let binary_path = match crate::providers::kiro::find_kiro_cli() {
        Some(p) => p,
        None => return missing_binary_result("kiro-cli"),
    };

    run_cli_login_path(
        &binary_path,
        &["login"],
        timeout_secs,
        &cancel,
        &on_phase,
        &on_link,
        &[
            "Successfully logged in",
            "Login successful",
            "Logged in successfully",
        ],
    )
}

/// Generic CLI login runner (uses a pre-resolved binary path)
fn run_cli_login_path<F, L>(
    binary_path: &std::path::Path,
    args: &[&str],
    timeout_secs: u64,
    cancel: &Arc<AtomicBool>,
    on_phase: &F,
    on_link: &L,
    success_markers: &[&str],
) -> LoginResult
where
    F: Fn(LoginPhase),
    L: Fn(&str),
{
    on_phase(LoginPhase::Requesting);

    let mut child = match spawn_login_process(binary_path, args) {
        Ok(c) => c,
        Err(e) => return launch_failed_result(e),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // The watchdog owns a handle to the child so it can stop the CLI on cancel
    // or timeout, even while the read below is blocked on a silent pipe.
    let child = Arc::new(Mutex::new(child));
    let done = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));
    spawn_watchdog(
        Arc::clone(&child),
        Arc::clone(cancel),
        Arc::clone(&timed_out),
        Arc::clone(&done),
        Duration::from_secs(timeout_secs),
    );

    let mut state = CliLoginState::new(timeout_secs, on_phase, on_link, success_markers);

    let read_outcome =
        read_login_stream(stdout, &mut state).or_else(|| read_login_stream(stderr, &mut state));
    let outcome = match read_outcome {
        Some(outcome) => {
            kill_child(&child);
            outcome
        }
        None => wait_for_login_exit(&child, on_phase),
    };
    done.store(true, Ordering::SeqCst);

    // A finished sign-in keeps its result. Otherwise a cancel or a timeout
    // explains why the CLI stopped.
    let outcome = if matches!(outcome, LoginOutcome::Success) {
        outcome
    } else if cancel.load(Ordering::SeqCst) {
        LoginOutcome::Cancelled
    } else if timed_out.load(Ordering::SeqCst) {
        LoginOutcome::TimedOut
    } else {
        outcome
    };
    state.into_result(outcome)
}

fn missing_binary_result(binary: &str) -> LoginResult {
    LoginResult {
        outcome: LoginOutcome::MissingBinary,
        output: format!("{} not found in PATH", binary),
        auth_link: None,
    }
}

fn launch_failed_result(error: String) -> LoginResult {
    LoginResult {
        outcome: LoginOutcome::LaunchFailed(error),
        output: String::new(),
        auth_link: None,
    }
}

fn spawn_login_process(binary_path: &std::path::Path, args: &[&str]) -> Result<Child, String> {
    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = Command::new(binary_path);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd.spawn().map_err(|e| e.to_string())
}

/// Stops the CLI when `cancel` is set or `timeout` passes. Sets `timed_out`
/// before a timeout kill. Exits once `done` is set, so a CLI that stays silent
/// is still stopped.
fn spawn_watchdog(
    child: Arc<Mutex<Child>>,
    cancel: Arc<AtomicBool>,
    timed_out: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    timeout: Duration,
) {
    std::thread::spawn(move || {
        let start = Instant::now();
        while !done.load(Ordering::SeqCst) {
            if cancel.load(Ordering::SeqCst) {
                kill_child(&child);
                return;
            }
            if start.elapsed() > timeout {
                timed_out.store(true, Ordering::SeqCst);
                kill_child(&child);
                return;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

/// Best-effort teardown; the child may already have exited.
fn kill_child(child: &Mutex<Child>) {
    if let Ok(mut child) = child.lock() {
        let _killed = child.kill();
    }
}

struct CliLoginState<'a, F>
where
    F: Fn(LoginPhase),
{
    output: String,
    auth_link: Option<String>,
    url_regex: Regex,
    on_phase: &'a F,
    on_link: &'a dyn Fn(&str),
    success_markers: &'a [&'a str],
    start: Instant,
    timeout: Duration,
}

impl<'a, F> CliLoginState<'a, F>
where
    F: Fn(LoginPhase),
{
    fn new(
        timeout_secs: u64,
        on_phase: &'a F,
        on_link: &'a dyn Fn(&str),
        success_markers: &'a [&'a str],
    ) -> Self {
        Self {
            output: String::new(),
            auth_link: None,
            url_regex: Regex::new(r"https?://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+").unwrap(),
            on_phase,
            on_link,
            success_markers,
            start: Instant::now(),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    fn handle_line(&mut self, line: &str) -> Option<LoginOutcome> {
        self.output.push_str(line);
        self.output.push('\n');
        self.capture_auth_link(line);

        if self
            .success_markers
            .iter()
            .any(|marker| line.contains(marker))
        {
            (self.on_phase)(LoginPhase::Complete);
            return Some(LoginOutcome::Success);
        }

        self.start
            .elapsed()
            .gt(&self.timeout)
            .then_some(LoginOutcome::TimedOut)
    }

    fn capture_auth_link(&mut self, line: &str) {
        if self.auth_link.is_some() {
            return;
        }

        let Some(m) = self.url_regex.find(line) else {
            return;
        };

        self.auth_link = Some(m.as_str().to_string());
        (self.on_link)(m.as_str());
        (self.on_phase)(LoginPhase::WaitingBrowser);
        open_auth_link(m.as_str());
    }

    fn into_result(self, outcome: LoginOutcome) -> LoginResult {
        LoginResult {
            outcome,
            output: self.output,
            auth_link: self.auth_link,
        }
    }
}

/// Best-effort browser open; the link stays on screen for manual use.
/// Detached because `open::that` waits for `cmd /c start`, and a stalled URL
/// handler would then block the read loop and with it cancel and timeout.
/// Tests drive a fake CLI that prints a sign-in link, so they never open it.
fn open_auth_link(url: &str) {
    #[cfg(not(test))]
    {
        let _opened_browser = open::that_detached(url);
    }
    #[cfg(test)]
    let _ = url;
}

fn read_login_stream<R, F>(
    stream: Option<R>,
    state: &mut CliLoginState<'_, F>,
) -> Option<LoginOutcome>
where
    R: Read,
    F: Fn(LoginPhase),
{
    let reader = BufReader::new(stream?);
    reader
        .lines()
        .map_while(Result::ok)
        .find_map(|line| state.handle_line(&line))
}

/// Waits for the CLI to exit without holding the lock across the wait, so the
/// watchdog can still kill it.
fn wait_for_login_exit<F>(child: &Mutex<Child>, on_phase: &F) -> LoginOutcome
where
    F: Fn(LoginPhase),
{
    loop {
        let status = match child.lock() {
            Ok(mut child) => child.try_wait(),
            Err(_) => {
                return LoginOutcome::LaunchFailed("login process lock poisoned".to_string());
            }
        };
        match status {
            Ok(Some(status)) => {
                if status.success() {
                    on_phase(LoginPhase::Complete);
                    return LoginOutcome::Success;
                }
                return LoginOutcome::Failed {
                    status: status.code().unwrap_or(-1),
                };
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(e) => return LoginOutcome::LaunchFailed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `fake_kiro_cli` is a cargo example. `cargo test` builds it into
    /// `target/<profile>/examples`, next to the `deps` folder of this test binary.
    fn fake_kiro_cli() -> PathBuf {
        let exe = std::env::current_exe().expect("test binary path");
        let profile_dir = exe
            .parent()
            .and_then(std::path::Path::parent)
            .expect("profile directory");
        profile_dir
            .join("examples")
            .join(format!("fake_kiro_cli{}", std::env::consts::EXE_SUFFIX))
    }

    fn run(
        binary: &std::path::Path,
        mode: &str,
        timeout_secs: u64,
        cancel: Arc<AtomicBool>,
    ) -> LoginResult {
        let markers = ["Successfully logged in"];
        run_cli_login_path(
            binary,
            &["login", mode],
            timeout_secs,
            &cancel,
            &|_: LoginPhase| {},
            &|_: &str| {},
            &markers,
        )
    }

    #[test]
    fn success_marker_finishes_and_keeps_the_sign_in_link() {
        let binary = fake_kiro_cli();
        assert!(
            binary.is_file(),
            "build the example first: cargo build --manifest-path rust/Cargo.toml --example fake_kiro_cli"
        );
        let result = run(&binary, "success", 30, Arc::new(AtomicBool::new(false)));
        assert!(matches!(result.outcome, LoginOutcome::Success));
        assert_eq!(
            result.auth_link.as_deref(),
            Some("https://example.com/device?code=ABCD-1234")
        );
    }

    #[test]
    fn nonzero_exit_is_a_failure_with_its_status() {
        let binary = fake_kiro_cli();
        let result = run(&binary, "fail", 30, Arc::new(AtomicBool::new(false)));
        assert!(matches!(result.outcome, LoginOutcome::Failed { status: 3 }));
    }

    #[test]
    fn cancel_stops_a_cli_that_is_waiting_for_the_browser() {
        let binary = fake_kiro_cli();
        let cancel = Arc::new(AtomicBool::new(false));
        let setter = Arc::clone(&cancel);
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            setter.store(true, Ordering::SeqCst);
        });
        let result = run(&binary, "wait", 30, cancel);
        assert!(matches!(result.outcome, LoginOutcome::Cancelled));
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "cancel must not wait for the CLI"
        );
    }

    #[test]
    fn a_silent_cli_times_out() {
        let binary = fake_kiro_cli();
        let started = Instant::now();
        let result = run(&binary, "silent", 1, Arc::new(AtomicBool::new(false)));
        assert!(matches!(result.outcome, LoginOutcome::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn a_missing_binary_is_a_launch_failure_not_a_hang() {
        let missing = std::env::temp_dir().join("codexbar-no-such-kiro-cli.exe");
        let result = run(&missing, "success", 30, Arc::new(AtomicBool::new(false)));
        assert!(matches!(result.outcome, LoginOutcome::LaunchFailed(_)));
    }
}
