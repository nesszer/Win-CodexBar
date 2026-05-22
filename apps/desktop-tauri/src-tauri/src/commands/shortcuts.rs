// ── Global shortcut capture (user-driven, emits events) ───────────────

#[tauri::command]
pub fn register_global_shortcut(app: tauri::AppHandle, accelerator: String) -> Result<(), String> {
    use tauri::Emitter;
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let shortcut = crate::shortcut_bridge::parse_shortcut(&accelerator)
        .ok_or_else(|| format!("Invalid shortcut \"{accelerator}\". Use e.g. Ctrl+Shift+U."))?;

    // Best-effort cleanup of any prior capture registration.
    let _ = app.global_shortcut().unregister(shortcut);

    let accel_emit = accelerator.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _sc, event| {
            if event.state == ShortcutState::Pressed {
                let _ = app.emit("global-shortcut-triggered", accel_emit.clone());
            }
        })
        .map_err(|e| format!("Failed to register shortcut \"{accelerator}\": {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn unregister_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // We don't know which accelerator was registered — unregister_all is a
    // too-wide hammer (it would also drop the persistent tray-toggle binding),
    // so re-register that afterwards.
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("Failed to clear shortcuts: {e}"))?;
    crate::shortcut_bridge::register(&app);
    Ok(())
}
