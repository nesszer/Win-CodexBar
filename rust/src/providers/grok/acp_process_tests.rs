//! End-to-end ACP tests against a real child process. `examples/fake_grok_acp`
//! stands in for `grok agent stdio`, and every call goes through the production
//! `spawn_child` path: hidden console, piped stdio, stderr discarded.
//!
//! `cargo test` builds the example next to the test binary. The fake writes
//! `"<pid> <console>"` to a pid file before its handshake, so each test can see
//! whether the child was hidden and whether it is gone afterwards.

use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use super::{AcpTimeouts, fetch_billing};
use crate::core::{FetchContext, ProviderError};
use crate::providers::grok::accounts::GrokAuthKind;
use crate::providers::grok::credits_proxy::BearerBilling;
use crate::providers::grok::{GrokCredentials, GrokProvider};

/// Budgets for tests that expect a real answer. Process start-up is slow on a
/// loaded machine, so these are far above the production budgets.
fn generous() -> AcpTimeouts {
    AcpTimeouts {
        initialize: Duration::from_secs(20),
        billing: Duration::from_secs(20),
    }
}

/// Budgets for tests that expect a timeout. The initialize budget starts after
/// spawn, so it also covers the child's own start-up before it writes its
/// record: at 300 ms a loaded CI runner could kill the child before that write,
/// leaving nothing to check the kill against.
fn short() -> AcpTimeouts {
    AcpTimeouts {
        initialize: Duration::from_secs(3),
        billing: Duration::from_millis(300),
    }
}

fn fake_grok() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    // target/<profile>/deps/<test>.exe -> target/<profile>/examples/fake_grok_acp.exe
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("cargo target layout");
    let program = profile_dir
        .join("examples")
        .join(format!("fake_grok_acp{}", std::env::consts::EXE_SUFFIX));
    assert!(
        program.is_file(),
        "{} is missing; cargo test builds the fake_grok_acp example",
        program.display()
    );
    program
}

/// What the fake child recorded about itself before the handshake.
struct ChildRecord {
    pid: u32,
    console: bool,
}

/// One fake child's scratch directory and pid file.
struct FakeRun {
    _dir: tempfile::TempDir,
    pid_file: PathBuf,
}

impl FakeRun {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let pid_file = dir.path().join("child.txt");
        Self {
            _dir: dir,
            pid_file,
        }
    }

    /// Arguments for `agent stdio <scenario> <pid-file>`.
    fn args(&self, scenario: &str) -> Vec<String> {
        vec![
            "agent".to_string(),
            "stdio".to_string(),
            scenario.to_string(),
            self.pid_file.to_string_lossy().into_owned(),
        ]
    }

    /// `None` when the child never started far enough to write its record.
    fn record(&self) -> Option<ChildRecord> {
        let text = std::fs::read_to_string(&self.pid_file).ok()?;
        let mut parts = text.split_whitespace();
        Some(ChildRecord {
            pid: parts.next()?.parse().ok()?,
            console: parts.next()? == "1",
        })
    }
}

async fn run_scenario(
    scenario: &str,
    timeouts: AcpTimeouts,
) -> (Result<BearerBilling, ProviderError>, FakeRun) {
    let run = FakeRun::new();
    let args = run.args(scenario);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = fetch_billing(&fake_grok(), &refs, timeouts).await;
    (outcome, run)
}

fn expect_error<T>(outcome: Result<T, ProviderError>) -> ProviderError {
    match outcome {
        Ok(_) => panic!("expected the ACP call to fail"),
        Err(error) => error,
    }
}

/// The child was killed and reaped: its process object is gone or has exited.
async fn assert_child_stopped(record: &ChildRecord) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_is_running(record.pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !process_is_running(record.pid),
        "child {} is still running after the call returned",
        record.pid
    );
}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: the handle is opened, queried and closed here, and never escapes.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let queried = GetExitCodeProcess(handle, &mut code).is_ok();
        CloseHandle(handle).ok();
        queried && code == STILL_ACTIVE.0 as u32
    }
}

