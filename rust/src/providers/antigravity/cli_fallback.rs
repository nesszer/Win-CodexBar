#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command as AsyncCommand;
use uuid::Uuid;

use super::AntigravityProvider;
use super::cli_print_failure::{CliPrintFailure, ExitClassification, classify_exit};
use super::offline_reason::LiveFailure;
use super::quota_summary;
use crate::core::{ProviderError, ProviderFetchResult};
#[cfg(windows)]
use crate::managed_process::ProcessJob;

const REPORT_TIMEOUT: Duration = Duration::from_secs(90);
const REPORT_TOO_LARGE: &str = "Antigravity CLI usage report is too large";
const VERSION_ARGS: [&str; 1] = ["--version"];
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
const VERSION_TOO_LARGE: &str = "Antigravity CLI version output is too large";
const USAGE_ARGS: [&str; 6] = [
    "-p",
    "/usage",
    "--output-format",
    "json",
    "--print-timeout",
    "90s",
];
const REPORT_MAX_OUTPUT_BYTES: usize = 1_048_576;
const WORKDIR_CREATE_ATTEMPTS: usize = 8;
const OAUTH_CREDENTIALS_ENV: &str = "ANTIGRAVITY_OAUTH_CREDENTIALS_JSON";

#[derive(Debug)]
struct PrivateWorkdir {
    path: PathBuf,
}

impl PrivateWorkdir {
    fn create() -> Result<Self, ProviderError> {
        let temp_root = std::env::temp_dir();
        for _ in 0..WORKDIR_CREATE_ATTEMPTS {
            let path = temp_root.join(format!(
                "codexbar-agy-{}-{}",
                std::process::id(),
                Uuid::new_v4()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;

                        if std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                            .is_err()
                        {
                            drop(std::fs::remove_dir(&path));
                            return Err(ProviderError::Other(
                                "Failed to prepare Antigravity CLI working directory".into(),
                            ));
                        }
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }

        Err(ProviderError::Other(
            "Failed to prepare Antigravity CLI working directory".into(),
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateWorkdir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.path));
    }
}

#[derive(Debug, PartialEq, Eq)]
struct BoundedOutput {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

/// Read a child output stream incrementally, retaining only the configured
/// prefix while continuing to drain the pipe so the child cannot block on a
/// full pipe buffer.
async fn read_output_limited<R>(mut reader: R, max_bytes: usize) -> std::io::Result<BoundedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(max_bytes.min(8192));
    let mut chunk = [0_u8; 8192];
    let mut exceeded_limit = false;

    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }

        let remaining = max_bytes.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&chunk[..retained]);
        if retained < read {
            exceeded_limit = true;
        }
    }

    Ok(BoundedOutput {
        bytes,
        exceeded_limit,
    })
}

pub(super) async fn try_fetch(
    binary: Option<PathBuf>,
) -> Result<Option<ProviderFetchResult>, LiveFailure> {
    let Some(binary) = binary else {
        return Ok(None);
    };
    match fetch_print_usage(&binary).await {
        Ok(usage) => Ok(Some(usage)),
        Err(error) => {
            tracing::debug!(%error, "Antigravity structured CLI usage report unavailable");
            Err(error)
        }
    }
}

/// Newer `agy` releases require a CSRF token for the local server started by
/// the CLI. CodexBar cannot obtain that token from a managed process, so let
/// the caller skip its readiness wait and continue to the print report.
pub(super) async fn managed_spawn_is_csrf_gated(binary: Option<PathBuf>) -> bool {
    let Some(binary) = binary else {
        return false;
    };
    agy_version(&binary, VERSION_TIMEOUT)
        .await
        .is_ok_and(|version| is_csrf_gated_version(&version))
}

/// `agy --version` output, trimmed.
async fn agy_version(binary: &Path, timeout: Duration) -> Result<String, LiveFailure> {
    let version = run_cli_command(binary, &VERSION_ARGS, timeout, VERSION_TOO_LARGE).await?;
    Ok(String::from_utf8_lossy(&version).trim().to_string())
}

async fn fetch_print_usage(binary: &Path) -> Result<ProviderFetchResult, LiveFailure> {
    fetch_print_usage_with_version_timeout(binary, VERSION_TIMEOUT).await
}

