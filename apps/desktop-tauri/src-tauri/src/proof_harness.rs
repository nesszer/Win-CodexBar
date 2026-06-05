//! Proof/debug harness for the Tauri desktop shell.
//!
//! Activated by the `CODEXBAR_PROOF_MODE` environment variable.  The value
//! specifies a target surface and optional settings tab to display on
//! startup, e.g.:
//!
//!   - `trayPanel`          — show the tray panel
//!   - `popOut`             — show the pop-out dashboard
//!   - `popOut:provider:codex` — show a provider pop-out
//!   - `settings`           — show settings (General tab)
//!   - `settings:apiKeys`   — show settings on the API Keys tab
//!   - `settings:cookies`   — show settings on the Cookies tab
//!   - `settings:about`     — show settings on the About tab
//!
//! In proof mode the shell immediately transitions to the requested surface
//! and suppresses blur-dismiss so the window stays visible for automated
//! screenshot capture.

use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::commands::get_provider_catalog;
use crate::events;
use crate::shell;
use crate::state::AppState;
use crate::surface::SurfaceMode;
use crate::surface_target::{SurfaceTarget, is_supported_provider_id, is_supported_settings_tab};
use crate::tray_menu;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofStatePayload {
    pub mode: String,
    pub target: SurfaceTarget,
    pub window_rect: Option<ProofRect>,
    pub tray_anchor: Option<ProofRect>,
    pub work_area: Option<ProofRect>,
    pub menu_path: Option<String>,
    pub menu_items: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ProofMenuSnapshot {
    menu_path: Option<String>,
    menu_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProofStateOverride {
    mode: SurfaceMode,
    target: SurfaceTarget,
    window_rect: Option<ProofRect>,
}

#[derive(Debug, Default)]
struct ProofSyncControl {
    suppressed_transitions: usize,
}

struct SurfaceTransitionSyncSuppression;

static PROOF_MENU_SNAPSHOT: LazyLock<Mutex<ProofMenuSnapshot>> =
    LazyLock::new(|| Mutex::new(ProofMenuSnapshot::default()));
static PROOF_STATE_OVERRIDE: LazyLock<Mutex<Option<ProofStateOverride>>> =
    LazyLock::new(|| Mutex::new(None));
static PROOF_SYNC_CONTROL: LazyLock<Mutex<ProofSyncControl>> =
    LazyLock::new(|| Mutex::new(ProofSyncControl::default()));
/// Proof configuration parsed from `CODEXBAR_PROOF_MODE`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofConfig {
    /// The surface to show on startup (serialized as the camelCase id).
    pub target_surface: String,
    /// Optional settings tab id (e.g. `"apiKeys"`, `"cookies"`).
    pub settings_tab: Option<String>,
    /// Optional target payload for richer proof routing, such as
    /// `"provider:codex"` for pop-out provider views.
    pub target_payload: Option<String>,
}

impl ProofConfig {
    /// Read proof configuration from the environment.
    ///
    /// Returns `None` when `CODEXBAR_PROOF_MODE` is unset or empty.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("CODEXBAR_PROOF_MODE").ok()?;
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }

        let (surface_str, payload) = if let Some((s, t)) = raw.split_once(':') {
            (s, Some(t.to_string()))
        } else {
            (raw, None)
        };

        let Some(surface_mode) = SurfaceMode::parse(surface_str) else {
            tracing::warn!("CODEXBAR_PROOF_MODE: unknown surface '{surface_str}', ignoring");
            return None;
        };

        if !proof_payload_is_supported(surface_mode, payload.as_deref()) {
            tracing::warn!("CODEXBAR_PROOF_MODE: unsupported target '{raw}', ignoring");
            return None;
        }

        Some(ProofConfig {
            target_surface: surface_str.to_string(),
            settings_tab: (surface_str == SurfaceMode::Settings.as_str())
                .then_some(payload.clone())
                .flatten(),
            target_payload: payload,
        })
    }

    /// Resolve the target `SurfaceMode` enum value.
    pub fn surface_mode(&self) -> SurfaceMode {
        SurfaceMode::parse(&self.target_surface).unwrap_or(SurfaceMode::TrayPanel)
    }

    pub fn surface_target(&self) -> SurfaceTarget {
        match self.surface_mode() {
            SurfaceMode::Hidden | SurfaceMode::TrayPanel => SurfaceTarget::Summary,
            SurfaceMode::PopOut => self
                .target_payload
                .as_deref()
                .and_then(SurfaceTarget::parse)
                .filter(|target| target.mode() == SurfaceMode::PopOut)
                .unwrap_or(SurfaceTarget::Dashboard),
            SurfaceMode::Settings => SurfaceTarget::Settings {
                tab: self
                    .settings_tab
                    .clone()
                    .unwrap_or_else(|| "general".into()),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofCommand {
    OpenTrayPanel,
    OpenNativeMenu,
    OpenDashboard,
    OpenProvider { provider_id: String },
    OpenSettings { tab: String },
    OpenAboutPath,
    HideSurface,
}

impl ProofCommand {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "open-tray-panel" => Some(Self::OpenTrayPanel),
            "open-native-menu" => Some(Self::OpenNativeMenu),
            "open-dashboard" => Some(Self::OpenDashboard),
            "open-about-path" => Some(Self::OpenAboutPath),
            "hide-surface" => Some(Self::HideSurface),
            _ => {
                if let Some(provider_id) = raw.strip_prefix("open-provider:")
                    && is_supported_provider_id(provider_id)
                {
                    return Some(Self::OpenProvider {
                        provider_id: provider_id.to_string(),
                    });
                }

                if let Some(tab) = raw.strip_prefix("open-settings:")
                    && is_supported_settings_tab(tab)
                {
                    return Some(Self::OpenSettings {
                        tab: tab.to_string(),
                    });
                }

                None
            }
        }
    }
}

/// Immediately transition to the proof-mode target surface.
///
/// Called from the Tauri `setup` closure when proof mode is active.
pub fn activate(app: &AppHandle) {
    let config = {
        let st = app.state::<Mutex<AppState>>();
        st.lock().unwrap().proof_config.clone()
    };

    let Some(config) = config else { return };
    let target = config.surface_mode();
    let position = match target {
        // Detached surfaces are larger than tray panels. Let their normal
        // positioning paths center/clamp them instead of reusing tray coords.
        SurfaceMode::Settings | SurfaceMode::PopOut => None,
        _ => proof_window_position(app),
    };
    tracing::info!(
        "proof-harness: activating surface={} tab={:?} position={:?}",
        config.target_surface,
        config.settings_tab,
        position,
    );

    match shell::transition_to_target(app, target, config.surface_target(), position) {
        Ok(mode) => tracing::info!("proof-harness: transition succeeded → {mode:?}"),
        Err(err) => tracing::error!("proof-harness: transition FAILED: {err}"),
    }
}

/// Calculate a predictable window position for proof captures.
///
/// Proof mode needs a reliable on-screen position. We skip
/// `inferred_tray_panel_position` because its DPI-scaled maths can
/// produce off-screen coords on high-DPI setups.
fn proof_window_position(app: &AppHandle) -> Option<(i32, i32)> {
    if let Some(pos) = shell::tray_panel_position(app) {
        return Some(pos);
    }
    let window = app.get_webview_window("main")?;
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.available_monitors().ok()?.into_iter().next());
    if let Some(m) = monitor {
        // Return coordinates in Tauri physical space (same as
        // monitor.size/work_area). transition.rs divides by scale before
        // calling set_position.
        let screen_w = m.size().width as i32;
        let work_area = m.work_area();
        let work_bottom = work_area.position.y + work_area.size.height as i32;
        let scale = m.scale_factor().max(1.0);
        let props = SurfaceMode::TrayPanel.window_properties();
        let panel_w = (props.width * scale) as i32;
        let panel_h = (props.height * scale) as i32;
        let margin = (12.0 * scale) as i32;
        let x = screen_w - panel_w - margin;
        let y = work_bottom - panel_h - margin;
        tracing::info!(
            "proof-pos: screen_w={screen_w} work_bottom={work_bottom} \
             panel={}x{} scale={scale} → ({x},{y})",
            panel_w,
            panel_h,
        );
        return Some((x, y));
    }
    Some((800, 25))
}

