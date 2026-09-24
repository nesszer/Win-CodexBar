//! Fail-closed runtime containment for deterministic provider proof runs.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tauri::WebviewWindow;

pub const CONTAINMENT_PROOF_ENV: &str = "CODEXBAR_CONTAINMENT_PROOF";
pub const LEGACY_PROOF_ENV: &str = "CODEXBAR_PROOF_MODE";
pub const LEGACY_SEED_ENV: &str = "CODEXBAR_SEED_USAGE_JSON";
pub const PROOF_KIND: &str = "antigravityUsageSpend";
pub const PROOF_PROVIDER: &str = "antigravity";
const PROOF_SCHEMA: &str = "codexbar.containment-proof";
const PROOF_VERSION: u32 = 1;

/// Return the native HWND for the proof window. The containment proof is a
/// Windows-only UI path; other platforms fail closed instead of falling back
/// to Tauri's activating `show()` behavior.
pub fn native_window_handle(window: &WebviewWindow) -> Result<isize, String> {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let handle = window
            .window_handle()
            .map_err(|error| format!("cannot inspect proof window handle: {error}"))?;
        match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
            _ => Err("containment proof requires a Win32 window handle".to_string()),
        }
    }

    #[cfg(not(windows))]
    {
        let _ = window;
        Err("containment proof UI is supported only on Windows".to_string())
    }
}

