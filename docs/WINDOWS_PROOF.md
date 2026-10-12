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

A file the app can't read or parse logs a warning, and the app starts without the seed. A normal refresh doesn't replace seeded data while the cache holds every provider the refresh asks for. A forced refresh, such as **Refresh All**, fetches live data.

`CODEXBAR_SEED_CLAUDE_ACCOUNTS_JSON` works the same way for the saved Claude account list. It takes effect only in proof mode. A file the app can't parse logs a warning, and the app uses the real account list.

## Drive the app

Drive the app without taking the user's focus or keyboard. Don't use cua-driver's `bring_to_front`, foreground mode, `hotkey` or `type_text`.

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
