//! Browser detection for Windows and WSL
//!
//! On native Windows, uses standard AppData paths.
//! On WSL, resolves browser paths via /mnt/c/ to access Windows browser data.

use std::path::{Path, PathBuf};

use crate::wsl;

/// Supported browser types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserType {
    Chrome,
    ChromeBeta,
    ChromeDev,
    ChromeCanary,
    ChromeForTesting,
    Edge,
    Brave,
    Arc,
    Firefox,
    Chromium,
}

impl BrowserType {
    /// Get all browser types
    pub fn all() -> &'static [BrowserType] {
        &[
            BrowserType::Chrome,
            BrowserType::ChromeBeta,
            BrowserType::ChromeDev,
            BrowserType::ChromeCanary,
            BrowserType::ChromeForTesting,
            BrowserType::Edge,
            BrowserType::Brave,
            BrowserType::Arc,
            BrowserType::Firefox,
            BrowserType::Chromium,
        ]
    }

    /// Check if this is a Chromium-based browser
    pub fn is_chromium_based(&self) -> bool {
        !matches!(self, BrowserType::Firefox)
    }

    /// Get the display name
    pub fn display_name(&self) -> &'static str {
        match self {
            BrowserType::Chrome => "Google Chrome",
            BrowserType::ChromeBeta => "Google Chrome Beta",
            BrowserType::ChromeDev => "Google Chrome Dev",
            BrowserType::ChromeCanary => "Google Chrome Canary",
            BrowserType::ChromeForTesting => "Chrome for Testing",
            BrowserType::Edge => "Microsoft Edge",
            BrowserType::Brave => "Brave",
            BrowserType::Arc => "Arc",
            BrowserType::Firefox => "Firefox",
            BrowserType::Chromium => "Chromium",
        }
    }

    /// Stable browser identifier shared by the core and desktop IPC bridge.
    pub fn key(&self) -> &'static str {
        match self {
            BrowserType::Chrome => "chrome",
            BrowserType::ChromeBeta => "chrome-beta",
            BrowserType::ChromeDev => "chrome-dev",
            BrowserType::ChromeCanary => "chrome-canary",
            BrowserType::ChromeForTesting => "chrome-for-testing",
            BrowserType::Edge => "edge",
            BrowserType::Brave => "brave",
            BrowserType::Arc => "arc",
            BrowserType::Firefox => "firefox",
            BrowserType::Chromium => "chromium",
        }
    }

    /// Resolve a profile root under Windows AppData. Firefox requires Roaming;
    /// local-root browsers remain resolvable when Roaming is unavailable.
    pub fn user_data_dir(&self, local: &Path, roaming: Option<&Path>) -> Option<PathBuf> {
        match self {
            BrowserType::Chrome => Some(local.join("Google/Chrome/User Data")),
            BrowserType::ChromeBeta => Some(local.join("Google/Chrome Beta/User Data")),
            BrowserType::ChromeDev => Some(local.join("Google/Chrome Dev/User Data")),
            BrowserType::ChromeCanary => Some(local.join("Google/Chrome SxS/User Data")),
            BrowserType::ChromeForTesting => {
                Some(local.join("Google/Chrome for Testing/User Data"))
            }
            BrowserType::Edge => Some(local.join("Microsoft/Edge/User Data")),
            BrowserType::Brave => Some(local.join("BraveSoftware/Brave-Browser/User Data")),
            BrowserType::Arc => Some(local.join("Arc/User Data")),
            BrowserType::Firefox => roaming.map(|root| root.join("Mozilla/Firefox/Profiles")),
            BrowserType::Chromium => Some(local.join("Chromium/User Data")),
        }
    }
}

/// A detected browser installation
#[derive(Debug, Clone)]
pub struct DetectedBrowser {
    pub browser_type: BrowserType,
    pub user_data_dir: PathBuf,
    pub profiles: Vec<BrowserProfile>,
}

/// A browser profile
#[derive(Debug, Clone)]
pub struct BrowserProfile {
    pub name: String,
    pub path: PathBuf,
    pub is_default: bool,
}

impl BrowserProfile {
    /// Get the cookies database path for Chromium browsers
    pub fn cookies_db_path(&self) -> PathBuf {
        self.path.join("Network").join("Cookies")
    }

    /// Get the Local State file path (contains encryption key)
    pub fn local_state_path(&self, user_data_dir: &Path) -> PathBuf {
        user_data_dir.join("Local State")
    }
}

/// Browser detector for Windows and WSL
pub struct BrowserDetector;

impl BrowserDetector {
    /// Detect all installed browsers.
    ///
    /// On native Windows, scans standard AppData directories.
    /// On WSL, also scans Windows browser paths via /mnt/c/.
    pub fn detect_all() -> Vec<DetectedBrowser> {
        let mut browsers = Vec::new();

        for browser_type in BrowserType::all() {
            if let Some(browser) = Self::detect(*browser_type) {
                browsers.push(browser);
            }
        }

        if wsl::is_wsl() {
            let wsl_browsers = super::wsl_paths::WslBrowserDetector::detect_all();
            for wsl_browser in wsl_browsers {
                let already_found = browsers
                    .iter()
                    .any(|b| b.browser_type == wsl_browser.browser_type);
                if !already_found {
                    browsers.push(wsl_browser);
                }
            }
        }

        browsers
    }

    /// Detect a specific browser
    pub fn detect(browser_type: BrowserType) -> Option<DetectedBrowser> {
        let user_data_dir = Self::get_user_data_dir(browser_type)?;
        Self::detect_at_path(browser_type, user_data_dir)
    }