/// Reveal the proof window behind other windows without activating it.
pub fn show_window_without_activation(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = native_window_handle(window)?;
    #[cfg(windows)]
    {
        const HWND_BOTTOM: isize = 1;
        const HWND_NOTOPMOST: isize = -2;
        const SW_SHOWNOACTIVATE: i32 = 4;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOACTIVATE: u32 = 0x0010;
        const SWP_SHOWWINDOW: u32 = 0x0040;

        let native = hwnd;
        // SAFETY: `native` comes from the live Tauri WebviewWindow above; all
        // calls preserve activation and only adjust this window's Z-order.
        unsafe {
            if set_window_pos(
                native,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            ) == 0
            {
                return Err("could not remove proof window from topmost Z-order".to_string());
            }
            show_window(native, SW_SHOWNOACTIVATE);
            if set_window_pos(
                native,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            ) == 0
            {
                return Err("could not show proof window without activation".to_string());
            }
            if get_foreground_window() == native {
                show_window(native, 0); // SW_HIDE
                return Err("proof window unexpectedly became foreground".to_string());
            }
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err("containment proof UI is supported only on Windows".to_string())
    }
}

/// Stop the proof run immediately if Windows ever reports its HWND as the
/// foreground window. This is a guard for regressions in the native reveal
/// path; normal proof flow never activates the window.
pub fn start_foreground_guard(app: tauri::AppHandle, hwnd: isize) {
    #[cfg(windows)]
    tauri::async_runtime::spawn(async move {
        use std::time::Duration;

        let native_handle = hwnd;
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            // SAFETY: `native_handle` is the HWND obtained from the live proof
            // WebviewWindow, and GetForegroundWindow is a read-only query.
            if unsafe { get_foreground_window() } == native_handle {
                tracing::error!("containment proof aborted: proof window became foreground");
                // SAFETY: hide only the proof HWND before exiting so the run
                // cannot continue interacting with the user's desktop.
                unsafe { show_window(native_handle, 0) }; // SW_HIDE
                app.exit(2);
                break;
            }
        }
    });

    #[cfg(not(windows))]
    {
        let _ = (app, hwnd);
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "ShowWindow"]
    fn show_window(hwnd: isize, command: i32) -> i32;
    #[link_name = "SetWindowPos"]
    fn set_window_pos(
        hwnd: isize,
        insert_after: isize,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    #[link_name = "GetForegroundWindow"]
    fn get_foreground_window() -> isize;
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFixtureRoots {
    #[serde(rename = "geminiCliHome")]
    gemini_cli_home: PathBuf,
    #[serde(rename = "tokscaleConfigDir")]
    tokscale_config_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema: String,
    #[serde(alias = "schemaVersion")]
    version: u32,
    kind: String,
    provider: String,
    #[serde(rename = "fixtureRoots")]
    fixture_roots: RawFixtureRoots,
    #[serde(rename = "scratchRoot")]
    scratch_root: PathBuf,
    #[serde(rename = "nowUtc")]
    now_utc: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureRoots {
    pub gemini_cli_home: PathBuf,
    pub tokscale_config_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainmentManifest {
    pub schema: String,
    pub version: u32,
    pub kind: String,
    pub provider: String,
    pub fixture_roots: FixtureRoots,
    pub scratch_root: PathBuf,
    pub now_utc: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ContainmentProof {
    pub manifest: ContainmentManifest,
    pub config_root: PathBuf,
    pub log_root: PathBuf,
    pub webview_user_data_folder: PathBuf,
}

impl ContainmentProof {
    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(raw_path) = std::env::var_os(CONTAINMENT_PROOF_ENV) else {
            return Ok(None);
        };

        for variable in [LEGACY_PROOF_ENV, LEGACY_SEED_ENV] {
            if std::env::var_os(variable).is_some() {
                return Err(format!(
                    "{CONTAINMENT_PROOF_ENV} cannot be combined with {variable}"
                ));
            }
        }

        let path = PathBuf::from(raw_path);
        if !path.is_absolute() {
            return Err(format!(
                "{CONTAINMENT_PROOF_ENV} must name an absolute manifest path"
            ));
        }
        let raw = fs::read_to_string(&path).map_err(|error| {
            format!(
                "cannot read {CONTAINMENT_PROOF_ENV} manifest {}: {error}",
                path.display()
            )
        })?;
        Self::from_manifest_json(&raw).map(Some)
    }

    pub fn from_manifest_json(raw: &str) -> Result<Self, String> {
        let raw_manifest: RawManifest = serde_json::from_str(raw)
            .map_err(|error| format!("invalid containment proof manifest: {error}"))?;
        if raw_manifest.schema != PROOF_SCHEMA {
            return Err(format!(
                "unsupported containment proof schema {:?}",
                raw_manifest.schema
            ));
        }
        if raw_manifest.version != PROOF_VERSION {
            return Err(format!(
                "unsupported containment proof version {}",
                raw_manifest.version
            ));
        }
        if raw_manifest.kind != PROOF_KIND {
            return Err(format!("containment proof kind must be {PROOF_KIND:?}"));
        }
        if raw_manifest.provider != PROOF_PROVIDER {
            return Err(format!(
                "containment proof provider must be {PROOF_PROVIDER:?}"
            ));
        }
        let now_utc = DateTime::parse_from_rfc3339(&raw_manifest.now_utc)
            .map_err(|error| format!("nowUtc must be RFC3339: {error}"))?
            .with_timezone(&Utc);

        let fixture_roots = FixtureRoots {
            gemini_cli_home: canonical_directory(
                "fixtureRoots.geminiCliHome",
                &raw_manifest.fixture_roots.gemini_cli_home,
            )?,
            tokscale_config_dir: canonical_directory(
                "fixtureRoots.tokscaleConfigDir",
                &raw_manifest.fixture_roots.tokscale_config_dir,
            )?,
        };
        let scratch_root = canonical_empty_directory("scratchRoot", &raw_manifest.scratch_root)?;
        let temp_root = fs::canonicalize(std::env::temp_dir())
            .map_err(|error| format!("system temporary root cannot be canonicalized: {error}"))?;
        let roots = [
            fixture_roots.gemini_cli_home.as_path(),
            fixture_roots.tokscale_config_dir.as_path(),
            scratch_root.as_path(),
        ];
        for (label, root) in [
            (
                "fixtureRoots.geminiCliHome",
                fixture_roots.gemini_cli_home.as_path(),
            ),
            (
                "fixtureRoots.tokscaleConfigDir",
                fixture_roots.tokscale_config_dir.as_path(),
            ),
            ("scratchRoot", scratch_root.as_path()),
        ] {
            if !is_dedicated_temp_child(root, &temp_root) {
                return Err(format!(
                    "{label} must be a child of the system temporary root {}",
                    temp_root.display()
                ));
            }
        }
        for (index, left) in roots.iter().enumerate() {
            for right in roots.iter().skip(index + 1) {
                if paths_overlap(left, right) {
                    return Err(format!(
                        "containment proof roots overlap: {} and {}",
                        left.display(),
                        right.display()
                    ));
                }
            }
        }

        Ok(Self {
            manifest: ContainmentManifest {
                schema: raw_manifest.schema,
                version: raw_manifest.version,
                kind: raw_manifest.kind,
                provider: raw_manifest.provider,
                fixture_roots,
                scratch_root: scratch_root.clone(),
                now_utc,
            },
            config_root: scratch_root.clone(),
            log_root: scratch_root.join("logs"),
            webview_user_data_folder: scratch_root.join("webview2-user-data"),
        })
    }

    pub fn proof_settings(&self) -> codexbar::settings::Settings {
        codexbar::settings::Settings {
            enabled_providers: HashSet::from([PROOF_PROVIDER.to_string()]),
            provider_order: vec![PROOF_PROVIDER.to_string()],
            refresh_interval_secs: 0,
            adaptive_refresh: false,
            refresh_all_providers_on_menu_open: false,
            start_minimized: false,
            powertoys_status_pipe_enabled: false,
            float_bar_enabled: false,
            auto_download_updates: false,
            install_updates_on_quit: false,
            ..Default::default()
        }
    }

    pub fn install(&self) -> Result<(), String> {
        fs::create_dir_all(&self.log_root)
            .map_err(|error| format!("cannot create proof log root: {error}"))?;
        fs::create_dir_all(&self.webview_user_data_folder)
            .map_err(|error| format!("cannot create proof WebView root: {error}"))?;
        codexbar::logging::install_config_root_override(self.config_root.clone())?;
        set_process_env("WEBVIEW2_USER_DATA_FOLDER", &self.webview_user_data_folder);
        set_process_env(
            "GEMINI_CLI_HOME",
            &self.manifest.fixture_roots.gemini_cli_home,
        );
        set_process_env(
            "TOKSCALE_CONFIG_DIR",
            &self.manifest.fixture_roots.tokscale_config_dir,
        );
        set_process_env(
            "CODEXBAR_CONTAINMENT_NOW_UTC",
            Path::new(&self.manifest.now_utc.to_rfc3339()),
        );
        Ok(())
    }
}

fn set_process_env(name: &str, value: &Path) {
    // SAFETY: startup runs before Tauri creates worker threads.
    unsafe { std::env::set_var(name, value.as_os_str()) };
}

fn canonical_directory(label: &str, path: &Path) -> Result<PathBuf, String> {
    ensure_absolute(label, path)?;
    reject_reparse_chain(label, path)?;
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("{label} cannot be canonicalized: {error}"))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("{label} metadata unavailable: {error}"))?;
    if !metadata.is_dir() {
        return Err(format!("{label} must be a directory"));
    }
    reject_reparse_tree(label, &canonical)?;
    Ok(canonical)
}

fn canonical_empty_directory(label: &str, path: &Path) -> Result<PathBuf, String> {
    let canonical = canonical_directory(label, path)?;
    if fs::read_dir(&canonical)
        .map_err(|error| format!("{label} cannot be read: {error}"))?
        .next()
        .is_some()
    {
        return Err(format!("{label} must be empty"));
    }
    Ok(canonical)
}

fn ensure_absolute(label: &str, path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(format!("{label} must be a non-empty absolute path"));
    }
    Ok(())
}

fn is_dedicated_temp_child(root: &Path, temp_root: &Path) -> bool {
    root != temp_root && root.starts_with(temp_root)
}

fn reject_reparse_tree(label: &str, root: &Path) -> Result<(), String> {
    reject_reparse_point(label, root)?;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in
            fs::read_dir(&directory).map_err(|error| format!("{label} cannot be read: {error}"))?
        {
            let entry = entry.map_err(|error| format!("{label} entry cannot be read: {error}"))?;
            let path = entry.path();
            reject_reparse_point(label, &path)?;
            if entry
                .file_type()
                .map_err(|error| format!("{label} entry type unavailable: {error}"))?
                .is_dir()
            {
                pending.push(path);
            }
        }
    }
    Ok(())
}