/// Linux only: a reaped child has no `/proc` entry.
#[cfg(not(windows))]
fn process_is_running(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn missing_login_error() -> ProviderError {
    ProviderError::NotInstalled("Grok auth.json not found. Run `grok login`.".to_string())
}

/// Guards the no-zombie assertions: the probe must see a live child as running
/// and a killed, reaped one as stopped. Otherwise `assert_child_stopped` would
/// pass for the wrong reason.
#[test]
fn the_liveness_probe_tells_a_running_child_from_a_stopped_one() {
    let run = FakeRun::new();
    // Stdin stays open, so the child blocks on its first read and keeps running.
    let mut command = StdCommand::new(fake_grok());
    command
        .args(run.args("hang_init"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // A console-less test runner would otherwise give the child a visible console.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().expect("probe child starts");
    let deadline = Instant::now() + Duration::from_secs(10);
    let record = loop {
        if let Some(record) = run.record() {
            break record;
        }
        assert!(Instant::now() < deadline, "probe child never started");
        std::thread::sleep(Duration::from_millis(10));
    };

    assert!(process_is_running(record.pid), "a live child must be seen");
    child.kill().expect("probe child is killed");
    child.wait().expect("probe child is reaped");
    assert!(
        !process_is_running(record.pid),
        "a reaped child must not be seen as running"
    );
}

// ---------------------------------------------------------------- success

#[tokio::test]
async fn credits_answer_comes_back_from_a_real_child_which_is_hidden_and_stopped() {
    let (outcome, run) = run_scenario("ok", generous()).await;

    let bearer = outcome.expect("billing answer");
    assert_eq!(bearer.billing.used_percent, Some(42.0));
    assert_eq!(bearer.subscription_tier.as_deref(), Some("SuperGrok Heavy"));
    assert!(bearer.billing.resets_at.is_some());

    let record = run.record().expect("child recorded itself");
    assert!(
        !record.console,
        "the production spawn must hide the console window"
    );
    assert_child_stopped(&record).await;
}

/// Control for the hidden-spawn check: a child started without
/// `CREATE_NO_WINDOW` inherits this test's console. If this test has no console,
/// the control proves nothing and is skipped.
#[cfg(windows)]
#[test]
fn the_console_probe_reports_a_console_when_one_is_inherited() {
    use windows::Win32::System::Console::GetConsoleWindow;

    // SAFETY: GetConsoleWindow takes no arguments and only reads state.
    let parent_has_console = !unsafe { GetConsoleWindow() }.0.is_null();
    if !parent_has_console {
        eprintln!("skipped: the test process has no console to inherit");
        return;
    }
    let run = FakeRun::new();
    let status = std::process::Command::new(fake_grok())
        .args(run.args("exit_early"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("control child runs");
    assert!(status.success());
    let record = run.record().expect("control child recorded itself");
    assert!(
        record.console,
        "the probe must see an inherited console, or the hidden check proves nothing"
    );
}

#[tokio::test]
async fn noise_and_a_colliding_server_request_are_skipped_before_the_reply() {
    // Non-JSON text, a notification, a reply to another id, and a server
    // request that reuses the pending id arrive before each reply.
    let (outcome, run) = run_scenario("noise", generous()).await;

    assert_eq!(
        outcome.expect("answer after noise").billing.used_percent,
        Some(42.0)
    );
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn truncated_and_garbled_frames_are_skipped_until_the_reply() {
    let (outcome, run) = run_scenario("garbled", generous()).await;

    assert_eq!(
        outcome
            .expect("answer after garbled frames")
            .billing
            .used_percent,
        Some(42.0)
    );
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn the_older_top_level_billing_shape_is_read_from_a_real_child() {
    let (outcome, run) = run_scenario("legacy", generous()).await;

    let bearer = outcome.expect("legacy answer");
    // 2500 of 10000 cents.
    assert_eq!(bearer.billing.used_percent, Some(25.0));
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

// ---------------------------------------------------------------- errors

#[tokio::test]
async fn method_not_found_means_this_cli_has_no_acp_billing() {
    let (outcome, run) = run_scenario("method_not_found", generous()).await;

    let error = expect_error(outcome);
    assert!(
        matches!(&error, ProviderError::Other(message) if message.contains("does not provide billing")),
        "{error}"
    );
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn an_authentication_error_on_billing_is_auth_required() {
    let (outcome, run) = run_scenario("auth_billing", generous()).await;

    let error = expect_error(outcome);
    assert!(matches!(error, ProviderError::AuthRequired), "{error}");
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn an_authentication_error_on_initialize_is_auth_required() {
    let (outcome, run) = run_scenario("auth_init", generous()).await;

    let error = expect_error(outcome);
    assert!(matches!(error, ProviderError::AuthRequired), "{error}");
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn a_child_that_never_answers_initialize_times_out_and_is_killed() {
    let run = FakeRun::new();
    let args = run.args("hang_init");
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let started = Instant::now();

    let outcome = fetch_billing(&fake_grok(), &refs, short()).await;

    assert!(matches!(expect_error(outcome), ProviderError::Timeout));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn a_child_that_never_answers_billing_times_out_and_is_killed() {
    // The handshake completes, then billing hangs. The short budget is only
    // for billing, so initialize has room to finish.
    let run = FakeRun::new();
    let args = run.args("hang");
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let timeouts = AcpTimeouts {
        initialize: Duration::from_secs(20),
        billing: Duration::from_millis(300),
    };

    let outcome = fetch_billing(&fake_grok(), &refs, timeouts).await;

    assert!(matches!(expect_error(outcome), ProviderError::Timeout));
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn a_child_that_exits_before_replying_is_an_error_not_a_hang() {
    let (outcome, run) = run_scenario("exit_early", generous()).await;

    let error = expect_error(outcome);
    assert!(
        matches!(&error, ProviderError::Other(message) if message.contains("closed")),
        "{error}"
    );
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn a_half_written_frame_followed_by_exit_is_an_error() {
    let (outcome, run) = run_scenario("partial_eof", generous()).await;

    let error = expect_error(outcome);
    assert!(
        matches!(&error, ProviderError::Other(message) if message.contains("closed")),
        "{error}"
    );
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn an_oversized_line_is_rejected_and_the_child_is_killed() {
    let (outcome, run) = run_scenario("oversized", generous()).await;

    let error = expect_error(outcome);
    assert!(
        matches!(&error, ProviderError::Other(message) if message.contains("oversized")),
        "{error}"
    );
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

// ---------------------------------------------------------------- fallback

#[tokio::test]
async fn a_missing_cli_login_is_answered_by_the_real_child() {
    let run = FakeRun::new();
    let provider =
        GrokProvider::new().with_acp_process_for_tests(fake_grok(), run.args("ok"), generous());

    let result = provider
        .fetch_cli_acp_without_login(missing_login_error(), &FetchContext::default())
        .await
        .expect("ACP answers without a login");

    assert_eq!(result.usage.primary.used_percent, 42.0);
    assert_eq!(result.source_label, "grok-cli");
    assert_eq!(result.usage.account_email, None);
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn an_expired_cli_login_is_answered_by_the_real_child() {
    // The error comes from the real parser, not a constructed value.
    let text = r#"{"https://accounts.x.ai/sign-in": {"key": "cli-token", "auth_mode": "session", "expires_at": "2000-01-01T00:00:00Z"}}"#;
    let expired = GrokCredentials::parse_for_kind(text, GrokAuthKind::Cli).unwrap_err();
    let run = FakeRun::new();
    let provider =
        GrokProvider::new().with_acp_process_for_tests(fake_grok(), run.args("ok"), generous());

    let result = provider
        .fetch_cli_acp_without_login(expired, &FetchContext::default())
        .await
        .expect("ACP answers for an expired login");

    assert_eq!(result.usage.primary.used_percent, 42.0);
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn a_failing_child_keeps_the_original_login_error() {
    let run = FakeRun::new();
    let provider = GrokProvider::new().with_acp_process_for_tests(
        fake_grok(),
        run.args("method_not_found"),
        generous(),
    );

    let error = provider
        .fetch_cli_acp_without_login(missing_login_error(), &FetchContext::default())
        .await
        .unwrap_err();

    assert!(
        matches!(&error, ProviderError::NotInstalled(message) if message.contains("grok login")),
        "{error}"
    );
    assert_child_stopped(&run.record().expect("child recorded itself")).await;
}

#[tokio::test]
async fn an_isolated_account_never_starts_the_child() {
    let run = FakeRun::new();
    let provider =
        GrokProvider::new().with_acp_process_for_tests(fake_grok(), run.args("ok"), generous());
    let isolated = FetchContext {
        token_account_isolated: true,
        ..FetchContext::default()
    };

    let error = provider
        .fetch_cli_acp_without_login(missing_login_error(), &isolated)
        .await
        .unwrap_err();

    assert!(matches!(error, ProviderError::NotInstalled(_)), "{error}");
    assert!(
        run.record().is_none(),
        "an isolated account must not start the ambient CLI"
    );
}
