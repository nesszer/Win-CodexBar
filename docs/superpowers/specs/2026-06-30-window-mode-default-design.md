# Window Mode Default Design

## Goal

Make Win-CodexBar open as a normal resizable Windows window by default, and add a persistent 125%-250% display scale control for that main window.

## Scope

This change targets the active Tauri desktop shell in `apps/desktop-tauri` and shared settings in `rust/src/settings`. The existing tray panel remains in the codebase for compatibility and proof flows, but the normal user-facing entry points should open the `PopOut` dashboard surface.

## Behavior

- Plain app launch opens `SurfaceMode::PopOut` with `SurfaceTarget::Dashboard` unless `startMinimized` is enabled.
- Tray icon left-click opens or focuses the PopOut dashboard instead of the anchored tray panel.
- The global shortcut opens or focuses the PopOut dashboard.
- A second instance launch opens or focuses the PopOut dashboard.
- Native tray menu items that previously represented "show the panel" should open the normal window. Existing provider deep links remain PopOut provider targets.
- The PopOut window uses native decorations, remains resizable, appears in the taskbar, and is not auto-dismissed on blur.
- Closing the PopOut window hides it to tray, matching the existing close-to-hide behavior.
- User-moved PopOut window position and user-resized PopOut size are persisted and restored on reopen.

## Display Scale

- Add a persisted `window_scale_percent` setting.
- Default: `125`.
- Supported range: inclusive `125..=250`.
- Values loaded from edited settings files are clamped into range.
- The settings bridge exposes this as `windowScalePercent`.
- The Display settings tab includes a slider labeled for the main window display size.
- The scale applies only to the main PopOut surface. It must not change Settings windows or the Float Bar, which already has a separate `floatBarScale`.
- The PopOut React surface applies the scale using a Chromium/WebView-compatible CSS zoom/style path, and tests assert that the persisted setting reaches the rendered surface.

## Architecture

Reuse the existing `PopOut` surface instead of adding a new surface mode. This keeps the change small: Rust routes default shell entry points to `SurfaceMode::PopOut`, geometry persistence is extended to PopOut, and React adds a scale wrapper around `PopOutPanel`.

Settings remain owned by the shared Rust `Settings` type. The bridge snapshot/update structs carry the new value to React. The frontend uses existing `useSettings` and settings-update patterns.

## Testing

- Rust settings tests cover default, clamp helper, and raw settings load clamp.
- Rust shell/menu tests cover default entry point routing to PopOut.
- Rust geometry tests cover PopOut eligibility for persistence.
- React tests cover PopOut scale propagation and the removal of forced PopOut resize-on-mount.
- Final verification should run focused Rust tests, focused Vitest tests, TypeScript build, and the desktop Tauri Rust crate tests when feasible.
