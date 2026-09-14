//! Runs `codex login` inside an isolated `CODEX_HOME`, with cancellation,
//! timeouts, and combined output capture. Split out of `account_manager.rs`
//! (port of the login-running slice of `windows/.../account_manager.py`, MIT).

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::core::SecretRedactor;

/// Outcome of a `codex login` subprocess run.
#[derive(Debug, Clone)]
pub enum CodexLoginOutcome {
    MissingBinary,
    LaunchFailed(String),
    TimedOut(String),
    Cancelled,
    Failed(String),
    Success(String),
}

impl CodexLoginOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodexLoginOutcome::MissingBinary => "missing_binary",
            CodexLoginOutcome::LaunchFailed(_) => "launch_failed",
            CodexLoginOutcome::TimedOut(_) => "timed_out",
            CodexLoginOutcome::Cancelled => "cancelled",
            CodexLoginOutcome::Failed(_) => "failed",
            CodexLoginOutcome::Success(_) => "success",
        }
    }

    pub fn output(&self) -> &str {
        match self {
            CodexLoginOutcome::MissingBinary => "",
            CodexLoginOutcome::LaunchFailed(output)
            | CodexLoginOutcome::TimedOut(output)
            | CodexLoginOutcome::Failed(output)
            | CodexLoginOutcome::Success(output) => output,
            CodexLoginOutcome::Cancelled => "",
        }
    }
}

/// Result of a `codex login` subprocess run.
#[derive(Debug, Clone)]
pub struct CodexLoginResult {
    pub outcome: CodexLoginOutcome,
}

/// Handle around an in-flight `codex login` process, for cancellation.
#[derive(Debug, Default, Clone)]
pub struct ManagedLoginProcess {
    inner: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl ManagedLoginProcess {
    fn bind(&self, process: Child) {
        *self.inner.lock().expect("login process lock") = Some(process);
        self.cancelled.store(false, Ordering::SeqCst);
    }

    fn clear(&self) {
        *self.inner.lock().expect("login process lock") = None;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let mut guard = self.inner.lock().expect("login process lock");
        if let Some(child) = guard.as_mut() {
            // Teardown: the cancellation outcome cannot change the result the
            // caller already observes, so ignore kill errors here.
            let _killed = child.kill();
        }
    }
}

/// Runs `codex login` inside an isolated `CODEX_HOME`.
pub struct CodexLoginRunner;

impl CodexLoginRunner {
    /// Resolve the `codex` executable through the crate-wide canonical locator.
    pub fn locate_codex_binary() -> Option<std::path::PathBuf> {
        crate::codex_cli::locate_codex_binary()
    }

    pub fn run(
        home_path: &Path,
        timeout: Duration,
        handle: Option<&ManagedLoginProcess>,
    ) -> CodexLoginResult {
        let active_handle = handle.cloned().unwrap_or_default();
        let Some(binary) = Self::locate_codex_binary() else {
            return CodexLoginResult {
                outcome: CodexLoginOutcome::MissingBinary,
            };
        };

        let mut command = Command::new(binary);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        command
            .args(["-c", "cli_auth_credentials_store=\"file\""])
            .arg("login")
            .env("CODEX_HOME", home_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return CodexLoginResult {
                    outcome: CodexLoginOutcome::LaunchFailed(error.to_string()),
                };
            }
        };
        active_handle.bind(child);

        let output = match wait_for_child(&active_handle, timeout) {
            Some(output) => output,
            None => {
                let output = kill_and_drain(&active_handle);
                active_handle.clear();
                return CodexLoginResult {
                    outcome: CodexLoginOutcome::TimedOut(combine_output(&output)),
                };
            }
        };

        active_handle.clear();
        let combined = combine_output(&output);
        if active_handle.is_cancelled() {
            return CodexLoginResult {
                outcome: CodexLoginOutcome::Cancelled,
            };
        }
        if output.status.success() {
            return CodexLoginResult {
                outcome: CodexLoginOutcome::Success(combined),
            };
        }
        CodexLoginResult {
            outcome: CodexLoginOutcome::Failed(combined),
        }
    }
}

fn wait_for_child(handle: &ManagedLoginProcess, timeout: Duration) -> Option<std::process::Output> {
    let deadline = Instant::now() + timeout;
    loop {
        if handle.is_cancelled() {
            let output = take_child(handle)?.wait_with_output().ok();
            return output;
        }
        let finished = {
            let mut guard = handle.inner.lock().expect("login process lock");
            matches!(
                guard.as_mut().map(|child| child.try_wait()),
                Some(Ok(Some(_))) | Some(Err(_))
            )
        };
        // Drop the polling lock before taking ownership of the child.
        if finished {
            return take_child(handle)?.wait_with_output().ok();
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn take_child(handle: &ManagedLoginProcess) -> Option<Child> {
    handle.inner.lock().expect("login process lock").take()
}

fn kill_and_drain(handle: &ManagedLoginProcess) -> std::process::Output {
    let mut child = take_child(handle).expect("login process present");
    // Teardown: kill errors cannot change the drained output already returned.
    let _killed = child.kill();
    child
        .wait_with_output()
        .unwrap_or_else(|_| std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
}

fn combine_output(output: &std::process::Output) -> String {
    let mut parts: Vec<String> = Vec::new();
    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    let merged = parts.join("\n");
    let redacted = SecretRedactor::redact(merged.trim());
    let merged = redacted.trim();
    if merged.is_empty() {
        "No output captured.".to_string()
    } else {
        merged.chars().take(4000).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_child_is_collected_without_locking_twice() {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            #[cfg(windows)]
            let child = Command::new("cmd.exe")
                .args(["/d", "/c", "echo login-complete"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            #[cfg(not(windows))]
            let child = Command::new("sh")
                .args(["-c", "echo login-complete"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let handle = ManagedLoginProcess::default();
            handle.bind(child);
            sender
                .send(wait_for_child(&handle, Duration::from_secs(2)))
                .unwrap();
        });
        let output = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("completed login must not deadlock")
            .expect("child output");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("login-complete"));
    }

    #[test]
    fn captured_login_output_redacts_credential_material() {
        let output = std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: b"Authorization: Bearer eyJheader.payload.signature\ndevice_code=DEV-SECRET\n"
                .to_vec(),
            stderr: b"callback?code=AUTH-SECRET\n".to_vec(),
        };

        let redacted = combine_output(&output);

        assert!(!redacted.contains("eyJheader.payload.signature"));
        assert!(!redacted.contains("DEV-SECRET"));
        assert!(!redacted.contains("AUTH-SECRET"));
        assert!(redacted.contains("[REDACTED]"));
    }
}
