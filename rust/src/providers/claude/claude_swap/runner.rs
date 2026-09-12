//! Bounded subprocess execution for the external cswap executable.
//!
//! Only fixed argument arrays are run, never a shell and never config-defined
//! passthrough arguments. Output and wall-clock runtime are bounded.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
#[cfg(windows)]
use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
#[cfg(windows)]
use windows::core::PCWSTR;

use super::parser::{parse_account_list, parse_switch_result, validate_switch_target};
use super::{ClaudeSwapAccountList, ClaudeSwapError, ClaudeSwapSwitchResult};

/// Upstream rejects list output larger than 256 KiB before parsing.
pub const MAX_OUTPUT_BYTES: usize = 262_144;
/// Default read-only probe timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Long upper bound for credential switches so a stalled helper cannot block forever.
pub const SWITCH_TIMEOUT: Duration = Duration::from_secs(300);
/// How often the runner polls the child and the reader channel.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Create the helper suspended so it cannot run before job containment.
#[cfg(windows)]
const CREATE_SUSPENDED: u32 = 0x0000_0004;
/// Keep the helper headless, matching the previous spawn behavior.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Upper bound for reaping a terminated direct child. The job guard is dropped
/// (reaping descendants) before this wait, so it can never block indefinitely.
#[cfg(windows)]
const CHILD_REAP_TIMEOUT: Duration = Duration::from_secs(2);

/// Exact argument arrays. Pure so the no-shell contract is testable.
pub fn list_arguments() -> Vec<String> {
    vec!["--list".to_string(), "--json".to_string()]
}

pub fn switch_arguments(slot: u32) -> Vec<String> {
    vec![
        "--switch-to".to_string(),
        slot.to_string(),
        "--json".to_string(),
    ]
}

/// Trim and expand a leading `~` exactly like upstream; no shell expansion.
pub fn resolve_executable_path(configured: &str) -> Result<PathBuf, ClaudeSwapError> {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        return Err(ClaudeSwapError::ExecutablePathNotConfigured);
    }
    if trimmed == "~" {
        return dirs::home_dir()
            .ok_or_else(|| ClaudeSwapError::Process("Home directory not found.".to_string()));
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return dirs::home_dir()
            .map(|home| home.join(rest))
            .ok_or_else(|| ClaudeSwapError::Process("Home directory not found.".to_string()));
    }
    Ok(PathBuf::from(trimmed))
}

/// A configured bare command name is resolved on `PATH` by the OS; an explicit
/// path must exist so a typo fails with a clear message instead of a spawn error.
fn validate_executable_path(path: &Path) -> Result<(), ClaudeSwapError> {
    let looks_like_path = path.is_absolute()
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
    if looks_like_path && !path.is_file() {
        return Err(ClaudeSwapError::ExecutableNotFound);
    }
    Ok(())
}

#[derive(Debug, Default)]
struct RunOutcome {
    stdout: Vec<u8>,
    total_stdout_bytes: usize,
}

fn read_bounded(mut reader: impl std::io::Read) -> RunOutcome {
    let mut outcome = RunOutcome::default();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                outcome.total_stdout_bytes += read;
                if outcome.stdout.len() < MAX_OUTPUT_BYTES + 1 {
                    let remaining = (MAX_OUTPUT_BYTES + 1) - outcome.stdout.len();
                    let take = remaining.min(read);
                    outcome.stdout.extend_from_slice(&chunk[..take]);
                }
            }
            Err(_) => break,
        }
    }
    outcome
}

/// Own the helper process tree so a timed-out `cswap` cannot leave a
/// descendant holding the captured pipes open. The job is private to the
/// process created by this invocation; it never adopts user-owned processes.
#[cfg(windows)]
struct ProcessTreeGuard {
    job: OwnedHandle,
}