async fn fetch_print_usage_with_version_timeout(
    binary: &Path,
    version_timeout: Duration,
) -> Result<ProviderFetchResult, LiveFailure> {
    if !is_supported_version(&agy_version(binary, version_timeout).await?) {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage reports require agy 1.1.11 or later".into(),
        )
        .into());
    }

    let output = run_cli_command(binary, &USAGE_ARGS, REPORT_TIMEOUT, REPORT_TOO_LARGE).await?;
    let usage = quota_summary::parse_cli_usage_report(&output)?;
    Ok(AntigravityProvider::fetch_result(
        usage,
        super::AntigravityStrategyId::Cli,
    ))
}

fn prepare_command(binary: &Path, args: &[&str], working_dir: &Path) -> AsyncCommand {
    let mut command = AsyncCommand::new(binary);
    command
        .args(args)
        .current_dir(working_dir)
        .env_remove(OAUTH_CREDENTIALS_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Captured only to classify a failed run; never logged or displayed.
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.as_std_mut().creation_flags(0x0800_0000);
    command
}

/// Kill-on-close job for one probe. Dropping it (success, error or timeout)
/// terminates the probe's whole process tree, not only the direct child that
/// `kill_on_drop` reaches; `agy` starts MCP server descendants that would
/// otherwise outlive the probe.
#[cfg(windows)]
type ProbeJob = ProcessJob;
#[cfg(not(windows))]
type ProbeJob = ();

/// Spawn the probe and place it in its own job. Only this probe's process tree
/// is ever in the job, so unrelated `agy` processes are never touched.
fn spawn_contained(
    command: &mut AsyncCommand,
) -> Result<(tokio::process::Child, Option<ProbeJob>), LiveFailure> {
    let child = command
        .spawn()
        .map_err(|error| LiveFailure::cli_report(CliPrintFailure::from_spawn_error(&error)))?;
    let job = contain_child(&child);
    Ok((child, job))
}

#[cfg(windows)]
fn contain_child(child: &tokio::process::Child) -> Option<ProbeJob> {
    let Some(handle) = child.raw_handle() else {
        tracing::warn!("Antigravity CLI probe exited before it could be job-contained");
        return None;
    };
    ProcessJob::create("agy-probe")
        .and_then(|job| job.contain(handle).map(|()| job))
        .inspect_err(|error| {
            tracing::warn!(%error, "Antigravity CLI probe could not be job-contained");
        })
        .ok()
}

#[cfg(not(windows))]
fn contain_child(_child: &tokio::process::Child) -> Option<ProbeJob> {
    None
}

async fn run_cli_command(
    binary: &Path,
    args: &[&str],
    timeout: Duration,
    too_large: &'static str,
) -> Result<Vec<u8>, LiveFailure> {
    let working_dir = PrivateWorkdir::create()?;
    let mut command = prepare_command(binary, args, working_dir.path());

    let (mut child, _containment) = spawn_contained(&mut command)?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(report_failed());
    };

    let (stdout, stderr, status) = tokio::time::timeout(timeout, async {
        tokio::join!(
            read_output_limited(stdout, REPORT_MAX_OUTPUT_BYTES),
            read_output_limited(stderr, REPORT_MAX_OUTPUT_BYTES),
            child.wait()
        )
    })
    .await
    .map_err(|_| LiveFailure::from(ProviderError::Timeout))?;
    let (Ok(stdout), Ok(stderr), Ok(status)) = (stdout, stderr, status) else {
        return Err(report_failed());
    };
    finish_run(stdout, stderr, status.code(), too_large)
}

fn report_failed() -> LiveFailure {
    ProviderError::Other("Antigravity CLI usage report failed".into()).into()
}

/// Map a finished `agy` run onto the probe policy. Like upstream
/// `SubprocessRunner`, oversized output on either stream is rejected before
/// the exit status is considered. stderr only feeds the fixed classification.
fn finish_run(
    stdout: BoundedOutput,
    stderr: BoundedOutput,
    exit_code: Option<i32>,
    too_large: &'static str,
) -> Result<Vec<u8>, LiveFailure> {
    if stdout.exceeded_limit || stderr.exceeded_limit {
        return Err(ProviderError::Parse(too_large.into()).into());
    }
    if exit_code == Some(0) {
        return Ok(stdout.bytes);
    }
    match classify_exit(exit_code.unwrap_or(-1), &stderr.bytes) {
        // A signed-out CLI stays a terminal, actionable sign-in error on
        // Windows instead of being hidden behind offline history.
        ExitClassification::SignedOut => Err(ProviderError::AuthRequired.into()),
        ExitClassification::Failed(failure) => Err(LiveFailure::cli_report(failure)),
    }
}

