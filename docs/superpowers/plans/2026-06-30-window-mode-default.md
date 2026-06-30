# Window Mode Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Win-CodexBar default to a normal resizable PopOut window and add a 125%-250% main-window display scale setting.

**Architecture:** Reuse the existing Tauri `SurfaceMode::PopOut` as the default main window. Extend persisted settings and bridge types with `windowScalePercent`, extend geometry persistence to PopOut, and apply the scale in the React PopOut surface.

**Tech Stack:** Rust, Tauri v2, React 18, TypeScript, Vitest, Cargo tests.

---

## File Structure

- `rust/src/settings.rs`: canonical `Settings` field, default, and clamp helper for `window_scale_percent`.
- `rust/src/settings/raw.rs`: raw settings load and clamp path for edited JSON.
- `rust/src/settings/tests.rs`: settings default, clamp, raw load, and round-trip tests.
- `apps/desktop-tauri/src-tauri/src/commands/bridge.rs`: bridge snapshot field `window_scale_percent`.
- `apps/desktop-tauri/src-tauri/src/commands/settings.rs`: settings update patch field `window_scale_percent`.
- `apps/desktop-tauri/src/types/bridge.ts`: TypeScript settings contract field `windowScalePercent`.
- `apps/desktop-tauri/src-tauri/src/main.rs`: startup and second-instance routing to PopOut.
- `apps/desktop-tauri/src-tauri/src/tray_bridge.rs`: tray click and native tray menu routing to PopOut.
- `apps/desktop-tauri/src-tauri/src/tray_menu.rs`: user-facing menu label/test expectations for the main window entry.
- `apps/desktop-tauri/src-tauri/src/shortcut_bridge.rs`: global shortcut opens PopOut.
- `apps/desktop-tauri/src-tauri/src/geometry_store.rs`: PopOut becomes an eligible remembered surface.
- `apps/desktop-tauri/src-tauri/src/shell/position.rs`: remembered PopOut position and size influence default placement.
- `apps/desktop-tauri/src-tauri/src/shell/window.rs`: remembered PopOut size overrides default layout size.
- `apps/desktop-tauri/src-tauri/src/shell/tests.rs`: routing/geometry behavior tests.
- `apps/desktop-tauri/src/App.tsx`: frontend fallback shortcut event opens PopOut.
- `apps/desktop-tauri/src/surfaces/PopOutPanel.tsx`: remove forced resize/position reset and apply scale wrapper.
- `apps/desktop-tauri/src/surfaces/PopOutPanel.test.tsx`: PopOut scale and no forced resize tests.
- `apps/desktop-tauri/src/surfaces/settings/tabs/DisplayTab.tsx`: main-window scale slider.
- `apps/desktop-tauri/src/floatbar/SettingsSection.tsx`: optional extraction/reuse of `useDraftNumber` if needed.
- `apps/desktop-tauri/src/styles.css`: PopOut scale wrapper styles if inline `zoom` alone is insufficient.

---

### Task 1: Add Main Window Scale To Settings And Bridge

**Files:**
- Modify: `rust/src/settings.rs`
- Modify: `rust/src/settings/raw.rs`
- Modify: `rust/src/settings/tests.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/commands/bridge.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/commands/settings.rs`
- Modify: `apps/desktop-tauri/src/types/bridge.ts`

- [ ] **Step 1: Write failing Rust settings tests**

Add tests to `rust/src/settings/tests.rs`:

```rust
#[test]
fn main_window_scale_defaults_to_125_percent() {
    let settings = Settings::default();
    assert_eq!(settings.window_scale_percent, 125);
}

#[test]
fn main_window_scale_clamp_pins_to_supported_range() {
    assert_eq!(clamp_window_scale_percent(0), 125);
    assert_eq!(clamp_window_scale_percent(124), 125);
    assert_eq!(clamp_window_scale_percent(125), 125);
    assert_eq!(clamp_window_scale_percent(180), 180);
    assert_eq!(clamp_window_scale_percent(250), 250);
    assert_eq!(clamp_window_scale_percent(251), 250);
}

#[test]
fn raw_settings_clamps_main_window_scale_on_load() {
    let json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "window_scale_percent": 300
        }"#;
    let loaded: Settings = serde_json::from_str(json).expect("parse settings");
    assert_eq!(loaded.window_scale_percent, 250);
}
```

