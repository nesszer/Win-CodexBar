# Configuration

This page tells you where Win-CodexBar keeps its settings, how to change them from the Settings window or the CLI, and what each Settings pane controls. It covers the Windows port. The upstream paths `~/.config/codexbar/config.json` and `~/.codexbar/config.json` and the macOS Keychain don't apply here.

## Change a setting

Use the Settings window for day-to-day changes. Open it from the tray menu with **Settings...**. Changes save at once.

Use the CLI for scripts. It reads and writes the same files as the app:

```powershell
codexbar config path
codexbar config providers
codexbar config providers --json --pretty
codexbar config enable -p grok
codexbar config disable -p cursor
codexbar config validate
codexbar config dump
```

To store an API key, pipe it on stdin:

```powershell
$env:OPENROUTER_API_KEY | codexbar config set-api-key -p openrouter --stdin
```

- `enable` and `disable` are saved.
- `codexbar usage -p <provider>` queries one provider for that run only. It doesn't change what is enabled.
- If every provider is disabled, `usage` has nothing to show. Pass `-p` to query a provider anyway.

[CLI.md](./CLI.md#manage-settings-from-the-cli) lists every `config` subcommand.

## Files and where they live

The app keeps its files in `%APPDATA%\CodexBar` (`dirs::config_dir()` plus `CodexBar`). Print the settings path with `codexbar config path`.

| File | Folder | DPAPI | Holds |
| --- | --- | --- | --- |
| `settings.json` | `%APPDATA%\CodexBar` | Yes | Everything in the Settings window |
| `api_keys.json` | `%APPDATA%\CodexBar` | Yes | API keys entered in the app |
| `manual_cookies.json` | `%APPDATA%\CodexBar` | Yes | Cookie headers pasted by hand |
| `token-accounts.json` | `%APPDATA%\CodexBar` | Yes | Token accounts |
| `claude-accounts\accounts.json` | `%APPDATA%\CodexBar` | Yes | Saved Claude Code logins |
| `grok-accounts\accounts.json` | `%APPDATA%\CodexBar` | Yes | Saved Grok logins |
| `codex-accounts\accounts.json` | `%APPDATA%\CodexBar` | Yes | Codex accounts |
| `codex-accounts\snapshots.json` | `%APPDATA%\CodexBar` | No | Codex account usage snapshots |
| `provider-account-usage-v1.json` | `%APPDATA%\CodexBar` | Yes | Last successful usage per Claude token account: rate windows and update time only, no credentials or identity |
| `claude-swap-usage-v1.json` | `%APPDATA%\CodexBar` | Yes | Last usage read per claude-swap or tcr pool account: pool slot, the account label Settings shows (the email, or the email with its organization name when two accounts share one), rate windows and update time; no tokens or organization IDs |
| `history\quota-burndown-v1.json` | `%APPDATA%\CodexBar` | Yes | Quota history |
| `claude\quota-reset-observations-v1.json`, `codex\quota-reset-observations-v1.json` | `%APPDATA%\CodexBar` | Yes | Observed reset times |
| `hooks.json` | `%APPDATA%\CodexBar` | No | Hook rules |
| `http_server.json` | `%APPDATA%\CodexBar` | Yes | Built-in HTTP server settings and its bearer token |
| `cli-usage-cache.json` | `%APPDATA%\CodexBar` | No | Usage limits shared with the CLI (only when **Share usage with the CLI** is on) |
| `<provider>-cookie.json` | `%LOCALAPPDATA%\CodexBar` | Yes | Cached browser cookies |
| `codex-weekly-reset-v1.json` | `%LOCALAPPDATA%\CodexBar` | Yes | Codex weekly reset cache |

- Files marked DPAPI are written through `secure_file` in `rust/src/settings/`. On Windows it wraps them with DPAPI for the current user every time it saves.
- `settings.json` is always wrapped. The app still reads a plain JSON file you edited by hand, but the next save wraps it again. If the file exists but can't be read or parsed (for example DPAPI can't decrypt it), the app runs on the defaults but never overwrites the file: every save fails and logs a warning until the file loads again. Repair or move the file to save settings again. A missing file is a first run and saves normally.
- Browser cookies are read from the browser profiles when a refresh needs them. See [COOKIES.md](./COOKIES.md).
- `token-accounts.json` falls back to `~/.codexbar/` only when Windows reports no config folder.
- A Claude token account can store an optional `claude_config_directory` (set with **Choose folder…** under Settings > Providers > Claude > Token accounts). While that account is active, local Claude cost estimates scan only `<folder>\projects`; the default `CLAUDE_CONFIG_DIR` / `%USERPROFILE%\.claude` history, claude-swap homes and Pi history are left out. A missing folder leaves the estimate unavailable. Clearing the folder restores the combined history. Quota fetching and credentials are unaffected.
- **Edit** on a saved token account (Settings > Providers > provider > Token accounts) changes its label and, optionally, its credential. The credential field starts empty and masked. Leave it blank to keep the saved credential; the saved secret is never sent back to the UI. **Up** and **Down** reorder the list. Editing or moving an account keeps the same account ID, timestamps and metadata, and never changes which account is active.
- Don't commit a real `settings.json` or key file to the repo.

