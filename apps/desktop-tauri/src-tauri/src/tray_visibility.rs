//! Windows-only tray icon visibility (NotifyIconSettings registry).
//!
//! On Windows 11 (build ≥ 22000) each icon's placement is stored in
//! `HKCU\Control Panel\NotifyIconSettings\<id>\IsPromoted` (REG_DWORD).
//! Writing 1 promotes the icon out of the overflow chevron; 0 demotes it.
//! HKCU requires no elevation. The key only appears after the icon has been
//! registered at least once.
//!
//! On Windows 10 and non-Windows platforms this module returns `Unsupported`
//! and never touches the registry.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrayVisibilitySupport {
    Supported,
    UnsupportedOs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PromotionState {
    Promoted,
    NotPromoted,
    EntryNotFound,
}

#[derive(Debug)]
pub enum TrayVisibilityError {
    Unsupported,
    #[cfg(target_os = "windows")]
    Registry(std::io::Error),
    #[cfg(target_os = "windows")]
    ExePathUnresolvable(std::io::Error),
}

impl std::fmt::Display for TrayVisibilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "tray visibility not supported on this OS"),
            #[cfg(target_os = "windows")]
            Self::Registry(e) => write!(f, "registry error: {e}"),
            #[cfg(target_os = "windows")]
            Self::ExePathUnresolvable(e) => write!(f, "cannot resolve current exe: {e}"),
        }
    }
}

/// DTO serialized to the frontend.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayVisibilityStatusDto {
    pub support: TrayVisibilitySupport,
    pub state: PromotionStateSafe,
}

/// PromotionState extended with an `Unknown` variant for the DTO when the
/// registry call fails but the OS is supported (e.g. entry not found yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PromotionStateSafe {
    Promoted,
    NotPromoted,
    EntryNotFound,
    Unknown,
}

impl From<PromotionState> for PromotionStateSafe {
    fn from(s: PromotionState) -> Self {
        match s {
            PromotionState::Promoted => Self::Promoted,
            PromotionState::NotPromoted => Self::NotPromoted,
            PromotionState::EntryNotFound => Self::EntryNotFound,
        }
    }
}

// ── Pure helpers (tested without registry access) ────────────────────────────

/// True for Windows 11 build numbers (≥ 22000).
pub fn is_win11_build(build: u32) -> bool {
    build >= 22000
}

/// Normalize a Windows executable path for equality checks.
///
/// Strips the `\\?\` / `\\?\UNC\` extended prefixes that `current_exe()` may
/// return, unifies separators, and lowercases.
pub fn normalize_exe_path(path: &str) -> String {
    let mut s = path.replace('/', "\\");
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    s.trim_end_matches('\\').to_ascii_lowercase()
}

/// Case-insensitive path comparison after stripping trailing separators and
/// Win32 extended-length prefixes.
/// Returns true when `entry_path` names the same executable as `exe_path`.
pub fn matches_current_exe(entry_path: &str, exe_path: &std::path::Path) -> bool {
    normalize_exe_path(entry_path) == normalize_exe_path(&exe_path.to_string_lossy())
}

/// True when both paths end with the same file name (case-insensitive).
/// Used as a fallback when the full path differs after an install-dir move.
pub fn matches_exe_file_name(entry_path: &str, exe_path: &std::path::Path) -> bool {
    let entry_name = normalize_exe_path(entry_path)
        .rsplit('\\')
        .next()
        .unwrap_or_default()
        .to_string();
    let exe_name = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(normalize_exe_path)
        .unwrap_or_default();
    !entry_name.is_empty() && entry_name == exe_name
}