- [ ] **Step 2: Run tests and verify they fail for missing symbols**

Run:

```powershell
cargo test --manifest-path rust/Cargo.toml main_window_scale --lib
```

Expected: compile failure mentioning `window_scale_percent` and `clamp_window_scale_percent`.

- [ ] **Step 3: Implement the settings field and clamp**

In `rust/src/settings.rs`, add the field near other display/UI settings:

```rust
/// Main PopOut window display scale, in the inclusive range 125..=250.
#[serde(default = "default_window_scale_percent")]
pub window_scale_percent: u16,
```

Add helpers:

```rust
fn default_window_scale_percent() -> u16 {
    125
}

pub fn clamp_window_scale_percent(value: u16) -> u16 {
    value.clamp(125, 250)
}
```

Set `window_scale_percent: default_window_scale_percent(),` in `impl Default for Settings`.

In `rust/src/settings/raw.rs`, add `window_scale_percent: u16` with `#[serde(default = "default_window_scale_percent")]` to `RawSettings`, pass the default in `impl Default for RawSettings`, and set:

```rust
window_scale_percent: clamp_window_scale_percent(raw.window_scale_percent),
```

inside `impl From<RawSettings> for Settings`.

- [ ] **Step 4: Run the focused Rust settings tests**

Run:

```powershell
cargo test --manifest-path rust/Cargo.toml main_window_scale --lib
```

Expected: tests pass.

- [ ] **Step 5: Add bridge/update fields**

In `apps/desktop-tauri/src-tauri/src/commands/bridge.rs`, add this field to `SettingsSnapshot`:

```rust
window_scale_percent: u16,
```

and set it in `impl From<Settings> for SettingsSnapshot`:

```rust
window_scale_percent: settings.window_scale_percent,
```

In `apps/desktop-tauri/src-tauri/src/commands/settings.rs`, add this field to `SettingsUpdate`:

```rust
pub window_scale_percent: Option<u16>,
```

and in `apply_display_settings`, add:

```rust
if let Some(v) = self.window_scale_percent {
    settings.window_scale_percent = codexbar::settings::clamp_window_scale_percent(v);
}
```

In `apps/desktop-tauri/src/types/bridge.ts`, add to `SettingsSnapshot`:

```ts
/** 125..=250 - clamped server-side. */
windowScalePercent: number;
```

and to `SettingsUpdate`:

```ts
windowScalePercent?: number;
```

- [ ] **Step 6: Run focused bridge/settings tests**

Run:

```powershell
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml commands:: --lib
```

Expected: command tests pass. If existing snapshot helper tests require new fields, update expected JSON to include `windowScalePercent`.

- [ ] **Step 7: Commit**

```powershell
git add rust/src/settings.rs rust/src/settings/raw.rs rust/src/settings/tests.rs apps/desktop-tauri/src-tauri/src/commands/bridge.rs apps/desktop-tauri/src-tauri/src/commands/settings.rs apps/desktop-tauri/src/types/bridge.ts
git commit -m "Add main window scale setting"
```

---

### Task 2: Route Default Entry Points To PopOut And Remember PopOut Geometry

**Files:**
- Modify: `apps/desktop-tauri/src-tauri/src/main.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/tray_bridge.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/tray_menu.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/shortcut_bridge.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/geometry_store.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/shell/position.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/shell/window.rs`
- Modify: `apps/desktop-tauri/src-tauri/src/shell/tests.rs`

- [ ] **Step 1: Write failing routing and geometry tests**

Update/add tests so they express the new default:

