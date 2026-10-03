//! Detached "Pop Out Dashboard" flyout window: a resizable, tray-anchored
//! panel with optional always-on-top behavior that auto-hides on click-outside.
//!
//! Runs as an auxiliary Tauri window labeled `flyout`, independent of the
//! `main` window's surface state machine. It is the only dashboard layout:
//! tray left-click, "Pop Out Dashboard", the global shortcut, app launch and
//! single-instance relaunch all open it. The legacy PopOut layout on `main`
//! is retired.
//!
//! Structurally modeled on `crate::floatbar` (self-contained module owning
//! its window + a `handle_window_event` hook dispatched from `main.rs`
//! before the `main`-window-only handling); the window itself is built with
//! `settings_window.rs`'s builder recipe (async open, manual DWM dark-caption
//! pass, `WebviewUrl::App` with a `?window=` query marker).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use codexbar::settings::Settings;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl};

use super::activation::{self, Activation};
use crate::geometry_store::{self, StoredSize};
use crate::state::AppState;
use crate::surface::SurfaceMode;

pub const FLYOUT_LABEL: &str = "flyout";

/// Geometry-store key for the flyout's remembered SIZE (position is never
/// stored — each open chooses a tray or captured cursor anchor). Kept as
/// its own key (distinct from the legacy `SurfaceMode::TrayPanel::as_str()`
/// `"trayPanel"` key) — `geometry_store::load_size` migrates a pre-existing
/// `"trayPanel"` entry into this key on first read, so upgrading users keep
/// their remembered flyout size.
const FLYOUT_SIZE_KEY: &str = "flyout";

/// Same window used to close a same-click blur-dismiss/reopen race as the
/// pre-split tray panel handling (formerly `shell::transition::handle_tray_panel_click`,
/// removed once its only caller — the tray-icon left-click handler — was
/// retargeted to call this module directly).
const BLUR_DISMISS_CLICK_WINDOW: Duration = Duration::from_millis(250);
/// Grace period after showing the flyout during which a spurious Windows
/// blur (tray click focus race) is ignored — mirrors `main.rs`'s 500ms
/// `was_tray_panel_recently_shown` guard for the old shared window.
const RECENTLY_SHOWN_GRACE: Duration = Duration::from_millis(500);

/// Set once at startup by `CODEXBAR_START_VISIBLE`: keeps the flyout open
/// when it loses focus, for automation flows that need it to stay visible.
static KEEP_OPEN_ON_BLUR: AtomicBool = AtomicBool::new(false);

/// Keep the flyout open on focus loss for the rest of this process.
pub fn keep_open_on_blur() {
    KEEP_OPEN_ON_BLUR.store(true, Ordering::Relaxed);
}

/// Read the remembered flyout size, if any (migrating a legacy
/// `"trayPanel"`-keyed size on first read — see `geometry_store::load_size`).
pub fn stored_size() -> Option<(u32, u32)> {
    geometry_store::load_size(FLYOUT_SIZE_KEY).map(|size| (size.width, size.height))
}

/// Persist a user-chosen flyout size. Size-only — no fabricated position.
pub fn save_stored_size(width: u32, height: u32) {
    geometry_store::save_size(FLYOUT_SIZE_KEY, StoredSize { width, height });
}

/// Whether the flyout window currently exists and is visible. Canonical
/// replacement for the pre-split `surface_machine.current() == TrayPanel`
/// check, now that the flyout is not a state of the shared machine.
pub fn is_open(app: &AppHandle) -> bool {
    app.get_webview_window(FLYOUT_LABEL)
        .is_some_and(|w| w.is_visible().unwrap_or(false))
}

