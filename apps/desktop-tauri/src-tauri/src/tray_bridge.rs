//! System tray icon setup: left-click toggle, right-click native menu.

use std::sync::Mutex;

use crate::commands::ProviderCatalogEntry;
use codexbar::core::ProviderId;
use codexbar::settings::{MetricPreference, Settings, TrayIconMode};
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use codexbar::tray::{render_bar_icon_rgba, render_percent_icon_rgba};

use crate::shell;
use crate::state::{AppState, TrayAnchor};
use crate::surface::SurfaceMode;
use crate::surface_target::SurfaceTarget;
#[cfg(test)]
use crate::tray_menu::build_tray_menu;
use crate::tray_menu::{TrayMenuEntry, build_tray_menu_with};

#[derive(Debug, Clone, Copy)]
struct MonitorScaleInfo {
    physical_x: i32,
    physical_y: i32,
    physical_width: u32,
    physical_height: u32,
    scale_factor: f64,
}

impl MonitorScaleInfo {
    fn from_monitor(monitor: &tauri::Monitor) -> Self {
        let scale_factor = monitor.scale_factor();
        let safe_scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let position = monitor.position();
        let size = monitor.size();

        Self {
            physical_x: position.x,
            physical_y: position.y,
            physical_width: size.width,
            physical_height: size.height,
            scale_factor: safe_scale,
        }
    }
}

fn scale_factor_for_physical_point(x: f64, y: f64, monitors: &[MonitorScaleInfo]) -> Option<f64> {
    monitors
        .iter()
        .find(|monitor| {
            x >= monitor.physical_x as f64
                && x < (monitor.physical_x + monitor.physical_width as i32) as f64
                && y >= monitor.physical_y as f64
                && y < (monitor.physical_y + monitor.physical_height as i32) as f64
        })
        .map(|monitor| monitor.scale_factor)
}

fn logical_to_physical_anchor(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale_factor: f64,
) -> TrayAnchor {
    let safe_scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };

    TrayAnchor {
        x: (x * safe_scale).round() as i32,
        y: (y * safe_scale).round() as i32,
        width: ((width * safe_scale).round().max(1.0)) as u32,
        height: ((height * safe_scale).round().max(1.0)) as u32,
    }
}

fn resolve_tray_anchor(
    rect: &tauri::Rect,
    click_position: tauri::PhysicalPosition<f64>,
    monitors: &[MonitorScaleInfo],
) -> Option<TrayAnchor> {
    let click_scale = scale_factor_for_physical_point(click_position.x, click_position.y, monitors);

    match (rect.position, rect.size) {
        (tauri::Position::Physical(position), tauri::Size::Physical(size)) => Some(TrayAnchor {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }),
        (tauri::Position::Logical(position), tauri::Size::Logical(size)) => {
            click_scale.map(|scale| {
                logical_to_physical_anchor(position.x, position.y, size.width, size.height, scale)
            })
        }
        (tauri::Position::Physical(position), tauri::Size::Logical(size)) => {
            click_scale.map(|scale| TrayAnchor {
                x: position.x,
                y: position.y,
                width: ((size.width * scale).round().max(1.0)) as u32,
                height: ((size.height * scale).round().max(1.0)) as u32,
            })
        }
        (tauri::Position::Logical(position), tauri::Size::Physical(size)) => {
            click_scale.map(|scale| TrayAnchor {
                x: (position.x * scale).round() as i32,
                y: (position.y * scale).round() as i32,
                width: size.width,
                height: size.height,
            })
        }
    }
}

fn build_native_tray_menu(
    app: &AppHandle,
    providers: &[ProviderCatalogEntry],
    status_labels: &[(String, String)],
) -> tauri::Result<Menu<tauri::Wry>> {
    let settings = Settings::load();
    let enabled = settings.enabled_providers.clone();
    let spec = build_tray_menu_with(
        providers,
        status_labels,
        &enabled,
        settings.float_bar_enabled,
    );
    let entries = spec
        .iter()
        .map(|entry| build_native_menu_entry(app, entry))
        .collect::<tauri::Result<Vec<_>>>()?;
    let item_refs = entries
        .iter()
        .map(NativeMenuEntry::as_item)
        .collect::<Vec<_>>();

    Menu::with_items(app, &item_refs)
}

fn resolve_menu_target(id: &str) -> Option<shell::ShellTransitionRequest> {
    match id {
        "show_panel" => Some(shell::ShellTransitionRequest {
            mode: SurfaceMode::TrayPanel,
            target: SurfaceTarget::Summary,
            position: None,
        }),
        "pop_out" => Some(shell::ShellTransitionRequest {
            mode: SurfaceMode::PopOut,
            target: SurfaceTarget::Dashboard,
            position: None,
        }),
        _ if id.starts_with("provider:") => Some(shell::ShellTransitionRequest {
            mode: SurfaceMode::PopOut,
            target: SurfaceTarget::parse(id)?,
            position: None,
        }),
        _ => None,
    }
}