fn is_supported_version(version: &str) -> bool {
    parse_version(version).is_some_and(|version| version >= (1, 1, 11))
}

fn is_csrf_gated_version(version: &str) -> bool {
    parse_version(version).is_some_and(|version| version >= (1, 2, 2))
}

fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::cli_print_failure::ExitReason;
    use super::super::offline_reason::LiveFailureReason;
    use super::*;

    #[test]
    fn structured_cli_report_requires_a_supported_semver_version() {
        assert!(is_supported_version("1.1.11"));
        assert!(is_supported_version("1.2.2"));
        assert!(is_supported_version("2.0.0"));
        assert!(!is_supported_version("1.1.10"));
        assert!(!is_supported_version("1.2.2-preview"));
        assert!(!is_supported_version("+1.2.2"));
        assert!(!is_supported_version("1.2.2.3"));
        assert!(!is_supported_version(""));
    }

    #[test]
    fn managed_spawn_is_skipped_only_for_known_csrf_gated_versions() {
        assert!(!is_csrf_gated_version("1.2.1"));
        assert!(is_csrf_gated_version("1.2.2"));
        assert!(is_csrf_gated_version("1.10.0"));
        assert!(is_csrf_gated_version("2.0.0"));
        assert!(!is_csrf_gated_version("1.2.2-preview"));
        assert!(!is_csrf_gated_version(""));
    }

    #[tokio::test]
    async fn output_capture_retains_only_the_configured_limit() {
        let output = read_output_limited(b"0123456789".as_slice(), 4)
            .await
            .expect("output reader should succeed");

        assert_eq!(output.bytes, b"0123");
        assert!(output.exceeded_limit);

        let output = read_output_limited(b"0123".as_slice(), 4)
            .await
            .expect("output reader should succeed");
        assert_eq!(output.bytes, b"0123");
        assert!(!output.exceeded_limit);
    }

    fn output(bytes: &[u8], exceeded_limit: bool) -> BoundedOutput {
        BoundedOutput {
            bytes: bytes.to_vec(),
            exceeded_limit,
        }
    }

    #[test]
    fn oversized_output_on_either_stream_fails_before_the_exit_status() {
        let cases = [
            (output(b"{}", true), output(b"", false), Some(0)),
            (output(b"{}", false), output(b"noise", true), Some(0)),
            (output(b"", true), output(b"not logged in", false), Some(1)),
        ];
        for (stdout, stderr, exit_code) in cases {
            let failure = finish_run(stdout, stderr, exit_code, REPORT_TOO_LARGE)
                .expect_err("oversized output must be rejected");
            assert_eq!(failure.reason(), LiveFailureReason::Unclassified);
            assert!(matches!(
                failure.into_error(),
                ProviderError::Parse(message) if message == REPORT_TOO_LARGE
            ));
        }
    }

    #[test]
    fn successful_run_returns_stdout_and_ignores_stderr() {
        let stdout = finish_run(
            output(b"{\"ok\":true}", false),
            output(b"warning: not logged in to telemetry", false),
            Some(0),
            REPORT_TOO_LARGE,
        )
        .expect("exit 0 succeeds");
        assert_eq!(stdout, b"{\"ok\":true}");
    }

    #[test]
    fn failed_run_is_classified_without_echoing_stderr() {
        let failure = finish_run(
            output(b"", false),
            output(b"synthetic-private-diagnostic", false),
            Some(7),
            REPORT_TOO_LARGE,
        )
        .expect_err("exit 7 fails");
        assert_eq!(
            failure.reason(),
            LiveFailureReason::CliReport(CliPrintFailure::Exited {
                code: 7,
                reason: ExitReason::Unspecified,
            })
        );
        let message = failure.into_error().to_string();
        assert_eq!(message, "Antigravity CLI usage report failed: agy exited 7");
        assert!(!message.contains("synthetic-private-diagnostic"));

        let failure = finish_run(
            output(b"", false),
            output(b"", false),
            None,
            REPORT_TOO_LARGE,
        )
        .expect_err("a run without an exit code fails");
        assert_eq!(
            failure.reason(),
            LiveFailureReason::CliReport(CliPrintFailure::Exited {
                code: -1,
                reason: ExitReason::Unspecified,
            })
        );
    }

    #[test]
    fn signed_out_run_requires_authentication() {
        let failure = finish_run(
            output(b"", false),
            output(b"Select login method:", false),
            Some(1),
            REPORT_TOO_LARGE,
        )
        .expect_err("a login prompt fails");
        assert!(failure.is_auth_required());
        assert!(failure.offline_detail().is_none());
    }

    #[tokio::test]
    async fn missing_agy_executable_is_classified() {
        let dir = tempfile::tempdir().expect("fixture directory");
        let failure = fetch_print_usage(&dir.path().join("agy.exe"))
            .await
            .expect_err("a missing executable cannot report usage");
        assert_eq!(
            failure.reason(),
            LiveFailureReason::CliReport(CliPrintFailure::ExecutableNotFound)
        );
        assert_eq!(
            failure.into_error().to_string(),
            "Antigravity CLI usage report failed: agy executable not found"
        );
    }

    /// Version-probe timeout for the `agy.cmd` fixtures. cmd.exe startup on a
    /// loaded CI runner can exceed the production 3 s limit.
    #[cfg(windows)]
    const FIXTURE_VERSION_TIMEOUT: Duration = Duration::from_secs(30);

    /// A stand-in `agy` that reports a supported version and then fails the
    /// usage report with fixed stderr and exit code.
    #[cfg(windows)]
    fn failing_agy(stderr: &str, exit_code: i32) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("fixture directory");
        std::fs::write(dir.path().join("stderr.txt"), stderr).expect("stderr fixture");
        std::fs::write(
            dir.path().join("agy.cmd"),
            format!(
                "@echo off\r\nif \"%~1\"==\"--version\" (\r\n  echo 1.2.2\r\n  exit /b 0\r\n)\r\n>&2 type \"%~dp0stderr.txt\"\r\nexit /b {exit_code}\r\n"
            ),
        )
        .expect("agy fixture");
        dir
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn failed_print_usage_process_is_classified_without_leaking_stderr() {
        let cases = [
            ("synthetic-private-diagnostic", 7, ExitReason::Unspecified),
            (
                r#"Eligibility check failed: failed to get profile picture: Get "https://lh3.googleusercontent.com/a/private": EOF"#,
                1,
                ExitReason::EligibilityNetwork,
            ),
            (
                "Eligibility check failed: account does not support Google ToS",
                1,
                ExitReason::Ineligible,
            ),
            (
                r#"Post "https://usage.invalid/v1": dial tcp: no such host"#,
                1,
                ExitReason::Network,
            ),
        ];
        for (stderr, code, reason) in cases {
            let fixture = failing_agy(stderr, code);
            let failure = fetch_print_usage_with_version_timeout(
                &fixture.path().join("agy.cmd"),
                FIXTURE_VERSION_TIMEOUT,
            )
            .await
            .expect_err("a failing agy cannot report usage");
            assert_eq!(
                failure.reason(),
                LiveFailureReason::CliReport(CliPrintFailure::Exited { code, reason }),
                "{stderr}"
            );
            let detail = failure
                .offline_detail()
                .expect("offline explanation")
                .value()
                .to_string();
            let message = failure.into_error().to_string();
            for secret in [
                "synthetic-private-diagnostic",
                "googleusercontent",
                "usage.invalid",
            ] {
                assert!(!message.contains(secret), "{secret} leaked into {message}");
                assert!(!detail.contains(secret), "{secret} leaked into {detail}");
            }
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn signed_out_print_usage_process_requires_authentication() {
        let fixture = failing_agy("You are not logged into Antigravity", 1);
        let failure = fetch_print_usage_with_version_timeout(
            &fixture.path().join("agy.cmd"),
            FIXTURE_VERSION_TIMEOUT,
        )
        .await
        .expect_err("a signed-out agy cannot report usage");
        assert!(failure.is_auth_required());
    }

    #[test]
    fn subprocess_uses_private_workdir_and_scrubs_oauth_credentials() {
        let workdir = PrivateWorkdir::create().expect("private working directory");
        let path = workdir.path().to_path_buf();
        assert!(path.is_dir());
        assert_ne!(path, std::env::temp_dir());

        {
            let command = prepare_command(Path::new("agy"), &["--version"], workdir.path());
            let standard_command = command.as_std();

            assert_eq!(standard_command.get_current_dir(), Some(workdir.path()));
            assert!(standard_command.get_envs().any(|(key, value)| {
                key == std::ffi::OsStr::new(OAUTH_CREDENTIALS_ENV) && value.is_none()
            }));
        }

        drop(workdir);
        assert!(!path.exists());
    }

    #[cfg(windows)]
    fn process_is_alive(pid: u32) -> bool {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        };

        // SAFETY: OpenProcess returns a handle owned by this function and closed below.
        match unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        } {
            Ok(handle) => {
                // SAFETY: `handle` is a valid process handle.
                let exited = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
                // SAFETY: closing the handle returned by OpenProcess exactly once.
                drop(unsafe { CloseHandle(handle) });
                !exited
            }
            Err(_) => false,
        }
    }

    #[cfg(windows)]
    struct KillOnDrop(std::process::Child);

    #[cfg(windows)]
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            drop(self.0.kill());
            drop(self.0.wait());
        }
    }

    #[cfg(windows)]
    async fn wait_until(mut condition: impl FnMut() -> bool, what: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !condition() {
            assert!(std::time::Instant::now() < deadline, "timed out: {what}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn dropping_the_probe_reaps_descendants_but_not_unrelated_processes() {
        let workdir = PrivateWorkdir::create().expect("private working directory");
        let marker = workdir.path().join("descendant.pid");
        let script = format!(
            "$p = Start-Process -PassThru -WindowStyle Hidden powershell.exe \
             -ArgumentList '-NoLogo','-NoProfile','-Command','Start-Sleep -Seconds 120'; \
             Set-Content -LiteralPath '{}' -Value $p.Id; Start-Sleep -Seconds 120",
            marker.display()
        );
        // An unrelated process started outside the probe must survive the reap.
        let bystander = KillOnDrop(
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-Command",
                    "Start-Sleep -Seconds 120",
                ])
                .spawn()
                .expect("start bystander"),
        );

        let mut command = AsyncCommand::new("powershell.exe");
        command
            .args(["-NoLogo", "-NoProfile", "-Command", &script])
            .current_dir(workdir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let (child, containment) = spawn_contained(&mut command).expect("spawn contained probe");

        let mut descendant = None;
        wait_until(
            || {
                descendant = std::fs::read_to_string(&marker)
                    .ok()
                    .and_then(|text| text.trim().parse::<u32>().ok());
                descendant.is_some()
            },
            "descendant pid",
        )
        .await;
        let descendant = descendant.expect("descendant pid");
        assert!(process_is_alive(descendant));

        // kill_on_drop only kills the direct child: the descendant survives it
        // and is reaped only when the job closes.
        drop(child);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            process_is_alive(descendant),
            "kill_on_drop alone must not be what reaps the descendant"
        );
        drop(containment);
        wait_until(|| !process_is_alive(descendant), "descendant reaped").await;
        assert!(process_is_alive(bystander.0.id()));
    }

    #[test]
    fn usage_fallback_uses_exact_noninteractive_command_and_bounded_timeouts() {
        assert_eq!(VERSION_ARGS, ["--version"]);
        assert_eq!(
            USAGE_ARGS,
            [
                "-p",
                "/usage",
                "--output-format",
                "json",
                "--print-timeout",
                "90s"
            ]
        );
        assert_eq!(VERSION_TIMEOUT, Duration::from_secs(3));
        assert_eq!(REPORT_TIMEOUT, Duration::from_secs(90));

        let workdir = PrivateWorkdir::create().expect("private working directory");
        let version_command = prepare_command(Path::new("agy"), &VERSION_ARGS, workdir.path());
        assert_eq!(
            version_command
                .as_std()
                .get_args()
                .map(|arg| arg.to_str())
                .collect::<Vec<_>>(),
            vec![Some("--version")]
        );

        let command = prepare_command(Path::new("agy"), &USAGE_ARGS, workdir.path());
        let standard_command = command.as_std();
        assert_eq!(
            standard_command
                .get_args()
                .map(|arg| arg.to_str())
                .collect::<Vec<_>>(),
            vec![
                Some("-p"),
                Some("/usage"),
                Some("--output-format"),
                Some("json"),
                Some("--print-timeout"),
                Some("90s")
            ]
        );
    }
}