/// Returns `true` when proof mode is active in the shared state.
pub fn is_proof_mode(app: &AppHandle) -> bool {
    app.try_state::<Mutex<AppState>>()
        .map(|st| st.lock().unwrap().proof_config.is_some())
        .unwrap_or(false)
}

pub fn capture_state(app: &AppHandle) -> Result<ProofStatePayload, String> {
    let (mode, target, tray_anchor) = {
        let st = app
            .try_state::<Mutex<AppState>>()
            .ok_or_else(|| "app state unavailable".to_string())?;
        let guard = st.lock().unwrap();
        (
            guard.surface_machine.current(),
            guard.current_target.clone(),
            guard.tray_anchor.map(|anchor| ProofRect {
                x: anchor.x,
                y: anchor.y,
                width: anchor.width,
                height: anchor.height,
            }),
        )
    };

    let menu_snapshot = PROOF_MENU_SNAPSHOT.lock().unwrap().clone();
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?;
    let (mode, target, window_rect) =
        resolve_surface_state(mode, target, window_rect(&window), proof_state_override());

    Ok(ProofStatePayload {
        mode,
        target,
        window_rect,
        tray_anchor: tray_anchor.clone(),
        work_area: resolve_work_area(&window, tray_anchor.as_ref()),
        menu_path: menu_snapshot.menu_path,
        menu_items: menu_snapshot.menu_items,
    })
}