/// Build (first open) or show (subsequent opens) the flyout window at
/// `position`, if given, else the default tray-anchored position.
/// `activation` says whether it may take focus once shown (see
/// [`super::activation`]); on first open the frontend reveals the window, so
/// the request is stored and applied by `reveal_tray_panel_window`.
///
/// `WebviewWindowBuilder::build` deadlocks when called synchronously from a
/// Tauri command on Windows (see `commands/surface.rs::open_settings_window`
/// precedent) — callers must invoke this from an async context (an `async`
/// command, or `tauri::async_runtime::spawn`), never a sync command handler.
pub fn open_or_focus(
    app: &AppHandle,
    position: Option<(i32, i32)>,
    activation: Activation,
) -> Result<(), String> {
    open_with_anchor(app, position, None, activation)
}

/// Open a desktop launch near its captured cursor location, retaining that
/// anchor through subsequent content-driven resizes.
pub fn open_near_cursor(
    app: &AppHandle,
    cursor: Option<(f64, f64)>,
    activation: Activation,
) -> Result<(), String> {
    open_with_anchor(app, None, cursor, activation)
}

fn open_with_anchor(
    app: &AppHandle,
    position: Option<(i32, i32)>,
    cursor: Option<(f64, f64)>,
    activation: Activation,
) -> Result<(), String> {
    app.state::<Mutex<AppState>>()
        .lock()
        .map_err(|e| e.to_string())?
        .flyout_cursor_anchor = cursor;
    let settings = Settings::load();
    if let Some(window) = app.get_webview_window(FLYOUT_LABEL) {
        apply_window_always_on_top(&window, settings.tray_panel_always_on_top)?;
        if let Some((x, y)) = position {
            // Best-effort reposition before show; the subsequent show/focus
            // is what the user sees, so the position result is non-fatal.
            let _set_position = window.set_position(PhysicalPosition::new(x, y));
        } else {
            reanchor(app)?;
        }
        window.show().map_err(|e| e.to_string())?;
        activation::apply(&window, activation)?;
        if show_grace_starts_now(false) {
            mark_shown(app);
        }
        return Ok(());
    }

    // Derive window properties from `SurfaceMode::TrayPanel.window_properties()`
    // — the historical single source of truth for the flyout's shape (size,
    // resizability, taskbar visibility). The variant is kept
    // specifically so this builder (and the geometry-store key, and
    // `default_surface_position`'s positioning branch) have one place to read
    // from, rather than duplicating these values as independent constants
    // that could silently drift from `surface.rs`.
    let props = SurfaceMode::TrayPanel.window_properties();
    let (width, height) = stored_size()
        .map(|(w, h)| (w as f64, h as f64))
        .unwrap_or((props.width, props.height));

    let url = WebviewUrl::App("index.html?window=flyout".into());

    let mut builder = tauri::WebviewWindowBuilder::new(app, FLYOUT_LABEL, url)
        .title("CodexBar")
        .inner_size(width, height)
        .decorations(props.decorations)
        .shadow(false)
        .resizable(props.resizable)
        .always_on_top(settings.tray_panel_always_on_top)
        .skip_taskbar(props.skip_taskbar)
        .theme(Some(tauri::Theme::Dark))
        // CRITICAL: dynamically-built windows default to drag-drop ENABLED,
        // which intercepts the HTML5 draggable events the provider grid's
        // drag-reorder (ProviderGrid.tsx) relies on — see `main`'s
        // `dragDropEnabled: false` in tauri.conf.json for why this must be
        // disabled explicitly on every window that hosts that grid.
        .disable_drag_drop_handler()
        // Showing never activates the flyout by itself (tao shows it with
        // SW_SHOWNOACTIVATE); `activation::apply` decides whether it takes
        // focus.
        .focused(false)
        .visible(false);
    if let (Some(min_w), Some(min_h)) = (props.min_width, props.min_height) {
        builder = builder.min_inner_size(min_w, min_h);
    }
    let win = builder.build().map_err(|e| e.to_string())?;
    apply_window_always_on_top(&win, settings.tray_panel_always_on_top)?;

    // Force DWM caption dark; keep WS_THICKFRAME (resizable) like the
    // Settings window.
    super::dwm::force_dark_caption_resizable(&win);

    if cursor.is_some() {
        reanchor(app)?;
    } else if let Some((x, y)) =
        position.or_else(|| super::position::default_surface_position(app, SurfaceMode::TrayPanel))
    {
        // Best-effort initial placement; the frontend re-reveals the
        // window, so a failed set_position here is non-fatal.
        let _set_initial = win.set_position(PhysicalPosition::new(x, y));
    }

    // Left `.visible(false)` above — the frontend reveals the window itself
    // after its first layout pass, then `reveal_tray_panel_window` starts the
    // recently-shown blur grace at the real show/focus boundary.
    if show_grace_starts_now(true) {
        mark_shown(app);
    }
    arm_reveal(app, activation)?;
    Ok(())
}

