//! Self-contained "Float Bar" feature module.
//!
//! Owns the auxiliary `floatbar` Tauri window, the Tauri commands that
//! mutate it, the settings-patch surface, the startup restore hook, the
//! window-event hook, and the tray-menu glue. The rest of the desktop
//! shell only needs to call into the small public API exported here.

mod commands;
mod window;

pub use commands::*;
pub use window::FLOAT_BAR_CONFIG_CHANGED_EVENT;
pub use window::FLOATBAR_LABEL;

use codexbar::settings::Settings;
use tauri::{Emitter, Manager};

/// Reopen the floating bar on app start if it was enabled previously.
///
/// Called once from `main.rs::setup`. No-op when the setting is off.
pub fn install(app: &tauri::AppHandle) {
    let persisted = Settings::load();
    if persisted.float_bar_enabled {
        let _ = window::show(
            app,
            persisted.float_bar_opacity,
            &persisted.float_bar_orientation,
            &persisted.float_bar_style,
            persisted.float_bar_click_through,
        );
    }
}

/// Handle a `WindowEvent` targeting the floatbar window. Returns `true`
/// when the event was for the floatbar (and was handled), `false`
/// otherwise so the caller can fall through to its own handling.
pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) -> bool {
    if window.label() != FLOATBAR_LABEL {
        return false;
    }
    match event {
        tauri::WindowEvent::Moved(_)
        | tauri::WindowEvent::Resized(_)
        | tauri::WindowEvent::CloseRequested { .. } => {
            window::remember_geometry(window);
        }
        _ => {}
    }
    true
}

/// Toggle the floating bar from the tray menu. Persists the new state
/// and shows or hides the window accordingly.
pub fn toggle(app: &tauri::AppHandle) {
    let mut settings = Settings::load();
    settings.float_bar_enabled = !settings.float_bar_enabled;
    let _ = settings.save();
    if settings.float_bar_enabled {
        let _ = window::show(
            app,
            settings.float_bar_opacity,
            &settings.float_bar_orientation,
            &settings.float_bar_style,
            settings.float_bar_click_through,
        );
    } else {
        let _ = window::hide(app);
    }
}

/// Bring the floating-bar window in line with persisted settings: open,
/// close, or re-apply opacity / click-through as appropriate. Used after
/// a settings patch is saved.
pub fn apply_state(app: &tauri::AppHandle, settings: &Settings) {
    let open = app.get_webview_window(FLOATBAR_LABEL).is_some();
    if settings.float_bar_enabled && !open {
        let _ = window::show(
            app,
            settings.float_bar_opacity,
            &settings.float_bar_orientation,
            &settings.float_bar_style,
            settings.float_bar_click_through,
        );
    } else if !settings.float_bar_enabled && open {
        let _ = window::hide(app);
    } else if let Some(w) = app.get_webview_window(FLOATBAR_LABEL) {
        window::apply_no_activate(&w);
        window::apply_opacity(&w, settings.float_bar_opacity);
        window::apply_click_through(&w, settings.float_bar_click_through);
    }
}

/// All five settings fields the float bar owns, in a single optional
/// patch. Used by `update_settings` so the bulk of float-bar plumbing
/// stays in this module rather than spread across the settings handler.
#[derive(Debug, Default)]
pub struct SettingsPatch {
    pub enabled: Option<bool>,
    pub opacity: Option<u8>,
    pub scale: Option<u8>,
    pub orientation: Option<String>,
    pub style: Option<String>,
    pub click_through: Option<bool>,
    pub provider_ids: Option<Vec<String>>,
    pub dark_text: Option<bool>,
    pub show_reset_inline: Option<bool>,
}

