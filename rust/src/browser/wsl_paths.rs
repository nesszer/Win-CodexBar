//! WSL-aware browser detection and path resolution
//!
//! When running inside WSL, Windows browser data lives under /mnt/c/...
//! This module provides path resolvers that detect WSL and map
//! browser profile paths to their Windows host equivalents.

use std::path::PathBuf;

use crate::wsl;

use super::detection::{BrowserProfile, BrowserType, DetectedBrowser};

/// WSL-aware browser detector
///
/// On native Linux, returns empty (no Windows browsers).
/// On WSL, detects Windows browsers via /mnt/c/ paths.
pub struct WslBrowserDetector;

impl WslBrowserDetector {
    /// Detect browsers accessible from WSL
    pub fn detect_all() -> Vec<DetectedBrowser> {
        if !wsl::is_wsl() {
            return Vec::new();
        }

        let appdata_local = match wsl::windows_appdata_local() {
            Some(p) => p,
            None => return Vec::new(),
        };

        let mut browsers = Vec::new();

        let candidates: &[(BrowserType, PathBuf)] = &[
            (
                BrowserType::Chrome,
                appdata_local.join("Google").join("Chrome").join("User Data"),
            ),
            (
                BrowserType::Edge,
                appdata_local.join("Microsoft").join("Edge").join("User Data"),
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
            (
                BrowserType::Chromium,
                appdata_local.join("Chromium").join("User Data"),
            ),
        ];

        for (browser_type, user_data_dir) in candidates {
            if user_data_dir.exists() {
                let profiles = detect_chromium_profiles(user_data_dir);
                if !profiles.is_empty() {
                    browsers.push(DetectedBrowser {
                        browser_type: *browser_type,
                        user_data_dir: user_data_dir.clone(),
                        profiles,
                    });
                }
            }
        }

        // Firefox: check AppData/Roaming
        if let Some(appdata_roaming) = wsl::windows_appdata_roaming() {
            let ff_dir = appdata_roaming.join("Mozilla").join("Firefox").join("Profiles");
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

    /// Check if we should use WSL browser detection
    pub fn should_use() -> bool {
        wsl::is_wsl()
    }
}

/// Detect Chromium-based browser profiles
fn detect_chromium_profiles(user_data_dir: &PathBuf) -> Vec<BrowserProfile> {
    let mut profiles = Vec::new();

    // Default profile
    let default_path = user_data_dir.join("Default");
    if default_path.exists() {
        profiles.push(BrowserProfile {
            name: "Default".to_string(),
            path: default_path,
            is_default: true,
        });
    }

    // Additional profiles (Profile 1, Profile 2, etc.)
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

/// Detect Firefox profiles
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
    fn test_wsl_browser_detection() {
        let browsers = WslBrowserDetector::detect_all();
        // On non-WSL, should be empty
        if !wsl::is_wsl() {
            assert!(browsers.is_empty());
        }
    }
}