```rust
#[test]
fn plain_desktop_launch_opens_primary_popout_unless_start_minimized() {
    assert_eq!(
        launch_behavior(false, false, std::iter::empty::<&str>()),
        LaunchBehavior {
            open_primary_window_at_start: true,
            suppress_blur_dismiss: false,
        }
    );
    assert_eq!(
        launch_behavior(false, true, std::iter::empty::<&str>()),
        LaunchBehavior {
            open_primary_window_at_start: false,
            suppress_blur_dismiss: false,
        }
    );
}
```

In `tray_menu.rs` tests, expect the main window menu label to be `Show Window` and assert `show_panel` resolves to PopOut Dashboard:

```rust
#[test]
fn show_panel_menu_target_opens_popout_dashboard() {
    let request = resolve_menu_target("show_panel").expect("menu target");
    assert_eq!(request.mode, SurfaceMode::PopOut);
    assert_eq!(request.target, SurfaceTarget::Dashboard);
}
```

In `geometry_store.rs` tests, assert PopOut is remembered:

```rust
#[test]
fn popout_and_settings_are_remembered() {
    assert!(should_remember(SurfaceMode::PopOut));
    assert!(should_remember(SurfaceMode::Settings));
    assert!(!should_remember(SurfaceMode::TrayPanel));
    assert!(!should_remember(SurfaceMode::Hidden));
}
```

- [ ] **Step 2: Run focused tests and verify failure**

Run:

```powershell
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml main::tests tray_menu::tests geometry_store::tests --lib
```

Expected: compile/test failures because code still names and routes tray panel defaults.

- [ ] **Step 3: Route startup and second-instance to PopOut**

In `main.rs`, rename `LaunchBehavior.open_tray_panel_at_start` to `open_primary_window_at_start`.

When the launch opens visibly, call:

```rust
let _ = shell::reopen_to_target(
    &app,
    SurfaceMode::PopOut,
    SurfaceTarget::Dashboard,
    None,
);
```

For the single-instance handler, when plain or tray aliases are used, call the same PopOut reopen target.

Keep `should_open_tray_panel_from_args` accepting existing `menubar`, `traypanel`, and `tray` aliases for backwards compatibility, but route them to the primary PopOut window.

- [ ] **Step 4: Route tray icon and shortcut to PopOut**

In `tray_bridge.rs`, left-click should still store the tray anchor, but then call:

```rust
let _ = shell::reopen_to_target(
    app,
    SurfaceMode::PopOut,
    SurfaceTarget::Dashboard,
    None,
);
```

In `shortcut_bridge.rs`, replace `shell::toggle_tray_panel(app, None);` with the same PopOut reopen call.

- [ ] **Step 5: Route native tray menu main action to PopOut**

In `tray_menu.rs`, change the label:

```rust
menu.push(TrayMenuEntry::item("show_panel", "Show Window"));
```

Make `resolve_menu_target("show_panel")` return:

```rust
Some(shell::ShellTransitionRequest {
    mode: SurfaceMode::PopOut,
    target: SurfaceTarget::Dashboard,
    position: None,
})
```

`pop_out` may continue to route to PopOut Dashboard for compatibility.

- [ ] **Step 6: Persist PopOut geometry**

In `geometry_store.rs`, update `should_remember` to:

```rust
matches!(mode, SurfaceMode::PopOut | SurfaceMode::Settings)
```

Update tests and comments so TrayPanel remains not remembered.

In `shell/position.rs`, replace `remember_current_geometry_if_settings` with a more general helper such as `remember_current_geometry_if_eligible`. It should save when `geometry_store::should_remember(current_mode)` is true. Store position from `outer_position()`. Store size from `outer_size()` converted to logical pixels:

```rust
let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
width: size.map(|s| (s.width as f64 / scale).round() as u32),
height: size.map(|s| (s.height as f64 / scale).round() as u32),
```

Update the window event call site in `main.rs`.

