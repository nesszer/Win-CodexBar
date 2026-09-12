//! Bounded subprocess execution for the external cswap executable.
//!
//! Only fixed argument arrays are run, never a shell and never config-defined
//! passthrough arguments. Output and wall-clock runtime are bounded.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

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
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| ClaudeSwapError::Process(e.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ClaudeSwapError::Process("Failed to capture stdout.".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ClaudeSwapError::Process("Failed to capture stderr.".to_string()))?;

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
        if stdout_outcome.is_some() && stderr_done {
            break;
        }
        if disconnected {
            return Err(ClaudeSwapError::Process(
                "claude-swap output streams closed unexpectedly.".to_string(),
            ));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ClaudeSwapError::TimedOut(timeout.as_secs()));
        }
        match child.try_wait() {
            // The direct child may exit while a descendant still owns the pipe
            // write handles; keep draining until the wall-clock deadline.
            Ok(Some(_)) | Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ClaudeSwapError::Process(e.to_string()));
            }
        }
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
}