pub fn run_command(app: &AppHandle, command: ProofCommand) -> Result<ProofStatePayload, String> {
    ensure_proof_mode(app)?;
    clear_menu_snapshot();
    clear_proof_state_override();

    let outcome = execute_proof_command(app, command)?;
    let payload = capture_state(app)?;
    if outcome.emit_after_command {
        events::emit_proof_state_changed(app, &payload);
    }
    Ok(payload)
}

struct ProofCommandOutcome {
    emit_after_command: bool,
}

impl ProofCommandOutcome {
    const SILENT: Self = Self {
        emit_after_command: false,
    };

    const EMIT_AFTER: Self = Self {
        emit_after_command: true,
    };
}

fn execute_proof_command(
    app: &AppHandle,
    command: ProofCommand,
) -> Result<ProofCommandOutcome, String> {
    match command {
        ProofCommand::OpenTrayPanel => open_proof_tray_panel(app),
        ProofCommand::OpenNativeMenu => open_proof_native_menu(),
        ProofCommand::OpenDashboard => open_proof_dashboard(app),
        ProofCommand::OpenProvider { provider_id } => open_proof_provider(app, provider_id),
        ProofCommand::OpenSettings { tab } => open_proof_settings(app, tab),
        ProofCommand::OpenAboutPath => open_proof_about_path(app),
        ProofCommand::HideSurface => hide_proof_surface(app),
    }
}

fn open_proof_tray_panel(app: &AppHandle) -> Result<ProofCommandOutcome, String> {
    let position = shell::tray_panel_position(app).or_else(|| shell::shortcut_panel_position(app));
    shell::reopen_to_target(
        app,
        SurfaceMode::TrayPanel,
        SurfaceTarget::Summary,
        position,
    )?;
    Ok(ProofCommandOutcome::SILENT)
}

fn open_proof_native_menu() -> Result<ProofCommandOutcome, String> {
    let (menu_path, menu_items) = native_menu_snapshot_for_path("tray");
    set_menu_snapshot(Some(menu_path), menu_items);
    set_proof_state_override(native_menu_state_override());
    Ok(ProofCommandOutcome::EMIT_AFTER)
}

fn open_proof_dashboard(app: &AppHandle) -> Result<ProofCommandOutcome, String> {
    shell::transition_to_target(app, SurfaceMode::PopOut, SurfaceTarget::Dashboard, None)?;
    Ok(ProofCommandOutcome::SILENT)
}

fn open_proof_provider(
    app: &AppHandle,
    provider_id: String,
) -> Result<ProofCommandOutcome, String> {
    shell::transition_to_target(
        app,
        SurfaceMode::PopOut,
        SurfaceTarget::Provider { provider_id },
        None,
    )?;
    Ok(ProofCommandOutcome::SILENT)
}

fn open_proof_settings(app: &AppHandle, tab: String) -> Result<ProofCommandOutcome, String> {
    shell::settings_window::open_or_focus(app, &tab)?;
    Ok(ProofCommandOutcome::SILENT)
}

fn open_proof_about_path(app: &AppHandle) -> Result<ProofCommandOutcome, String> {
    transition_about_path(app)?;
    Ok(ProofCommandOutcome::EMIT_AFTER)
}

fn hide_proof_surface(app: &AppHandle) -> Result<ProofCommandOutcome, String> {
    shell::hide_to_tray(app)?;
    Ok(ProofCommandOutcome::SILENT)
}

pub fn ensure_proof_mode(app: &AppHandle) -> Result<(), String> {
    if is_proof_mode(app) {
        Ok(())
    } else {
        Err("proof harness is disabled".into())
    }
}