#[cfg(windows)]
impl ProcessTreeGuard {
    fn attach(child: &Child) -> Result<Self, String> {
        // SAFETY: a successful call returns a unique job handle owned below.
        let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| format!("failed to create cswap job: {error}"))?;
        // SAFETY: `raw` is a valid, unique handle returned by CreateJobObjectW.
        let job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        Self::set_kill_on_close(&job)?;
        // SAFETY: `job` is a valid job handle and `child` is a live process.
        unsafe {
            // The direct child is the only process created by this runner and
            // was created suspended, so this assignment always happens before
            // it can run user code. Descendants it creates are contained too.
            AssignProcessToJobObject(raw_handle(&job), HANDLE(child.as_raw_handle()))
                .map_err(|error| format!("failed to contain cswap process tree: {error}"))?;
        }
        Ok(Self { job })
    }

    /// Arm `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so closing the last job handle
    /// reaps whatever remains in the job. Idempotent, and re-used as the
    /// fail-safe when explicit termination fails.
    fn set_kill_on_close(job: &OwnedHandle) -> Result<(), String> {
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..Default::default()
            },
            ..Default::default()
        };
        let size = u32::try_from(std::mem::size_of_val(&limits))
            .map_err(|error| format!("invalid cswap job limit size: {error}"))?;
        // SAFETY: `job` is valid and `limits` is initialized for this API.
        unsafe {
            SetInformationJobObject(
                raw_handle(job),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size,
            )
        }
        .map_err(|error| format!("failed to configure cswap job: {error}"))
    }

    /// Explicitly terminate every process in the job. The error is returned so
    /// the caller can fall back to `KILL_ON_JOB_CLOSE` instead of assuming the
    /// tree is gone.
    fn terminate(&self) -> Result<(), String> {
        // SAFETY: this job contains only the helper process tree created above.
        unsafe { TerminateJobObject(raw_handle(&self.job), 1) }
            .map_err(|error| format!("failed to terminate cswap process tree: {error}"))
    }
}

#[cfg(windows)]
fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// Resume the single thread of a child created with `CREATE_SUSPENDED`.
///
/// `std::process::Child` does not expose the primary thread handle, so the
/// suspended thread is located through a snapshot. The child was assigned to
/// the containment job before this runs, closing the pre-assignment escape
/// race; only a successfully contained child is ever released.
#[cfg(windows)]
fn resume_suspended_child(process_id: u32) -> Result<(), String> {
    // SAFETY: a successful call returns a unique snapshot handle owned below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|error| format!("failed to enumerate cswap threads: {error}"))?;
    // SAFETY: `snapshot` is a valid, unique handle returned just above.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot.0) };

    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>())
            .map_err(|error| format!("invalid cswap thread entry size: {error}"))?,
        ..Default::default()
    };
    let mut resumed = 0usize;
    // SAFETY: the snapshot handle is live and `entry.dwSize` is correct.
    if unsafe { Thread32First(raw_handle(&snapshot), &mut entry) }.is_ok() {
        loop {
            if entry.th32OwnerProcessID == process_id {
                // SAFETY: the thread id comes from a live snapshot and the
                // returned handle is owned below.
                if let Ok(thread) =
                    unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                {
                    // SAFETY: `thread` is a live handle returned by OpenThread;
                    // wrapping and resuming it in one scope keeps it open for
                    // the resume call, which releases the suspended child.
                    let previous = unsafe {
                        let thread = OwnedHandle::from_raw_handle(thread.0);
                        ResumeThread(raw_handle(&thread))
                    };
                    if previous != u32::MAX {
                        resumed += 1;
                    }
                }
            }
            // SAFETY: `entry` is valid for the next snapshot entry.
            if unsafe { Thread32Next(raw_handle(&snapshot), &mut entry) }.is_err() {
                break;
            }
        }
    }

    if resumed == 0 {
        return Err("failed to resume cswap process after containment.".to_string());
    }
    Ok(())
}