enum MenuAction {
    Transition(shell::ShellTransitionRequest),
    /// Open Settings/About in a detached window.
    OpenSettings(String),
    Refresh,
    CheckForUpdates,
    /// Toggle the enabled/disabled state of the provider with the given CLI name.
    ToggleProvider(String),
    /// Toggle the floating bar window on/off.
    ToggleFloatBar,
    Quit,
}

enum MenuTransitionDispatch {
    Transition(shell::ShellTransitionRequest),
    Reopen(shell::ShellTransitionRequest),
}

fn resolve_menu_action(id: &str) -> Option<MenuAction> {
    match id {
        "refresh" => Some(MenuAction::Refresh),
        "check_for_updates" => Some(MenuAction::CheckForUpdates),
        "quit" => Some(MenuAction::Quit),
        "settings" => Some(MenuAction::OpenSettings("general".into())),
        "about" => Some(MenuAction::OpenSettings("about".into())),
        "toggle_float_bar" => Some(MenuAction::ToggleFloatBar),
        _ if id.starts_with("toggle_provider:") => {
            let provider_id = id["toggle_provider:".len()..].to_string();
            Some(MenuAction::ToggleProvider(provider_id))
        }
        _ => resolve_menu_target(id).map(MenuAction::Transition),
    }
}

fn resolve_menu_transition_dispatch(
    id: &str,
    request: shell::ShellTransitionRequest,
) -> MenuTransitionDispatch {
    if id == "show_panel" {
        MenuTransitionDispatch::Reopen(shell::ShellTransitionRequest {
            mode: request.mode,
            target: request.target,
            position: None,
        })
    } else {
        MenuTransitionDispatch::Transition(request)
    }
}

/// Store the tray icon bounds from a click event into shared state.
fn store_anchor(app: &AppHandle, rect: &tauri::Rect, click_position: tauri::PhysicalPosition<f64>) {
    let monitors = app
        .get_webview_window("main")
        .and_then(|window| window.available_monitors().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|monitor| MonitorScaleInfo::from_monitor(&monitor))
        .collect::<Vec<_>>();

    let Some(anchor) = resolve_tray_anchor(rect, click_position, &monitors) else {
        return;
    };

    if let Some(st) = app.try_state::<Mutex<AppState>>() {
        let mut guard = st.lock().unwrap();
        guard.tray_anchor = Some(anchor);
    }
}

/// Initialise the system tray icon, context menu, and event handlers.
///
/// - **Left-click** toggles the custom tray panel via the surface state machine.
/// - **Right-click** opens the native context menu with shell actions.
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = build_native_tray_menu(app.handle(), &crate::commands::get_provider_catalog(), &[])?;

    // Embed the icon at compile time so it works regardless of working directory.
    let icon_bytes = include_bytes!("../../../../rust/icons/icon.png");
    let icon = Image::from_bytes(icon_bytes)?;

    let _tray = TrayIconBuilder::with_id("codexbar-main")
        .icon(icon)
        .tooltip("CodexBar Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                position,
                rect,
                ..
            } = event
            {
                let app = tray.app_handle();
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    store_anchor(app, &rect, position);
                    let position = shell::tray_panel_position(app);
                    shell::handle_tray_panel_click(app, position);
                }
            }
        })
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .build(app)?;

    Ok(())
}

/// Route a native menu-item click to the corresponding shell action.
fn handle_menu_event(app: &AppHandle, id: &str) {
    match resolve_menu_action(id) {
        Some(MenuAction::Transition(request)) => {
            match resolve_menu_transition_dispatch(id, request) {
                // Pass None so default_surface_position resolves the full chain:
                // tray_panel_position → inferred_tray_panel_position → shortcut_panel_position.
                // This mirrors the CODEXBAR_START_VISIBLE path and ensures the panel
                // opens near the taskbar tray corner even without a prior anchor click.
                MenuTransitionDispatch::Reopen(request) => {
                    let _ = shell::reopen_to_target(
                        app,
                        request.mode,
                        request.target,
                        request.position,
                    );
                }
                MenuTransitionDispatch::Transition(request) => {
                    let _ = shell::transition_to_target(
                        app,
                        request.mode,
                        request.target,
                        request.position,
                    );
                }
            }
        }
        Some(MenuAction::OpenSettings(tab)) => {
            let _ = shell::settings_window::open_or_focus(app, &tab);
        }
        Some(MenuAction::Refresh) => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::commands::do_refresh_providers(&handle).await;
            });
        }
        Some(MenuAction::CheckForUpdates) => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<Mutex<AppState>>();
                let _ = crate::commands::check_for_updates(handle.clone(), state).await;
            });
        }
        Some(MenuAction::ToggleProvider(provider_id)) => {
            let mut settings = Settings::load();
            if settings.enabled_providers.contains(&provider_id) {
                settings.enabled_providers.remove(&provider_id);
            } else {
                settings.enabled_providers.insert(provider_id);
            }
            let _ = settings.save();
            crate::floatbar::notify_settings_changed(app);
            rebuild_tray_menu(app);
        }
        Some(MenuAction::ToggleFloatBar) => {
            crate::floatbar::toggle(app);
            rebuild_tray_menu(app);
        }
        Some(MenuAction::Quit) => {
            app.exit(0);
        }
        None => {}
    }
}

