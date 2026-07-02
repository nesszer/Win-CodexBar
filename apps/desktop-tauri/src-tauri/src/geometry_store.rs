//! Persistent window-geometry store for the Tauri desktop shell.
//!
//! Remembers position (and size where applicable) for detached user surfaces:
//! PopOut and Settings.
//!
//! TrayPanel stays computed from the tray anchor/work-area because it is a
//! temporary anchored panel, not a user-resizable standalone window.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::surface::SurfaceMode;

const GEOMETRY_FILENAME: &str = "window_geometry.json";

/// Bumped when the meaning of stored fields changes. v1 switched the stored
/// window SIZE from physical to logical pixels, so legacy (versionless) files
/// hold physical sizes that must be discarded on load.
const GEOMETRY_VERSION: u32 = 1;

/// Persisted window geometry entry. Size is optional because not every surface
/// is resizable; we always persist position when available.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredGeometry {
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// All persisted geometries keyed by surface mode string (`settings`, ...).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeometryFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: std::collections::BTreeMap<String, StoredGeometry>,
}

fn geometry_path() -> Option<PathBuf> {
    // Reuse the same CodexBar config directory as Settings, so remembered
    // geometry lives alongside `settings.json` on every platform.
    codexbar::settings::Settings::settings_path()
        .and_then(|p| p.parent().map(|parent| parent.join(GEOMETRY_FILENAME)))
}

/// Surface modes eligible for geometry persistence.
///
/// - `Hidden` / `TrayPanel`: anchored to tray, never remembered.
/// - `PopOut` / `Settings`: user-movable, remembered across restarts.
pub fn should_remember(mode: SurfaceMode) -> bool {
    matches!(mode, SurfaceMode::PopOut | SurfaceMode::Settings)
}

fn load_file() -> GeometryFile {
    let Some(path) = geometry_path() else {
        return GeometryFile::default();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return GeometryFile::default();
    };
    let mut file: GeometryFile = serde_json::from_str(&raw).unwrap_or_default();
    migrate(&mut file);
    file
}

/// Bring an on-disk file up to `GEOMETRY_VERSION`. Legacy (versionless) files
/// stored window SIZE in physical pixels, but the restore path now treats
/// stored size as logical; drop those sizes so windows reopen at their default
/// (logical) size and re-persist correct dimensions on the first user move,
/// instead of opening ~scale_factor too large on HiDPI displays.
fn migrate(file: &mut GeometryFile) {
    if file.version < GEOMETRY_VERSION {
        for geometry in file.entries.values_mut() {
            geometry.width = None;
            geometry.height = None;
        }
        file.version = GEOMETRY_VERSION;
    }
}

fn save_file(file: &GeometryFile) -> Result<(), String> {
    let Some(path) = geometry_path() else {
        return Err("No config directory available".into());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Look up remembered geometry for a surface mode. Returns `None` when the
/// mode is not eligible or no entry has been persisted yet.
pub fn load(mode: SurfaceMode) -> Option<StoredGeometry> {
    if !should_remember(mode) {
        return None;
    }
    load_entry(mode.as_str())
}

/// Persist geometry for an eligible surface mode. No-op for modes where
/// `should_remember` returns `false`.
pub fn save(mode: SurfaceMode, geometry: StoredGeometry) {
    if !should_remember(mode) {
        return;
    }
    save_entry(mode.as_str(), geometry);
}

/// Look up remembered geometry for an arbitrary key (e.g. an auxiliary
/// window label like `floatbar`).
pub fn load_entry(key: &str) -> Option<StoredGeometry> {
    load_file().entries.get(key).copied()
}

/// Persist geometry under an arbitrary key.
pub fn save_entry(key: &str, geometry: StoredGeometry) {
    let mut file = load_file();
    file.version = GEOMETRY_VERSION;
    file.entries.insert(key.to_string(), geometry);
    if let Err(err) = save_file(&file) {
        tracing::warn!(target: "codexbar::geometry", %err, "failed to persist geometry");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pop_out_and_settings_are_remembered() {
        assert!(should_remember(SurfaceMode::PopOut));
        assert!(should_remember(SurfaceMode::Settings));
    }

    #[test]
    fn tray_panel_and_hidden_are_not_remembered() {
        assert!(!should_remember(SurfaceMode::TrayPanel));
        assert!(!should_remember(SurfaceMode::Hidden));
    }
    #[test]
    fn non_remembered_mode_save_is_noop() {
        // Call should not panic or error for ineligible modes.
        save(
            SurfaceMode::TrayPanel,
            StoredGeometry {
                x: 1,
                y: 2,
                width: Some(420),
                height: Some(560),
            },
        );
        assert!(load(SurfaceMode::TrayPanel).is_none());
    }

    #[test]
    fn geometry_file_round_trip() {
        let mut f = GeometryFile::default();
        f.entries.insert(
            "settings".into(),
            StoredGeometry {
                x: 100,
                y: 200,
                width: Some(520),
                height: Some(600),
            },
        );
        let json = serde_json::to_string(&f).unwrap();
        let parsed: GeometryFile = serde_json::from_str(&json).unwrap();
        let entry = parsed.entries.get("settings").unwrap();
        assert_eq!(entry.x, 100);
        assert_eq!(entry.y, 200);
        assert_eq!(entry.width, Some(520));
        assert_eq!(entry.height, Some(600));
    }

    #[test]
    fn legacy_versionless_file_drops_physical_sizes_on_load() {
        // Pre-v1 files stored SIZE in physical pixels and had no `version`.
        // Migration must drop those sizes (keeping position) so a HiDPI upgrade
        // doesn't reopen the window scale_factor-too-large.
        let json = r#"{"entries":{"settings":{"x":10,"y":20,"width":744,"height":1116}}}"#;
        let mut file: GeometryFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.version, 0);
        migrate(&mut file);
        assert_eq!(file.version, GEOMETRY_VERSION);
        let entry = file.entries.get("settings").unwrap();
        assert_eq!(entry.x, 10);
        assert_eq!(entry.y, 20);
        assert_eq!(entry.width, None);
        assert_eq!(entry.height, None);
    }

    #[test]
    fn current_version_file_keeps_sizes() {
        let json = r#"{"version":1,"entries":{"settings":{"x":10,"y":20,"width":520,"height":600}}}"#;
        let mut file: GeometryFile = serde_json::from_str(json).unwrap();
        migrate(&mut file);
        let entry = file.entries.get("settings").unwrap();
        assert_eq!(entry.width, Some(520));
        assert_eq!(entry.height, Some(600));
    }

    #[test]
    fn geometry_file_parses_without_size() {
        let json = r#"{"entries":{"settings":{"x":10,"y":20}}}"#;
        let parsed: GeometryFile = serde_json::from_str(json).unwrap();
        let entry = parsed.entries.get("settings").unwrap();
        assert_eq!(entry.x, 10);
        assert_eq!(entry.y, 20);
        assert_eq!(entry.width, None);
        assert_eq!(entry.height, None);
    }
}
