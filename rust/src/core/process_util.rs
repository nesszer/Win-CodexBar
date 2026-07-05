//! Hardened resolution of external executables before spawning them.
//!
//! On Windows, `Command::new("name")` is handed to `CreateProcess`, which
//! searches the current working directory *before* System32 and `PATH`. A
//! binary planted in a writable working directory (`gh.exe`, `aws.exe`,
//! `where.exe`, `powershell.exe`, ...) would then run as the current user.
//! Resolving to an absolute path first removes that vector:
//!
//! * third-party CLIs are resolved with [`resolve_in_path`], which mirrors
//!   `PATH` (and never prepends the CWD), returning `None` when the tool is
//!   not installed;
//! * Windows system binaries are resolved to their absolute System32 path
//!   with [`system32_exe`] / [`windows_powershell`].
//!
//! This mirrors the hardening already applied to Kiro CLI discovery
//! (`providers::kiro::version`).

use std::path::PathBuf;

/// Resolve a third-party executable via `PATH` only (never the CWD).
///
/// Returns `None` when the tool is not installed, matching the previous
/// "spawn failed => treat as unavailable" behaviour of the call sites.
pub fn resolve_in_path(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

/// Absolute path to a Windows System32 executable (e.g. `where.exe`).
///
/// Falls back to the bare name only if `%SystemRoot%` is unset or the file is
/// missing; on non-Windows targets it returns the bare name unchanged (these
/// call sites only do meaningful work on Windows).
pub fn system32_exe(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let candidate = PathBuf::from(root).join("System32").join(name);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

/// Absolute path to the bundled Windows PowerShell 5.1 host, hardened against
/// CWD hijacking. Falls back to `powershell.exe` when the well-known location
/// is unavailable (and on non-Windows targets, where these call sites only
/// attempt to run on Windows anyway).
pub fn windows_powershell() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let candidate = PathBuf::from(root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("powershell.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_in_path_returns_none_for_missing_tool() {
        assert!(resolve_in_path("codexbar-definitely-not-a-real-binary-xyz").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn system32_exe_resolves_where_to_absolute_path() {
        let p = system32_exe("where.exe");
        assert!(p.is_absolute(), "expected absolute System32 path, got {p:?}");
        assert!(p.ends_with("where.exe"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_powershell_resolves_to_absolute_path() {
        let p = windows_powershell();
        assert!(p.is_absolute(), "expected absolute path, got {p:?}");
        assert!(p.ends_with("powershell.exe"));
    }
}
