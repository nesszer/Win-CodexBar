//! Canonical Codex CLI executable discovery.
//!
//! Keep install-layout knowledge here so login, provider version detection, and
//! TTY launching do not drift into separate path lists.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Locate the Codex CLI for shell integrations that need to reopen a session.
pub fn locate_codex_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_BINARY")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    if let Ok(path) = which::which("codex") {
        return Some(path);
    }

    known_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
        .or_else(desktop_package_binary)
}

fn known_candidates() -> Vec<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("AppData")
                .join("Local")
        });
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let desktop_bin = local_app_data.join("OpenAI").join("Codex").join("bin");

    let mut candidates = vec![
        desktop_bin.join("codex.exe"),
        home.join(".bun").join("bin").join("codex.exe"),
        local_app_data
            .join("Microsoft")
            .join("WindowsApps")
            .join("codex.exe"),
        local_app_data
            .join("Programs")
            .join("codex")
            .join("codex.exe"),
    ];
    candidates.extend(versioned_binaries(&desktop_bin));

    if let Some(roaming) = dirs::config_dir().or_else(dirs::data_dir) {
        candidates.push(roaming.join("npm").join("codex.cmd"));
        candidates.push(
            roaming
                .join("fnm")
                .join("aliases")
                .join("default")
                .join("codex.cmd"),
        );
    }
    candidates
}

fn versioned_binaries(root: &Path) -> Vec<PathBuf> {
    let mut binaries: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("codex.exe"))
        .filter(|path| path.is_file())
        .collect();
    binaries.sort_by_key(|path| {
        std::cmp::Reverse(path.metadata().and_then(|meta| meta.modified()).ok())
    });
    binaries
}

#[cfg(windows)]
fn desktop_package_binary() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;

    let powershell = PathBuf::from(std::env::var_os("WINDIR")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | ForEach-Object { Join-Path $_.InstallLocation 'app\\resources\\codex.exe' } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1",
        ])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    path.is_file().then_some(path)
}

#[cfg(not(windows))]
fn desktop_package_binary() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_versioned_desktop_cli_and_ignores_incomplete_updates() {
        let root = tempfile::tempdir().unwrap();
        let installed = root.path().join("hash with spaces");
        std::fs::create_dir(&installed).unwrap();
        std::fs::write(installed.join("codex.exe"), b"fixture").unwrap();
        std::fs::create_dir(root.path().join("incomplete")).unwrap();
        std::fs::write(root.path().join("unrelated"), b"fixture").unwrap();
        assert_eq!(
            versioned_binaries(root.path()),
            vec![installed.join("codex.exe")]
        );
        assert!(versioned_binaries(&root.path().join("missing")).is_empty());
    }
}
