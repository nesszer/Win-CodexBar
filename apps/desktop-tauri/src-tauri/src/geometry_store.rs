//! Persistent window-geometry store for the Tauri desktop shell.
//!
//! Remembers position (and size where applicable) for detached user surfaces:
//! PopOut and Settings.
//!
//! The flyout window stays computed from the tray anchor/work-area because it
//! is a temporary anchored panel, not a user-movable standalone window — but
//! its SIZE is remembered via the size-only [`StoredSize`] entries below, kept
//! separate from [`StoredGeometry`] so the flyout's persisted size can never
//! carry fabricated `x`/`y` coordinates.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::surface::SurfaceMode;

/// Serializes read-modify-write of the geometry file across threads: main-thread
/// window `Moved`/`Resized` events and the async flyout-open command both
/// persist here, so without this an interleaved load/save could drop an entry
/// or wipe the whole file.
static GEOMETRY_LOCK: Mutex<()> = Mutex::new(());

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

/// A size-only persisted entry, for windows that are always re-anchored (never
/// remember position) so storing `x`/`y` would be fabricated data. Used by the
/// detached flyout window, which is anchored above the tray on every open —
/// only its user-chosen width/height is meaningful to remember.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSize {
    pub width: u32,
    pub height: u32,
}

/// All persisted geometries keyed by surface mode string (`settings`, ...).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeometryFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: std::collections::BTreeMap<String, StoredGeometry>,
    /// Size-only entries (no position), keyed by an arbitrary label (e.g. the
    /// `"flyout"` window). Kept in a separate map — rather than reusing
    /// `entries` with a fabricated `x: 0, y: 0` — so the on-disk shape can't
    /// be misread as a remembered position.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub size_entries: std::collections::BTreeMap<String, StoredSize>,
}

fn geometry_path() -> Option<PathBuf> {
    // Reuse the same CodexBar config directory as Settings, so remembered
    // geometry lives alongside `settings.json` on every platform.
    codexbar::settings::Settings::settings_path()
        .and_then(|p| p.parent().map(|parent| parent.join(GEOMETRY_FILENAME)))
}

/// Surface modes eligible for geometry persistence.
///
/// - `Hidden`: never remembered.
/// - `TrayPanel`: remembered for SIZE — the "Pop Out Dashboard" flyout is always
///   re-anchored above the tray icon (its position comes from
///   `default_surface_position`, which ignores the stored x/y), but its
///   width/height persist so a user resize sticks across opens.
/// - `PopOut` / `Settings`: user-movable, position + size remembered.
pub fn should_remember(mode: SurfaceMode) -> bool {
    matches!(
        mode,
        SurfaceMode::TrayPanel | SurfaceMode::PopOut | SurfaceMode::Settings
    )
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
    // Atomic replace: stage to a sibling temp file then rename, so a concurrent
    // reader never observes a half-written file (which parses to empty and, on
    // the next save, would persist a file missing every other entry).
    let tmp = temp_sibling(&path);
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(())
}

/// A unique sibling path (same directory) for staging an atomic write.
fn temp_sibling(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map(|f| f.to_os_string())
        .unwrap_or_default();
    name.push(format!(".tmp.{}.{}", std::process::id(), n));
    path.with_file_name(name)
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
    let _guard = GEOMETRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut file = load_file();
    file.version = GEOMETRY_VERSION;
    file.entries.insert(key.to_string(), geometry);
    if let Err(err) = save_file(&file) {
        tracing::warn!(target: "codexbar::geometry", %err, "failed to persist geometry");
    }
}

/// Legacy key the flyout size was stored under before it became a dedicated
/// window: the old `SurfaceMode::TrayPanel` shared-window geometry entry.
const LEGACY_FLYOUT_SIZE_KEY: &str = "trayPanel";

/// Resolve a size-only entry from an in-memory [`GeometryFile`]: prefer the
/// new `size_entries` map, falling back to migrating a pre-existing
/// `"trayPanel"` [`StoredGeometry`] width/height (from before the flyout was
/// split into its own window) so upgrading users keep their remembered size
/// instead of it silently resetting to the default. Pure/side-effect-free so
/// it can be unit-tested without touching disk; [`load_size`] is the
/// disk-backed wrapper that also persists the migrated value.
fn resolve_size(file: &GeometryFile, key: &str) -> Option<StoredSize> {
    if let Some(size) = file.size_entries.get(key).copied() {
        return Some(size);
    }
    if key != LEGACY_FLYOUT_SIZE_KEY {
        let legacy = file.entries.get(LEGACY_FLYOUT_SIZE_KEY)?;
        let (width, height) = (legacy.width?, legacy.height?);
        return Some(StoredSize { width, height });
    }
    None
}

/// Look up a remembered size-only entry (e.g. the flyout window's
/// user-chosen width/height). See [`resolve_size`] for the migration
/// fallback; a migrated value is re-persisted under the new key so this
/// lookup path is only taken once (the legacy entry is left in place —
/// harmless, since nothing reads `entries["trayPanel"]` as a position
/// anymore).
pub fn load_size(key: &str) -> Option<StoredSize> {
    let file = load_file();
    let size = resolve_size(&file, key)?;
    if !file.size_entries.contains_key(key) {
        save_size(key, size);
    }
    Some(size)
}