fn apply_window_always_on_top(window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    window
        .set_always_on_top(enabled)
        .map_err(|error| error.to_string())
}

/// Reapply the setting to the already-created flyout. The label lookup keeps
/// this native mutation scoped to the tray-panel window.
pub fn apply_always_on_top(app: &AppHandle, settings: &Settings) {
    let Some(window) = app.get_webview_window(FLYOUT_LABEL) else {
        return;
    };
    if let Err(error) = apply_window_always_on_top(&window, settings.tray_panel_always_on_top) {
        tracing::warn!(%error, "failed to apply tray panel always-on-top setting");
    }
}

fn should_dismiss_on_blur(settings: &Settings) -> bool {
    !settings.tray_panel_always_on_top
}

fn show_grace_starts_now(first_build_hidden: bool) -> bool {
    !first_build_hidden
}

fn arm_reveal(app: &AppHandle, activation: Activation) -> Result<(), String> {
    let state = app
        .try_state::<Mutex<AppState>>()
        .ok_or_else(|| "app state unavailable".to_string())?;
    state
        .lock()
        .map_err(|e| e.to_string())?
        .arm_flyout_reveal(activation);
    Ok(())
}

/// Toggle the flyout: hide if open, open (or focus) otherwise. Consumes a
/// same-click blur-dismissal first (see `handle_window_event`'s
/// `Focused(false)` path) so a tray click that just blur-dismissed the
/// flyout cleanly closes it instead of instantly reopening.
///
/// Must be called from an async context — see [`open_or_focus`].
pub fn toggle_with_blur_consume(app: &AppHandle, position: Option<(i32, i32)>) {
    let consumed_blur_dismissal = {
        let st = app.state::<Mutex<AppState>>();
        st.lock()
            .unwrap()
            .take_recent_blur_dismissal(Instant::now(), BLUR_DISMISS_CLICK_WINDOW)
    };

    if consumed_blur_dismissal {
        return;
    }

    if is_open(app) {
        // Tray-toggle is fire-and-forget; a hide error leaves the window
        // closed from the user's perspective and is non-fatal.
        let _hide = hide(app);
    } else {
        // Tray-toggle is fire-and-forget; open_or_focus surfaces its own
        // errors via tracing, so the toggle call site discards the result.
        // Both callers (tray left-click, global hotkey) are user actions.
        let _open = open_or_focus(app, position, Activation::UserAction);
    }
}

