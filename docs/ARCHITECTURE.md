# Architecture

This page shows how Win-CodexBar is put together: the crates, the windows, and how usage data and settings move between them. It covers the Windows port only. The upstream `docs/architecture.md` describes Swift modules, WidgetKit and Sparkle, which this port doesn't use.

## Crates and folders

| Area | Path | Role |
| --- | --- | --- |
| Shared backend and CLI | `rust/` (crate `codexbar`) | Providers, settings, browser cookies, tray icon pixels, the CLI |
| Desktop shell | `apps/desktop-tauri/src-tauri/` (crate `codexbar-desktop-tauri`) | Tauri 2 host: tray, windows, commands, float bar, proof harness |
| Frontend | `apps/desktop-tauri/src/` | React 18 and Vite: tray panel, Settings, float bar |

The root `Cargo.toml` is a workspace with the members `rust` and `apps/desktop-tauri/src-tauri`. The Tauri crate is the default member. It depends on `codexbar = { path = "../../../rust" }`.

## Entry points

- **Desktop app:** `apps/desktop-tauri/src-tauri/src/main.rs`. It loads settings, reads the proof harness variables, sets up the tray and float bar, starts auto refresh and registers the commands. Cargo builds it as `codexbar-desktop-tauri.exe`.
- **CLI:** `rust/src/main.rs`. It has no GUI. Run with no subcommand, it prints a hint and exits with code 64. Cargo builds it as `codexbar.exe`.
- **Frontend:** `apps/desktop-tauri/src/App.tsx` picks the surface from the window label.

Release builds rename both binaries. See [BUILDING.md](./BUILDING.md#build-a-release-on-windows).

## Windows

The app runs one hidden window and opens the others on demand.

| Label | Page | What it shows |
| --- | --- | --- |
| `main` | `index.html` | Nothing. It stays hidden and hosts app state. Proof mode `trayPanel` draws the tray panel here. |
| `flyout` | `index.html?window=flyout` | The tray panel |
| `settings` | `index.html?window=settings&tab=<tab>` | Settings |
| `floatbar` | `index.html?window=floatbar&orientation=<orientation>` | The float bar |

### Tray panel

The tray panel is the only dashboard layout. Tray left-click, **Pop Out Dashboard** in the tray menu, the global shortcut, app launch and a second launch of the app all open the `flyout` window. `shell/flyout_window.rs` owns it.

- It is a light panel in the Mac style. It is 310 px wide, times the **Panel scale** setting, and its height fits its content.
- It paints `#DEDEE2` before the page loads, so it never flashes white.
- It opens next to the tray and can't be resized. With **Settings > Menu > Movable panel** on, a strip along its top drags it elsewhere; `shell/flyout_placement.rs` records the spot when the native move loop ends (so the panel's own re-anchoring never counts as a move), later opens and content-driven resizes keep the panel there, clamped into the work area it overlaps most, and double-clicking the strip puts it back next to the tray.
- It closes when it loses focus. Proof mode, `CODEXBAR_START_VISIBLE` and dragging a provider tab to reorder keep it open, as does a mouse button held on a movable panel (a native move blurs the WebView before the drag begins). A movable panel the user moved away from the tray stays open like a window: it can be covered, and closes from the tray icon, Escape, or by being put back next to the tray.

The panel shows one card per provider. A card has metric rows for each usage window, with a bar, the reset time and, when the provider supports it, a pace line. [PROVIDERS.md](./PROVIDERS.md) lists the windows and pace support per provider.

The older PopOut layout on `main` is retired. `SurfaceMode::PopOut` remains only as a key in stored data.

### Settings

Settings is a separate window with a sidebar. `shell/settings_window.rs` owns it. The pane ids are `general`, `providers`, `notifications`, `menuBar`, `menu`, `usageSpend`, `advanced`, `hooks` and `about`. `surface_target.rs` in the shell and `TAB_META` in `apps/desktop-tauri/src/surfaces/settings/settingsTabs.ts` must list the same ids. An unknown id opens General. [CONFIGURATION.md](./CONFIGURATION.md) describes each pane.

### Float bar

The float bar is an optional always-on-top strip. `floatbar/` owns it. Its window builder must pin `.theme(Some(tauri::Theme::Dark))`. WebView2 resolves `prefers-color-scheme` on a process profile that all windows share, so an unpinned float bar flips the other windows when the theme is `auto`.

## Data flow

### Provider refresh

1. `codexbar::core::instantiate_provider` in `rust/src/core/provider_factory.rs` builds each provider. It is the only provider factory.
2. `commands/providers.rs` in the shell calls `Provider::fetch_usage`. It runs at most 8 fetches at once.
3. Each fetch has a timeout: 75 seconds for Claude, Codex and Copilot, and 35 seconds for the others. LiteLLM gets 40 seconds when optional details are on. A longer web timeout in settings (plus 5 seconds) can raise the 35-second and 40-second timeouts, but never above 65 seconds. It doesn't change the 75-second timeout for Claude, Codex and Copilot.
4. Results go into `AppState.provider_cache`.
5. The shell sends `provider-updated` for each provider and `refresh-complete` at the end. The React hook `useProviders` listens and redraws.