A CLI process can use another settings file through `CODEXBAR_CONFIG`. The API key, manual cookie and token account files then sit beside that file. The desktop app ignores the variable. See [CLI.md](./CLI.md#separate-config-file-codexbar_config).

## Windows update helper

Installing an update runs a background Windows PowerShell inline command. It waits for CodexBar to exit, runs the verified installer, and reopens the desktop app after a successful install. The helper doesn't change the PowerShell execution policy you inherit, so it doesn't need unsigned script files to be allowed.

Removing that policy override is a hardening step. It doesn't prove that a Defender detection is a false positive or that a release build is safe. To check a detected file, compare its build provenance and hashes. For malware or false-positive analysis, use [Microsoft's file submission portal](https://www.microsoft.com/en-us/wdsi/filesubmission).

## Settings window

The Settings window has a sidebar. The top part lists the panes in this order: General, Usage & Spend, Notifications, Menu Bar, Menu, Advanced, Hooks and About. Below them is a **Providers** section with one row per provider and a count of how many are on.

### Settings tab ids

Code, links and proof mode open a pane by its id. The ids are case-sensitive:

`general`, `providers`, `notifications`, `menuBar`, `menu`, `usageSpend`, `advanced`, `hooks`, `about`

- `surface_target.rs` in the shell and `TAB_META` in `apps/desktop-tauri/src/surfaces/settings/settingsTabs.ts` must list the same ids.
- An unknown id opens General.
- The old ids `display`, `apiKeys` and `cookies` aren't tabs any more. Their controls moved to other panes and to the provider pages.

To open a pane for automation, set proof mode before you launch the app:

```powershell
$env:CODEXBAR_PROOF_MODE = 'settings:menu'
```

See [WINDOWS_PROOF.md](./WINDOWS_PROOF.md#proof-mode).

### Provider list in the sidebar

- **Search providers** filters by name or id. Esc clears it.
- The sort button turns on alphabetical order (`providers_sorted_alphabetically`). Enabled providers stay first. It doesn't change your saved order.
- Reorder providers by dragging, with Alt+Up and Alt+Down, or from the right-click menu (**Move Up**, **Move Down**). Reordering only works while the list isn't sorted and the search box is empty. The order is saved in `provider_order`.
- The right-click menu can also enable or disable a provider.
- Each enabled provider has a status dot:

| Dot | Meaning |
| --- | --- |
| Green | Up to date |
| Amber | Stale: the last update is more than 10 minutes old |
| Red | The last refresh failed |
| Blue, pulsing | Loading: no data yet |

Disabled providers are dimmed and have no dot.

While **Check provider status** (General) is on, a provider that has a status page shows a service dot instead: green, yellow, orange or red by incident level, gray for maintenance or unknown. See [PROVIDERS.md](./PROVIDERS.md#status-pages).

## What each pane controls

Each row gives the label, the `settings.json` key and the default for a new install.

### General

| Control | Key | Default |
| --- | --- | --- |
| Language | `ui_language`, `ui_language_follows_system` | System |
| Preferred currency | `preferred_currency_code` | `AUTO` |
| Appearance | `theme` (`auto`, `light`, `dark`) | `auto` |
| Start at login | `start_at_login` | off |
| Start minimized | `start_minimized` | off |
| Refresh interval | `refresh_interval_secs`, `adaptive_refresh` | 300 seconds, adaptive on |
| Refresh when the menu opens | `refresh_all_providers_on_menu_open` | off |
| Low power mode | `low_power_mode_preference` (`off`, `on`, `automatic`) | `off` |
| Check provider status | `status_checks_enabled` | on |
| Open menu (global shortcut) | `global_shortcut` | `Ctrl+Shift+U` |

General also has the **Portable preferences** group with **Export Preferences...**, **Import Preferences...** and **Provider Switcher Shortcuts...**. See [Portable preferences](#portable-preferences) and [Provider switcher keys](#provider-switcher-keys).

**Language.** The first option, **System**, follows the Windows display language. New installs use it. A settings file saved before this option existed keeps the language it had. At load, the app reads these HKCU values in order and uses the first language it supports:

1. `Control Panel\Desktop` `PreferredUILanguages`
2. `Control Panel\International\User Profile` `Languages`
3. `Control Panel\Desktop\MuiCached` `MachinePreferredUILanguages`

If none match, it uses English. The app ships 10 languages: English, Simplified Chinese, Traditional Chinese, Japanese, Korean, Spanish, Brazilian Portuguese, Russian, Turkish and Ukrainian.

**Refresh interval.** The options are Manual (0), 1, 2, 5, 15, 30 and 60 minutes, and Adaptive. Adaptive sets `adaptive_refresh` to true. Any other choice sets it to false. A settings file without `adaptive_refresh` gets false.

**Adaptive activity.** While Adaptive is on, a running coding agent process, or a Codex rollout (`rollout-*.jsonl`) modified in the last 5 minutes, caps the delay at 5 minutes. Rollouts are read from today's and yesterday's `YYYY/MM/DD` folders under the Codex sessions directory (`CODEX_HOME\sessions`, or `%USERPROFILE%\.codex\sessions`). Only modification times are read, never contents. The rollout signal is skipped under Low Power Mode. Turning Adaptive off stops all of this.

**Start at login** writes the value `CodexBar` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, pointing at the desktop app. The CLI has `codexbar autostart` for the same job.

### Usage & Spend

| Control | Key | Default |
| --- | --- | --- |
| Track costs | `cost_usage_enabled` | on |
| Period | `spend_dashboard_period` (`rolling:7`, `rolling:30`, `rolling:90`, `month-to-date`, `all`) | copies `cost_reporting_period` |
| Hidden sources | `spend_dashboard_hidden_source_ids` | none |
| OpenCodex usage logs | `open_codex_usage_logs_enabled` | off |
| Hide native Codex cost when OpenCodex is present | `hide_native_codex_cost_when_open_codex_present` | off |

When **Track costs** is off, the pane hides the period picker, the dashboard, OpenCodex and the source list, and shows "Cost tracking is off". **Refresh**, **Copy JSON**, **Save JSON** and **Share PNG** stay visible but are disabled.

Costs are list-price estimates, not billing receipts.

**Sessions view.** The Sessions view lists the Codex conversations of the selected period. Search matches the shown title or the session id; under Hide personal information titles are already replaced, so hidden titles can't be searched. Sort by recent activity, tokens or cost, in either direction; sessions with no known value sort last. Selecting a session reads its model requests from its own rollout files only, grouped by turn, with usage no turn owns listed last as "Unattributed usage". The read is bounded: it stops at 5,000 requests or 256 MiB of rollout data and then shows "Request details exceed the limit." It also checks the result against the session's summary; if the logs changed since the summary was built, the details are not shown and the pane asks you to refresh usage and try again. Tool activity has no cost and is shown separately.
### Notifications

| Control | Key | Default |
| --- | --- | --- |
| Show notifications | `show_notifications` | on |
| Session quota | `session_quota_notifications_enabled` | on |
| Credential expiry | `credential_expiry_notifications_enabled` | off |
| Predictive pace | `predictive_pace_warning_enabled` | off |
| Warn when the run-out is within | `predictive_pace_warning_lead_hours` (0 = any time, up to 168) | 0 |
| Warning at % used | `high_usage_threshold` | 70 |
| Critical at % used | `critical_usage_threshold` | 90 |
| Sound | `sound_enabled` | on |
| Sound theme | `notification_sound_theme` (`windows`, `codexBar`) | `windows` |
| Sound files per event | `notification_sound_paths` | none |

- Credential expiry and Predictive pace are disabled while **Show notifications** is off.
- The two thresholds stay editable when notifications are off, because they also color the bars and the float bar. They range from 0 to 100 in steps of 5.
- The sound controls show only while sound is on. Each of these events can have its own WAV file: `predictiveWarning`, `highUsage`, `criticalUsage`, `exhausted`, `statusIssue`, `sessionDepleted` and `sessionRestored`.
- A provider can override the thresholds. See [Quota warnings per provider](#quota-warnings-per-provider).

### Menu Bar

These controls set the tray icon.

| Control | Key | Default |
| --- | --- | --- |
| Icon style | `menu_bar_icon_style` (`bars`, `percent`) | `bars` |
| Color Pace Indicator | `menu_bar_color_pace` | off |
| Pin to Taskbar (Windows 11) | `promote_tray_icon` | on |
| Combined icon style | `tray_icon_mode` (`single`, `stacked`) | `single` |
| Stacked top and bottom provider | `stacked_tray_top_provider`, `stacked_tray_bottom_provider` | Automatic |
| Show every provider in the tray menu | `tray_icon_mode` = `perprovider` | off |
| Switcher rows | `switcher_shows_icons` | icons |
| Show most-used provider | `menu_bar_shows_highest_usage` | off |
| Overview providers | `merged_overview_selected_providers` | first 6 enabled |
| Show all accounts in Overview | `merged_overview_all_accounts` | off |

- **Color Pace Indicator** tints a single-provider icon green when usage is behind pace and red when it is ahead. Stacked icons keep the usage color. [ARCHITECTURE.md](./ARCHITECTURE.md#tray-icon) explains how the window is picked.
- Icon style, **Show every provider** and **Show most-used provider** are disabled while the icon is stacked.
- The Overview can hold up to 6 providers.
- **Show all accounts in Overview** expands Codex into one row per cached Codex account, and Claude into one row per saved Claude token account when two or more are saved. Rows use only usage already cached on this machine and never start a fetch. Rows do not take provider slots in the 6-provider limit. A token account's usage is cached when its refresh succeeds while it is the active account, and the cache lives in `provider-account-usage-v1.json`. A saved account that has never been the active one shows `No usage data` until it is. Removing a token account, or clearing a provider's credentials, drops its cached usage. Token edits do not emit an event, so the Claude rows pick up renamed or new accounts on the next refresh. claude-swap and tcr pool accounts get one row each when the pool lists two or more. Their rows use the usage last read from the pool (`claude-swap-usage-v1.json`), which fills whenever the Claude settings pane reads the pool, when the setting is turned on, and during full provider refreshes (at most once every 5 minutes) while the setting is on and Claude is enabled. The read runs the same read-only `cswap --list --json` or `tcr list --json` listing as the settings pane. A row shows `No usage data` until a read reports usage for that account. tcr accounts stay read-only. Managed Claude logins stay one card. Local-log charts and conversations are not shown on account rows. With Hide personal information on, rows read `Account 1`, `Account 2` and so on. Claude token-account rows number accounts by their sorted id, as the Claude accounts list does. claude-swap and tcr pool rows use the pool's slot number instead.
- In `settings.json`, `tray_icon_mode` is lowercase: `single`, `perprovider` or `stacked`.

### Menu

These controls set the tray panel and the float bar.

| Control | Key | Default |
| --- | --- | --- |
| Bars show used instead of remaining | `show_as_used` | off |
| Show quota warning markers | `quota_warning_markers_visible` | on |
| Show pace | `show_pace` | on |
| Work days | `weekly_progress_work_days` (Automatic, 4, 5, 6 or 7) | Automatic |
| Workday ticks | `workday_tick_style` (`hidden`, `subtle`, `high_contrast`) | `subtle` |
| Reset times as countdown | `reset_time_relative` | on |
| Keep the tray panel on top | `tray_panel_always_on_top` | off |
| Show reset time when exhausted | `show_reset_when_exhausted` | off |
| Panel scale | `tray_scale_percent` (100 to 200, step 5) | 100 |
| Overview layout | `overview_layout` (`detailed`, `compact`) | `detailed` |
| Show all token accounts | `show_all_token_accounts_in_menu` | off |
| Stay awake | `stay_awake_enabled` | off |
| Agent sessions | `agent_sessions_enabled` | off |
| Local sessions only | `agent_sessions_local_only` (skips Tailscale and SSH discovery) | off |
| SSH hosts for agent sessions | `agent_session_ssh_hosts` (comma-separated) | none |
| Cost summary | `cost_summary_display_style` (`compact`, `detailed`, `hidden`) | `compact` |
| Cost reporting period | `cost_reporting_period` | `rolling:30` |

Workday ticks are disabled while Work days is Automatic. SSH hosts are disabled while agent sessions are off or local-only is on.

With **Local sessions only** on, the desktop app's agent sessions never run `tailscale status` and never open SSH connections. The `codexbar sessions` CLI is not affected by this setting (see [CLI.md](CLI.md)). The setting is re-read from `settings.json` immediately before every Tailscale or SSH spawn, so a discovery that began before the toggle starts no new remote process after it. Results from remote processes that finished before the toggle are dropped too. A process that was already running when the toggle landed can run until its own timeout (5 seconds at most), but its output is discarded. The tray panel's session list reloads when the setting changes while the panel is open, and on its next open otherwise, so remote rows already shown are dropped then. Remote discovery stays on by default, so existing installs behave as before until this is turned on.

Tailscale discovery runs only a standalone `tailscale.exe` found on `PATH`. The GUI client `tailscale-ipn.exe`, `.cmd`/`.bat` launchers, and symlinks are never executed. If no standalone CLI is found, Tailscale discovery is skipped without an error row.

The float bar section is on this pane too. Every control except the first is disabled while the float bar is off.

| Key | Values | Default |
| --- | --- | --- |
| `float_bar_enabled` | on, off | off |
| `float_bar_orientation` | `horizontal`, `vertical` | `horizontal` |
| `float_bar_style` | `floating`, `taskbar` | `floating` |
| `float_bar_opacity` | 30 to 100 | 80 |
| `float_bar_scale` | 75 to 200 | 100 |
| `float_bar_show_cost` | on, off | off |
| `float_bar_show_reset_inline` | on, off | off |
| `float_bar_dark_text` (Invert Colors) | on, off | off |
| `float_bar_click_through` | on, off | off |

### Advanced

| Control | Key | Default |
| --- | --- | --- |
| Hide personal information | `hide_personal_info` | off |
| Disable Credential Manager access | `disable_keychain_access` | off |
| Codex local log folders | `codex_custom_sessions_dirs` (`;`-separated) | none |
| PowerToys status pipe | `powertoys_status_pipe_enabled` | off |
| Share usage with the CLI | `share_usage_with_cli` | off |
| Network proxy | `http_proxy_enabled`, `http_proxy_url`, `http_proxy_username`, `http_proxy_password` | off |

The proxy password is stored in `settings.json`, which is DPAPI-wrapped. Advanced also has a button that copies diagnostics.

**HTTP server** runs the same server as `codexbar serve` inside the desktop app. It is off by default. Its settings live in `http_server.json`, not in `settings.json`, so they stay on this PC and are not part of portable preferences.

- Fields: bind address (default `127.0.0.1`; `localhost` means `127.0.0.1`), port (8080), bearer token, cache interval (60 s), request timeout (30 s, 0 disables), dashboard identity (redacted or full), and, off loopback, an "accept unencrypted HTTP" switch.
- A token is required. **Generate** fills in 32 random bytes as hex. Requests without `Authorization: Bearer <token>` get `401`.
- **Apply** validates the fields, saves them and restarts the server. Invalid fields show an error and are neither saved nor started; an invalid apply also stops a running server (closing its port) until a valid apply restarts it. If the saved file is corrupt, the server stays stopped and Apply asks for explicit confirmation before replacing the unreadable file.
- The server starts with the app when enabled and stops when the app quits. The status row shows Stopped, Running (with the dashboard URL), an invalid-field error, a start failure such as a port already in use, or an unexpected exit.
- Proof mode (`CODEXBAR_PROOF_MODE`) never starts the server.

**Share usage with the CLI** is in the Local integrations group. It is a machine-local setting and is not part of portable preferences. When it is on, the app writes `cli-usage-cache.json` after each refresh with the rate windows of every provider that last refreshed without an error. The file holds only limit windows, labels and the update time: no emails, plans, costs, credits or credentials. It is plain JSON written atomically, because `codexbar usage` must read it in sessions where DPAPI can't decrypt `settings.json`. Providers refreshed through a token account are left out. Turning the setting off removes the file. See [CLI.md](./CLI.md#app-usage-cache-fallback) for when the CLI uses it.

**Hide personal information** is in the Privacy group. When it is on:

- Email addresses are removed from the tray panel, the tray tooltip and provider error text. No placeholder is shown.
- The account line on each tray panel card is hidden.
- Account lists in the tray menu and in Settings show "Account 1", "Account 2" and so on. For Codex this covers every account, ambient and managed. The number comes from the sorted account id, not the list position.
- Settings > Providers hides the email and organization, and shows an OpenAI API project id as "Project: ••••".
- Usage & Spend and cost history show Codex project names as "Workspace", drop session titles and clear paths.
- Hook events that the desktop app sends leave out the account identity. `codexbar hooks watch` does not apply this setting yet.

### Hooks

The Hooks pane edits `hooks.json`. See [Hooks](#hooks).

### About

| Control | Key | Default |
| --- | --- | --- |
| Download updates automatically | `auto_download_updates` | off |
| Update channel | `update_channel` (`stable`, `beta`) | `stable` |

About also has **Check for Updates** and links.

### Providers

Each provider page has, from top to bottom:

1. Identity, with a notice when the last fetch failed.
2. Usage: usage, cost, details, charts and quick actions.
3. Which usage rows to show.
4. The metric the tray icon uses (`provider_metrics`).
5. Connection: usage source, cookie source, region, API key, cookie and token accounts, as the provider supports them.
6. Accounts, for Codex, Claude and Grok only.
7. Accent color.
8. Quota warnings.
9. Options. For Claude this includes **Allow reading Claude Code's credentials** (`claude_allow_reading_claude_code_credentials`, off).

## Quota warnings per provider

Each provider page has a **Quota warnings** group with a **Session** row and a **Weekly** row. Each row has three modes:

| Mode | Effect | Stored as |
| --- | --- | --- |
| Global | Uses the Notifications thresholds | nothing |
| Custom | Uses its own **Warning** and **Critical** values. A blank value uses the global one. | `high`, `critical` |
| Off | No threshold notifications and no bar markers for that window | `enabled: false` |

- The values live under `provider_configs.<id>.quota_warnings` as `{session, weekly}`. Each is `{enabled, high, critical}` in percent used, from 0 to 100.
- Off keeps the saved values, so switching back to Custom restores them.
- Threshold hooks still fire when a window is Off.
- The group is locked when both **Show notifications** and **Show quota warning markers** are off.
- The older `provider_usage_thresholds` key is folded into `quota_warnings` once at startup. A window that already has a non-global setting is left alone. If that save fails, the app retries on the next launch.

## Portable preferences

Portable preferences copy your display and behavior choices to another machine. They never include secrets or machine-specific paths.

From the app, use **Settings > General > Portable preferences**. **Import Preferences...** applies the file at once. From the CLI:

```powershell
codexbar config preferences export --file prefs.json
codexbar config preferences import --file prefs.json
```

Leave out `--file` on export to print to stdout. The CLI import only writes `settings.json`, so restart a running CodexBar afterwards.

The file is plain JSON, at most 256 KiB, in this shape:

```json
{ "version": 1, "preferences": { "theme": "dark", "show_pace": true } }
```

These keys are exported (`ALLOWED_PREFERENCES` in `rust/src/settings/preferences_document.rs`):

| Group | Keys |
| --- | --- |
| Refresh | `refresh_interval_secs`, `adaptive_refresh`, `refresh_all_providers_on_menu_open`, `status_checks_enabled`, `low_power_mode_preference` |
| Notifications | `show_notifications`, `session_quota_notifications_enabled`, `sound_enabled`, `high_usage_threshold`, `critical_usage_threshold`, `predictive_pace_warning_enabled`, `predictive_pace_warning_lead_hours` |
| Display | `show_pace`, `show_as_used`, `reset_time_relative`, `show_reset_when_exhausted`, `hide_personal_info`, `menu_bar_shows_highest_usage`, `menu_bar_shows_percent`, `menu_bar_icon_style`, `merged_overview_selected_providers`, `merged_overview_all_accounts`, `quota_warning_markers_visible`, `workday_tick_style`, `tray_icon_mode`, `switcher_shows_icons`, `providers_sorted_alphabetically`, `overview_layout`, `menu_bar_display_mode` |
| Providers | `provider_order`, `enabled_providers`, `provider_metrics`, `provider_quota_warnings` |
| Look | `theme`, `ui_language`, `ui_language_follows_system`, `window_scale_percent`, `tray_scale_percent` |
| Float bar | `float_bar_opacity`, `float_bar_scale`, `float_bar_orientation`, `float_bar_style`, `float_bar_dark_text`, `float_bar_show_reset_inline`, `float_bar_show_cost` |
| Cost | `cost_reporting_period`, `cost_usage_enabled`, `spend_dashboard_period`, `spend_dashboard_hidden_source_ids`, `preferred_currency_code` |
| Shortcuts | `switcher_shortcuts` |

`provider_quota_warnings` holds the per-provider quota warnings, for providers that don't use Global only. It exists only in the export file; in `settings.json` the values live under `provider_configs`.

These are never exported:

- Secrets: `provider_configs` (API keys, cookies, token accounts) and every `http_proxy_*` key.
- Machine paths and hosts: sound file paths, Codex log folders, SSH hosts and the pinned cost time zone.
- Choices that need consent or change the system: start at login, start minimized, hooks, the global shortcut, agent sessions, Credential Manager access, reading Claude Code's credentials, Codex external OAuth sources, OpenCodex logs, the PowerToys pipe, float bar on and click-through, Pin to Taskbar, always on top and Stay Awake.
- Update settings.
- Keys outside the version 1 list, such as `menu_bar_color_pace`, `weekly_progress_work_days` and `cost_summary_display_style`.

Import rules:

- An unknown key, a wrong type, a value out of range or another version rejects the whole file. Nothing changes.
- A missing key leaves that setting as it is. `null` restores the default.
- `provider_quota_warnings` replaces the warnings of every provider. Providers not in the file go back to Global.
- An older file with `ui_language` but no `ui_language_follows_system` turns System off, so it keeps the chosen language.
- The older `provider_usage_thresholds` key is accepted on import.

## Provider switcher keys

The tray panel switches providers from the keyboard. The keys work only inside the panel; they aren't global hotkeys.

| Action | Default key |
| --- | --- |
| Previous, next | `Left`, `Right` (wraps around through Overview) |
| Overview, then providers in display order | `Ctrl+1` to `Ctrl+9` |

The keys are ignored while focus is in a text field, a select, an editable area or a slider, and while a grid drag is in progress.

### Customizing the keys

1. Open **Settings > General**.
2. Under **Portable preferences**, choose **Provider Switcher Shortcuts...**. A sheet lists all 11 actions.
3. Choose **Record** for an action, then press the key or combination. Esc cancels.
4. Press Backspace while recording, or choose **Clear**, to disable the action. It then shows "Disabled".
5. **Restore Defaults** resets every action.

The sheet rejects a duplicate or reserved key with a message and saves nothing.

Only the actions you changed are stored, under `switcher_shortcuts` in `settings.json`. The key is left out when everything is default:

```json
{ "switcher_shortcuts": { "previous": "shift+left", "select2": "ctrl+alt+2", "next": "none" } }
```

`rust/src/switcher_shortcuts.rs` checks the grammar and `apps/desktop-tauri/src/lib/switcherShortcuts.ts` mirrors it:

- The actions are `previous`, `next` and `select1` to `select9`. Unknown actions are rejected.
- A shortcut is any of `ctrl`, `alt` and `shift` plus one key: `left`, `right`, `,`, a letter or a digit. `cmd` means `ctrl`, so shortcuts copied from macOS still work.
- Letters, digits and `,` need `ctrl` or `alt`. `ctrl+r`, `ctrl+q`, `ctrl+,` and `ctrl+w` are reserved for Refresh, Quit, Settings and closing the window.
- Two actions can't share a shortcut. `none` disables an action and frees its key.
- If the stored map is invalid, the app logs a warning and uses the defaults. The rest of the settings still load.

The global shortcut that opens the tray panel is separate. It is `global_shortcut`, set under **Settings > General > Keyboard shortcut**.

## Claude Code accounts

Open **Settings > Providers > Claude > Claude Code accounts**.

- **Save current account** keeps the login the CLI uses now.
- **Add account** signs in to another Claude subscription in your browser. The current CLI login stays active. The native Claude Code executable must be installed.
- **Switch** makes a saved account the CLI login. Close running Claude Code CLI sessions first and reopen them after. The app saves the outgoing login, with its latest refresh token, before it switches.
- **Remove** forgets the saved copy. It doesn't log out an active CLI session.
- **Refresh login** and **Cancel sign-in** are also on this page.

The tray menu's **Claude Code accounts** submenu can switch accounts, add an account, save the current one and cancel a sign-in. Remove and Refresh login are only in Settings.

Saved logins are in `%APPDATA%\CodexBar\claude-accounts\accounts.json`, protected with DPAPI.

- Sign-in runs with a temporary `CLAUDE_CONFIG_DIR`. The app removes that folder whether the sign-in succeeds, fails, is cancelled or times out.
- On Windows the sign-in process belongs to a job object, so it ends if Win-CodexBar exits. At startup the app also removes abandoned sign-in folders. It skips links and reparse points.
- Switching updates `claudeAiOauth` in the CLI credentials file and `oauthAccount` in the CLI configuration. Other settings and MCP secrets stay as they are.
- An absolute `CLAUDE_CONFIG_DIR` that Win-CodexBar inherits selects a custom CLI home.
- The feature uses the [Windows Claude Code credentials file](https://code.claude.com/docs/en/authentication), `.claude\.credentials.json`. It doesn't manage macOS Keychain logins or custom keyring setups.

These controls switch the **Claude Code CLI** only, not Claude Desktop or a browser session.

- Usage monitoring still follows the provider's source settings and **Allow reading Claude Code's credentials**.
- Remove API-key or OAuth-token environment overrides before you use saved subscription logins.
- With credential reading off, the active account is unknown and every saved account can be switched to.
- The list doesn't open ambient credential or identity files.
- Choosing the account that is already active does nothing.

## Grok accounts

Open **Settings > Providers > Grok > Grok accounts**.

- **Save current account** keeps the login in `~/.grok/auth.json`.
- **Add account** runs `grok login --oauth` in a separate `GROK_HOME`. Finish the sign-in in the browser profile of the second SuperGrok account. The current CLI login stays active until you choose **Switch**.
- **Switch** saves the outgoing login, then replaces it. Restart running Grok CLI sessions afterwards.
- **Remove** forgets the saved copy. It doesn't log out the active CLI session.
- **Sign in again**, **Refresh usage** and **Cancel sign-in** are also on this page.

The tray menu's **Grok accounts** submenu can switch accounts, add an account, save the current one and cancel a sign-in. Remove is only in Settings.

Saved logins are in `%APPDATA%\CodexBar\grok-accounts\accounts.json`, protected with DPAPI.

- Sign-in uses a temporary `GROK_HOME`, which the app removes however the sign-in ends.
- Switching replaces only `~/.grok/auth.json`, or `$GROK_HOME/auth.json` when that variable is set. Sessions, skills and other Grok files stay in place.
- The app unsets `XAI_API_KEY` and `GROK_OAUTH_TOKEN` for the sign-in, so the browser OAuth flow runs.

## Codex accounts

Open **Settings > Providers > Codex > Codex Accounts**. It has **Add account**, **Refresh login**, **Refresh usage**, **Switch**, **Remove** and **Restart Codex Desktop**. Each account carries an **Ambient** or **Managed** badge. The accounts are in `%APPDATA%\CodexBar\codex-accounts\accounts.json`, protected with DPAPI. The tray menu's **Codex Accounts** submenu can switch accounts and add one.

## Antigravity CLI path

When neither the Antigravity desktop app nor a signed-in `agy` session is running, the Antigravity provider can use the `agy` CLI. Win-CodexBar looks for it in this order:

1. on `PATH`
2. `%LOCALAPPDATA%\agy\bin\agy.exe`
3. `%USERPROFILE%\.local\bin\agy.exe`

If `agy.exe` is somewhere else, set `ANTIGRAVITY_CLI_PATH` to its full path.

When `ANTIGRAVITY_CLI_PATH` is set, the app uses only that path.

- If the variable is empty or doesn't point to a file, the app skips the CLI source and names the variable in the provider error. It doesn't search `PATH` or the install folders. This keeps a background refresh from starting another `agy` that might ask you to sign in.
- A running desktop app still answers first, and offline conversation history still shows when it exists.
- Unset the variable to go back to automatic discovery.

Each `agy` process that Win-CodexBar starts (the version check, the usage report and the short-lived managed session) gets `AGY_CLI_DISABLE_AUTO_UPDATE=true`, even if your environment sets it to `false`. Passive refreshes then don't start agy's background updater. Your own `agy` sessions and agy's saved settings are not changed. agy ignores `1`, so the value must be lowercase `true`.

Other providers keep their own override rules.

## Source mode

The CLI `--source` values are `auto`, `web`, `cli` and `oauth` (see `codexbar usage --help`). Which ones a provider supports is defined by its module in `rust/src/providers/`. Upstream also documents `api`; don't copy upstream tables.

## Hooks

Hooks run a local program when something happens to a provider's usage. Edit them in **Settings > Hooks** or with `codexbar hooks` (see [CLI.md](./CLI.md#run-hooks)).

- **Enable hooks** is on only when both `hooks_enabled` in `settings.json` and `enabled` in `hooks.json` are true. The pane shows a note when only one is on. The switch writes both.
- Each rule has an on switch, an event, a provider (or **Any provider**), an executable, arguments and a timeout. `quota_low` rules also have a threshold in percent.
- **+ Add rule** adds an enabled `quota_reached` rule with a 10-second timeout. Edits save at once. Text fields save when you leave them.
- If `hooks.json` can't be read, the editor locks and the app never overwrites the file.

`hooks.json` is plain JSON in `%APPDATA%\CodexBar`, written atomically:

| Field | Meaning |
| --- | --- |
| `enabled` | Master switch, default false |
| `events` | The list of rules |
| rule `enabled`, `event` or `events`, `provider`, `threshold` | When the rule fires. `threshold` is a fraction from 0 to 1. |
| rule `executable` | Absolute path to the program |
| rule `arguments` | Argument list |
| rule `timeoutSecs` | 1 to 300, default 10 |

Limits: 32 rules, 32 arguments per rule and a 4096-byte payload.

The events are `quota_low`, `quota_reached`, `quota_reset`, `usage_updated`, `provider_unavailable`, `provider_recovered` and `refresh_failed`.

- `usage_updated`, `provider_unavailable` and `refresh_failed` fire at most once per provider account every 600 seconds. The account key used for that limit is kept in memory and never passed to the hook.
- The desktop app sends `usage_updated` after a successful, current refresh. `codexbar hooks watch` sends it after `fetch_usage` succeeds. A failed or replaced refresh sends nothing.
- The `usage_updated` payload can include the primary and secondary usage fractions, the window lengths and the reset times.

Point hooks only at programs you trust.

## Security

- Don't log cookies, tokens or API keys. Use `tracing` and the redaction helpers.
- Treat pasted cookies and API keys like passwords.
- `codexbar serve` on a non-loopback address without TLS sends bearer tokens in clear text. It needs a dashboard token and `--allow-plain-http`. See [CLI.md](./CLI.md#serve-the-local-dashboard).

## Related docs

- [CLI.md](./CLI.md)
- [COOKIES.md](./COOKIES.md)
- [PROVIDERS.md](./PROVIDERS.md)
- [AGENTS.md](../AGENTS.md)
