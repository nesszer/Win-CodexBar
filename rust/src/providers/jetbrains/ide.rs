//! JetBrains IDE installation discovery and path layout on Windows.
//!
//! Layout (vendor is `JetBrains` or `Google`):
//! - config: `%APPDATA%\<vendor>\<IDE><version>\options\AIAssistantQuotaManager2.xml`
//! - log:    `%LOCALAPPDATA%\<vendor>\<IDE><version>\log\idea.log`

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(super) const QUOTA_FILE_NAME: &str = "AIAssistantQuotaManager2.xml";

/// IDE directory-name prefixes in match order, with their display names.
const IDE_PATTERNS: &[(&str, &str)] = &[
    ("IntelliJIdea", "IntelliJ IDEA"),
    ("PyCharm", "PyCharm"),
    ("WebStorm", "WebStorm"),
    ("GoLand", "GoLand"),
    ("CLion", "CLion"),
    ("DataGrip", "DataGrip"),
    ("RubyMine", "RubyMine"),
    ("Rider", "Rider"),
    ("PhpStorm", "PhpStorm"),
    ("AppCode", "AppCode"),
    ("Fleet", "Fleet"),
    ("AndroidStudio", "Android Studio"),
    ("RustRover", "RustRover"),
    ("Aqua", "Aqua"),
    ("DataSpell", "DataSpell"),
];

/// A vendor config root and the matching local log root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VendorRoot {
    /// `%APPDATA%\<vendor>`
    pub config: PathBuf,
    /// `%LOCALAPPDATA%\<vendor>`
    pub logs: PathBuf,
}

/// One installed IDE configuration directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IdeInstall {
    pub name: &'static str,
    pub version: String,
    /// IDE configuration directory (contains `options\`).
    pub base_path: PathBuf,
}

impl IdeInstall {
    /// `IntelliJ IDEA 2025.3`; `Unknown` stands in for a missing version.
    pub fn display_name(&self) -> String {
        format!("{} {}", self.name, self.version)
    }

    pub fn quota_file(&self) -> PathBuf {
        quota_file_for(&self.base_path)
    }
}

/// Quota XML path for an IDE configuration directory.
pub(super) fn quota_file_for(base_path: &Path) -> PathBuf {
    base_path.join("options").join(QUOTA_FILE_NAME)
}

/// The Windows JetBrains and Google vendor roots, when the user dirs are known.
pub(super) fn default_vendor_roots() -> Vec<VendorRoot> {
    let (Some(config), Some(local)) = (dirs::config_dir(), dirs::data_local_dir()) else {
        return Vec::new();
    };
    ["JetBrains", "Google"]
        .iter()
        .map(|vendor| VendorRoot {
            config: config.join(vendor),
            logs: local.join(vendor),
        })
        .collect()
}

/// IDE configuration directories under each vendor root.
///
/// Sorted by display name, then newest version first. With
/// `include_missing_quota` false, only directories that hold a quota XML count.
pub(super) fn detect_installs(
    roots: &[VendorRoot],
    include_missing_quota: bool,
) -> Vec<IdeInstall> {
    let mut installs = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root.config) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some((name, version)) = path
                .file_name()
                .and_then(|dirname| dirname.to_str())
                .and_then(parse_dirname)
            else {
                continue;
            };
            let install = IdeInstall {
                name,
                version,
                base_path: path,
            };
            if include_missing_quota || install.quota_file().is_file() {
                installs.push(install);
            }
        }
    }
    installs.sort_by(|left, right| {
        left.name
            .cmp(right.name)
            .then_with(|| compare_versions(&right.version, &left.version))
    });
    installs
}

/// The IDE whose quota XML was written most recently. Falls back to the first
/// detected IDE when no modification time is available.
pub(super) fn latest_install(installs: &[IdeInstall]) -> Option<IdeInstall> {
    installs
        .iter()
        .filter_map(|install| modified_at(&install.quota_file()).map(|time| (time, install)))
        .max_by_key(|(time, _)| *time)
        .map(|(_, install)| install.clone())
        .or_else(|| installs.first().cloned())
}

/// `idea.log` for an IDE configuration directory.
///
/// Only a directory sitting directly under a vendor config root maps to a log.
/// A copied, nested, or custom location has no proven log, so `None` is returned.
pub(super) fn log_path_for(base_path: &Path, roots: &[VendorRoot]) -> Option<PathBuf> {
    let parent = base_path.parent()?;
    let name = base_path.file_name()?;
    let root = roots.iter().find(|root| same_dir(parent, &root.config))?;
    Some(root.logs.join(name).join("log").join("idea.log"))
}

/// Modification time of `path`, when it exists.
pub(super) fn modified_at(path: &Path) -> Option<chrono::DateTime<chrono::Utc>> {
    let time: SystemTime = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(time.into())
}

/// Match a configuration directory name to an IDE. The prefix is
/// case-insensitive and whatever follows it is the version.
fn parse_dirname(dirname: &str) -> Option<(&'static str, String)> {
    let lower = dirname.to_ascii_lowercase();
    IDE_PATTERNS.iter().find_map(|(prefix, display)| {
        if !lower.starts_with(&prefix.to_ascii_lowercase()) {
            return None;
        }
        let version = &dirname[prefix.len()..];
        let version = if version.is_empty() {
            "Unknown".to_string()
        } else {
            version.to_string()
        };
        Some((*display, version))
    })
}

