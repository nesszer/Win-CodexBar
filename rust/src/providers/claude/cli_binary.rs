#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::{Command as StdCommand, Stdio};

/// Locate the Claude CLI for shell integrations that need to reopen a session.
pub fn locate_claude_binary() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("CLAUDE_BINARY")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }

    #[cfg(windows)]
    {
        let candidates = [
            // Direct install
            dirs::data_local_dir().map(|p| p.join("Programs").join("claude").join("claude.exe")),
            // npm global (AppData\Roaming\npm)
            dirs::data_local_dir().map(|p| p.join("npm").join("claude.cmd")),
            dirs::home_dir().map(|h| {
                h.join("AppData")
                    .join("Roaming")
                    .join("npm")
                    .join("claude.cmd")
            }),
            // npm global alternate (~\.npm-global)
            dirs::home_dir().map(|h| h.join(".npm-global").join("claude.cmd")),
            // Volta managed
            dirs::data_local_dir().map(|p| {
                p.join("Volta")
                    .join("tools")
                    .join("image")
                    .join("packages")
                    .join("@anthropic-ai")
                    .join("claude-code")
                    .join("bin")
                    .join("claude.cmd")
            }),
            // fnm managed (via shim)
            dirs::data_local_dir().map(|p| p.join("fnm_multishells").join("claude.cmd")),
            // PATH lookup
            find_windows_claude_in_path(),
        ];

        candidates.into_iter().flatten().find(|p| p.exists())
    }

    #[cfg(not(windows))]
    {
        which::which("claude").ok()
    }
}

#[cfg(windows)]
fn find_windows_claude_in_path() -> Option<std::path::PathBuf> {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut command = StdCommand::new("where");
    command
        .arg("claude")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let output = command.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let mut matches: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect();

    matches.sort_by_key(|path| {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("cmd") => 0,
            Some("bat") => 1,
            Some("exe") => 2,
            _ => 3,
        }
    });

    matches.into_iter().find(|path| path.exists())
}

/// Detect the version of the claude CLI
pub(super) fn detect_claude_version() -> Option<String> {
    let claude_path = locate_claude_binary()?;

    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new(claude_path);
    cmd.args(["--version"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().ok()?;

    if output.status.success() {
        let version_str = String::from_utf8_lossy(&output.stdout);
        super::super::extract_semver(&version_str)
    } else {
        None
    }
}
