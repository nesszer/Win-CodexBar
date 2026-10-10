//! System tray icon setup: left-click opens the tray panel, right-click native menu.

use std::sync::Mutex;

use crate::commands::ProviderCatalogEntry;
use codexbar::settings::Settings;
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::shell;
use crate::state::{AppState, TrayAnchor};
#[cfg(test)]
use crate::tray_menu::build_tray_menu;
use crate::tray_menu::{MenuState, TrayMenuEntry, build_tray_menu_with};
use crate::tray_presentation::{TrayPresentationPlan, headline_window};

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
    let mut spec = build_tray_menu_with(
        providers,
        status_labels,
        &enabled,
        MenuState {
            float_bar_enabled: settings.float_bar_enabled,
            stay_awake_held: crate::stay_awake::is_held(),
        },
        settings.ui_language,
    );
    crate::tray_accounts::prepend_account_menus(&mut spec, &settings);
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

enum MenuAction {
    /// Open Settings/About in a detached window.
    OpenSettings(String),
    /// Open (or focus) the dedicated flyout ("Pop Out Dashboard") window.
    OpenFlyout,
    Refresh,
    CheckForUpdates,
    /// Toggle the enabled/disabled state of the provider with the given CLI name.
    ToggleProvider(String),
    /// Toggle the floating bar window on/off.
    ToggleFloatBar,
    Account(crate::tray_accounts::AccountMenuAction),
    Quit,
}

fn resolve_menu_action(id: &str) -> Option<MenuAction> {
    if let Some(action) = crate::tray_accounts::resolve_action(id) {
        return Some(MenuAction::Account(action));
    }
    match id {
        "refresh" => Some(MenuAction::Refresh),
        "check_for_updates" => Some(MenuAction::CheckForUpdates),
        "quit" => Some(MenuAction::Quit),
        "settings" => Some(MenuAction::OpenSettings("general".into())),
        "about" => Some(MenuAction::OpenSettings("about".into())),
        "toggle_float_bar" => Some(MenuAction::ToggleFloatBar),
        "pop_out" => Some(MenuAction::OpenFlyout),
        _ if id.starts_with("toggle_provider:") => {
            let provider_id = id["toggle_provider:".len()..].to_string();
            Some(MenuAction::ToggleProvider(provider_id))
        }
        _ => None,
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
                    // Left-click toggles the tray-panel flyout window (Pop Out
                    // Dashboard): open it, or cleanly close it when this same
                    // click already blur-dismissed it (no open→close flicker).
                    // The flyout is the only dashboard layout; the legacy
                    // PopOut layout on `main` is retired. Called directly (not spawned): native
                    // tray-icon event callbacks run on the same main-thread
                    // event-loop context as `on_menu_event` below, where
                    // `settings_window::open_or_focus` is also called
                    // synchronously — the WebviewWindowBuilder deadlock only
                    // affects builds invoked from *synchronous Tauri IPC
                    // commands*, not native event-loop callbacks.
                    shell::flyout_window::toggle_with_blur_consume(app, None);
                }
            }
        })
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .build(app)?;

    // Apply tray promotion on startup. The NotifyIconSettings entry is created
    // by Windows only after the icon is first registered, so on first run /
    // post-upgrade the subkey may not exist yet — apply_promotion tolerates
    // EntryNotFound. Retry a few times while explorer finishes registration.
    schedule_tray_promotion_retries(app.handle().clone());

    Ok(())
}

/// Re-apply Win11 tray promotion a few times after startup.
///
/// Windows often creates the NotifyIconSettings subkey only after the first
/// successful NIM_ADD (and sometimes only after the icon is refreshed). A
/// single immediate write is not enough after upgrades.
fn schedule_tray_promotion_retries(app_handle: AppHandle) {
    if !codexbar::settings::Settings::load().promote_tray_icon {
        return;
    }
    crate::tray_visibility::apply_promotion(true);
    tauri::async_runtime::spawn(async move {
        for secs in [1_u64, 3, 8] {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            if !codexbar::settings::Settings::load().promote_tray_icon {
                break;
            }
            crate::tray_visibility::apply_promotion(true);
        }
        drop(app_handle);
    });
}

