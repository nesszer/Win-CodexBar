//! The Windows "start at login" Run-key registration.

use super::*;

impl Settings {
    pub(super) fn start_at_login_exe_path(current_exe: &std::path::Path) -> std::path::PathBuf {
        let file_name = current_exe.file_name().and_then(|name| name.to_str());
        if file_name.is_some_and(|name| {
            name.eq_ignore_ascii_case("codexbar-cli.exe")
                || name.eq_ignore_ascii_case("codexbar-desktop.exe")
        }) && let Some(desktop_exe) = current_exe
            .parent()
            .map(|dir| dir.join("codexbar.exe"))
            .filter(|path| path.exists())
        {
            return desktop_exe;
        }

        current_exe.to_path_buf()
    }

    pub(super) fn start_at_login_command(current_exe: &std::path::Path) -> String {
        let exe_path = Self::start_at_login_exe_path(current_exe);
        format!("\"{}\"", exe_path.display())
    }

    pub(super) fn start_at_login_command_needs_repair(
        existing: &str,
        current_exe: &std::path::Path,
    ) -> bool {
        existing != Self::start_at_login_command(current_exe)
    }

    #[cfg(target_os = "windows")]
    pub fn apply_start_at_login_registry(enabled: bool) -> anyhow::Result<()> {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_READ | KEY_WRITE,
        )?;

        if enabled {
            let exe_path = std::env::current_exe()?;
            let command = Self::start_at_login_command(&exe_path);
            run_key.set_value("CodexBar", &command)?;
        } else {
            // Best-effort removal; a missing value means the desired state already.
            let _removed_value = run_key.delete_value("CodexBar");
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    pub(super) fn sync_start_at_login_registry() -> bool {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(run_key) = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_READ | KEY_WRITE,
        ) else {
            return false;
        };

        let Ok(existing) = run_key.get_value::<String, _>("CodexBar") else {
            return false;
        };

        match std::env::current_exe() {
            Ok(exe_path) if Self::start_at_login_command_needs_repair(&existing, &exe_path) => {
                let command = Self::start_at_login_command(&exe_path);
                if let Err(error) = run_key.set_value("CodexBar", &command) {
                    tracing::warn!("Failed to repair CodexBar start-at-login command: {error}");
                }
            }
            Err(error) => {
                tracing::warn!(
                    "Failed to resolve current executable for start-at-login sync: {error}"
                );
            }
            _ => {}
        }

        true
    }

    #[cfg(not(target_os = "windows"))]
    pub fn apply_start_at_login_registry(_enabled: bool) -> anyhow::Result<()> {
        Ok(())
    }

    /// Set start at login (updates Windows registry)
    pub fn set_start_at_login(&mut self, enabled: bool) -> anyhow::Result<()> {
        self.start_at_login = enabled;
        Self::apply_start_at_login_registry(enabled)?;
        Ok(())
    }

    /// Check if start at login is actually enabled in registry
    #[cfg(target_os = "windows")]
    pub fn is_start_at_login_enabled() -> bool {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") {
            run_key.get_value::<String, _>("CodexBar").is_ok()
        } else {
            false
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn is_start_at_login_enabled() -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