pub fn emit_state_changed(app: &AppHandle) {
    if let Ok(payload) = capture_state(app) {
        events::emit_proof_state_changed(app, &payload);
    }
}

pub fn sync_after_surface_transition(app: &AppHandle) {
    if !is_proof_mode(app) {
        return;
    }

    if surface_transition_sync_is_suppressed() {
        return;
    }

    clear_menu_snapshot();
    clear_proof_state_override();
    emit_state_changed(app);
}

fn clear_menu_snapshot() {
    set_menu_snapshot(None, Vec::new());
}

fn transition_about_path(app: &AppHandle) -> Result<(), String> {
    let _suppression = suppress_surface_transition_sync();
    persist_about_path_snapshot(
        shell::transition_to_target(
            app,
            SurfaceMode::Settings,
            SurfaceTarget::Settings {
                tab: "about".into(),
            },
            None,
        )
        .map(|_| ()),
    )
}

fn persist_about_path_snapshot(result: Result<(), String>) -> Result<(), String> {
    persist_about_path_snapshot_for_item("about", result)
}

fn persist_about_path_snapshot_for_item(
    item_id: &str,
    result: Result<(), String>,
) -> Result<(), String> {
    match result {
        Ok(()) => {
            let (menu_path, menu_items) = match native_menu_context_for_item(item_id) {
                Ok(context) => context,
                Err(err) => {
                    clear_menu_snapshot();
                    return Err(err);
                }
            };
            set_menu_snapshot(Some(menu_path), menu_items);
            Ok(())
        }
        Err(err) => {
            clear_menu_snapshot();
            Err(err)
        }
    }
}

fn set_menu_snapshot(menu_path: Option<String>, menu_items: Vec<String>) {
    let mut snapshot = PROOF_MENU_SNAPSHOT.lock().unwrap();
    snapshot.menu_path = menu_path;
    snapshot.menu_items = menu_items;
}

fn native_menu_state_override() -> ProofStateOverride {
    ProofStateOverride {
        mode: SurfaceMode::Hidden,
        target: SurfaceTarget::default_for_mode(SurfaceMode::Hidden),
        window_rect: None,
    }
}

fn resolve_surface_state(
    mode: SurfaceMode,
    target: SurfaceTarget,
    window_rect: Option<ProofRect>,
    state_override: Option<ProofStateOverride>,
) -> (String, SurfaceTarget, Option<ProofRect>) {
    if let Some(state_override) = state_override {
        (
            state_override.mode.as_str().to_string(),
            state_override.target,
            state_override.window_rect,
        )
    } else {
        (mode.as_str().to_string(), target, window_rect)
    }
}

fn proof_state_override() -> Option<ProofStateOverride> {
    PROOF_STATE_OVERRIDE.lock().unwrap().clone()
}

fn set_proof_state_override(state_override: ProofStateOverride) {
    *PROOF_STATE_OVERRIDE.lock().unwrap() = Some(state_override);
}

fn clear_proof_state_override() {
    *PROOF_STATE_OVERRIDE.lock().unwrap() = None;
}

fn suppress_surface_transition_sync() -> SurfaceTransitionSyncSuppression {
    PROOF_SYNC_CONTROL.lock().unwrap().suppressed_transitions += 1;
    SurfaceTransitionSyncSuppression
}

fn surface_transition_sync_is_suppressed() -> bool {
    PROOF_SYNC_CONTROL.lock().unwrap().suppressed_transitions > 0
}

impl Drop for SurfaceTransitionSyncSuppression {
    fn drop(&mut self) {
        let mut control = PROOF_SYNC_CONTROL.lock().unwrap();
        control.suppressed_transitions = control.suppressed_transitions.saturating_sub(1);
    }
}

#[cfg(test)]
fn menu_snapshot() -> ProofMenuSnapshot {
    PROOF_MENU_SNAPSHOT.lock().unwrap().clone()
}

#[cfg(test)]
fn current_proof_state_override() -> Option<ProofStateOverride> {
    proof_state_override()
}

fn native_menu_snapshot_for_path(menu_path: &str) -> (String, Vec<String>) {
    let providers = get_provider_catalog();
    let enabled = codexbar::settings::Settings::load().enabled_providers;
    let entries = tray_menu::build_tray_menu(&providers, &[], &enabled);
    let menu_items = tray_menu::proof_menu_items(&entries, menu_path).unwrap_or_default();
    (menu_path.to_string(), menu_items)
}

