#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command as AsyncCommand;
use uuid::Uuid;

use super::AntigravityProvider;
use super::quota_summary;
use crate::core::{ProviderError, ProviderFetchResult};

const REPORT_TIMEOUT: Duration = Duration::from_secs(90);
const VERSION_ARGS: [&str; 1] = ["--version"];
const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
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
struct BoundedStdout {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

/// Read a child stdout stream incrementally, retaining only the configured
/// prefix while continuing to drain the pipe so the child cannot block on a
/// full stdout buffer.
async fn read_stdout_limited<R>(mut reader: R, max_bytes: usize) -> std::io::Result<BoundedStdout>
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

    Ok(BoundedStdout {
        bytes,
        exceeded_limit,
    })
}

pub(super) async fn try_fetch(
    binary: Option<PathBuf>,
) -> Result<Option<ProviderFetchResult>, ProviderError> {
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
    let Ok(version) = run_cli_command(&binary, &VERSION_ARGS, VERSION_TIMEOUT).await else {
        return false;
    };
    if version.exceeded_limit {
        return false;
    }
    let version = String::from_utf8_lossy(&version.bytes);
    is_csrf_gated_version(version.trim())
}

async fn fetch_print_usage(binary: &Path) -> Result<ProviderFetchResult, ProviderError> {
    let version = run_cli_command(binary, &VERSION_ARGS, VERSION_TIMEOUT).await?;
    if version.exceeded_limit {
        return Err(ProviderError::Parse(
            "Antigravity CLI version output is too large".into(),
        ));
    }
    let version = String::from_utf8_lossy(&version.bytes);
    if !is_supported_version(version.trim()) {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage reports require agy 1.1.11 or later".into(),
        ));
    }

    let output = run_cli_command(binary, &USAGE_ARGS, REPORT_TIMEOUT).await?;
    if output.exceeded_limit {
        return Err(ProviderError::Parse(
            "Antigravity CLI usage report is too large".into(),
        ));
    }
    let usage = quota_summary::parse_cli_usage_report(&output.bytes)?;
    Ok(AntigravityProvider::fetch_result(usage, "cli"))
}

fn prepare_command(binary: &Path, args: &[&str], working_dir: &Path) -> AsyncCommand {
    let mut command = AsyncCommand::new(binary);
    command
        .args(args)
        .current_dir(working_dir)
        .env_remove(OAUTH_CREDENTIALS_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.as_std_mut().creation_flags(0x0800_0000);
    command
}

async fn run_cli_command(
    binary: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<BoundedStdout, ProviderError> {
    let working_dir = PrivateWorkdir::create()?;
    let mut command = prepare_command(binary, args, working_dir.path());

    let mut child = command
        .spawn()
        .map_err(|_| ProviderError::Other("Failed to start Antigravity CLI".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProviderError::Other("Antigravity CLI usage report failed".into()))?;

    tokio::time::timeout(timeout, async {
        let (stdout_result, status_result) = tokio::join!(
            read_stdout_limited(stdout, REPORT_MAX_OUTPUT_BYTES),
            child.wait()
        );
        let stdout = stdout_result
            .map_err(|_| ProviderError::Other("Antigravity CLI usage report failed".into()))?;
        let status = status_result
            .map_err(|_| ProviderError::Other("Antigravity CLI usage report failed".into()))?;
        if !status.success() {
            return Err(ProviderError::Other(
                "Antigravity CLI usage report failed".into(),
            ));
        }
        Ok(stdout)
    })
    .await
    .map_err(|_| ProviderError::Timeout)?
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
    async fn stdout_capture_retains_only_the_configured_limit() {
        let output = read_stdout_limited(b"0123456789".as_slice(), 4)
            .await
            .expect("stdout reader should succeed");

        assert_eq!(output.bytes, b"0123");
        assert!(output.exceeded_limit);

        let output = read_stdout_limited(b"0123".as_slice(), 4)
            .await
            .expect("stdout reader should succeed");
        assert_eq!(output.bytes, b"0123");
        assert!(!output.exceeded_limit);
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