A full provider refresh also starts a status poll (`status_poller.rs`) while `status_checks_enabled` is on (the auto-resume probe does not). The poller fetches the status page of each enabled provider that has one, keeps the results in `AppState.provider_statuses`, and sends `provider-statuses-updated` with the full list when a status changes. The React hook `useProviderStatuses` reads it, and the tray tooltip adds a line per incident. See [PROVIDERS.md](./PROVIDERS.md#status-pages).

`auto_refresh.rs` starts refreshes on a timer. The adaptive mode also checks for running coding agents (`coding_activity.rs`). Without a running agent process, a Codex rollout modified recently counts as coding activity: `coding_activity.rs` reads the modification times of today's and yesterday's `rollout-*.jsonl` files under the Codex sessions directory. It reads no file contents, caps the entries it inspects, and clamps future mtimes to the first value seen. The signal is skipped under Low Power Mode.

### Settings

1. The desktop shell's `Settings::load` reads `%APPDATA%\CodexBar\settings.json`. Other stores, such as `api_keys.json`, `manual_cookies.json` and `hooks.json`, sit in the same folder. `settings.json`, `api_keys.json` and `manual_cookies.json` are written through `secure_file`, which wraps them with DPAPI on Windows. `hooks.json` is plain JSON and always stays at its default path. The CLI's `CODEXBAR_CONFIG` variable moves the settings, API key, manual cookie and token account files, but not `hooks.json`. See [CONFIGURATION.md](./CONFIGURATION.md#files-and-where-they-live).
2. The frontend calls `updateSettings` with a patch.
3. The shell saves the file and sends `settings-changed`. In the page, the DOM event `codexbar:settings-updated` tells other parts of the UI to reload. The float bar has its own config events.

The Hooks pane reads and writes `hooks.json` through the commands `get_hooks_config` and `save_hooks_config` (`commands/hooks.rs`). Hooks run only when both `hooks_enabled` in settings and `enabled` in `hooks.json` are on.

### Events

The shell sends these Tauri events (`events.rs`): `surface-mode-changed`, `provider-updated`, `refresh-started`, `refresh-complete`, `update-state-changed`, `locale-changed`, `settings-changed`, `stay-awake-changed`, `codex-accounts-updated`, `login-phase` and `provider-statuses-updated`.

### Tray icon

- `tray_bridge.rs` owns the icon and `tray_menu.rs` builds the right-click menu.
- `tray_presentation.rs` decides what the icon shows. The pixels come from `codexbar::tray` in `rust/src/tray/render.rs`.
- With **Color Pace Indicator** on (setting `menu_bar_color_pace`, in Settings > Menu Bar), a single-provider bar or percent icon turns green when usage is behind pace and red when it is ahead. On track or unknown pace keeps the normal usage color. Stacked icons keep the usage color.
- The pace for the tint is scored on one window. A provider can name that window with `Provider::menu_bar_pace_lane()`. When it returns `None`, the app uses the window the icon draws for the selected metric.

### Pace

Pace compares how much of a window you have used with how much time of it has passed. `commands/bridge/pace.rs` scores it into seven stages, from `far_behind` to `far_ahead`.

Session and weekly windows get pace from their length and reset time. Providers that refill on a billing date opt in to reset-window pace: each declares a `PACE_CAPABILITY` in its own module, and `pace_capability()` in `provider_factory.rs` maps every `ProviderId` to it or to `ProviderPaceCapability::UNSUPPORTED`. The match is exhaustive, so a new provider must state its pace support to compile. [PROVIDERS.md](./PROVIDERS.md#pace) lists the rules.

### CLI

The CLI uses the same provider factory and the same settings stores as the app. Use it for scripts without a UI, for example `usage`, `cost`, `guard` and `serve`. See [CLI.md](./CLI.md).

## Proof harness

`proof_harness.rs` reads `CODEXBAR_PROOF_MODE` and `CODEXBAR_SEED_USAGE_JSON` at startup. Proof mode opens one surface and keeps it open for automation. The seed fills the provider cache with synthetic data. See [WINDOWS_PROOF.md](./WINDOWS_PROOF.md).

## Platform notes

- Rust edition 2024. The CLI and the shell use Tokio.
- Windows-only parts: DPAPI cookie decryption, the dark DWM caption, tray icon promotion, start at login under `HKCU\...\Run`, and WebView2.
- Test tray, DPAPI and cookie behavior on Windows. WSL can't decrypt DPAPI data. See [WSL.md](./WSL.md).

## Related docs

- [BUILDING.md](./BUILDING.md): build, test and release.
- [CLI.md](./CLI.md): the command-line tool.
- [CONFIGURATION.md](./CONFIGURATION.md): settings files and the Settings window.
- [PROVIDERS.md](./PROVIDERS.md): providers, sign-in sources, windows and pace.
- [COOKIES.md](./COOKIES.md): browser cookie import.
- [WINDOWS_PROOF.md](./WINDOWS_PROOF.md): proving UI changes.
- [AGENTS.md](../AGENTS.md): rules for agents and contributors.