/// Route a native menu-item click to the corresponding shell action.
fn handle_menu_event(app: &AppHandle, id: &str) {
    match resolve_menu_action(id) {
        Some(MenuAction::Account(action)) => crate::tray_accounts::handle_action(app, action),
        Some(MenuAction::OpenSettings(tab)) => {
            let _ = shell::settings_window::open_or_focus(app, &tab);
        }
        Some(MenuAction::OpenFlyout) => {
            // Pass None: open_or_focus falls back to the tray-anchored
            // default position (same placement chain the old TrayPanel
            // transition used) when no explicit position is given.
            crate::auto_refresh::note_menu_open();
            let _ = shell::flyout_window::open_or_focus(
                app,
                None,
                shell::activation::Activation::UserAction,
            );
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
    let settings = Settings::load();
    let rates_cache = app
        .try_state::<crate::commands::CurrencyRateCache>()
        .map(|state| state.inner());
    let status_labels = if let Some(st) = app.try_state::<Mutex<AppState>>() {
        let guard = st.lock().unwrap();
        TrayPresentationPlan::resolve(&settings, &guard.provider_cache, rates_cache)
            .status_labels(settings.ui_language)
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
    let settings = Settings::load();
    let rates_cache = app
        .try_state::<crate::commands::CurrencyRateCache>()
        .map(|state| state.inner());
    let status_labels = TrayPresentationPlan::resolve(&settings, snapshots, rates_cache)
        .status_labels(settings.ui_language);

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

    let settings = Settings::load();
    let rates_cache = app
        .try_state::<crate::commands::CurrencyRateCache>()
        .map(|state| state.inner());
    let plan = TrayPresentationPlan::resolve(&settings, snapshots, rates_cache);
    let (rgba, w, h) = plan.render_icon();
    let icon = Image::new_owned(rgba, w, h);
    let _ = tray.set_icon(Some(icon));

    let tooltip = build_tooltip(snapshots, settings.ui_language);
    let _ = tray.set_tooltip(Some(tooltip));
}

/// Build a compact multi-line tooltip string from provider snapshots.
fn build_tooltip(
    snapshots: &[crate::commands::ProviderUsageSnapshot],
    lang: codexbar::settings::Language,
) -> String {
    use codexbar::locale::{LocaleKey, get_text};

    if snapshots.is_empty() {
        return "CodexBar Desktop".to_string();
    }

    let error_label = get_text(lang, LocaleKey::TrayStatusRowError);
    let mut lines = Vec::with_capacity(snapshots.len() + 1);
    for s in snapshots {
        let status = if let Some(ref err) = s.error {
            let short = truncate_tooltip_text(err, 36);
            format!("{}: {} ({})", s.display_name, error_label, short)
        } else {
            let label = crate::commands::compact_tray_status_label(headline_window(s), lang);
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

#[allow(
    dead_code,
    reason = "tray bridge helper reserved for future system tray integration"
)]
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

    use crate::surface::SurfaceMode;
    use crate::tray_presentation::{
        codex_lane_headline_window, pick_tray_provider, selected_tray_percents,
    };
    use codexbar::core::ProviderId;
    use codexbar::settings::MetricPreference;

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
    fn pop_out_menu_routes_to_open_flyout_action() {
        // "Pop Out Dashboard" opens the tray-panel flyout window, the only
        // dashboard layout.
        let action = resolve_menu_action("pop_out").expect("pop_out action");
        assert!(matches!(action, MenuAction::OpenFlyout));

        // SurfaceMode::TrayPanel is retained purely as a data key (geometry
        // key / window_properties source / panel-size reference) for the
        // flyout window's builder. The panel is a fixed-width menu.
        let props = SurfaceMode::TrayPanel.window_properties();
        assert!(!props.resizable && props.blur_dismiss && props.skip_taskbar);
    }

    #[test]
    fn legacy_popout_menu_ids_no_longer_route_anywhere() {
        // "show_panel" ("Show Window") and "provider:<id>" used to open the
        // retired PopOut layout on `main`.
        assert!(resolve_menu_action("show_panel").is_none());
        assert!(resolve_menu_action("provider:codex").is_none());
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
                is_informational: false,
                reserve_percent: None,
                reserve_description: None,
                reserve_will_last_to_reset: false,
                reserve_eta_seconds: None,
                monthly_limit_block: None,
                description_is_detail: false,
            },
            primary_label: None,
            secondary: secondary_percent.map(|pct| crate::commands::RateWindowSnapshot {
                used_percent: pct,
                remaining_percent: 100.0 - pct,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
                is_exhausted: false,
                is_informational: false,
                reserve_percent: None,
                reserve_description: None,
                reserve_will_last_to_reset: false,
                reserve_eta_seconds: None,
                monthly_limit_block: None,
                description_is_detail: false,
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
                is_informational: false,
                reserve_percent: None,
                reserve_description: None,
                reserve_will_last_to_reset: false,
                reserve_eta_seconds: None,
                monthly_limit_block: None,
                description_is_detail: false,
            }),
            tertiary_label: None,
            extra_rate_windows: Vec::new(),
            inventory: Vec::new(),
            display_details: Vec::new(),
            cost: cost.map(|(used, limit)| crate::commands::CostSnapshotBridge {
                used,
                limit: Some(limit),
                remaining: Some((limit - used).max(0.0)),
                currency_code: "USD".to_string(),
                currency_symbol: None,
                period: "monthly".to_string(),
                resets_at: None,
                formatted_used: format!("${used:.2}"),
                formatted_limit: Some(format!("${limit:.2}")),
                balance: None,
                formatted_balance: None,
                balance_updated_at: None,
                account_id: None,
                daily: Vec::new(),
                always_visible: false,
            }),
            plan_name: None,
            account_email: None,
            subscription: None,
            source_label: String::new(),
            has_successful_claude_cli_quota: false,
            updated_at: "2025-01-01T00:00:00Z".into(),
            error: None,
            error_state: codexbar::core::ProviderStateKind::Ready,
            pace: None,
            account_organization: None,
            tray_status_label: None,
            fetch_duration_ms: None,
            wayfinder_usage: None,
            quota_burndown: None,
            open_ai_api_usage: None,
            session_equivalent_forecast: None,
        }
    }

    fn fake_snapshot(
        id: &str,
        display: &str,
        used_percent: f64,
    ) -> crate::commands::ProviderUsageSnapshot {
        fake_snapshot_with(id, display, used_percent, None, None, None)
    }

    fn fake_extra_window(percent: f64) -> crate::commands::NamedRateWindowSnapshot {
        crate::commands::NamedRateWindowSnapshot {
            id: "additional_budget".to_string(),
            title: "Additional Budget".to_string(),
            fallback_lane: false,
            icon_fallback: None,
            window: crate::commands::RateWindowSnapshot {
                used_percent: percent,
                remaining_percent: 100.0 - percent,
                window_minutes: None,
                resets_at: None,
                reset_description: None,
                is_exhausted: false,
                is_informational: false,
                reserve_percent: None,
                reserve_description: None,
                reserve_will_last_to_reset: false,
                reserve_eta_seconds: None,
                description_is_detail: false,
                monthly_limit_block: None,
            },
        }
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
    fn tooltip_uses_compact_status_labels() {
        let mut claude = fake_snapshot("claude", "Claude", 13.0);
        claude.primary.reset_description = Some("2h 05m".to_string());
        let mut codex = fake_snapshot("codex", "Codex", 8.0);
        codex.primary.reset_description = Some("4h 10m".to_string());

        let tooltip = build_tooltip(&[claude, codex], codexbar::settings::Language::English);

        assert_eq!(
            tooltip,
            "CodexBar\nClaude: 13% • Resets in 2h 05m\nCodex: 8% • Resets in 4h 10m"
        );
    }

    #[test]
    fn tooltip_skips_informational_codex_primary() {
        // Weekly-only Codex plan: the 5h lane is an informational placeholder,
        // so the tooltip must label the real weekly lane instead of echoing
        // "No active 5h session" back at the user.
        let mut codex = fake_snapshot_with("codex", "Codex", 0.0, Some(16.0), None, None);
        codex.primary.is_informational = true;
        codex.primary.reset_description = Some("No active 5h session".to_string());
        codex.secondary.as_mut().unwrap().reset_description = Some("3d 17h".to_string());

        let tooltip = build_tooltip(&[codex], codexbar::settings::Language::English);

        assert_eq!(tooltip, "CodexBar\nCodex: 16% • Resets in 3d 17h");
    }

    #[test]
    fn tooltip_truncates_long_provider_lines() {
        let mut claude = fake_snapshot("claude", "Claude", 13.0);
        claude.primary.reset_description =
            Some("resets in Jun 10 at 3:00PM with extra noisy suffix".to_string());

        let tooltip = build_tooltip(&[claude], codexbar::settings::Language::English);

        let line = tooltip.lines().nth(1).expect("provider tooltip line");
        assert!(line.starts_with("Claude: 13% • Resets in Jun 10 at 3:00PM"));
        assert!(line.ends_with("..."));
        assert!(line.chars().count() <= 53);
    }

    #[test]
    fn japanese_tooltip_localizes_error_status() {
        let mut claude = fake_snapshot("claude", "Claude", 13.0);
        claude.error = Some("network timeout".to_string());

        let tooltip = build_tooltip(&[claude], codexbar::settings::Language::Japanese);

        assert!(tooltip.contains("エラー"), "{tooltip}");
        assert!(!tooltip.contains(": error ("), "{tooltip}");
    }

    #[test]
    fn tray_labels_relocalize_on_language_change_without_refetch() {
        let mut claude = fake_snapshot("claude", "Claude", 13.0);
        claude.primary.resets_at =
            Some((chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339());

        let english_tooltip =
            build_tooltip(&[claude.clone()], codexbar::settings::Language::English);
        let japanese_tooltip =
            build_tooltip(&[claude.clone()], codexbar::settings::Language::Japanese);

        assert!(english_tooltip.contains("Resets in"), "{english_tooltip}");
        assert!(
            japanese_tooltip.contains("リセットまで"),
            "{japanese_tooltip}"
        );
        assert!(
            !japanese_tooltip.to_ascii_lowercase().contains("resets in"),
            "{japanese_tooltip}"
        );

        let settings = Settings::default();
        let snapshots = vec![claude];
        let rates_cache = None;
        let plan = TrayPresentationPlan::resolve(&settings, &snapshots, rates_cache);
        let english_label = plan.status_labels(codexbar::settings::Language::English)[0]
            .1
            .clone();
        let japanese_label = plan.status_labels(codexbar::settings::Language::Japanese)[0]
            .1
            .clone();
        assert!(english_label.contains("Resets in"), "{english_label}");
        assert!(japanese_label.contains("リセットまで"), "{japanese_label}");
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
    fn selected_tray_percent_tracks_extra_rate_window() {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Copilot, MetricPreference::ExtraUsage);
        let mut snapshot = fake_snapshot("copilot", "Copilot", 20.0);
        snapshot.extra_rate_windows.push(fake_extra_window(42.0));
        let (primary, secondary) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 42.0);
        assert_eq!(secondary, None);
    }
    #[test]
    fn copilot_automatic_tracks_highest_extra_rate_window() {
        let settings = Settings::default();
        let mut snapshot = fake_snapshot("copilot", "Copilot", 20.0);
        snapshot.extra_rate_windows.push(fake_extra_window(42.0));
        let (primary, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 42.0);
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
    fn exhausted_automatic_window_never_renders_as_remaining_progress() {
        let mut settings = Settings {
            show_as_used: false,
            ..Settings::default()
        };
        let mut snapshot = fake_snapshot_with(
            "opencodego",
            "OpenCode Go",
            20.0,
            Some(60.0),
            Some(40.0),
            None,
        );
        snapshot
            .tertiary
            .as_mut()
            .expect("monthly quota")
            .is_exhausted = true;
        let (remaining, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(remaining, 0.0);
        settings.show_as_used = true;
        let (used, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(used, 100.0);
    }
    #[test]
    fn full_automatic_window_without_exhausted_flag_has_zero_remaining_progress() {
        let mut settings = Settings {
            show_as_used: false,
            ..Settings::default()
        };
        let mut snapshot = fake_snapshot_with(
            "opencodego",
            "OpenCode Go",
            20.0,
            Some(60.0),
            Some(100.0),
            None,
        );
        snapshot
            .tertiary
            .as_mut()
            .expect("monthly quota")
            .is_exhausted = false;
        let (remaining, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(remaining, 0.0);
        settings.show_as_used = true;
        let (used, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(used, 100.0);
    }
    #[test]
    fn missing_automatic_window_does_not_look_like_available_remaining_progress() {
        let settings = Settings {
            show_as_used: false,
            ..Settings::default()
        };
        let mut snapshot = fake_snapshot_with("opencodego", "OpenCode Go", 0.0, None, None, None);
        snapshot.primary.is_informational = true;
        let (remaining, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(remaining, 0.0);
    }
    #[test]
    fn selected_tray_percent_falls_back_when_extra_usage_missing() {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Cursor, MetricPreference::ExtraUsage);
        let snapshot = fake_snapshot_with("cursor", "Cursor", 10.0, Some(72.0), None, None);
        let (primary, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 72.0);
    }
    #[test]
    fn single_meaningful_secondary_quota_uses_full_single_meter() {
        let settings = Settings::default();
        let mut snapshot = fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), None, None);
        snapshot.primary.is_informational = true;
        let (primary, secondary) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 42.0);
        assert_eq!(secondary, None);
    }
    #[test]
    fn selected_secondary_quota_is_not_duplicated_when_tertiary_is_meaningful() {
        let settings = Settings::default();
        let mut snapshot =
            fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), Some(30.0), None);
        snapshot.primary.is_informational = true;
        let (primary, secondary) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 42.0);
        assert_eq!(secondary, Some(30.0));
    }
    #[test]
    fn two_meaningful_quotas_keep_two_meter_layout() {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Cursor, MetricPreference::Session);
        let snapshot = fake_snapshot_with("cursor", "Cursor", 15.0, Some(40.0), None, None);
        let (primary, secondary) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 15.0);
        assert_eq!(secondary, Some(40.0));
    }
    #[test]
    fn informational_primary_skips_session_and_automatic_phantom_zero() {
        let mut settings = Settings::default();
        settings.set_provider_metric(ProviderId::Claude, MetricPreference::Session);
        let mut snapshot = fake_snapshot_with("claude", "Claude", 0.0, Some(42.0), None, None);
        snapshot.primary.is_informational = true;
        // Session preference must not paint the synthetic 0% primary;
        // it falls through to Automatic which prefers weekly (42%).
        let (primary, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 42.0);
        assert_ne!(primary, 0.0);
        // Automatic also prefers weekly over informational primary.
        settings.set_provider_metric(ProviderId::Claude, MetricPreference::Automatic);
        let (primary, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 42.0);
    }
    #[test]
    fn claude_automatic_prefers_weekly_when_model_exhausted() {
        let settings = Settings::default();
        let mut snapshot = fake_snapshot_with("claude", "Claude", 40.0, Some(22.0), None, None);
        snapshot.model_specific = Some(crate::commands::RateWindowSnapshot {
            used_percent: 100.0,
            remaining_percent: 0.0,
            window_minutes: Some(10080),
            resets_at: None,
            reset_description: None,
            is_exhausted: true,
            is_informational: false,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
            description_is_detail: false,
            monthly_limit_block: None,
        });
        let (primary, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 22.0);
        // Explicit model override is untouched.
        let mut overridden = settings.clone();
        overridden.set_provider_metric(ProviderId::Claude, MetricPreference::Model);
        let (primary, _) = selected_tray_percents(&snapshot, &overridden);
        assert_eq!(primary, 100.0);
    }
    #[test]
    fn automatic_prefers_exhausted_weekly_over_low_session() {
        let settings = Settings::default();
        let snapshot = fake_snapshot_with("codex", "Codex", 20.0, Some(100.0), None, None);
        let (primary, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 100.0);
        // Explicit session override still wins.
        let mut overridden = settings.clone();
        overridden.set_provider_metric(ProviderId::Codex, MetricPreference::Session);
        let (primary, _) = selected_tray_percents(&snapshot, &overridden);
        assert_eq!(primary, 20.0);
    }
    #[test]
    fn automatic_picks_highest_among_model_and_extra_windows() {
        let settings = Settings::default();
        let mut snapshot =
            fake_snapshot_with("gemini", "Gemini", 10.0, Some(30.0), Some(40.0), None);
        snapshot.model_specific = Some(crate::commands::RateWindowSnapshot {
            used_percent: 55.0,
            remaining_percent: 45.0,
            window_minutes: None,
            resets_at: None,
            reset_description: None,
            is_exhausted: false,
            is_informational: false,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
            description_is_detail: false,
            monthly_limit_block: None,
        });
        snapshot.extra_rate_windows.push(fake_extra_window(90.0));
        let (primary, _) = selected_tray_percents(&snapshot, &settings);
        assert_eq!(primary, 90.0);
    }
    #[test]
    fn f5_headline_prefers_non_informational_primary() {
        let snapshot = fake_snapshot_with("codex", "Codex", 50.0, Some(20.0), Some(30.0), None);
        let headline = codex_lane_headline_window(&snapshot);
        assert!((headline.used_percent - 50.0).abs() < f64::EPSILON);
    }
    #[test]
    fn f5_headline_falls_back_to_secondary_when_primary_informational() {
        let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(25.0), Some(30.0), None);
        snapshot.primary.is_informational = true;
        let headline = codex_lane_headline_window(&snapshot);
        assert!((headline.used_percent - 25.0).abs() < f64::EPSILON);
    }
    #[test]
    fn f5_headline_falls_back_to_tertiary_when_primary_and_secondary_informational() {
        let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(0.0), Some(35.0), None);
        snapshot.primary.is_informational = true;
        snapshot.secondary.as_mut().unwrap().is_informational = true;
        let headline = codex_lane_headline_window(&snapshot);
        assert!((headline.used_percent - 35.0).abs() < f64::EPSILON);
    }
    #[test]
    fn f5_headline_returns_primary_when_all_informational() {
        let mut snapshot = fake_snapshot_with("codex", "Codex", 0.0, Some(0.0), Some(0.0), None);
        snapshot.primary.is_informational = true;
        if let Some(sec) = &mut snapshot.secondary {
            sec.is_informational = true;
        }
        if let Some(ter) = &mut snapshot.tertiary {
            ter.is_informational = true;
        }
        let headline = codex_lane_headline_window(&snapshot);
        // Falls back to primary (the placeholder) when all are informational.
        assert!(headline.is_informational);
    }
}