fn native_menu_context_for_item(item_id: &str) -> Result<(String, Vec<String>), String> {
    let providers = get_provider_catalog();
    let enabled = codexbar::settings::Settings::load().enabled_providers;
    let entries = tray_menu::build_tray_menu(&providers, &[], &enabled);
    tray_menu::proof_menu_context_for_item(&entries, item_id)
        .ok_or_else(|| format!("proof menu context missing tray item: {item_id}"))
}

fn proof_payload_is_supported(surface_mode: SurfaceMode, payload: Option<&str>) -> bool {
    match (surface_mode, payload) {
        (SurfaceMode::Hidden | SurfaceMode::TrayPanel, None) => true,
        (SurfaceMode::Hidden | SurfaceMode::TrayPanel, Some(_)) => false,
        (SurfaceMode::Settings, None) => true,
        (SurfaceMode::Settings, Some(tab)) => is_supported_settings_tab(tab),
        (SurfaceMode::PopOut, None) => true,
        (SurfaceMode::PopOut, Some(raw_target)) => {
            let Some(target) = SurfaceTarget::parse(raw_target) else {
                return false;
            };

            match target {
                SurfaceTarget::Dashboard => true,
                SurfaceTarget::Provider { provider_id } => is_supported_provider_id(&provider_id),
                _ => false,
            }
        }
    }
}

fn window_rect(window: &WebviewWindow) -> Option<ProofRect> {
    if !window.is_visible().ok()? {
        return None;
    }

    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some(ProofRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn resolve_work_area(window: &WebviewWindow, tray_anchor: Option<&ProofRect>) -> Option<ProofRect> {
    let monitors = window.available_monitors().ok()?;
    if let Some(anchor) = tray_anchor
        && let Some(monitor) = monitor_containing_rect(&monitors, anchor)
    {
        return Some(monitor_work_area_rect(monitor));
    }

    if let Ok(Some(monitor)) = window.current_monitor() {
        return Some(monitor_work_area_rect(&monitor));
    }

    window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor_work_area_rect(&monitor))
}

fn monitor_containing_rect<'a>(
    monitors: &'a [tauri::Monitor],
    rect: &ProofRect,
) -> Option<&'a tauri::Monitor> {
    let center_x = rect.x + rect.width as i32 / 2;
    let center_y = rect.y + rect.height as i32 / 2;

    monitors.iter().find(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        center_x >= position.x
            && center_x < position.x + size.width as i32
            && center_y >= position.y
            && center_y < position.y + size.height as i32
    })
}