impl SettingsPatch {
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.opacity.is_none()
            && self.scale.is_none()
            && self.orientation.is_none()
            && self.style.is_none()
            && self.click_through.is_none()
            && self.provider_ids.is_none()
            && self.dark_text.is_none()
            && self.show_reset_inline.is_none()
    }

    /// Apply this patch to a mutable `Settings`. Values are clamped and
    /// normalized before assignment to keep the on-disk state safe.
    pub fn apply(&self, settings: &mut Settings) {
        if let Some(v) = self.enabled {
            settings.float_bar_enabled = v;
        }
        if let Some(v) = self.opacity {
            settings.float_bar_opacity = codexbar::settings::clamp_float_bar_opacity(v);
        }
        if let Some(v) = self.scale {
            settings.float_bar_scale = codexbar::settings::clamp_float_bar_scale(v);
        }
        if let Some(v) = &self.orientation {
            settings.float_bar_orientation = codexbar::settings::normalize_float_bar_orientation(v);
        }
        if let Some(v) = &self.style {
            settings.float_bar_style = codexbar::settings::normalize_float_bar_style(v);
        }
        if let Some(v) = self.click_through {
            settings.float_bar_click_through = v;
        }
        if let Some(v) = &self.provider_ids {
            settings.float_bar_provider_ids = v.clone();
        }
        if let Some(v) = self.dark_text {
            settings.float_bar_dark_text = v;
        }
        if let Some(v) = self.show_reset_inline {
            settings.float_bar_show_reset_inline = v;
        }
    }
}

/// React to a saved settings patch: emit the live-config event and bring
/// the window in line. No-op when nothing in the patch was float-bar
/// related.
pub fn after_settings_saved(
    app: &tauri::AppHandle,
    patch: &SettingsPatch,
    settings: &Settings,
    notify_live_config: bool,
) {
    if notify_live_config || !patch.is_empty() {
        notify_settings_changed(app);
    }
    if !patch.is_empty() {
        apply_state(app, settings);
    }
}

pub fn notify_settings_changed(app: &tauri::AppHandle) {
    let _ = app.emit(FLOAT_BAR_CONFIG_CHANGED_EVENT, ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_is_empty_by_default() {
        assert!(SettingsPatch::default().is_empty());
    }

    #[test]
    fn settings_patch_apply_only_writes_present_fields() {
        let mut s = Settings {
            float_bar_enabled: false,
            float_bar_opacity: 80,
            float_bar_scale: 100,
            float_bar_orientation: "horizontal".into(),
            float_bar_style: "floating".into(),
            float_bar_dark_text: false,
            float_bar_show_reset_inline: false,
            ..Settings::default()
        };

        let patch = SettingsPatch {
            enabled: Some(true),
            opacity: Some(45),
            scale: Some(135),
            style: Some("taskbar".into()),
            dark_text: Some(true),
            show_reset_inline: Some(true),
            ..SettingsPatch::default()
        };
        patch.apply(&mut s);
        assert!(s.float_bar_enabled);
        assert_eq!(s.float_bar_opacity, 45);
        assert_eq!(s.float_bar_scale, 135);
        assert_eq!(s.float_bar_style, "taskbar");
        assert!(s.float_bar_dark_text);
        assert!(s.float_bar_show_reset_inline);
        // Orientation untouched by the patch.
        assert_eq!(s.float_bar_orientation, "horizontal");
    }

    #[test]
    fn settings_patch_clamps_and_normalizes_on_apply() {
        let mut s = Settings::default();
        let patch = SettingsPatch {
            opacity: Some(250),
            scale: Some(250),
            orientation: Some("diagonal".into()),
            style: Some("glass".into()),
            ..SettingsPatch::default()
        };
        patch.apply(&mut s);
        assert_eq!(s.float_bar_opacity, 100);
        assert_eq!(s.float_bar_scale, 200);
        assert_eq!(s.float_bar_orientation, "horizontal");
        assert_eq!(s.float_bar_style, "floating");
    }

    #[test]
    fn empty_patch_leaves_settings_unchanged() {
        let original = Settings::default();
        let mut s = Settings::default();
        SettingsPatch::default().apply(&mut s);
        assert_eq!(s.float_bar_enabled, original.float_bar_enabled);
        assert_eq!(s.float_bar_opacity, original.float_bar_opacity);
        assert_eq!(s.float_bar_scale, original.float_bar_scale);
        assert_eq!(s.float_bar_orientation, original.float_bar_orientation);
        assert_eq!(s.float_bar_style, original.float_bar_style);
        assert_eq!(s.float_bar_dark_text, original.float_bar_dark_text);
        assert_eq!(
            s.float_bar_show_reset_inline,
            original.float_bar_show_reset_inline
        );
    }
}