// ── Platform-specific implementation ─────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::*;

    const NOTIFY_ICON_SETTINGS: &str = r"Control Panel\NotifyIconSettings";
    const IS_PROMOTED: &str = "IsPromoted";
    const EXECUTABLE_PATH: &str = "ExecutablePath";

    fn current_build() -> u32 {
        use winreg::enums::*;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(key) = hklm.open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") else {
            return 0;
        };
        let Ok(build_str): Result<String, _> = key.get_value("CurrentBuild") else {
            return 0;
        };
        build_str.trim().parse().unwrap_or(0)
    }

    pub fn support_status() -> TrayVisibilitySupport {
        if is_win11_build(current_build()) {
            TrayVisibilitySupport::Supported
        } else {
            TrayVisibilitySupport::UnsupportedOs
        }
    }

    fn find_own_subkey(
        settings_key: &RegKey,
        exe_path: &std::path::Path,
    ) -> Result<Option<RegKey>, std::io::Error> {
        let mut by_name: Option<RegKey> = None;
        let mut name_matches = 0u32;

        for name in settings_key.enum_keys().flatten() {
            let Ok(subkey) = settings_key.open_subkey_with_flags(&name, KEY_READ | KEY_WRITE)
            else {
                continue;
            };
            let Ok(entry_path): Result<String, _> = subkey.get_value(EXECUTABLE_PATH) else {
                continue;
            };
            if matches_current_exe(&entry_path, exe_path) {
                return Ok(Some(subkey));
            }
            if matches_exe_file_name(&entry_path, exe_path) {
                name_matches += 1;
                by_name = Some(subkey);
            }
        }

        // Fallback: unique file-name match (handles install-dir moves / path
        // rewrites where Windows still has a single CodexBar notify entry).
        if name_matches == 1 {
            return Ok(by_name);
        }
        Ok(None)
    }

    pub fn current_state() -> Result<PromotionState, TrayVisibilityError> {
        if !matches!(support_status(), TrayVisibilitySupport::Supported) {
            return Err(TrayVisibilityError::Unsupported);
        }
        let exe_path = std::env::current_exe().map_err(TrayVisibilityError::ExePathUnresolvable)?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(settings_key) = hkcu.open_subkey_with_flags(NOTIFY_ICON_SETTINGS, KEY_READ) else {
            return Ok(PromotionState::EntryNotFound);
        };
        let Some(subkey) =
            find_own_subkey(&settings_key, &exe_path).map_err(TrayVisibilityError::Registry)?
        else {
            return Ok(PromotionState::EntryNotFound);
        };
        let promoted: u32 = subkey.get_value(IS_PROMOTED).unwrap_or(0);
        if promoted != 0 {
            Ok(PromotionState::Promoted)
        } else {
            Ok(PromotionState::NotPromoted)
        }
    }

    pub fn set_promoted(promoted: bool) -> Result<PromotionState, TrayVisibilityError> {
        if !matches!(support_status(), TrayVisibilitySupport::Supported) {
            return Err(TrayVisibilityError::Unsupported);
        }
        let exe_path = std::env::current_exe().map_err(TrayVisibilityError::ExePathUnresolvable)?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(settings_key) =
            hkcu.open_subkey_with_flags(NOTIFY_ICON_SETTINGS, KEY_READ | KEY_WRITE)
        else {
            return Ok(PromotionState::EntryNotFound);
        };
        let Some(subkey) =
            find_own_subkey(&settings_key, &exe_path).map_err(TrayVisibilityError::Registry)?
        else {
            return Ok(PromotionState::EntryNotFound);
        };
        let value: u32 = if promoted { 1 } else { 0 };
        subkey
            .set_value(IS_PROMOTED, &value)
            .map_err(TrayVisibilityError::Registry)?;
        if promoted {
            Ok(PromotionState::Promoted)
        } else {
            Ok(PromotionState::NotPromoted)
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod non_windows {
    use super::*;

    pub fn support_status() -> TrayVisibilitySupport {
        TrayVisibilitySupport::UnsupportedOs
    }

    pub fn current_state() -> Result<PromotionState, TrayVisibilityError> {
        Err(TrayVisibilityError::Unsupported)
    }

    pub fn set_promoted(_promoted: bool) -> Result<PromotionState, TrayVisibilityError> {
        Err(TrayVisibilityError::Unsupported)
    }
}

#[cfg(not(target_os = "windows"))]
use non_windows as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub fn support_status() -> TrayVisibilitySupport {
    platform::support_status()
}

pub fn current_state() -> Result<PromotionState, TrayVisibilityError> {
    platform::current_state()
}

pub fn set_promoted(promoted: bool) -> Result<PromotionState, TrayVisibilityError> {
    platform::set_promoted(promoted)
}

/// Whether `set_promoted(false)` should actually write to the registry.
/// We only demote when the setting was previously on — never fight a user who
/// pinned the icon manually while the setting was off.
pub fn should_write_demotion(previously_promoted: bool, now_promoted: bool) -> bool {
    previously_promoted && !now_promoted
}

/// Apply the promotion setting as a best-effort side effect. Logs warnings on
/// failure; never propagates errors to the caller.
pub fn apply_promotion(promote: bool) {
    if !matches!(support_status(), TrayVisibilitySupport::Supported) {
        return;
    }
    match set_promoted(promote) {
        Ok(state) => {
            tracing::debug!("tray promotion applied: {state:?}");
        }
        Err(TrayVisibilityError::Unsupported) => {}
        Err(ref e) => {
            tracing::warn!("tray visibility: could not apply promotion: {e}");
        }
    }
}

// ── Status command ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn tray_visibility_status() -> TrayVisibilityStatusDto {
    let support = support_status();
    let state = if matches!(support, TrayVisibilitySupport::Supported) {
        match current_state() {
            Ok(s) => PromotionStateSafe::from(s),
            Err(_) => PromotionStateSafe::Unknown,
        }
    } else {
        PromotionStateSafe::Unknown
    };
    TrayVisibilityStatusDto { support, state }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn win10_build_not_supported() {
        assert!(!is_win11_build(19045));
    }

    #[test]
    fn win11_rtm_build_supported() {
        assert!(is_win11_build(22000));
    }

    #[test]
    fn win11_later_build_supported() {
        assert!(is_win11_build(26200));
    }

    #[test]
    fn matches_same_path_case_insensitive() {
        assert!(matches_current_exe(
            r"C:\APPS\CodexBar.exe",
            Path::new(r"c:\apps\codexbar.exe")
        ));
    }

    #[test]
    fn rejects_different_exe_same_dir() {
        assert!(!matches_current_exe(
            r"C:\apps\Other.exe",
            Path::new(r"C:\apps\CodexBar.exe")
        ));
    }

    #[test]
    fn matches_forward_slashes() {
        assert!(matches_current_exe(
            r"C:/apps/CodexBar.exe",
            Path::new(r"C:\apps\codexbar.exe")
        ));
    }

    #[test]
    fn matches_extended_length_prefix_from_current_exe() {
        assert!(matches_current_exe(
            r"C:\Users\mac\AppData\Local\Programs\CodexBar\codexbar.exe",
            Path::new(r"\\?\C:\Users\mac\AppData\Local\Programs\CodexBar\codexbar.exe")
        ));
    }

    #[test]
    fn matches_exe_file_name_across_install_dirs() {
        assert!(matches_exe_file_name(
            r"C:\Old\CodexBar\codexbar.exe",
            Path::new(r"C:\Users\mac\AppData\Local\Programs\CodexBar\codexbar.exe")
        ));
        assert!(!matches_exe_file_name(
            r"C:\Old\CodexBar\codexbar-cli.exe",
            Path::new(r"C:\Users\mac\AppData\Local\Programs\CodexBar\codexbar.exe")
        ));
    }

    #[test]
    fn normalize_strips_unc_extended_prefix() {
        assert_eq!(
            normalize_exe_path(r"\\?\UNC\server\share\codexbar.exe"),
            r"\\server\share\codexbar.exe"
        );
    }

    #[test]
    fn should_write_demotion_only_when_previously_on() {
        assert!(should_write_demotion(true, false));
        assert!(!should_write_demotion(false, false));
        assert!(!should_write_demotion(false, true));
        assert!(!should_write_demotion(true, true));
    }

    #[test]
    fn dto_serializes_support_and_state() {
        let dto = TrayVisibilityStatusDto {
            support: TrayVisibilitySupport::UnsupportedOs,
            state: PromotionStateSafe::Unknown,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"unsupportedOs\""));
        assert!(json.contains("\"unknown\""));
    }
}
