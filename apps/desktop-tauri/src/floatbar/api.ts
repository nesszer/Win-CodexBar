/**
 * Tauri command wrappers for the floating-bar window.
 *
 * Kept inside the module so the rest of the desktop shell doesn't see
 * these — the Settings UI uses the regular `update_settings` bridge to
 * mutate float-bar fields, while these commands are only used when we
 * need to nudge the window directly (e.g. from a tray toggle path that
 * lives outside the React tree).
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

export function setFloatBarOrientation(orientation: string): Promise<void> {
  return invoke<void>("set_float_bar_orientation", { orientation });
}

/** Window label used by the floatbar webview. */
export const FLOATBAR_WINDOW_LABEL = "floatbar";

/** Tauri event emitted when float-bar settings change. */
export const FLOAT_BAR_CONFIG_CHANGED_EVENT = "float-bar-config-changed";