    /// Detect from explicit AppData roots, including WSL-mounted Windows roots.
    pub(super) fn detect_in_roots(
        browser_type: BrowserType,
        local: &Path,
        roaming: Option<&Path>,
    ) -> Option<DetectedBrowser> {
        let user_data_dir = browser_type.user_data_dir(local, roaming)?;
        Self::detect_at_path(browser_type, user_data_dir)
    }

    fn detect_at_path(
        browser_type: BrowserType,
        user_data_dir: PathBuf,
    ) -> Option<DetectedBrowser> {
        if !user_data_dir.exists() {
            return None;
        }

        let profiles = Self::detect_profiles(browser_type, &user_data_dir);

        if profiles.is_empty() {
            return None;
        }

        Some(DetectedBrowser {
            browser_type,
            user_data_dir,
            profiles,
        })
    }

    /// Get the user data directory for a browser
    fn get_user_data_dir(browser_type: BrowserType) -> Option<PathBuf> {
        // In WSL, prefer Windows AppData paths when available
        if wsl::is_wsl()
            && let Some(appdata_local) = wsl::windows_appdata_local()
        {
            let roaming = wsl::windows_appdata_roaming();
            if let Some(path) = browser_type.user_data_dir(&appdata_local, roaming.as_deref())
                && path.exists()
            {
                return Some(path);
            }
        }

        let local_app_data = dirs::data_local_dir()?;
        let app_data = dirs::data_dir();
        browser_type.user_data_dir(&local_app_data, app_data.as_deref())
    }

    /// Detect profiles within a browser's user data directory
    fn detect_profiles(browser_type: BrowserType, user_data_dir: &PathBuf) -> Vec<BrowserProfile> {
        if browser_type == BrowserType::Firefox {
            return Self::detect_firefox_profiles(user_data_dir);
        }

        Self::detect_chromium_profiles(user_data_dir)
    }

    /// Detect Chromium-based browser profiles
    pub(super) fn detect_chromium_profiles(user_data_dir: &Path) -> Vec<BrowserProfile> {
        let Ok(entries) = std::fs::read_dir(user_data_dir) else {
            return Vec::new();
        };

        let mut profiles: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let path = entry.path();
                let is_profile =
                    name == "Default" || name.starts_with("Profile ") || name.starts_with("user-");
                (is_profile && path.is_dir()).then(|| BrowserProfile {
                    is_default: name == "Default",
                    name,
                    path,
                })
            })
            .collect();
        profiles.sort_by(|left, right| left.name.cmp(&right.name));
        profiles
    }

    /// Detect Firefox profiles
    fn detect_firefox_profiles(profiles_dir: &PathBuf) -> Vec<BrowserProfile> {
        let mut profiles = Vec::new();

        if let Ok(entries) = std::fs::read_dir(profiles_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();

                // Firefox profiles are named like "abcd1234.default" or "abcd1234.default-release"
                if path.is_dir() && name.contains('.') {
                    let is_default = name.contains("default");
                    profiles.push(BrowserProfile {
                        name,
                        path,
                        is_default,
                    });
                }
            }
        }

        profiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_paths_and_keys_cover_every_type() {
        let local = Path::new("C:/Users/test/AppData/Local");
        let roaming = Path::new("C:/Users/test/AppData/Roaming");
        let expected = [
            (
                BrowserType::Chrome,
                "Google/Chrome/User Data",
                "Google Chrome",
                "chrome",
            ),
            (
                BrowserType::ChromeBeta,
                "Google/Chrome Beta/User Data",
                "Google Chrome Beta",
                "chrome-beta",
            ),
            (
                BrowserType::ChromeDev,
                "Google/Chrome Dev/User Data",
                "Google Chrome Dev",
                "chrome-dev",
            ),
            (
                BrowserType::ChromeCanary,
                "Google/Chrome SxS/User Data",
                "Google Chrome Canary",
                "chrome-canary",
            ),
            (
                BrowserType::ChromeForTesting,
                "Google/Chrome for Testing/User Data",
                "Chrome for Testing",
                "chrome-for-testing",
            ),
            (
                BrowserType::Edge,
                "Microsoft/Edge/User Data",
                "Microsoft Edge",
                "edge",
            ),
            (
                BrowserType::Brave,
                "BraveSoftware/Brave-Browser/User Data",
                "Brave",
                "brave",
            ),
            (BrowserType::Arc, "Arc/User Data", "Arc", "arc"),
            (
                BrowserType::Firefox,
                "Mozilla/Firefox/Profiles",
                "Firefox",
                "firefox",
            ),
            (
                BrowserType::Chromium,
                "Chromium/User Data",
                "Chromium",
                "chromium",
            ),
        ];

        assert_eq!(BrowserType::all().len(), expected.len());
        let mut keys = std::collections::HashSet::new();
        for (browser, relative_path, display_name, key) in expected {
            let root = if browser == BrowserType::Firefox {
                roaming
            } else {
                local
            };
            assert_eq!(
                browser.user_data_dir(local, Some(roaming)),
                Some(root.join(relative_path)),
                "wrong profile root for {display_name}"
            );
            assert_eq!(browser.display_name(), display_name);
            assert_eq!(browser.key(), key);
            assert!(keys.insert(browser.key()), "duplicate IPC key: {key}");
            assert!(BrowserType::all().contains(&browser));
            assert_eq!(
                browser.user_data_dir(local, None),
                (browser != BrowserType::Firefox).then(|| local.join(relative_path))
            );
        }
    }

    #[test]
    fn test_browser_detection() {
        let browsers = BrowserDetector::detect_all();
        println!("Detected {} browsers", browsers.len());
        for browser in &browsers {
            println!(
                "  {} at {:?} ({} profiles)",
                browser.browser_type.display_name(),
                browser.user_data_dir,
                browser.profiles.len()
            );
        }
    }
}