/// Persist a size-only entry under an arbitrary key.
pub fn save_size(key: &str, size: StoredSize) {
    // NB: `load_size` calls this to persist a migrated value, but it does not
    // hold GEOMETRY_LOCK itself, so this non-reentrant lock does not deadlock.
    let _guard = GEOMETRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut file = load_file();
    file.version = GEOMETRY_VERSION;
    file.size_entries.insert(key.to_string(), size);
    if let Err(err) = save_file(&file) {
        tracing::warn!(target: "codexbar::geometry", %err, "failed to persist size");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_sibling_is_a_unique_sibling_of_the_target() {
        let base = Path::new("C:/cfg/window_geometry.json");
        let a = temp_sibling(base);
        let b = temp_sibling(base);
        assert_eq!(a.parent(), base.parent());
        assert_ne!(a, b, "staged temp paths must be unique per call");
        assert!(
            a.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("window_geometry.json.tmp.")
        );
    }

    #[test]
    fn pop_out_and_settings_are_remembered() {
        assert!(should_remember(SurfaceMode::PopOut));
        assert!(should_remember(SurfaceMode::Settings));
    }

    #[test]
    fn tray_panel_is_remembered_for_size() {
        // The flyout persists its size (position is always re-anchored).
        assert!(should_remember(SurfaceMode::TrayPanel));
    }

    #[test]
    fn hidden_is_not_remembered() {
        assert!(!should_remember(SurfaceMode::Hidden));
    }

    #[test]
    fn non_remembered_mode_save_is_noop() {
        // Call should not panic or error for ineligible modes.
        save(
            SurfaceMode::Hidden,
            StoredGeometry {
                x: 1,
                y: 2,
                width: Some(420),
                height: Some(560),
            },
        );
        assert!(load(SurfaceMode::Hidden).is_none());
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
        let json =
            r#"{"version":1,"entries":{"settings":{"x":10,"y":20,"width":520,"height":600}}}"#;
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

    #[test]
    fn stored_size_round_trips_without_position_fields() {
        let mut f = GeometryFile::default();
        f.size_entries.insert(
            "flyout".into(),
            StoredSize {
                width: 400,
                height: 820,
            },
        );
        let json = serde_json::to_string(&f).unwrap();
        // The size-only entry must never carry x/y — that's the whole point
        // of keeping it out of `entries: BTreeMap<String, StoredGeometry>`.
        assert!(!json.contains("\"x\""));
        assert!(!json.contains("\"y\""));
        let parsed: GeometryFile = serde_json::from_str(&json).unwrap();
        let entry = parsed.size_entries.get("flyout").unwrap();
        assert_eq!(entry.width, 400);
        assert_eq!(entry.height, 820);
    }

    #[test]
    fn resolve_size_prefers_new_key_over_legacy() {
        let mut file = GeometryFile::default();
        file.size_entries.insert(
            "flyout".into(),
            StoredSize {
                width: 500,
                height: 900,
            },
        );
        file.entries.insert(
            LEGACY_FLYOUT_SIZE_KEY.into(),
            StoredGeometry {
                x: 0,
                y: 0,
                width: Some(640),
                height: Some(720),
            },
        );

        let resolved = resolve_size(&file, "flyout").expect("size present");
        assert_eq!(resolved.width, 500);
        assert_eq!(resolved.height, 900);
    }

    #[test]
    fn resolve_size_migrates_legacy_tray_panel_geometry_when_no_new_entry() {
        // Simulates an upgrading user: pre-refactor size lived under the
        // `SurfaceMode::TrayPanel` shared-window geometry key.
        let mut file = GeometryFile::default();
        file.entries.insert(
            LEGACY_FLYOUT_SIZE_KEY.into(),
            StoredGeometry {
                x: 0,
                y: 0,
                width: Some(640),
                height: Some(720),
            },
        );

        let resolved = resolve_size(&file, "flyout").expect("legacy size migrates");
        assert_eq!(resolved.width, 640);
        assert_eq!(resolved.height, 720);
    }

    #[test]
    fn resolve_size_ignores_legacy_entry_missing_width_or_height() {
        let mut file = GeometryFile::default();
        file.entries.insert(
            LEGACY_FLYOUT_SIZE_KEY.into(),
            StoredGeometry {
                x: 0,
                y: 0,
                width: Some(640),
                height: None,
            },
        );

        assert!(resolve_size(&file, "flyout").is_none());
    }

    #[test]
    fn resolve_size_returns_none_when_nothing_stored() {
        let file = GeometryFile::default();
        assert!(resolve_size(&file, "flyout").is_none());
    }

    #[test]
    fn resolve_size_for_legacy_key_itself_does_not_self_migrate() {
        // Looking up the legacy key directly should only consult
        // `size_entries` (the `key != LEGACY_FLYOUT_SIZE_KEY` guard) — no
        // infinite fallback to itself.
        let file = GeometryFile::default();
        assert!(resolve_size(&file, LEGACY_FLYOUT_SIZE_KEY).is_none());
    }
}
