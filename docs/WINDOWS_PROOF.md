# Proving UI changes on Windows

Unit tests and `scripts/local-check.ps1` don't prove tray, Settings, float bar, theme or WebView2 behavior. For a change to any of these, rebuild the app on Windows, drive the real window, and attach the proof to the PR. This page shows how. The rule itself is in [AGENTS.md](../AGENTS.md#ui-validation-with-cua-trycuacua).

## The proof loop

1. **Rebuild.** Build a desktop binary that includes the change:

   ```powershell
   pnpm --dir apps/desktop-tauri run tauri:build:debug
   ```

   Or run `.\scripts\dev.ps1`. A binary built before the change proves nothing.
2. **Close CodexBar.** The app uses the single-instance plugin, so a new launch can hand off to an old process that is still running.
3. **Launch in proof mode.** Set `CODEXBAR_PROOF_MODE` before you start the new binary. See [Proof mode](#proof-mode).
4. **Drive the window in the background.** Use CDP for webview controls and cua-driver for native checks. See [Drive the app](#drive-the-app).
5. **Check what you can see**, not only exit codes. For example: pixels, the window list, a toggle's checked state, the persisted setting, the theme under `auto`, or whether the float bar window exists.
6. **Attach the proof** to the PR: CDP assertions and CUA screenshots. List what the proof doesn't cover, such as tray icon or tray menu pixels. If CUA can't run, say why and attach equivalent manual proof, as the PR template asks.

## Proof mode

`CODEXBAR_PROOF_MODE` opens one surface at startup and keeps it open for automation. In proof mode the app:

- doesn't close the surface when it loses focus;
- shows each surface without activating it, so it never takes focus from the app you are using.

| Value | What opens |
| --- | --- |
| `trayPanel` | The tray panel on the `main` window, at a fixed position above the taskbar |
| `popOut` | The tray panel flyout window, the same window a tray left-click opens |
| `hidden` | Nothing |
| `settings` | Settings on the General pane |
| `settings:<tab>` | Settings on one pane, for example `settings:menuBar` |
| `settings:providers:<cli_name>` | Settings on one provider, for example `settings:providers:codex` |

The tab ids are `general`, `providers`, `notifications`, `menuBar`, `menu`, `usageSpend`, `advanced`, `hooks` and `about`. They are case-sensitive. The provider part must be the exact `cli_name`.

The app logs a warning and starts normally when the value is wrong. That includes an unknown surface, an unknown tab, a payload on `trayPanel`, `hidden` or `popOut` (for example `popOut:codex`), and `floatbar`, which is not a proof surface.

```powershell
$env:CODEXBAR_PROOF_MODE = 'settings:menu'
```

### Seed usage data

`CODEXBAR_SEED_USAGE_JSON` points at a JSON file with synthetic usage. The app loads it into the provider cache before the first window reads it, so a proof run doesn't need real accounts.

```powershell
$env:CODEXBAR_SEED_USAGE_JSON = 'C:\proof\seed.json'
```

The file holds one of two shapes:

- **One Codex snapshot.** A single JSON object in the bridge `ProviderUsageSnapshot` shape. Its `providerId` must be `codex`. This works with or without proof mode.
- **A list of snapshots.** A non-empty JSON array of snapshots. This works only when `CODEXBAR_PROOF_MODE` holds a valid value. Each `providerId` must be an exact bridge id (the `cli_name`), and each provider may appear once. The array is all or nothing: one bad entry skips the whole file.

The app fills fields that the file leaves out:

- `remainingPercent` from `usedPercent`;
- `formattedUsed` from `used`;
- `updatedAt` from the current time;
- the pace for any window that has `resetsAt` but no `pace`, with the same math and work-days setting as a live refresh.

A file the app can't read or parse logs a warning, and the app starts without the seed.

Seeded data is pinned per provider. A normal refresh skips every seeded provider and fetches only the enabled providers that have no seed, so seeding `codex` alone with `codex` and `claude` enabled keeps the Codex seed and refreshes Claude. A forced refresh, such as **Refresh All**, fetches live data for every provider and drops their pins.

The tray icon and tooltip show the seeded data from launch, even when every enabled provider is seeded and the startup refresh is skipped.

`CODEXBAR_SEED_CLAUDE_ACCOUNTS_JSON` works the same way for the saved Claude account list. It takes effect only in proof mode. A file the app can't parse logs a warning, and the app uses the real account list.

### Place proof windows

`CODEXBAR_PROOF_WINDOW_ORIGIN` sets where proof windows open, as `<x>,<y>` in physical pixels on the virtual desktop. Negative values are fine for monitors left of or above the primary one.

```powershell
$env:CODEXBAR_PROOF_WINDOW_ORIGIN = '-3600,-1188'
```

When it is set in proof mode:

- the tray panel flyout, Settings, the float bar and the `trayPanel` proof surface open with their top-left corner at that point, instead of next to the tray or centered on the primary monitor;
- the window goes on the monitor whose bounds contain the point and is moved, if needed, so it stays at least 8 px inside that monitor's work area;
- the flyout stays there when its content resizes;
- the float bar treats the point as its placement, so it isn't moved back, and the app doesn't save it as your float bar position.

The app ignores the variable outside proof mode. It logs a warning and uses normal placement when the value isn't two whole numbers separated by a comma, or when no monitor contains the point.

### Read and activate the tray menu

A right-click on the tray icon takes focus, so a proof run can't open the tray menu the normal way. Two proof-only hooks cover it.

`CODEXBAR_PROOF_TRAY_MENU_DUMP` names an absolute path. In proof mode the app writes the live tray context menu there as JSON after startup and after every menu rebuild (provider toggles, status updates after a refresh, the float bar toggle, Stay Awake). Each write replaces the file in one step, so a reader sees a whole document. Outside proof mode, or when the variable is unset, nothing is written.

```powershell
$env:CODEXBAR_PROOF_TRAY_MENU_DUMP = 'C:\proof\tray-menu.json'
```

The document holds `generation` (1 for the first write in a process, then +1 per write), `timestamp` (UTC, RFC 3339) and `items`. Each node has `id`, `text`, `kind`, `enabled`, `checked` and `children`:

- `kind` is `item`, `check`, `submenu`, `separator` or `icon`.
- `checked` is `true` or `false` for `check` rows and `null` otherwise.
- `id` is `null` for separators.
- `text` is the label as built, so Hide Personal Info masking shows here as it does on screen.

```json
{
  "generation": 2,
  "timestamp": "2026-05-01T12:00:00.123456789+00:00",
  "items": [
    { "id": "status_claude", "text": "Claude 60% session", "kind": "item", "enabled": false, "checked": null, "children": [] },
    { "id": null, "text": "", "kind": "separator", "enabled": true, "checked": null, "children": [] },
    { "id": "refresh", "text": "Refresh all", "kind": "item", "enabled": true, "checked": null, "children": [] },
    { "id": "toggle_float_bar", "text": "Show Float Bar", "kind": "check", "enabled": true, "checked": false, "children": [] },
    { "id": "providers", "text": "Providers", "kind": "submenu", "enabled": true, "checked": null, "children": [
      { "id": "toggle_provider:codex", "text": "Codex", "kind": "check", "enabled": true, "checked": true, "children": [] }
    ] }
  ]
}
```

`proof_activate_tray_item` runs the handler a native click on a row would run. Call it through the Tauri bridge from a CDP session attached to a CodexBar page target (found by URL, never the first target):

```js
await window.__TAURI_INTERNALS__.invoke('proof_activate_tray_item', { id: 'toggle_float_bar' });
```

- It fails with an error unless proof mode is active and `id` is an enabled row of the menu now on the tray that maps to a menu action. Status rows (`status_*`), disabled rows, unknown ids and rows the current menu doesn't show (such as `toggle_provider:<id>` for a provider missing from the menu) are refused. The error doesn't echo the id.
- The check and the handler run on the main thread, after any menu install queued before the call. Nothing on screen opens or takes focus unless the action itself opens a window.
- Some actions write to the running profile. A provider toggle saves `settings.json`, and Quit exits the app. Use an isolated `APPDATA` home for proof runs.
- Rows that change a setting show the new state once the menu is rebuilt. The call returns when the handler's synchronous part is done, and some rebuilds and all account actions run after that, so the dump can still show the old state right after the call. Poll for a higher `generation` before you read the state.

### Report a fixed system theme

`CODEXBAR_PROOF_SYSTEM_THEME` makes the app report a fixed Windows app theme to `get_system_theme`. The `auto` theme in Settings then follows that value, so both directions can be proven without changing the real OS setting.

```powershell
$env:CODEXBAR_PROOF_SYSTEM_THEME = 'light'   # or 'dark'
```

It takes effect only in proof mode and accepts `light` or `dark`, case-insensitive. Outside proof mode the app logs a warning and reads the OS setting. An unset or blank value, or any other value, is ignored, and the app reads the OS setting. The override changes only what the app reports. It does not change the Windows setting, so the DWM caption, scrollbars and `<select>` popups keep their pinned dark theme.

## Drive the app

Drive the app without taking the user's focus or keyboard. Don't use cua-driver's `bring_to_front`, foreground mode, `hotkey` or `type_text`.

The one exception is the global-shortcut checks in the [checklist](#checklist) (`runtime.shortcut-capture-records` and `runtime.shortcut-toggles-panel`). They test a system-wide key binding, so they need a real key press on a Windows desktop. Run them by hand, and note in the PR that they were a manual check. Don't use `hotkey` or `type_text` for them.

### CDP for webview controls

Set a remote debugging port before you launch the app. Use one port for each app you run in parallel.

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=<port>'
```

Read `http://127.0.0.1:<port>/json/list` and attach to the window's own page target. Pick the target by URL:

| Window | Label | Page URL |
| --- | --- | --- |
| Hidden main window (and `trayPanel` proof) | `main` | `http://tauri.localhost/` |
| Tray panel flyout | `flyout` | `http://tauri.localhost/index.html?window=flyout` |
| Settings | `settings` | `http://tauri.localhost/index.html?window=settings&tab=<tab>` |
| Float bar | `floatbar` | `http://tauri.localhost/index.html?window=floatbar&orientation=<orientation>` |

Don't attach to the first target in the list by default. Tools that do that, or that open `about:blank` (for example browser-use), see a blank page.

Click with `Input.dispatchMouseEvent`. It doesn't move the cursor or take focus. Check the DOM state, and check the saved value through the app bridge command `get_settings_snapshot`.

### cua-driver for native checks

[cua-driver](https://github.com/trycua/cua) from trycua/cua does background computer use on Windows: window lists, UI Automation, clicks and real screenshots. See the [docs](https://cua.ai/docs), the [install guide](https://cua.ai/docs/how-to-guides/driver/install) and the [CLI reference](https://cua.ai/docs/reference/cua-driver/cli-reference). Install it in PowerShell:

```powershell
irm https://cua.ai/driver/install.ps1 | iex
```

Then follow the steps the installer prints. The binary is usually `%LOCALAPPDATA%\Programs\Cua\cua-driver\bin\cua-driver.exe`.

- `cua-driver serve` runs a long-lived daemon. `cua-driver call <tool> '<json>'` runs one tool, for example `list_windows`, `get_window_state` or `click`.
- In cua-driver 0.31 and later, actions take an `element_token` from `get_window_state`. `element_index` and `snapshot_id` are rejected.
- A pixel click needs a screenshot from the same session first.
- Pass the same `session` label on the state read and on every action that uses its tokens, or keep one `serve` connection. Separate one-shot `call`s without a session retire the snapshot, and its tokens go stale.

Use CUA in every proof for what CDP can't see:

- `verify_state` (with `timeout_ms` and `stable_samples`) for window existence and bounds;
- real-pixel screenshots through `get_window_state` with `--screenshot-out-file`;
- native facts, such as a dark DWM frame under theme `auto`, the float bar being topmost and layered, and the window frame.

Every native window stays DWM dark under every theme preference, including `light`, because each one is pinned Dark to prevent the theme flip. Only the page's `data-theme` turns light. So a light Settings run that reports `dwm_dark: true` is expected. It is a bug only if dark native chrome shows up in the screenshot.

CDP screenshots are not proof.

## What to check after an install

Install the build, start **CodexBar**, and walk through this list.

1. **Tray icon.** The icon appears in the notification area. On first run you may need to turn it on under taskbar settings.
2. **Left-click the tray icon.** The tray panel flyout opens near the tray, on the monitor that holds the taskbar.
3. **Right-click the tray icon.** The native menu opens. From top to bottom it has the account submenus (Codex, Claude Code and Grok), the provider status rows, **Refresh All**, **Pop Out Dashboard**, **Show Float Bar**, **Providers**, **Settings...**, **Check for Updates**, **About CodexBar** and **Quit**.
4. **Settings.** Settings opens from the tray menu and from the gear in the tray panel. Every sidebar pane opens.
5. **Theme.** In General, switch the theme between Automatic, Light and Dark. The open windows repaint. Automatic follows the Windows color mode.
6. **Global shortcut.** In General > Keyboard, record a shortcut, for example `Ctrl+Shift+K`. The shortcut opens and closes the tray panel from any app.
7. **Provider order.** Reorder providers in Settings, then restart the app. The tray panel and Settings keep the new order.
8. **Reset countdown.** A card shows "Resets in" with a time. The text updates within a minute; the timer ticks every 30 seconds.
9. **Updates.** In About, click **Check for Updates**. When an update exists, a banner appears at the top of the tray panel with buttons to download, view the release and dismiss.

## Build and release checks

Run these from the repo root. [BUILDING.md](./BUILDING.md) explains each one.

```powershell
.\scripts\local-check.ps1 -Slice ci
pnpm --dir apps/desktop-tauri run tauri:build
.\scripts\windows-release-build.ps1 -Ref vX.Y.Z -SmokeInstall
.\scripts\release-doctor.ps1 -Version X.Y.Z
```

`tauri:build` makes no installer. The release builder makes the Inno Setup installer, the portable binary and the CLI zip.

## Known limits

- On Linux hosts, the Tauri work-area helper returns the whole monitor, because it doesn't exclude panels. On Windows, `SystemParametersInfoW(SPI_GETWORKAREA)` gives the correct work area.
- DPAPI cookie import, the single-instance lock and the installer-based update work only on Windows.
- On a multi-user Windows server, the tray icon may not appear after install. The host policy `HKLM\SYSTEM\CurrentControlSet\Control\Terminal Server\fSingleSessionPerUser` can cause this. Setting it to `0` on the host fixes it. The installer doesn't change it, and this repo must not ship a registry change for it.
- `pnpm test` runs unit tests in jsdom. It doesn't drive a real window.

## Checklist

```
[ ] build.local-check-ci          .\scripts\local-check.ps1 -Slice ci
[ ] build.tauri-build             pnpm --dir apps/desktop-tauri run tauri:build
[ ] release.cached-build          .\scripts\windows-release-build.ps1 -Ref <tag> -SmokeInstall
[ ] release.doctor                .\scripts\release-doctor.ps1 -Version <version>

[ ] runtime.tray-icon-visible
[ ] runtime.tray-left-click-opens-panel
[ ] runtime.tray-context-menu
[ ] runtime.settings-opens
[ ] runtime.settings-all-panes
[ ] runtime.theme-light
[ ] runtime.theme-dark
[ ] runtime.theme-auto-tracks-os
[ ] runtime.shortcut-capture-records
[ ] runtime.shortcut-toggles-panel
[ ] runtime.provider-reorder
[ ] runtime.provider-order-persists
[ ] runtime.reset-countdown-ticks
[ ] runtime.update-banner-flow
[ ] runtime.single-instance
```

---

## 5. Mocked-IPC proof build

`apps/desktop-tauri/proof.html` runs the real React surfaces in any browser.
The Tauri bridge is replaced by `mockIPC` / `mockWindows` from
`@tauri-apps/api/mocks`. Commands are answered by synthetic fixtures in
`src/proof/fixtures/` (typed against `types/bridge.ts`). The desktop bundle
(`index.html` → `src/main.tsx`) never imports `src/proof`.

```powershell
cd apps/desktop-tauri
pnpm run dev:proof      # http://127.0.0.1:1420/proof.html
pnpm run build:proof    # static output in dist-proof/
```

URL parameters:

| Param | Values | Default |
| --- | --- | --- |
| `window` | `main`, `settings`, `flyout`, `floatbar` | `main` |
| `tab` | any settings tab id, or `providers:<id>` | `general` |
| `fixture` | `default`, `two-codex-accounts`, `errors`, `empty`, `claude-plan-hidden`, `kpi-long-titles`, `codex-conversations`, `codex-conversations-private` | `default` |
| `theme` | `dark`, `light`, `auto` (overrides the fixture) | fixture value |
| `system` | `dark`, `light`: the Windows app theme that `get_system_theme` reports, which Settings follows under `auto` | `dark` |

Example: `proof.html?window=settings&tab=providers:codex&fixture=two-codex-accounts`.

`codex-conversations` adds three synthetic Codex conversations to the Usage &
Spend contract, for the Codex card's Conversations list (scenario
`tray-codex-conversations`). `codex-conversations-private` serves the same
data with Hide personal information on, so every title is the masked
`Session <id>` label (scenario `tray-codex-conversations-private`).

A scenario may also set the fixture field `localeOverrides`, a map that
replaces single English locale keys for that fixture only. It stands in for a
long translation, so layout checks such as KPI-title wrapping run without
shipping another locale (scenario `tray-kpi-long-titles`).

Every command the frontend invokes either has a fixture handler or is listed
in `INERT_ACTIONS` (`src/proof/router.ts`): user actions such as sign-ins,
credential writes, and OS side effects that return `null`.
`src/proof/coverage.test.ts` fails when a new `invoke("...")` has neither, so
the mocks cannot drift silently. An unknown command still returns `null` and
logs `[proof-mock] unhandled command: <name>` once. Window, webview, dialog,
and event plugin calls are inert.

### Automated check: `pnpm proof:check`

`scripts/proof-check.mjs` runs the scenarios in `scripts/proof-scenarios.json`
in Chrome, Edge, or Chromium over CDP and prints `PASS` / `FAIL` per scenario.
It uses Node built-ins only.

```powershell
cd apps/desktop-tauri
pnpm proof:check                        # starts the proof server, headless browser
pnpm proof:check -- --headed            # visible window, pauses on each screenshot
pnpm proof:check -- --only tray         # scenarios whose name contains "tray"
pnpm proof:check -- --url <base>        # an already-served proof build (dev or dist-proof/)
pnpm proof:check -- --cdp <ws-url>      # attach to a running browser on this host
pnpm proof:check -- --cdp <ws-url> --url <base>  # remote browser; <base> must be reachable from it
```

Without `--url`, the runner serves the proof build on `127.0.0.1`, which a
browser on another host cannot reach.

With `--cdp`, the runner drives whatever browser it is given, and its isolated
`.proof/browser-profile/` applies only to browsers it launches itself. Point
`--cdp` at a browser started with its own empty `--user-data-dir`, never one
with real logins. CDP has no authentication, so keep a remote browser's
debugging port bound to loopback and reach it through an SSH tunnel or an
authenticated bridge, not an open port.

Screenshots go to `apps/desktop-tauri/.proof/screens/` (git-ignored). The
browser runs on its own profile in `.proof/browser-profile/`, never a user
profile. Exit code `0` means every scenario passed, `1` means one failed, and
`2` means setup failed.

A scenario names the URL query and the window size, then runs its steps in
order. Every scenario also fails on a console error or warning, an uncaught
exception, or an unhandled backend command.

| Step | Checks |
| --- | --- |
| `expectText` / `expectNoText` | page text (string or list), polled up to 8 s |
| `expectSelector` | an element exists |
| `expectNoOverflow` | a selector's matches fit their boxes horizontally, so no text is clipped (wrapped text passes, `nowrap` + ellipsis fails) |
| `expectTheme` | `<html data-theme>` |
| `expectStyle` | `{ selector, property, value }`: the first match's computed style, polled |
| `click` | `{ selector, label, near? }`: a real mouse click on the element whose aria-label or text equals `label`; `near` picks the match closest to that text |
| `expectCall` | `{ cmd, args? }`: the frontend invoked that backend command (args are a subset match) |
| `screenshot` | saves `<name>.png`; `"clip": "panel"` crops a tray panel to the height it committed for its window |

Add or extend a scenario for the UI a PR changes.

Screenshots from this build are labelled **"Chromium (Linux), mocked IPC"**
(or the browser actually used). They support layout, copy, rendered data, and
provider-siloing review; they do not replace the CUA proof for tray, theme,
float-bar, DWM, or WebView2 behaviour (see AGENTS.md).
