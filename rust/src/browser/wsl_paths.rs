//! WSL-aware browser detection and path resolution
//!
//! When running inside WSL, Windows browser data lives under /mnt/c/...
//! This module provides path resolvers that detect WSL and map
//! browser profile paths to their Windows host equivalents.

use std::path::PathBuf;

use crate::wsl;

use super::detection::{BrowserProfile, BrowserType, DetectedBrowser};

/// WSL-aware browser detector.
///
/// On native Linux, returns empty (no Windows browsers).
/// On WSL, detects Windows browsers via /mnt/c/ paths.
pub struct WslBrowserDetector;

impl WslBrowserDetector {
    pub fn detect_all() -> Vec<DetectedBrowser> {
        if !wsl::is_wsl() {
            return Vec::new();
        }

        let appdata_local = match wsl::windows_appdata_local() {
            Some(p) => p,
            None => return Vec::new(),
        };

        let mut browsers = Vec::new();

        let candidates = windows_browser_candidates(&appdata_local);
        let candidates: Vec<(BrowserType, PathBuf)> = candidates
            .into_iter()
            .chain([
                (
                    BrowserType::Edge,
                    appdata_local
                        .join("Microsoft")
                        .join("Edge")
                        .join("User Data"),
                ),
                (
                    BrowserType::Brave,
                    appdata_local
                        .join("BraveSoftware")
                        .join("Brave-Browser")
                        .join("User Data"),
                ),
                (
                    BrowserType::Arc,
                    appdata_local.join("Arc").join("User Data"),
                ),
            ])
            .collect();

        for (browser_type, user_data_dir) in candidates {
            if user_data_dir.exists() {
                let profiles = detect_chromium_profiles(&user_data_dir);
                if !profiles.is_empty() {
                    browsers.push(DetectedBrowser {
                        browser_type,
                        user_data_dir,
                        profiles,
                    });
                }
            }
        }

        if let Some(appdata_roaming) = wsl::windows_appdata_roaming() {
            let ff_dir = appdata_roaming
                .join("Mozilla")
                .join("Firefox")
                .join("Profiles");
            if ff_dir.exists() {
                let profiles = detect_firefox_profiles(&ff_dir);
                if !profiles.is_empty() {
                    browsers.push(DetectedBrowser {
                        browser_type: BrowserType::Firefox,
                        user_data_dir: ff_dir,
                        profiles,
                    });
                }
            }
        }

        browsers
    }
}

fn windows_browser_candidates(appdata_local: &std::path::Path) -> Vec<(BrowserType, PathBuf)> {
    [
        BrowserType::Chrome,
        BrowserType::ChromeBeta,
        BrowserType::ChromeDev,
        BrowserType::ChromeCanary,
        BrowserType::ChromeForTesting,
        BrowserType::Chromium,
    ]
    .into_iter()
    .filter_map(|browser_type| {
        browser_type
            .user_data_dir_under(appdata_local)
            .map(|path| (browser_type, path))
    })
    .collect()
}

fn detect_chromium_profiles(user_data_dir: &PathBuf) -> Vec<BrowserProfile> {
    let mut profiles = Vec::new();

    let default_path = user_data_dir.join("Default");
    if default_path.exists() {
        profiles.push(BrowserProfile {
            name: "Default".to_string(),
            path: default_path,
            is_default: true,
        });
    }

    if let Ok(entries) = std::fs::read_dir(user_data_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("Profile ") {
                let path = entry.path();
                if path.is_dir() {
                    profiles.push(BrowserProfile {
                        name,
                        path,
                        is_default: false,
                    });
                }
            }
        }
    }

    profiles
}

fn detect_firefox_profiles(profiles_dir: &PathBuf) -> Vec<BrowserProfile> {
    let mut profiles = Vec::new();

    if let Ok(entries) = std::fs::read_dir(profiles_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_profile_roots_include_every_chrome_channel_and_chromium() {
        let appdata = PathBuf::from("/mnt/c/Users/test/AppData/Local");
        let expected = [
            (BrowserType::Chrome, "Google/Chrome/User Data"),
            (BrowserType::ChromeBeta, "Google/Chrome Beta/User Data"),
            (BrowserType::ChromeDev, "Google/Chrome Dev/User Data"),
            (BrowserType::ChromeCanary, "Google/Chrome SxS/User Data"),
            (
                BrowserType::ChromeForTesting,
                "Google/Chrome for Testing/User Data",
            ),
            (BrowserType::Chromium, "Chromium/User Data"),
        ];

        let actual = windows_browser_candidates(&appdata);
        for (browser_type, relative_path) in expected {
            assert!(actual.contains(&(browser_type, appdata.join(relative_path))));
        }
    }

    #[test]
    fn test_wsl_browser_detection() {
        let browsers = WslBrowserDetector::detect_all();
        if !wsl::is_wsl() {
            assert!(browsers.is_empty());
        }
    }
}
