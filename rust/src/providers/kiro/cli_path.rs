//! Kiro CLI binary detection.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Cached CLI path
static CLI_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
const KIRO_CLI_PATH_ENV: &str = "CODEXBAR_KIRO_CLI_PATH";

fn is_allowed_kiro_binary(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    #[cfg(target_os = "windows")]
    {
        file_name.eq_ignore_ascii_case("kiro-cli.exe")
    }

    #[cfg(not(target_os = "windows"))]
    {
        file_name == "kiro-cli" || file_name == "kiro"
    }
}

fn env_override_cli_path() -> Option<PathBuf> {
    let raw = std::env::var(KIRO_CLI_PATH_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = PathBuf::from(trimmed);
    if is_allowed_kiro_binary(&path) {
        return Some(path);
    }

    None
}

/// Find Kiro CLI binary path
pub fn find_kiro_cli() -> Option<PathBuf> {
    CLI_PATH
        .get_or_init(|| {
            // 1. Check explicit environment override first
            if let Some(path) = env_override_cli_path() {
                return Some(path);
            }

            // 2. Hardened PATH lookup - use which but validate the result
            //    (avoids CWD hijacking by not executing bare command names)
            if let Ok(path) = which::which("kiro-cli")
                && is_allowed_kiro_binary(&path)
            {
                return Some(path);
            }
            if let Ok(path) = which::which("kiro")
                && is_allowed_kiro_binary(&path)
            {
                return Some(path);
            }

            // 3. Fall back to known install locations
            #[cfg(target_os = "windows")]
            {
                let possible_paths = [
                    dirs::data_local_dir()
                        .map(|p| p.join("Programs").join("Kiro").join("kiro-cli.exe")),
                    Some(PathBuf::from("C:\\Program Files\\Kiro\\kiro-cli.exe")),
                ];
                for path in possible_paths.into_iter().flatten() {
                    if is_allowed_kiro_binary(&path) {
                        return Some(path);
                    }
                }
            }

            None
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn temp_binary_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("codexbar-kiro-version-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp binary directory");
        let path = dir.join(name);
        File::create(&path).expect("create temp binary placeholder");
        path
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_rejects_gui_kiro_binary_as_cli() {
        let cli_path = temp_binary_path("kiro-cli.exe");
        let gui_path = temp_binary_path("kiro.exe");

        assert!(is_allowed_kiro_binary(&cli_path));
        assert!(
            !is_allowed_kiro_binary(&gui_path),
            "kiro.exe is the Electron GUI app on Windows; running it as a CLI spawns the IDE"
        );

        // Best-effort cleanup of temp binaries; leftover files are harmless.
        let _removed_cli = std::fs::remove_file(cli_path);
        let _removed_gui = std::fs::remove_file(gui_path);
    }
}