/// Hide (never close) the flyout window. Hiding — rather than closing —
/// keeps the window's WebView2 instance alive across opens, matching
/// `settings_window::dismiss`'s rationale: closing risks Tauri's
/// process/window lifecycle treating it as an app-relevant close.
pub fn hide(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FLYOUT_LABEL) {
        let state = app
            .try_state::<Mutex<AppState>>()
            .ok_or_else(|| "app state unavailable".to_string())?;
        state
            .lock()
            .map_err(|e| e.to_string())?
            .clear_flyout_reveal();
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn mark_shown(app: &AppHandle) {
    if let Some(st) = app.try_state::<Mutex<AppState>>()
        && let Ok(mut guard) = st.lock()
    {
        guard.mark_tray_panel_shown(Instant::now());
    }
}

/// Handle a `WindowEvent` targeting the flyout window. Returns `true` when
/// the event was for the flyout (and was handled), `false` otherwise so the
/// caller (`main.rs`'s single `on_window_event` dispatcher) can fall through
/// to its own `main`-window-only handling.
///
/// Ports the same four-guard chain `main.rs` applies to the old shared
/// window's `Focused(false)` (proof-mode suppression / startup grace /
/// recently-shown 500ms grace / gesture blur guard) so every previously
/// shipped anti-flicker behavior survives the window split.
pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) -> bool {
    if window.label() != FLYOUT_LABEL {
        return false;
    }

    let app = window.app_handle();

    match event {
        tauri::WindowEvent::Focused(false) => {
            let settings = Settings::load();
            if !should_dismiss_on_blur(&settings) {
                // Keep the opt-in flyout visible when focus moves elsewhere;
                // reapplying the native state also repairs any transient z-order
                // change caused by the focus transition.
                let _ = window.set_always_on_top(true);
                return true;
            }
            if crate::proof_harness::is_proof_mode(app) || KEEP_OPEN_ON_BLUR.load(Ordering::Relaxed)
            {
                return true;
            }
            let Some(st) = app.try_state::<Mutex<AppState>>() else {
                return true;
            };
            {
                let mut guard = st.lock().unwrap();
                if guard.take_startup_tray_blur_grace(Instant::now()) {
                    return true;
                }
                if guard.was_tray_panel_recently_shown(Instant::now(), RECENTLY_SHOWN_GRACE) {
                    return true;
                }
                if guard.is_gesture_blur_guard_active(Instant::now()) {
                    return true;
                }
            }
            if hide(app).is_ok() {
                st.lock().unwrap().mark_blur_dismissed(Instant::now());
            }
            true
        }
        tauri::WindowEvent::Focused(true) => {
            if let Some(st) = app.try_state::<Mutex<AppState>>() {
                st.lock()
                    .unwrap()
                    .clear_gesture_guard_on_refocus(Instant::now());
            }
            true
        }
        // Size persistence is entirely frontend-driven (genuine user
        // drag-resizes call `set_flyout_size`, auto-fit resizes never do) —
        // mirrors `shell::position::remember_current_geometry_if_eligible`
        // skipping TrayPanel for the same reason on the old shared window.
        // Position is never persisted; resizing follows this open's selected anchor.
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => true,
        tauri::WindowEvent::CloseRequested { api, .. } => {
            // Hide-not-close on a native close request (Alt+F4-equivalent from
            // a screen reader): the flyout survives so it can be reopened without
            // rebuilding the WebView2 instance. A hide failure is non-fatal and
            // the window stays for reuse.
            api.prevent_close();
            let _hide_on_close = hide(app);
            true
        }
        _ => true,
    }
}