In `shell/window.rs`, when applying visible layout, load stored size for remembered modes. One clean way is to add `geometry_key: Option<&'static str>` to `WindowProperties`, set it from `SurfaceMode::window_properties`, and use `crate::geometry_store::load_entry(key)` to override width/height when both are present.

In `shell/position.rs`, let PopOut use remembered position before tray/current-monitor placement. Clamp it to the current work area using the stored logical width/height when present.

- [ ] **Step 7: Run focused Rust tests**

Run:

```powershell
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml main::tests tray_menu::tests geometry_store::tests shell::tests --lib
```

Expected: focused tests pass.

- [ ] **Step 8: Commit**

```powershell
git add apps/desktop-tauri/src-tauri/src/main.rs apps/desktop-tauri/src-tauri/src/tray_bridge.rs apps/desktop-tauri/src-tauri/src/tray_menu.rs apps/desktop-tauri/src-tauri/src/shortcut_bridge.rs apps/desktop-tauri/src-tauri/src/geometry_store.rs apps/desktop-tauri/src-tauri/src/shell/position.rs apps/desktop-tauri/src-tauri/src/shell/window.rs apps/desktop-tauri/src-tauri/src/shell/tests.rs
git commit -m "Open PopOut as the default window"
```

---

### Task 3: Apply PopOut Scale In React And Add Settings UI

**Files:**
- Modify: `apps/desktop-tauri/src/App.tsx`
- Modify: `apps/desktop-tauri/src/surfaces/PopOutPanel.tsx`
- Modify: `apps/desktop-tauri/src/surfaces/PopOutPanel.test.tsx`
- Modify: `apps/desktop-tauri/src/surfaces/settings/tabs/DisplayTab.tsx`
- Modify: `apps/desktop-tauri/src/types/bridge.ts` if Task 1 did not already update all local test fixtures.
- Modify: `apps/desktop-tauri/src/styles.css` if needed for scale wrapper stability.

- [ ] **Step 1: Write failing PopOut tests**

In `PopOutPanel.test.tsx`, update the settings helper to include:

```ts
windowScalePercent: 125,
```

Add a test helper override if needed:

```ts
function settings(overrides: Partial<SettingsSnapshot> = {}): SettingsSnapshot {
  return {
    // existing fields...
    windowScalePercent: 125,
    // existing fields...
    ...overrides,
  };
}
```

Add tests:

```ts
it("applies the persisted main-window display scale to the popout surface", async () => {
  const { container } = renderPopOut(
    [provider("codex", "Codex", 80)],
    undefined,
    [],
    { windowScalePercent: 150 },
  );

  await waitFor(() => {
    expect(container.querySelector(".menu-stack__item")).not.toBeNull();
  });

  const shell = container.querySelector<HTMLElement>(".popout-scale-shell");
  expect(shell).not.toBeNull();
  expect(shell?.style.getPropertyValue("--window-scale")).toBe("1.5");
});

it("does not force native resize or reposition on mount", async () => {
  renderPopOut([provider("codex", "Codex", 80)]);

  await waitFor(() => {
    expect(windowMocks.getCurrentWindow).not.toHaveBeenCalled();
  });
});
```

Adapt `renderPopOut` to accept a settings override and pass it through `bootstrap`.

- [ ] **Step 2: Run PopOut tests and verify failure**

Run:

```powershell
pnpm --dir apps/desktop-tauri test src/surfaces/PopOutPanel.test.tsx
```

Expected: test fails because PopOut still calls `getCurrentWindow` and no scale shell exists.

- [ ] **Step 3: Remove forced PopOut native sizing**

In `PopOutPanel.tsx`, delete the imports and effect that call:

```ts
getCurrentWindow()
new LogicalSize(...)
new LogicalPosition(...)
win.setSize(...)
win.setPosition(...)
```

Rust is responsible for default/restored window geometry.

- [ ] **Step 4: Apply scale wrapper**

In `PopOutPanel.tsx`, compute:

