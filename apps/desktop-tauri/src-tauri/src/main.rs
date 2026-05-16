#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::time::Duration;

mod commands;
mod events;
mod geometry_store;
mod proof_harness;
mod shell;
mod shortcut_bridge;
mod state;
mod surface;
mod surface_target;
mod tray_bridge;
mod tray_menu;
mod window_positioner;

use std::sync::Mutex;

use state::AppState;
use surface::SurfaceMode;
use surface_target::SurfaceTarget;
use tauri::Manager;

fn should_hide_close_request(mode: SurfaceMode) -> bool {
    matches!(
        mode,
        SurfaceMode::TrayPanel | SurfaceMode::PopOut | SurfaceMode::Settings
    )
}

fn main() {
    codexbar::logging::init(false, false).expect("failed to initialize logging");

    let proof_config = proof_harness::ProofConfig::from_env();
    let is_proof_mode = proof_config.is_some();
    let force_start_visible = std::env::var_os("CODEXBAR_START_VISIBLE").is_some();

    let mut initial_state = AppState::new();
    initial_state.proof_config = proof_config;

    tauri::Builder::default()
        .manage(Mutex::new(initial_state))
        .plugin(shortcut_bridge::plugin())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ =
                shell::reopen_to_target(app, SurfaceMode::TrayPanel, SurfaceTarget::Summary, None);
        }))
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap_state,
            commands::get_provider_catalog,
            commands::get_settings_snapshot,
            commands::update_settings,
            commands::set_surface_mode,
            commands::open_settings_window,
            commands::close_settings_window,
            commands::get_current_surface_mode,
            commands::get_current_surface_state,
            commands::get_proof_state,
            commands::run_proof_command,
            commands::refresh_providers,
            commands::refresh_providers_if_stale,
            commands::get_cached_providers,
            commands::get_safe_diagnostics,
            commands::get_credential_storage_status,
            commands::get_update_state,
            commands::check_for_updates,
            commands::download_update,
            commands::apply_update,
            commands::dismiss_update,
            commands::open_release_page,
            commands::get_api_keys,
            commands::get_api_key_providers,
            commands::set_api_key,
            commands::remove_api_key,
            commands::get_manual_cookies,
            commands::set_manual_cookie,
            commands::remove_manual_cookie,
            commands::list_detected_browsers,
            commands::import_browser_cookies,
            commands::get_token_account_providers,
            commands::get_token_accounts,
            commands::add_token_account,
            commands::remove_token_account,
            commands::set_active_token_account,
            commands::get_app_info,
            commands::get_provider_chart_data,
            commands::reorder_providers,
            commands::set_provider_cookie_source,
            commands::get_provider_cookie_source,
            commands::get_provider_cookie_source_options,
            commands::set_provider_region,
            commands::get_provider_region,
            commands::get_provider_region_options,
            commands::get_gemini_cli_signed_in,
            commands::get_vertexai_status,
            commands::list_jetbrains_detected_ides,
            commands::set_jetbrains_ide_path,
            commands::get_kiro_status,
            commands::register_global_shortcut,
            commands::unregister_global_shortcut,
            commands::is_remote_session,
            commands::get_launch_block_reason,
            commands::get_work_area_rect,
            commands::play_notification_sound,
            commands::reanchor_tray_panel,
            commands::quit_app,
            commands::open_provider_dashboard,
            commands::open_provider_status_page,
            commands::get_provider_detail,
            commands::trigger_provider_login,
            commands::revoke_provider_credentials,
            commands::get_locale_strings,
            commands::set_ui_language,
            commands::open_path,
        ])
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                shell::dwm::force_dark_caption(&window);
                window.hide()?;
            }
            tray_bridge::setup(app)?;
            shortcut_bridge::register(app.handle());

            // In proof mode, show the target surface after a brief delay
            // so WebView2 has time to initialize.
            if is_proof_mode {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    proof_harness::activate(&app_handle);
                });
            } else if force_start_visible {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    let _ = shell::reopen_to_target(
                        &app,
                        SurfaceMode::TrayPanel,
                        SurfaceTarget::Summary,
                        None,
                    );
                });
            }

            Ok(())
        })
        .on_window_event(move |window, event| {
            // Only the main window participates in blur-dismiss and close-to-hide.
            // The detached settings window uses normal OS close behavior.
            if window.label() != "main" {
                return;
            }
            match event {
                tauri::WindowEvent::Focused(false) => {
                    // Suppress blur-dismiss in proof mode so the window stays
                    // visible for automated screenshot capture.
                    if force_start_visible || proof_harness::is_proof_mode(window.app_handle()) {
                        return;
                    }
                    // Grace period: ignore blur within 500ms of showing the panel.
                    // On Windows, the tray click can cause a spurious blur before
                    // the window fully acquires focus.
                    if let Some(st) = window.app_handle().try_state::<Mutex<AppState>>()
                        && let Some(shown_at) = st.lock().unwrap().last_shown_at
                        && shown_at.elapsed() < Duration::from_millis(500)
                    {
                        return;
                    }
                    // Blur in TrayPanel mode → auto-hide.
                    let _ = shell::hide_to_tray_if_current(window.app_handle(), |mode| {
                        mode == SurfaceMode::TrayPanel
                    });
                }
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                    // Capture geometry for surfaces eligible for persistence
                    // (currently only Settings). The helper is a no-op when the
                    // current surface is not eligible.
                    shell::remember_current_geometry_if_settings(window);
                }
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // Close visible shell surfaces → hide instead of quitting.
                    if matches!(
                        shell::hide_to_tray_if_current(
                            window.app_handle(),
                            should_hide_close_request
                        ),
                        Ok(Some(_))
                    ) {
                        api.prevent_close();
                    }
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run CodexBar desktop shell");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_request_hides_tray_first_surfaces() {
        assert!(should_hide_close_request(SurfaceMode::TrayPanel));
        assert!(should_hide_close_request(SurfaceMode::PopOut));
        assert!(should_hide_close_request(SurfaceMode::Settings));
    }

    #[test]
    fn close_request_leaves_hidden_surface_alone() {
        assert!(!should_hide_close_request(SurfaceMode::Hidden));
    }
}