fn reject_reparse_point(label: &str, path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "{label} metadata unavailable for {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata) {
        return Err(format!(
            "{label} contains a reparse or symlink path: {}",
            path.display()
        ));
    }
    Ok(())
}

fn reject_reparse_chain(label: &str, path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if current.exists() {
            reject_reparse_point(label, &current)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(target_os = "windows"))]
fn is_windows_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let left = left.to_string_lossy().to_ascii_lowercase();
        let right = right.to_string_lossy().to_ascii_lowercase();
        left == right
            || Path::new(&left).starts_with(Path::new(&right))
            || Path::new(&right).starts_with(Path::new(&left))
    }
    #[cfg(not(target_os = "windows"))]
    {
        left == right || left.starts_with(right) || right.starts_with(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_json(fixture_a: &Path, fixture_b: &Path, scratch: &Path) -> String {
        serde_json::json!({
            "schema": PROOF_SCHEMA,
            "version": PROOF_VERSION,
            "kind": PROOF_KIND,
            "provider": PROOF_PROVIDER,
            "fixtureRoots": {
                "geminiCliHome": fixture_a,
                "tokscaleConfigDir": fixture_b
            },
            "scratchRoot": scratch,
            "nowUtc": "2026-09-24T12:34:56Z"
        })
        .to_string()
    }

    #[test]
    fn strict_manifest_accepts_absolute_disjoint_directories() {
        let root = crate::test_support::TempDir::new();
        let fixture_a = root.path().join("gemini");
        let fixture_b = root.path().join("tokscale");
        let scratch = root.path().join("scratch");
        fs::create_dir_all(&fixture_a).unwrap();
        fs::create_dir_all(&fixture_b).unwrap();
        fs::create_dir_all(&scratch).unwrap();

        let proof =
            ContainmentProof::from_manifest_json(&manifest_json(&fixture_a, &fixture_b, &scratch))
                .unwrap();
        assert_eq!(proof.manifest.provider, PROOF_PROVIDER);
        assert_eq!(
            proof.manifest.now_utc.to_rfc3339(),
            "2026-09-24T12:34:56+00:00"
        );
    }

    #[test]
    fn strict_manifest_rejects_unknown_fields_and_wrong_kind() {
        let root = crate::test_support::TempDir::new();
        let fixture_a = root.path().join("gemini");
        let fixture_b = root.path().join("tokscale");
        let scratch = root.path().join("scratch");
        fs::create_dir_all(&fixture_a).unwrap();
        fs::create_dir_all(&fixture_b).unwrap();
        fs::create_dir_all(&scratch).unwrap();

        let mut value: serde_json::Value =
            serde_json::from_str(&manifest_json(&fixture_a, &fixture_b, &scratch)).unwrap();
        value["unexpected"] = serde_json::json!(true);
        assert!(ContainmentProof::from_manifest_json(&value.to_string()).is_err());
        value.as_object_mut().unwrap().remove("unexpected");
        value["kind"] = serde_json::json!("settings");
        assert!(ContainmentProof::from_manifest_json(&value.to_string()).is_err());
    }

    #[test]
    fn path_safety_rejects_nonempty_scratch_and_overlapping_roots() {
        let root = crate::test_support::TempDir::new();
        let fixture_a = root.path().join("fixture");
        let fixture_b = root.path().join("fixture").join("nested");
        let scratch = root.path().join("scratch");
        fs::create_dir_all(&fixture_b).unwrap();
        fs::create_dir_all(&scratch).unwrap();
        fs::write(scratch.join("not-empty"), b"x").unwrap();
        assert!(
            ContainmentProof::from_manifest_json(&manifest_json(&fixture_a, &fixture_b, &scratch,))
                .is_err()
        );

        fs::remove_file(scratch.join("not-empty")).unwrap();
        assert!(
            ContainmentProof::from_manifest_json(&manifest_json(&fixture_a, &fixture_b, &scratch,))
                .is_err()
        );
    }

    #[test]
    fn fixture_roots_must_be_dedicated_children_of_system_temp() {
        let temp_root = std::env::temp_dir();
        assert!(is_dedicated_temp_child(
            &temp_root.join("codexbar-proof-fixtures/run-1/gemini"),
            &temp_root
        ));
        assert!(!is_dedicated_temp_child(&temp_root, &temp_root));
        assert!(!is_dedicated_temp_child(
            &temp_root.parent().unwrap().join("user-data"),
            &temp_root
        ));
    }
}