/// Terminate the helper process tree without ever waiting unbounded.
///
/// `TerminateJobObject` is attempted first. If it fails, the direct child is
/// still killed, `KILL_ON_JOB_CLOSE` is re-armed, and the guard is dropped so
/// closing the job handle reaps any survivor. The child is then reaped under a
/// bounded timeout rather than an unbounded `wait()`.
#[cfg(windows)]
fn terminate_child_tree(child: &mut Child, tree: Option<ProcessTreeGuard>) {
    // Kill the direct child unconditionally as the fallback; skip the kill
    // when it already exited so a benign race is not reported as a failure.
    let direct_kill_failed = match child.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) | Err(_) => child.kill().is_err(),
    };

    if let Some(tree) = tree {
        if let Err(error) = tree.terminate() {
            tracing::warn!(
                %error,
                "cswap job termination failed; arming kill-on-close and killing direct child"
            );
            if let Err(limit_error) = ProcessTreeGuard::set_kill_on_close(&tree.job) {
                tracing::warn!(%limit_error, "failed to re-arm cswap kill-on-close");
            }
        }
        // Close the last job handle before waiting. When KILL_ON_JOB_CLOSE is
        // armed this reaps every descendant that explicit termination missed.
        drop(tree);
    }

    if direct_kill_failed {
        tracing::warn!("failed to kill the direct cswap child; relying on job containment");
    }

    wait_bounded_for_child(child);
}

