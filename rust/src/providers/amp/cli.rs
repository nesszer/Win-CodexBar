use chrono::Utc;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::core::ProviderError;

use super::display::{AmpDisplayUsage, parse_amp_display_text};

fn find_amp_cli() -> Option<PathBuf> {
    which::which("amp").ok().filter(|path| path.exists())
}

pub(super) async fn fetch_usage() -> Result<AmpDisplayUsage, ProviderError> {
    let executable = find_amp_cli().ok_or_else(|| {
        ProviderError::NotInstalled(
            "Amp CLI not found. Install it from https://ampcode.com".to_string(),
        )
    })?;

    let mut command = Command::new(executable);
    command
        .args(["usage"])
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    hide_windows_console(&mut command);
    let output = timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| ProviderError::Timeout)?
        .map_err(|error| ProviderError::Other(format!("Failed to run Amp CLI: {error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };

    if !output.status.success() {
        let lowercase = text.to_ascii_lowercase();
        if lowercase.contains("login") || lowercase.contains("auth") {
            return Err(ProviderError::AuthRequired);
        }
        return Err(ProviderError::Other(format!("Amp CLI failed: {text}")));
    }
    usage_from_amp_cli_output(text, Utc::now())
}

pub(super) fn usage_from_amp_cli_output(
    text: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<AmpDisplayUsage, ProviderError> {
    parse_amp_display_text(text, now).map_err(|error| match error {
        ProviderError::Parse(_) => {
            ProviderError::Parse("Amp CLI returned unrecognized usage output".to_string())
        }
        other => other,
    })
}

#[cfg(windows)]
fn hide_windows_console(command: &mut Command) {
    command.creation_flags(0x08000000);
}

#[cfg(not(windows))]
fn hide_windows_console(command: &mut Command) {
    let _ = command;
}