/// Reposition the flyout at its captured cursor or system-tray anchor,
/// using the window's CURRENT logical size (after a
/// frontend-driven resize). Canonical anchor-math implementation for the
/// flyout window; the `reanchor_tray_panel` Tauri command
/// (`commands/system.rs`) is a thin retarget onto this function.
pub fn reanchor(app: &AppHandle) -> Result<(), String> {
    use crate::window_positioner::{PanelSize, Rect};

    let window = app
        .get_webview_window(FLYOUT_LABEL)
        .ok_or_else(|| "flyout window unavailable".to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);

    let outer = window.outer_size().map_err(|e| e.to_string())?;
    let panel_size = PanelSize {
        // Rounded logical px fit u32 by design (whole physical px / scale).
        width: (outer.width as f64 / scale).round() as u32,
        height: (outer.height as f64 / scale).round() as u32,
    };

    let (anchor, cursor) = app
        .try_state::<Mutex<AppState>>()
        .and_then(|state| {
            let state = state.lock().ok()?;
            Some((state.tray_anchor, state.flyout_cursor_anchor))
        })
        .unwrap_or_default();
    let monitors = window.available_monitors().unwrap_or_default();
    let monitor = cursor
        .and_then(|(x, y)| app.monitor_from_point(x, y).ok().flatten())
        .or_else(|| {
            anchor
                .and_then(|anchor| crate::shell::geometry::monitor_for_anchor(&monitors, anchor))
                .cloned()
        })
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "no monitor".to_string())?;

    let work_area = crate::shell::geometry::monitor_work_area_rect(&monitor);
    // The target monitor can differ from the window's previous monitor.
    let scale = monitor.scale_factor().max(1.0);
    let monitor_bounds = Rect {
        x: monitor.position().x,
        y: monitor.position().y,
        width: monitor.size().width,
        height: monitor.size().height,
    };

    let (x, y) = {
        if let Some(cursor) = cursor {
            crate::window_positioner::calculate_cursor_position(
                cursor,
                &work_area,
                &panel_size,
                scale,
            )
        } else if let Some(a) = anchor {
            crate::window_positioner::calculate_panel_position(
                &Rect {
                    x: a.x,
                    y: a.y,
                    width: a.width,
                    height: a.height,
                },
                &monitor_bounds,
                &work_area,
                &panel_size,
                scale,
            )
        } else {
            // No real click anchor yet: infer one from the taskbar side
            // (handles left/right/top-docked taskbars, not just bottom-right).
            crate::shell::inferred_tray_panel_position_for_monitor_size(&monitor, &panel_size)
        }
    };

    // Pass physical coordinates directly — tao converts PhysicalPosition to
    // OS logical internally by dividing by the window's scale factor.
    let pos = tauri::PhysicalPosition::new(x, y);
    tracing::debug!(
        "flyout_window::reanchor: panel={}x{} => ({},{})",
        panel_size.width,
        panel_size.height,
        pos.x,
        pos.y
    );
    // Best-effort re-anchor after resize; the position result is non-fatal
    // and the next geometry event re-anchors again.
    let _set_position = window.set_position(pos);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flyout_label_is_stable() {
        assert_eq!(FLYOUT_LABEL, "flyout");
    }

    #[test]
    fn flyout_size_key_is_distinct_from_legacy_tray_panel_key() {
        // The whole point of the migration in geometry_store::load_size is
        // that this key differs from the legacy SurfaceMode::TrayPanel key
        // ("trayPanel") — otherwise there'd be nothing to migrate FROM.
        assert_ne!(FLYOUT_SIZE_KEY, SurfaceMode::TrayPanel.as_str());
    }

    #[test]
    fn tray_panel_window_properties_still_the_single_source_for_flyout_shape() {
        // `open_or_focus`'s builder reads size/decorations/resizable/
        // skip_taskbar/min-size from
        // `SurfaceMode::TrayPanel.window_properties()` directly (not
        // independent duplicated constants) — this pins down the values that
        // relationship depends on, so a change to `surface.rs` shows up here
        // instead of silently drifting from what the flyout actually builds.
        let props = SurfaceMode::TrayPanel.window_properties();
        assert_eq!(props.width, 328.0);
        assert_eq!(props.height, 776.0);
        assert_eq!(props.min_width, Some(300.0));
        assert_eq!(props.min_height, Some(360.0));
        assert!(props.resizable);
        assert!(!props.always_on_top);
        assert!(props.skip_taskbar);
        assert!(!props.decorations);
    }

    #[test]
    fn first_hidden_build_does_not_start_show_grace() {
        assert!(show_grace_starts_now(false));
        assert!(!show_grace_starts_now(true));
    }

    #[test]
    fn tray_panel_blur_dismissal_is_disabled_only_when_opted_in() {
        assert!(should_dismiss_on_blur(&Settings::default()));
        assert!(!should_dismiss_on_blur(&Settings {
            tray_panel_always_on_top: true,
            ..Settings::default()
        }));
    }
}