/// Reap the direct child for at most [`CHILD_REAP_TIMEOUT`], never blocking
/// indefinitely even if containment failed to terminate it.
#[cfg(windows)]
fn wait_bounded_for_child(child: &mut Child) {
    let deadline = Instant::now() + CHILD_REAP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => {
                if Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

#[cfg(not(windows))]
fn terminate_child_tree(child: &mut Child) {
    if let Err(error) = child.kill() {
        tracing::warn!(%error, "failed to kill the direct claude-swap child");
    }
    if let Err(error) = child.wait() {
        tracing::warn!(%error, "failed to reap the direct claude-swap child");
    }
}

/// Run a fixed argument array against a wall-clock deadline.
///
/// Reader threads are never joined: a cswap descendant can inherit the
/// stdout/stderr write handles and keep them open after the direct child exits,
/// so a `join()` on a pipe reader can block indefinitely. Results travel over a
/// channel, and a run that reaches its deadline kills the child and returns
/// without waiting for still-blocked readers.
fn run_bounded(
    program: &Path,
    arguments: &[String],
    timeout: Duration,
) -> Result<String, ClaudeSwapError> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The helper starts suspended so it cannot execute any user code before
        // it is assigned to the containment job below.
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| ClaudeSwapError::Process(e.to_string()))?;
    #[cfg(windows)]
    let mut process_tree = match ProcessTreeGuard::attach(&child) {
        Ok(tree) => {
            // Assignment succeeded while the only thread is still suspended;
            // release it now so containment always precedes execution.
            if let Err(error) = resume_suspended_child(child.id()) {
                terminate_child_tree(&mut child, Some(tree));
                return Err(ClaudeSwapError::Process(error));
            }
            Some(tree)
        }
        Err(error) => {
            terminate_child_tree(&mut child, None);
            return Err(ClaudeSwapError::Process(error));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            #[cfg(windows)]
            terminate_child_tree(&mut child, process_tree.take());
            #[cfg(not(windows))]
            terminate_child_tree(&mut child);
            return Err(ClaudeSwapError::Process(
                "Failed to capture stdout.".to_string(),
            ));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            #[cfg(windows)]
            terminate_child_tree(&mut child, process_tree.take());
            #[cfg(not(windows))]
            terminate_child_tree(&mut child);
            return Err(ClaudeSwapError::Process(
                "Failed to capture stderr.".to_string(),
            ));
        }
    };

    let (sender, receiver) = mpsc::channel::<(bool, RunOutcome)>();
    let stdout_sender = sender.clone();
    std::thread::spawn(move || {
        let _sent = stdout_sender.send((true, read_bounded(stdout)));
    });
    std::thread::spawn(move || {
        let _sent = sender.send((false, read_bounded(stderr)));
    });

    let deadline = Instant::now() + timeout;
    let mut stdout_outcome: Option<RunOutcome> = None;
    let mut stderr_done = false;
    let mut child_exited = false;
    loop {
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok((true, outcome)) => stdout_outcome = Some(outcome),
                Ok((false, _outcome)) => stderr_done = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        let readers_done = stdout_outcome.is_some() && stderr_done;

        match child.try_wait() {
            // The direct child reporting an exit status is required for
            // success; readers reaching EOF is not sufficient. A child may
            // close its streams while still running, and a descendant can own
            // the pipe write handles, so keep bounded polling until exit or
            // the wall-clock deadline.
            Ok(Some(_status)) => child_exited = true,
            Ok(None) => {}
            Err(e) => {
                #[cfg(windows)]
                terminate_child_tree(&mut child, process_tree.take());
                #[cfg(not(windows))]
                terminate_child_tree(&mut child);
                return Err(ClaudeSwapError::Process(e.to_string()));
            }
        }

        if readers_done && child_exited {
            break;
        }
        // A disconnected channel only means failure when a reader ended
        // without delivering its outcome; after both readers send it is normal.
        if disconnected && !readers_done {
            #[cfg(windows)]
            terminate_child_tree(&mut child, process_tree.take());
            #[cfg(not(windows))]
            terminate_child_tree(&mut child);
            return Err(ClaudeSwapError::Process(
                "claude-swap output streams closed unexpectedly.".to_string(),
            ));
        }
        if Instant::now() >= deadline {
            #[cfg(windows)]
            terminate_child_tree(&mut child, process_tree.take());
            #[cfg(not(windows))]
            terminate_child_tree(&mut child);
            return Err(ClaudeSwapError::TimedOut(timeout.as_secs()));
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    let stdout = stdout_outcome.ok_or_else(|| {
        ClaudeSwapError::Process("claude-swap stdout reader ended without output.".to_string())
    })?;
    if stdout.total_stdout_bytes > MAX_OUTPUT_BYTES {
        return Err(ClaudeSwapError::OutputTooLarge {
            actual: stdout.total_stdout_bytes,
            limit: MAX_OUTPUT_BYTES,
        });
    }
    Ok(String::from_utf8_lossy(&stdout.stdout).into_owned())
}

/// Read-only `cswap --list --json`.
pub fn read_account_list(configured_path: &str) -> Result<ClaudeSwapAccountList, ClaudeSwapError> {
    read_account_list_with_timeout(configured_path, DEFAULT_TIMEOUT)
}

pub fn read_account_list_with_timeout(
    configured_path: &str,
    timeout: Duration,
) -> Result<ClaudeSwapAccountList, ClaudeSwapError> {
    let program = resolve_executable_path(configured_path)?;
    validate_executable_path(&program)?;
    // Handled cswap failures print a schema-v1 error envelope to stdout and exit
    // non-zero, so parse stdout regardless of the exit status.
    let output = run_bounded(&program, &list_arguments(), timeout)?;
    parse_account_list(&output)
}

/// Explicit `cswap --switch-to <slot> --json`, bounded by [`SWITCH_TIMEOUT`].
pub fn switch_account(
    configured_path: &str,
    slot: u32,
) -> Result<ClaudeSwapSwitchResult, ClaudeSwapError> {
    if slot == 0 {
        return Err(ClaudeSwapError::MalformedShape(
            "requested account slot must be positive".to_string(),
        ));
    }
    let program = resolve_executable_path(configured_path)?;
    validate_executable_path(&program)?;
    let output = run_bounded(&program, &switch_arguments(slot), SWITCH_TIMEOUT)?;
    let parsed = parse_switch_result(&output)?;
    validate_switch_target(slot, &parsed)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_argument_arrays_never_use_a_shell() {
        assert_eq!(list_arguments(), vec!["--list", "--json"]);
        assert_eq!(switch_arguments(7), vec!["--switch-to", "7", "--json"]);
        // A slot is rendered as a plain decimal token, so it cannot smuggle
        // extra arguments or shell metacharacters.
        assert_eq!(list_arguments().len(), 2);
        assert_eq!(switch_arguments(1).len(), 3);
    }

    #[test]
    fn executable_path_requires_a_non_empty_value_and_expands_tilde() {
        assert!(matches!(
            resolve_executable_path("   "),
            Err(ClaudeSwapError::ExecutablePathNotConfigured)
        ));
        let expanded = resolve_executable_path("~/bin/cswap").unwrap();
        assert!(expanded.ends_with("bin/cswap") || expanded.ends_with("bin\\cswap"));
        assert!(!expanded.to_string_lossy().starts_with('~'));
        let explicit = resolve_executable_path("C:/tools/cswap.exe").unwrap();
        assert_eq!(explicit, PathBuf::from("C:/tools/cswap.exe"));
    }

    #[test]
    fn missing_explicit_executable_path_fails_before_spawn() {
        let missing = if cfg!(windows) {
            "C:/definitely/not/here/cswap.exe"
        } else {
            "/definitely/not/here/cswap"
        };
        assert!(matches!(
            read_account_list(missing),
            Err(ClaudeSwapError::ExecutableNotFound)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn captures_stdout_from_a_completed_process() {
        let output = run_bounded(
            Path::new("cmd.exe"),
            &["/C".to_string(), "echo codexbar-runner".to_string()],
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(output.contains("codexbar-runner"), "stdout: {output:?}");
    }

    #[cfg(windows)]
    #[test]
    fn timeout_does_not_hang_on_a_pipe_holding_descendant() {
        // `cmd /C start /B ...` exits immediately, but the background
        // PowerShell inherits the stdout/stderr pipes and sleeps long past the
        // deadline. The bounded runner must return on its own deadline instead
        // of blocking on the reader threads until the descendant exits.
        let started = Instant::now();
        let result = run_bounded(
            Path::new("cmd.exe"),
            &[
                "/C".to_string(),
                "start /B powershell -NoProfile -Command Start-Sleep -Seconds 5".to_string(),
            ],
            Duration::from_millis(500),
        );
        assert!(matches!(result, Err(ClaudeSwapError::TimedOut(_))));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "runner blocked past its deadline: {:?}",
            started.elapsed()
        );
    }

    #[cfg(windows)]
    #[test]
    fn stream_eof_alone_is_not_reported_as_success() {
        // The helper closes both captured streams immediately but keeps running
        // past the deadline. EOF alone must not be treated as a completed run,
        // so the runner waits for an actual exit status or times out.
        let result = run_bounded(
            Path::new("powershell.exe"),
            &[
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "[Console]::Out.Close(); [Console]::Error.Close(); Start-Sleep -Seconds 5"
                    .to_string(),
            ],
            Duration::from_millis(500),
        );
        assert!(
            matches!(result, Err(ClaudeSwapError::TimedOut(_))),
            "EOF must not complete the run: {result:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn timeout_terminates_the_contained_descendant() {
        use base64::Engine;

        let dir = std::env::temp_dir().join(format!("codexbar-cswap-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let marker = dir.join("descendant-alive.txt");

        // The direct child exits immediately, leaving a background PowerShell
        // that would create the marker well after the runner's deadline. Job
        // containment must kill it, so the marker never appears. The script is
        // passed as base64 UTF-16 to avoid cmd/PowerShell quoting ambiguity.
        let script = format!(
            "Start-Sleep -Seconds 2; Set-Content -LiteralPath '{}' -Value alive",
            marker.display()
        );
        let mut utf16 = Vec::with_capacity(script.len() * 2);
        for unit in script.encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(&utf16);
        let command = format!("start /B powershell -NoProfile -EncodedCommand {encoded}");

        let result = run_bounded(
            Path::new("cmd.exe"),
            &["/C".to_string(), command],
            Duration::from_millis(300),
        );
        assert!(matches!(result, Err(ClaudeSwapError::TimedOut(_))));

        std::thread::sleep(Duration::from_millis(2500));
        assert!(
            !marker.exists(),
            "contained descendant survived the job and wrote {}",
            marker.display()
        );
        drop(std::fs::remove_dir_all(&dir));
    }
}