/// Rebuild the native tray menu from current provider + settings state.
pub(crate) fn rebuild_tray_menu(app: &AppHandle) {
    let catalog = crate::commands::get_provider_catalog();
    let status_labels = if let Some(st) = app.try_state::<Mutex<AppState>>() {
        let guard = st.lock().unwrap();
        status_labels_for_settings(&Settings::load(), &guard.provider_cache)
    } else {
        vec![]
    };
    if let Ok(menu) = build_native_tray_menu(app, &catalog, &status_labels)
        && let Some(tray) = app.tray_by_id("codexbar-main")
    {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Rebuild the tray menu with current provider status labels after a refresh cycle.
pub fn update_tray_status_items(
    app: &AppHandle,
    snapshots: &[crate::commands::ProviderUsageSnapshot],
) {
    let catalog = crate::commands::get_provider_catalog();
    let status_labels = status_labels_for_settings(&Settings::load(), snapshots);

    if let Ok(menu) = build_native_tray_menu(app, &catalog, &status_labels)
        && let Some(tray) = app.tray_by_id("codexbar-main")
    {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Refresh every native tray surface that depends on settings and cached provider data.
pub(crate) fn refresh_tray_presentation(app: &AppHandle) {
    let snapshots = app
        .try_state::<Mutex<AppState>>()
        .map(|st| st.lock().unwrap().provider_cache.clone())
        .unwrap_or_default();

    update_tray_status_items(app, &snapshots);
    update_tray_icon_and_tooltip(app, &snapshots);
}

/// Update the tray icon pixels and tooltip text to reflect current provider usage.
///
/// Behaviour mirrors egui's `choose_tray_update_plan` (rust/src/native_ui/app.rs):
/// - If `menu_bar_shows_highest_usage` is on OR `menu_bar_display_mode == "minimal"`,
///   render the bar from the healthy provider with the highest session usage.
/// - Otherwise render from the first enabled healthy provider (catalog order).
/// - When any provider exposes a weekly/secondary window, the icon shows both
///   bars from the same picked provider.
/// - With zero healthy providers but at least one error, fall back to an
///   error-styled icon using the last known max percentage so the tray
///   still communicates "something is wrong".
pub fn update_tray_icon_and_tooltip(
    app: &AppHandle,
    snapshots: &[crate::commands::ProviderUsageSnapshot],
) {
    let Some(tray) = app.tray_by_id("codexbar-main") else {
        return;
    };

    // ── Icon ─────────────────────────────────────────────────────────────
    let settings = Settings::load();
    let ordered_snapshots = ordered_snapshot_refs(&settings, snapshots);
    let ok_snapshots: Vec<_> = ordered_snapshots
        .iter()
        .copied()
        .filter(|s| s.error.is_none())
        .collect();
    let all_error = ok_snapshots.is_empty() && !snapshots.is_empty();

    let prefer_highest = settings.menu_bar_shows_highest_usage
        || settings.menu_bar_display_mode.as_str() == "minimal";

    let picked = pick_tray_provider(&ok_snapshots, prefer_highest);

    let (session_pct, weekly_pct) = match picked {
        Some(s) => selected_tray_percents(s, &settings),
        None => (
            ok_snapshots
                .iter()
                .map(|s| selected_tray_percents(s, &settings).0)
                .fold(0.0_f64, f64::max),
            None,
        ),
    };

    let (rgba, w, h) = render_tray_icon_for_settings(&settings, session_pct, weekly_pct, all_error);
    let icon = Image::new_owned(rgba, w, h);
    let _ = tray.set_icon(Some(icon));

    // ── Tooltip ───────────────────────────────────────────────────────────
    let tooltip = build_tooltip(snapshots);
    let _ = tray.set_tooltip(Some(tooltip));
}

fn status_labels_for_settings(
    settings: &Settings,
    snapshots: &[crate::commands::ProviderUsageSnapshot],
) -> Vec<(String, String)> {
    let ordered_snapshots = ordered_snapshot_refs(settings, snapshots);
    let healthy: Vec<_> = ordered_snapshots
        .into_iter()
        .filter(|s| s.error.is_none())
        .collect();
    if settings.tray_icon_mode == TrayIconMode::PerProvider {
        return healthy
            .into_iter()
            .map(provider_status_label)
            .collect::<Vec<_>>();
    }

    let Some(selected) = pick_tray_provider(
        &healthy,
        settings.menu_bar_shows_highest_usage || settings.menu_bar_display_mode == "minimal",
    ) else {
        return vec![];
    };

    let (_, label) = provider_status_label(selected);
    vec![("status_summary".to_string(), label)]
}

fn ordered_snapshot_refs<'a>(
    settings: &Settings,
    snapshots: &'a [crate::commands::ProviderUsageSnapshot],
) -> Vec<&'a crate::commands::ProviderUsageSnapshot> {
    let order = settings
        .provider_display_order_names()
        .into_iter()
        .enumerate()
        .map(|(index, provider_id)| (provider_id, index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut ordered = snapshots.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| {
        let a_order = order.get(&a.provider_id);
        let b_order = order.get(&b.provider_id);
        match (a_order, b_order) {
            (Some(a_order), Some(b_order)) if a_order != b_order => a_order.cmp(b_order),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => a.display_name.cmp(&b.display_name),
        }
    });
    ordered
}

fn provider_status_label(snapshot: &crate::commands::ProviderUsageSnapshot) -> (String, String) {
    let label = snapshot
        .tray_status_label
        .clone()
        .unwrap_or_else(|| format!("{:.0}%", snapshot.primary.used_percent));
    (
        snapshot.provider_id.clone(),
        format!("{} {}", snapshot.display_name, label),
    )
}

fn render_tray_icon_for_settings(
    settings: &Settings,
    session_pct: f64,
    weekly_pct: Option<f64>,
    all_error: bool,
) -> (Vec<u8>, u32, u32) {
    if settings.menu_bar_shows_percent {
        render_percent_icon_rgba(session_pct, all_error)
    } else {
        render_bar_icon_rgba(session_pct, weekly_pct, all_error)
    }
}

/// Pick the provider whose usage the tray icon should render.
///
/// Exposed so that the unit tests can exercise both `highest` and `first`
/// paths without needing a live Tauri app handle.
fn pick_tray_provider<'a>(
    ok_snapshots: &'a [&'a crate::commands::ProviderUsageSnapshot],
    prefer_highest: bool,
) -> Option<&'a crate::commands::ProviderUsageSnapshot> {
    if ok_snapshots.is_empty() {
        return None;
    }
    if prefer_highest {
        ok_snapshots.iter().copied().max_by(|a, b| {
            a.primary
                .used_percent
                .partial_cmp(&b.primary.used_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    } else {
        Some(ok_snapshots[0])
    }
}

fn selected_tray_percents(
    snapshot: &crate::commands::ProviderUsageSnapshot,
    settings: &Settings,
) -> (f64, Option<f64>) {
    let provider = ProviderId::from_cli_name(snapshot.provider_id.as_str());
    let preference = provider
        .map(|id| settings.get_provider_metric(id))
        .unwrap_or(MetricPreference::Automatic);
    let primary = selected_metric_percent(snapshot, provider, preference)
        .or_else(|| selected_metric_percent(snapshot, provider, MetricPreference::Automatic))
        .unwrap_or(snapshot.primary.used_percent);

    let secondary = snapshot
        .secondary
        .as_ref()
        .map(|w| display_metric_percent(w.used_percent, settings.show_as_used));

    (
        display_metric_percent(primary, settings.show_as_used),
        secondary,
    )
}

fn display_metric_percent(used_percent: f64, show_as_used: bool) -> f64 {
    let used = used_percent.clamp(0.0, 100.0);
    if show_as_used { used } else { 100.0 - used }
}

fn selected_metric_percent(
    snapshot: &crate::commands::ProviderUsageSnapshot,
    provider: Option<ProviderId>,
    preference: MetricPreference,
) -> Option<f64> {
    match preference {
        MetricPreference::Automatic => automatic_metric_percent(snapshot, provider),
        MetricPreference::Session => Some(snapshot.primary.used_percent),
        MetricPreference::Weekly => snapshot
            .secondary
            .as_ref()
            .map(|w| w.used_percent)
            .or(Some(snapshot.primary.used_percent)),
        MetricPreference::Model => snapshot
            .model_specific
            .as_ref()
            .map(|w| w.used_percent)
            .or(Some(snapshot.primary.used_percent)),
        MetricPreference::Tertiary => snapshot
            .tertiary
            .as_ref()
            .map(|w| w.used_percent)
            .or_else(|| snapshot.secondary.as_ref().map(|w| w.used_percent))
            .or(Some(snapshot.primary.used_percent)),
        MetricPreference::Credits | MetricPreference::ExtraUsage => cost_metric_percent(snapshot),
        MetricPreference::Average => average_metric_percent(snapshot),
    }
}

fn automatic_metric_percent(
    snapshot: &crate::commands::ProviderUsageSnapshot,
    provider: Option<ProviderId>,
) -> Option<f64> {
    match provider {
        Some(ProviderId::Cursor) => max_metric_percent([
            Some(snapshot.primary.used_percent),
            snapshot.secondary.as_ref().map(|w| w.used_percent),
            snapshot.tertiary.as_ref().map(|w| w.used_percent),
        ]),
        Some(ProviderId::Zai) => max_metric_percent([
            Some(snapshot.primary.used_percent),
            snapshot.tertiary.as_ref().map(|w| w.used_percent),
            None,
        ])
        .or_else(|| snapshot.secondary.as_ref().map(|w| w.used_percent)),
        Some(ProviderId::Factory) | Some(ProviderId::Kimi) => snapshot
            .secondary
            .as_ref()
            .map(|w| w.used_percent)
            .or(Some(snapshot.primary.used_percent)),
        Some(ProviderId::Copilot) => max_metric_percent([
            Some(snapshot.primary.used_percent),
            snapshot.secondary.as_ref().map(|w| w.used_percent),
            None,
        ]),
        _ => Some(snapshot.primary.used_percent),
    }
}

fn average_metric_percent(snapshot: &crate::commands::ProviderUsageSnapshot) -> Option<f64> {
    let secondary = snapshot.secondary.as_ref()?;
    Some((snapshot.primary.used_percent + secondary.used_percent) / 2.0)
}

fn cost_metric_percent(snapshot: &crate::commands::ProviderUsageSnapshot) -> Option<f64> {
    let cost = snapshot.cost.as_ref()?;
    let limit = cost.limit?;
    if limit <= 0.0 {
        return None;
    }
    Some(((cost.used / limit) * 100.0).clamp(0.0, 100.0))
}

fn max_metric_percent<const N: usize>(values: [Option<f64>; N]) -> Option<f64> {
    values
        .into_iter()
        .flatten()
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

/// Build a compact multi-line tooltip string from provider snapshots.
fn build_tooltip(snapshots: &[crate::commands::ProviderUsageSnapshot]) -> String {
    if snapshots.is_empty() {
        return "CodexBar Desktop".to_string();
    }

    let mut lines = Vec::with_capacity(snapshots.len() + 1);
    for s in snapshots {
        let status = if let Some(ref err) = s.error {
            let short = truncate_tooltip_text(err, 36);
            format!("{}: error ({})", s.display_name, short)
        } else {
            let label = s
                .tray_status_label
                .clone()
                .unwrap_or_else(|| format!("{:.0}%", s.primary.used_percent));
            format!("{}: {}", s.display_name, truncate_tooltip_text(&label, 42))
        };
        lines.push(status);
    }

    format!("CodexBar\n{}", lines.join("\n"))
}

fn truncate_tooltip_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[allow(dead_code)]
fn menu_contains(menu: &[TrayMenuEntry], id: &str) -> bool {
    menu.iter().any(|entry| {
        entry.id.as_deref() == Some(id)
            || (!entry.children.is_empty() && menu_contains(&entry.children, id))
    })
}

enum NativeMenuEntry {
    Item(MenuItem<tauri::Wry>),
    CheckItem(tauri::menu::CheckMenuItem<tauri::Wry>),
    Submenu(Submenu<tauri::Wry>),
    Separator(PredefinedMenuItem<tauri::Wry>),
}

impl NativeMenuEntry {
    fn as_item(&self) -> &dyn IsMenuItem<tauri::Wry> {
        match self {
            Self::Item(item) => item,
            Self::CheckItem(item) => item,
            Self::Submenu(item) => item,
            Self::Separator(item) => item,
        }
    }
}

fn build_native_menu_entry(
    app: &AppHandle,
    entry: &TrayMenuEntry,
) -> tauri::Result<NativeMenuEntry> {
    if entry.is_separator {
        return Ok(NativeMenuEntry::Separator(PredefinedMenuItem::separator(
            app,
        )?));
    }

    if !entry.children.is_empty() {
        let children = entry
            .children
            .iter()
            .map(|child| build_native_menu_entry(app, child))
            .collect::<tauri::Result<Vec<_>>>()?;
        let child_refs = children
            .iter()
            .map(NativeMenuEntry::as_item)
            .collect::<Vec<_>>();

        return Ok(NativeMenuEntry::Submenu(Submenu::with_items(
            app,
            &entry.label,
            true,
            &child_refs,
        )?));
    }

    // Render as a checkbox item when `checked` is set.
    if let Some(checked) = entry.checked {
        return Ok(NativeMenuEntry::CheckItem(
            CheckMenuItemBuilder::with_id(entry.id.clone().unwrap_or_default(), &entry.label)
                .enabled(!entry.disabled)
                .checked(checked)
                .build(app)?,
        ));
    }

    Ok(NativeMenuEntry::Item(MenuItem::with_id(
        app,
        entry.id.clone().unwrap_or_default(),
        &entry.label,
        !entry.disabled,
        None::<&str>,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_provider_catalog() -> Vec<ProviderCatalogEntry> {
        vec![
            ProviderCatalogEntry {
                id: "codex".into(),
                display_name: "Codex".into(),
                cookie_domain: None,
            },
            ProviderCatalogEntry {
                id: "claude".into(),
                display_name: "Claude".into(),
                cookie_domain: None,
            },
        ]
    }

    #[test]
    fn tray_menu_includes_about_and_provider_entries() {
        let menu = build_tray_menu(
            &sample_provider_catalog(),
            &[],
            &["codex".to_string(), "claude".to_string()]
                .into_iter()
                .collect(),
        );
        assert!(menu_contains(&menu, "about"));
        assert!(menu_contains(&menu, "toggle_provider:codex"));
        assert!(menu_contains(&menu, "quit"));
    }

    #[test]
    fn toggle_float_bar_routes_to_toggle_action() {
        let action = resolve_menu_action("toggle_float_bar").expect("float bar action");
        assert!(matches!(action, MenuAction::ToggleFloatBar));
    }

    #[test]
    fn settings_menu_routes_to_open_settings_action() {
        let action = resolve_menu_action("about").expect("about action");
        match action {
            MenuAction::OpenSettings(tab) => assert_eq!(tab, "about"),
            _ => panic!("expected OpenSettings for 'about'"),
        }

        let action = resolve_menu_action("settings").expect("settings action");
        match action {
            MenuAction::OpenSettings(tab) => assert_eq!(tab, "general"),
            _ => panic!("expected OpenSettings for 'settings'"),
        }
    }

    #[test]
    fn provider_menu_routes_to_provider_popout_target() {
        let action = resolve_menu_target("provider:codex").expect("provider target");
        assert_eq!(action.mode, SurfaceMode::PopOut);
        assert_eq!(
            action.target,
            SurfaceTarget::Provider {
                provider_id: "codex".into()
            }
        );
    }

    #[test]
    fn show_panel_menu_reopens_with_default_position_chain() {
        let dispatch = resolve_menu_transition_dispatch(
            "show_panel",
            shell::ShellTransitionRequest {
                mode: SurfaceMode::TrayPanel,
                target: SurfaceTarget::Summary,
                position: Some((320, 240)),
            },
        );

        match dispatch {
            MenuTransitionDispatch::Reopen(request) => {
                assert_eq!(request.mode, SurfaceMode::TrayPanel);
                assert_eq!(request.target, SurfaceTarget::Summary);
                assert_eq!(request.position, None);
            }
            MenuTransitionDispatch::Transition(_) => {
                panic!("show_panel should reopen via default tray positioning")
            }
        }
    }

    #[test]
    fn non_show_panel_menu_keeps_explicit_position() {
        let dispatch = resolve_menu_transition_dispatch(
            "pop_out",
            shell::ShellTransitionRequest {
                mode: SurfaceMode::PopOut,
                target: SurfaceTarget::Dashboard,
                position: Some((320, 240)),
            },
        );

        match dispatch {
            MenuTransitionDispatch::Transition(request) => {
                assert_eq!(request.mode, SurfaceMode::PopOut);
                assert_eq!(request.target, SurfaceTarget::Dashboard);
                assert_eq!(request.position, Some((320, 240)));
            }
            MenuTransitionDispatch::Reopen(_) => {
                panic!("non-show-panel actions should use direct transitions")
            }
        }
    }

    #[test]
    fn logical_tray_anchor_uses_click_monitor_scale() {
        let monitors = vec![
            MonitorScaleInfo {
                physical_x: 0,
                physical_y: 0,
                physical_width: 1920,
                physical_height: 1080,
                scale_factor: 1.0,
            },
            MonitorScaleInfo {
                physical_x: 1920,
                physical_y: 0,
                physical_width: 2560,
                physical_height: 1440,
                scale_factor: 2.0,
            },
        ];

        let rect = tauri::Rect {
            position: tauri::Position::Logical(tauri::LogicalPosition::new(1500.0, 500.0)),
            size: tauri::Size::Logical(tauri::LogicalSize::new(12.0, 12.0)),
        };
        let anchor = resolve_tray_anchor(
            &rect,
            tauri::PhysicalPosition::new(1510.0, 500.0),
            &monitors,
        )
        .expect("matching click monitor scale");

        assert_eq!(anchor.x, 1500);
        assert_eq!(anchor.y, 500);
        assert_eq!(anchor.width, 12);
        assert_eq!(anchor.height, 12);
    }

    #[test]
    fn logical_tray_anchor_skips_conversion_without_click_monitor() {
        let monitors = vec![MonitorScaleInfo {
            physical_x: 0,
            physical_y: 0,
            physical_width: 1920,
            physical_height: 1080,
            scale_factor: 1.0,
        }];
        let rect = tauri::Rect {
            position: tauri::Position::Logical(tauri::LogicalPosition::new(1500.0, 500.0)),
            size: tauri::Size::Logical(tauri::LogicalSize::new(12.0, 12.0)),
        };

        let anchor = resolve_tray_anchor(
            &rect,
            tauri::PhysicalPosition::new(2500.0, 500.0),
            &monitors,
        );

        assert!(anchor.is_none());
    }

    fn fake_snapshot_with(
        id: &str,
        display: &str,
        used_percent: f64,
        secondary_percent: Option<f64>,
        tertiary_percent: Option<f64>,
        cost: Option<(f64, f64)>,
    ) -> crate::commands::ProviderUsageSnapshot {
        crate::commands::ProviderUsageSnapshot {
            provider_id: id.into(),
            display_name: display.into(),
            primary: crate::commands::RateWindowSnapshot {
                used_percent,
                remaining_percent: 100.0 - used_percent,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
                is_exhausted: false,
                reserve_percent: None,
                reserve_description: None,
            },
            primary_label: None,
            secondary: secondary_percent.map(|pct| crate::commands::RateWindowSnapshot {
                used_percent: pct,
                remaining_percent: 100.0 - pct,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
                is_exhausted: false,
                reserve_percent: None,
                reserve_description: None,
            }),
            secondary_label: None,
            model_specific: None,
            tertiary: tertiary_percent.map(|pct| crate::commands::RateWindowSnapshot {
                used_percent: pct,
                remaining_percent: 100.0 - pct,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
                is_exhausted: false,
                reserve_percent: None,
                reserve_description: None,
            }),
            extra_rate_windows: Vec::new(),
            cost: cost.map(|(used, limit)| crate::commands::CostSnapshotBridge {
                used,
                limit: Some(limit),
                remaining: Some((limit - used).max(0.0)),
                currency_code: "USD".to_string(),
                period: "monthly".to_string(),
                resets_at: None,
                formatted_used: format!("${used:.2}"),
                formatted_limit: Some(format!("${limit:.2}")),
            }),
            plan_name: None,
            account_email: None,
            source_label: String::new(),
            updated_at: "2025-01-01T00:00:00Z".into(),
            error: None,
            pace: None,
            account_organization: None,
            tray_status_label: None,
            fetch_duration_ms: None,
        }
    }

    fn fake_snapshot(
        id: &str,
        display: &str,
        used_percent: f64,
    ) -> crate::commands::ProviderUsageSnapshot {
        fake_snapshot_with(id, display, used_percent, None, None, None)
    }

    #[test]
    fn pick_tray_provider_highest_picks_max_primary() {
        let a = fake_snapshot("codex", "Codex", 30.0);
        let b = fake_snapshot("claude", "Claude", 72.5);
        let c = fake_snapshot("gemini", "Gemini", 50.0);
        let refs: Vec<&crate::commands::ProviderUsageSnapshot> = vec![&a, &b, &c];

        let picked = pick_tray_provider(&refs, /* prefer_highest = */ true)
            .expect("highest mode should pick a provider");
        assert_eq!(picked.provider_id, "claude");
    }

    #[test]
    fn pick_tray_provider_first_preserves_catalog_order() {
        let a = fake_snapshot("codex", "Codex", 30.0);
        let b = fake_snapshot("claude", "Claude", 72.5);
        let refs: Vec<&crate::commands::ProviderUsageSnapshot> = vec![&a, &b];

        let picked = pick_tray_provider(&refs, /* prefer_highest = */ false)
            .expect("non-highest mode should still pick the first entry");
        assert_eq!(picked.provider_id, "codex");
    }

    #[test]
    fn pick_tray_provider_none_when_empty() {
        let refs: Vec<&crate::commands::ProviderUsageSnapshot> = vec![];
        assert!(pick_tray_provider(&refs, true).is_none());
        assert!(pick_tray_provider(&refs, false).is_none());
    }

    #[test]
    fn status_labels_per_provider_mode_lists_each_healthy_provider() {
        let settings = Settings {
            tray_icon_mode: TrayIconMode::PerProvider,
            provider_order: codexbar::settings::normalize_provider_order(&[
                "claude".to_string(),
                "codex".to_string(),
            ]),
            ..Settings::default()
        };
        let snapshots = vec![
            fake_snapshot("codex", "Codex", 30.0),
            fake_snapshot("claude", "Claude", 72.0),
        ];

        let labels = status_labels_for_settings(&settings, &snapshots);

        assert_eq!(
            labels,
            vec![
                ("claude".to_string(), "Claude 72%".to_string()),
                ("codex".to_string(), "Codex 30%".to_string()),
            ]
        );
    }

    #[test]
    fn status_labels_single_mode_collapses_to_selected_provider() {
        let settings = Settings {
            tray_icon_mode: TrayIconMode::Single,
            menu_bar_shows_highest_usage: true,
            ..Settings::default()
        };
        let snapshots = vec![
            fake_snapshot("codex", "Codex", 30.0),
            fake_snapshot("claude", "Claude", 72.0),
        ];

        let labels = status_labels_for_settings(&settings, &snapshots);

        assert_eq!(
            labels,
            vec![("status_summary".to_string(), "Claude 72%".to_string())]
        );
    }

    #[test]
    fn tray_icon_renderer_uses_percent_mode_when_enabled() {
        let bar_settings = Settings {
            menu_bar_shows_percent: false,
            ..Settings::default()
        };
        let percent_settings = Settings {
            menu_bar_shows_percent: true,
            ..Settings::default()
        };

        let (bar, bar_w, bar_h) =
            render_tray_icon_for_settings(&bar_settings, 72.0, Some(40.0), false);
        let (percent, pct_w, pct_h) =
            render_tray_icon_for_settings(&percent_settings, 72.0, Some(40.0), false);

        assert_eq!((bar_w, bar_h), (pct_w, pct_h));
        assert_ne!(bar, percent);
    }

    #[test]
    fn tooltip_uses_compact_status_labels() {
        let mut claude = fake_snapshot("claude", "Claude", 13.0);
        claude.tray_status_label = Some("13% • resets in 2h 05m".to_string());
        let mut codex = fake_snapshot("codex", "Codex", 8.0);
        codex.tray_status_label = Some("8% • resets in 4h 10m".to_string());

        let tooltip = build_tooltip(&[claude, codex]);

        assert_eq!(
            tooltip,
            "CodexBar\nClaude: 13% • resets in 2h 05m\nCodex: 8% • resets in 4h 10m"
        );
    }

    #[test]
    fn tooltip_truncates_long_provider_lines() {
        let mut claude = fake_snapshot("claude", "Claude", 13.0);
        claude.tray_status_label =
            Some("13% • resets in Jun 10 at 3:00PM with extra noisy suffix".to_string());

        let tooltip = build_tooltip(&[claude]);

        let line = tooltip.lines().nth(1).expect("provider tooltip line");
        assert!(line.starts_with("Claude: 13% • resets in Jun 10 at 3:00PM"));
        assert!(line.ends_with("..."));
        assert!(line.chars().count() <= 53);
    }

    #[test]
    fn selected_tray_percent_uses_cursor_extra_usage_cost() {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
        let snapshot = fake_snapshot_with(
            "cursor",
            "Cursor",
            10.0,
            Some(20.0),
            Some(72.0),
            Some((15.0, 100.0)),
        );

        let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

        assert_eq!(primary, 15.0);
        assert_eq!(secondary, Some(20.0));
    }

    #[test]
    fn selected_tray_percent_respects_remaining_display_mode() {
        let mut settings = Settings {
            show_as_used: false,
            ..Settings::default()
        };
        settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
        let snapshot = fake_snapshot_with(
            "cursor",
            "Cursor",
            10.0,
            Some(20.0),
            Some(72.0),
            Some((15.0, 100.0)),
        );

        let (primary, secondary) = selected_tray_percents(&snapshot, &settings);

        assert_eq!(primary, 85.0);
        assert_eq!(secondary, Some(80.0));
    }

    #[test]
    fn selected_tray_percent_falls_back_when_extra_usage_missing() {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
        let snapshot = fake_snapshot_with("cursor", "Cursor", 10.0, Some(72.0), None, None);

        let (primary, _) = selected_tray_percents(&snapshot, &settings);

        assert_eq!(primary, 72.0);
    }
}