fn monitor_work_area_rect(monitor: &tauri::Monitor) -> ProofRect {
    let work_area = monitor.work_area();
    ProofRect {
        x: work_area.position.x,
        y: work_area.position.y,
        width: work_area.size.width,
        height: work_area.size.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
    // Serializes tests that read/write the shared PROOF_MENU_SNAPSHOT global.
    static MENU_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn with_proof_mode_env(value: Option<&str>, test: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CODEXBAR_PROOF_MODE").ok();

        match value {
            Some(value) => unsafe { std::env::set_var("CODEXBAR_PROOF_MODE", value) },
            None => unsafe { std::env::remove_var("CODEXBAR_PROOF_MODE") },
        }

        test();

        match prev {
            Some(prev) => unsafe { std::env::set_var("CODEXBAR_PROOF_MODE", prev) },
            None => unsafe { std::env::remove_var("CODEXBAR_PROOF_MODE") },
        }
    }

    #[test]
    fn parse_simple_surface() {
        with_proof_mode_env(Some("trayPanel"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.target_surface, "trayPanel");
            assert!(cfg.settings_tab.is_none());
            assert_eq!(cfg.surface_mode(), SurfaceMode::TrayPanel);
        });
    }

    #[test]
    fn parse_settings_with_tab() {
        with_proof_mode_env(Some("settings:apiKeys"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.target_surface, "settings");
            assert_eq!(cfg.settings_tab.as_deref(), Some("apiKeys"));
            assert_eq!(cfg.surface_mode(), SurfaceMode::Settings);
            assert_eq!(
                cfg.surface_target(),
                SurfaceTarget::Settings {
                    tab: "apiKeys".into()
                }
            );
        });
    }

    #[test]
    fn parse_settings_about_proof_target() {
        with_proof_mode_env(Some("settings:about"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.target_surface, "settings");
            assert_eq!(cfg.settings_tab.as_deref(), Some("about"));
        });
    }

    #[test]
    fn parse_provider_popout_proof_target() {
        with_proof_mode_env(Some("popOut:provider:codex"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.target_surface, "popOut");
            assert_eq!(cfg.target_payload.as_deref(), Some("provider:codex"));
            assert_eq!(
                cfg.surface_target(),
                SurfaceTarget::Provider {
                    provider_id: "codex".into()
                }
            );
        });
    }

    #[test]
    fn parse_proof_command_for_native_menu() {
        let command = ProofCommand::parse("open-native-menu").unwrap();
        assert_eq!(command, ProofCommand::OpenNativeMenu);
    }

    #[test]
    fn parse_proof_command_for_about_entry_path() {
        let command = ProofCommand::parse("open-about-path").unwrap();
        assert_eq!(command, ProofCommand::OpenAboutPath);
    }

    #[test]
    fn empty_env_returns_none() {
        with_proof_mode_env(Some(""), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn unset_env_returns_none() {
        with_proof_mode_env(None, || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn invalid_surface_returns_none() {
        with_proof_mode_env(Some("bogus"), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn invalid_settings_tab_returns_none() {
        with_proof_mode_env(Some("settings:security"), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn invalid_provider_target_returns_none() {
        with_proof_mode_env(Some("popOut:provider:not-a-provider"), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn pop_out_surface() {
        with_proof_mode_env(Some("popOut"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.surface_mode(), SurfaceMode::PopOut);
            assert_eq!(cfg.surface_target(), SurfaceTarget::Dashboard);
        });
    }

    #[test]
    fn parse_proof_command_rejects_unknown_provider() {
        assert!(ProofCommand::parse("open-provider:not-a-provider").is_none());
    }

    #[test]
    fn parse_proof_command_rejects_unknown_settings_tab() {
        assert!(ProofCommand::parse("open-settings:security").is_none());
    }

    #[test]
    fn about_path_snapshot_persists_only_after_success() {
        let _guard = MENU_LOCK.lock().unwrap();
        clear_menu_snapshot();

        let result = persist_about_path_snapshot(Ok(()));

        assert!(result.is_ok());
        let snapshot = menu_snapshot();
        assert_eq!(snapshot.menu_path.as_deref(), Some("tray"));
        assert!(snapshot.menu_items.iter().any(|item| item == "About"));
    }

    #[test]
    fn about_path_snapshot_clears_on_failure() {
        let _guard = MENU_LOCK.lock().unwrap();
        set_menu_snapshot(Some("tray".into()), vec!["About CodexBar".into()]);

        let result = persist_about_path_snapshot(Err("boom".into()));

        assert_eq!(result.unwrap_err(), "boom");
        let snapshot = menu_snapshot();
        assert!(snapshot.menu_path.is_none());
        assert!(snapshot.menu_items.is_empty());
    }

    #[test]
    fn about_path_snapshot_fails_when_about_item_is_missing() {
        let _guard = MENU_LOCK.lock().unwrap();
        set_menu_snapshot(Some("tray".into()), vec!["About".into()]);

        let result = persist_about_path_snapshot_for_item("missing-about", Ok(()));

        assert_eq!(
            result.unwrap_err(),
            "proof menu context missing tray item: missing-about"
        );
        let snapshot = menu_snapshot();
        assert!(snapshot.menu_path.is_none());
        assert!(snapshot.menu_items.is_empty());
    }

    #[test]
    fn native_menu_override_normalizes_visible_surface_state() {
        let (mode, target, window_rect) = resolve_surface_state(
            SurfaceMode::PopOut,
            SurfaceTarget::Dashboard,
            Some(ProofRect {
                x: 10,
                y: 20,
                width: 300,
                height: 200,
            }),
            Some(native_menu_state_override()),
        );

        assert_eq!(mode, SurfaceMode::Hidden.as_str());
        assert_eq!(target, SurfaceTarget::Summary);
        assert!(window_rect.is_none());
    }

    #[test]
    fn clearing_override_restores_surface_state_passthrough() {
        set_proof_state_override(native_menu_state_override());
        clear_proof_state_override();

        assert!(current_proof_state_override().is_none());
    }

    #[test]
    fn surface_transition_sync_suppression_is_scoped() {
        clear_proof_state_override();
        assert!(!surface_transition_sync_is_suppressed());

        {
            let _suppression = suppress_surface_transition_sync();
            assert!(surface_transition_sync_is_suppressed());
        }

        assert!(!surface_transition_sync_is_suppressed());
    }
}