```ts
const windowScale = Math.min(250, Math.max(125, settings.windowScalePercent)) / 100;
const scaleStyle = {
  "--window-scale": String(windowScale),
  zoom: windowScale,
} as React.CSSProperties & { zoom: number };
```

Wrap every `MenuSurface variant="popout"` return in:

```tsx
<div className="popout-scale-shell" style={scaleStyle}>
  <MenuSurface ...>
    ...
  </MenuSurface>
</div>
```

If TypeScript objects complain about `zoom`, keep the custom intersection type.

In `styles.css`, add:

```css
.popout-scale-shell {
  width: 100%;
  min-height: 100vh;
  height: 100vh;
  overflow: hidden;
}
```

- [ ] **Step 5: Add Display tab slider**

In `DisplayTab.tsx`, add a section before `FloatBarSettingsSection`:

```tsx
<section className="settings-section">
  <h3 className="settings-section__title">Window</h3>
  <div className="settings-section__group">
    <Field
      label={`Main window size (${settings.windowScalePercent}%)`}
      description="Scales the dashboard window contents. Window size itself remains freely resizable."
    >
      <input
        type="range"
        min={125}
        max={250}
        step={5}
        value={settings.windowScalePercent}
        disabled={saving}
        onChange={(e) =>
          set({ windowScalePercent: Number(e.target.value) })
        }
        aria-label="Main window display size"
      />
    </Field>
  </div>
</section>
```

This can commit on change directly, matching simple existing settings controls. Do not reuse `floatBarScale`; it is a separate setting.

- [ ] **Step 6: Update App shortcut fallback**

In `App.tsx`, change the `global-shortcut-triggered` listener from:

```ts
void setSurfaceMode("trayPanel", { kind: "summary" }).catch(() => {});
```

to:

```ts
void setSurfaceMode("popOut", { kind: "dashboard" }).catch(() => {});
```

- [ ] **Step 7: Run frontend focused tests and typecheck**

Run:

```powershell
pnpm --dir apps/desktop-tauri test src/surfaces/PopOutPanel.test.tsx
pnpm --dir apps/desktop-tauri build
```

Expected: focused test and build pass.

- [ ] **Step 8: Commit**

```powershell
git add apps/desktop-tauri/src/App.tsx apps/desktop-tauri/src/surfaces/PopOutPanel.tsx apps/desktop-tauri/src/surfaces/PopOutPanel.test.tsx apps/desktop-tauri/src/surfaces/settings/tabs/DisplayTab.tsx apps/desktop-tauri/src/styles.css apps/desktop-tauri/src/types/bridge.ts
git commit -m "Scale the default PopOut window"
```

---

### Task 4: Final Verification And Cleanup

**Files:**
- Modify only files needed for fixes found by verification.

- [ ] **Step 1: Run formatters**

Run:

```powershell
cargo fmt --all
```

Expected: completes successfully.

- [ ] **Step 2: Run Rust tests**

Run:

```powershell
cargo test --manifest-path rust/Cargo.toml --lib
cargo test --manifest-path apps/desktop-tauri/src-tauri/Cargo.toml --lib
```

Expected: both pass.

- [ ] **Step 3: Run frontend tests and build**

Run:

```powershell
pnpm --dir apps/desktop-tauri test
pnpm --dir apps/desktop-tauri build
```

Expected: tests and build pass.

- [ ] **Step 4: Inspect diff for scope**

Run:

```powershell
git diff --stat HEAD~3..HEAD
git diff HEAD~3..HEAD -- apps/desktop-tauri/src-tauri/src apps/desktop-tauri/src rust/src/settings.rs rust/src/settings docs/superpowers
```

Expected: only files related to default window mode, geometry persistence, settings bridge, scale UI, tests, and docs changed.

- [ ] **Step 5: Commit any verification fixes**

If format or verification changed files:

```powershell
git add <changed-files>
git commit -m "Verify window mode defaults"
```

If no files changed, do not create an empty commit.