/// Numeric dotted-version comparison; missing components count as zero.
fn compare_versions(left: &str, right: &str) -> Ordering {
    let parse = |version: &str| -> Vec<u64> {
        version
            .split('.')
            .filter_map(|part| part.parse().ok())
            .collect()
    };
    let (left, right) = (parse(left), parse(right));
    let width = left.len().max(right.len());
    for index in 0..width {
        let l = left.get(index).copied().unwrap_or(0);
        let r = right.get(index).copied().unwrap_or(0);
        match l.cmp(&r) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// Case-insensitive, separator-tolerant path equality (Windows paths).
fn same_dir(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    };
    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn roots_for(dir: &Path) -> Vec<VendorRoot> {
        vec![VendorRoot {
            config: dir.join("Roaming").join("JetBrains"),
            logs: dir.join("Local").join("JetBrains"),
        }]
    }

    fn write_quota(base: &Path) {
        let file = quota_file_for(base);
        fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        fs::write(file, "<application/>").expect("write");
    }

    #[test]
    fn parse_dirname_maps_prefix_case_insensitively_and_keeps_version() {
        assert_eq!(
            parse_dirname("IntelliJIdea2025.3"),
            Some(("IntelliJ IDEA", "2025.3".to_string()))
        );
        assert_eq!(
            parse_dirname("pycharm2024.1"),
            Some(("PyCharm", "2024.1".to_string()))
        );
        assert_eq!(
            parse_dirname("AndroidStudioPanda"),
            Some(("Android Studio", "Panda".to_string()))
        );
        assert_eq!(
            parse_dirname("IntelliJIdea"),
            Some(("IntelliJ IDEA", "Unknown".to_string()))
        );
        assert_eq!(parse_dirname("Toolbox"), None);
    }

    #[test]
    fn compare_versions_is_numeric_per_component() {
        assert_eq!(compare_versions("2025.10", "2025.9"), Ordering::Greater);
        assert_eq!(compare_versions("2024.3", "2024.3.0"), Ordering::Equal);
        assert_eq!(compare_versions("Unknown", "2024.1"), Ordering::Less);
    }

    #[test]
    fn detect_installs_lists_quota_ides_by_name_then_newest_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let roots = roots_for(dir.path());
        let config = &roots[0].config;
        write_quota(&config.join("IntelliJIdea2024.3"));
        write_quota(&config.join("IntelliJIdea2025.1"));
        fs::create_dir_all(config.join("PyCharm2025.2")).expect("no quota");
        fs::write(config.join("WebStorm2025.1"), "file, not a dir").expect("file");

        let found = detect_installs(&roots, false);
        let names: Vec<String> = found.iter().map(IdeInstall::display_name).collect();
        assert_eq!(names, ["IntelliJ IDEA 2025.1", "IntelliJ IDEA 2024.3"]);

        let with_missing = detect_installs(&roots, true);
        assert_eq!(with_missing.len(), 3);
    }

    #[test]
    fn latest_install_prefers_the_most_recently_written_quota_xml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let roots = roots_for(dir.path());
        let config = &roots[0].config;
        let older = config.join("IntelliJIdea2025.1");
        let newer = config.join("PyCharm2025.2");
        write_quota(&older);
        write_quota(&newer);
        let set_time = |base: &Path, when: SystemTime| {
            let file = fs::OpenOptions::new()
                .write(true)
                .open(quota_file_for(base))
                .expect("open");
            file.set_modified(when).expect("set mtime");
        };
        let epoch = SystemTime::UNIX_EPOCH;
        set_time(
            &older,
            epoch + std::time::Duration::from_secs(1_700_000_000),
        );
        set_time(
            &newer,
            epoch + std::time::Duration::from_secs(1_800_000_000),
        );

        let installs = detect_installs(&roots, false);
        let latest = latest_install(&installs).expect("latest");
        assert_eq!(latest.name, "PyCharm");
    }

    #[test]
    fn log_path_requires_a_direct_child_of_a_vendor_config_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let roots = roots_for(dir.path());
        let config = roots[0].config.clone();

        let direct = config.join("IntelliJIdea2025.3");
        assert_eq!(
            log_path_for(&direct, &roots),
            Some(
                dir.path()
                    .join("Local")
                    .join("JetBrains")
                    .join("IntelliJIdea2025.3")
                    .join("log")
                    .join("idea.log")
            )
        );

        let nested = config.join("extra").join("IntelliJIdea2025.3");
        assert!(log_path_for(&nested, &roots).is_none());
        let custom = dir.path().join("Custom").join("IntelliJIdea2025.3");
        assert!(log_path_for(&custom, &roots).is_none());
        let traversal = config.join("..").join("JetBrains").join("..");
        assert!(log_path_for(&traversal, &roots).is_none());
    }

    #[test]
    fn log_path_match_ignores_trailing_separator_and_case() {
        let dir = tempfile::tempdir().expect("tempdir");
        let roots = roots_for(dir.path());
        let shouted = PathBuf::from(
            roots[0]
                .config
                .to_string_lossy()
                .to_ascii_uppercase()
                .to_string()
                + "\\IntelliJIdea2025.3\\",
        );
        assert!(log_path_for(&shouted, &roots).is_some());
    }
}
