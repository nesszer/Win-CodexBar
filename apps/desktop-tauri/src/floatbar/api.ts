/**
 * Tauri command wrappers for the floating-bar window.
 *
 * Kept inside the module so the rest of the desktop shell doesn't see
 * these — the Settings UI uses the regular `update_settings` bridge to
 * mutate float-bar fields, while these commands drive the native window
 * directly: tray toggle paths outside the React tree, plus `resizeFloatBar`,
 * which lets the floatbar webview hand a size change to the native layer so
 * the resize and the Win32 interaction state are applied together.
 */

import { invoke } from "@tauri-apps/api/core";

export function showFloatBar(): Promise<void> {
  return invoke<void>("show_float_bar");
}

export function hideFloatBar(): Promise<void> {
  return invoke<void>("hide_float_bar");
}

export function setFloatBarOpacity(opacity: number): Promise<void> {
  return invoke<void>("set_float_bar_opacity", { opacity });
}

export function setFloatBarClickThrough(enabled: boolean): Promise<void> {
  return invoke<void>("set_float_bar_click_through", { enabled });
}

export function resizeFloatBar(width: number, height: number): Promise<void> {
  return invoke<void>("resize_float_bar", { width, height });
}

export function setFloatBarOrientation(orientation: string): Promise<void> {
  return invoke<void>("set_float_bar_orientation", { orientation });
}

/** Window label used by the floatbar webview. */
export const FLOATBAR_WINDOW_LABEL = "floatbar";

/** Tauri event emitted when float-bar settings change. */
export const FLOAT_BAR_CONFIG_CHANGED_EVENT = "float-bar-config-changed";
