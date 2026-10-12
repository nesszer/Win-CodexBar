# CodexBar CLI on Windows

The `codexbar` CLI prints provider usage and local cost, gates scripts on remaining quota, and serves a local dashboard. It is built from the `rust/` crate and shares its settings with the desktop app.

This page covers the Windows port. The upstream macOS install paths (`/Applications`, Homebrew, Sparkle helpers) don't apply.

## Build and run the CLI

Run these from the repo root:

```powershell
cargo build -p codexbar --release
cargo run -p codexbar -- --help
```

The build writes `target\release\codexbar.exe`. If `CARGO_TARGET_DIR` is set, the exe is under that folder instead.

The installer and release ZIP name the CLI `codexbar-cli.exe`. The installed `codexbar.exe` is the desktop app. See [BUILDING.md](./BUILDING.md) for the release names. There is no "Install CLI" button like on macOS, so put the exe on `PATH` yourself.

`codexbar` with no subcommand prints a hint and exits with code 64. Always pass a subcommand.

## Point the CLI at your settings

The CLI and the desktop app read the same settings folder, `%APPDATA%\CodexBar\`. See [CONFIGURATION.md](./CONFIGURATION.md) for the files in it.

```powershell
codexbar config path
codexbar config validate
codexbar config dump
```

`config dump` redacts secrets unless you pass `--show-secrets`. It writes JSON by default, or TOML with `-f toml`.

To use a different settings file, set `CODEXBAR_CONFIG`. See [Separate config file](#separate-config-file-codexbar_config).

## Check usage

```powershell
codexbar usage
codexbar usage -p claude -f json --pretty
codexbar usage -p all --status
codexbar usage --source oauth
codexbar usage --brief
```

`usage` options:

| Option | What it does |
| --- | --- |
| `-p`, `--provider` | A provider CLI name, `both` (Codex and Claude) or `all`. Default: the providers enabled in settings. |
| `-f`, `--format` | `text` (default), `json` or `toon`. |
| `--json` | Same as `--format json`. |
| `--pretty` | Pretty-prints JSON. |
| `--status` | Also fetches each provider's status page. |
| `--all-accounts` | Fetches every token account, where the provider supports it. |
| `--account` | A token-account label or 1-based index. Needs exactly one provider. |
| `--no-credits` | Leaves out the credits line. |
| `--source` | `auto` (default), `oauth`, `web` or `cli`. |
| `--web-timeout` | Web fetch timeout in seconds. Default 60. |
| `--brief` | One short line per provider. |

The global options are `-v`/`--verbose`, `--json-output` (JSON logs on stderr) and `--no-color`.

Which providers `usage` fetches without `-p`:

- If exactly Codex and Claude are enabled, it fetches them as `both`.
- If one provider is enabled, it fetches that one.
- Any other set is fetched in display order.
- If nothing is enabled, JSON and TOON print an empty list. Text output explains how to enable a provider (`codexbar config enable <provider>`) or pass `--provider`.

An explicit `-p` never reads the enabled list. `--account` needs exactly one provider, even when the enabled list has several.

### Error rows

In JSON and TOON output, a provider that fails becomes a row with `provider`, the readable `error` and an `errorKind`. Scripts can branch on `errorKind` without parsing text.

| `errorKind` | Meaning |
| --- | --- |
| `needsAuthentication` | Credentials are missing or were rejected. Sign in again or add a key. |
| `expiredSession` | The credentials worked, but the session or token expired. |
| `localRuntimeOffline` | A local runtime the provider reads is not running. For example, the Claude CLI is not installed. |
| `browserSignInRequired` | Only a browser sign-in can restore usage. The row adds `signInUrl`, the page to open. |
| `timeout` | The fetch ran out of time. |
| `unknown` | Anything else, including failures before the provider runs (account selection, stored keys). |

The first three use the same names as the desktop's provider states.

Claude reports `browserSignInRequired` with `signInUrl` `https://claude.ai/login` when all three of these happen: its OAuth usage endpoint is rate limited, no claude.ai browser session is found, and the CLI probe fails. Until the rate limit lifts, signing in to claude.ai in a browser is the only fix. The `error` text says so.

```json
{
  "provider": "claude",
  "error": "Claude usage failed from all configured sources. ... Sign in at https://claude.ai/login in your browser, then refresh.",
  "errorKind": "browserSignInRequired",
  "signInUrl": "https://claude.ai/login"
}
```

## Check local cost

```powershell
codexbar cost
codexbar cost -p codex -f json --pretty
codexbar cost -p codex --remote user@mac-host
codexbar cost -p codex --format json --summary-only --provider-native-only --days 30
codexbar cost --period month-to-date --json
codexbar cost --period all --json
codexbar cost --days 7 --json
```

`cost` reads local logs and makes no web or CLI calls for Claude and Codex. `-p` takes `codex`, `claude`, `pi`, `muse`, `antigravity`, `cursor`, `gemini`, `copilot`, `all` or `both`. The default is Claude. `-f` takes `text` or `json`.

Claude and Codex costs come from local session logs. Other providers may work differently. Don't assume upstream Cursor dashboard cost behavior unless this tree implements it.

### Antigravity cost

Antigravity reads supported local token history. Known models get API list-price estimates from the bundled price table or the models.dev pricing catalog. Unknown models stay unpriced, never a false `$0`. These estimates are not Antigravity charges or credit deductions.

Antigravity's Gemini 3.1 Pro aliases (`gemini-pro-default`, `gemini-pro-agent` and the `gemini-3.1-pro` effort tiers) price as `gemini-3.1-pro-preview`, the only Gemini 3.1 Pro row in the catalog. The alias applies only inside this provider. Recorded model names don't change.

Incomplete Antigravity history is never shown as exact:

- A scan with trustworthy decoded rows reports `tokensAreLowerBound` and `costIsLowerBound`. Text output says "at least N".
- A scan cut off by a hard limit, or one whose sources contradict each other, is withheld and publishes no total.
- Within one process, a later partial read doesn't replace an earlier complete read of the same window and roots.

Reading the history never waits on the network. When a recorded model has no known price, the desktop Usage & Spend view and `serve /cost` start one bounded models.dev pricing refresh in the background, and a later read uses the new prices. `codexbar cost --provider antigravity --refresh` waits for that refresh and rescans. A plain `codexbar cost` starts no download. The request carries no account identity or usage data. If it fails, the affected models stay unpriced. Empty or missing history never starts a pricing download.

### Reporting period

| Flag | Window |
| --- | --- |
| `--days N` (1 to 365) | A rolling window of N days. Wins over `--period`. |
| `--period month-to-date` | From midnight on the first of the month, local time, through today. |
| `--period all` | All available local history. |
| Neither flag | The saved `cost_reporting_period` in `settings.json`: `rolling:30`, `month-to-date` or `all`. 30 days when unset. |

Text headings show the period label: `Last 30 days`, `Month to date`, `All` or `Today`.

JSON output adds:

- `reportingPeriod`, the raw value;
- `historyLabel`;
- a `totals` object for the selected window, with `inputTokens`, `outputTokens`, `cachedTokens`, `reasoningTokens`, `totalTokens` and `totalCost`.

`days_scanned` is the resolved day count. The `cost`, `tokens` and `sessions_count` fields keep their meaning. `totals` is `null` when a scan found nothing and couldn't establish a known zero.

`totalTokens` follows each provider's token rule. Claude and Pi add cache reads and writes to input plus output. Codex input already includes cached input, so its total is input plus output.

`--remote` and `--summary-only` keep the 1 to 365 day protocol. They send month to date as its current day count, and they reject All unless you also pass `--days N`. `--group-by session` lists at most the latest 365 days when All is selected. All reads every available local log, including logs older than a year. Missing or deleted logs can't be recovered, and incomplete scans stay marked incomplete.

`serve` `/cost` reads the saved period on every request. `days_scanned`, `totals`, the scanner windows and the `daily` chart rows follow it. `daily` covers at most the latest 365 days for All. The dashboard cost line, the dashboard snapshot and the Prometheus gauge `codexbar_cost_last_30_days_usd` always mean 30 days.

### Remote and summary reports

`--remote <host>` adds one separate native Codex report, fetched over non-interactive SSH. Local and remote histories are never merged. `--summary-only` prints the versioned, path-free JSON contract that the remote comparison uses. It accepts only `--provider codex --format json`. Both modes reject session grouping and other providers.

`--provider-native-only` leaves out pi/OMP agent session mirrors. This Windows build has no such mirrors, so the flag is accepted but changes nothing here.

### Scan timing

Codex local-history scans debounce ordinary disk-cache reads for 60 seconds. This is separate from the desktop refresh setting.

With Adaptive refresh off, **Manual** (`refresh_interval_secs = 0`) turns off the recurring desktop refresh timer. Startup reads, stale-aware reads, explicit refreshes and pending Codex catch-up scans still run. Low Power Mode makes recurring automatic refreshes wait at least 30 minutes. Explicit refreshes still run at once.

## Gate a script on quota

```powershell
codexbar guard -p claude --min-remaining 10 --window session
codexbar guard -p codex --json --pretty --fail-open
```

`guard` checks one provider. `--window` is `session` (the primary window, default) or `weekly` (the secondary window). `--min-remaining` is a percent, default 10. `--timeout` is the overall fetch timeout in seconds, default 60, where 0 turns it off.

| Exit code | Meaning |
| --- | --- |
| `0` | Remaining quota is at or above the threshold. |
| `1` | Remaining quota is below the threshold. |
| `64` | Invalid arguments. |
| `69` | Quota is unavailable. `--fail-open` turns this into `0`. |

## Export diagnostics

```powershell
codexbar diagnose --pretty
codexbar diagnose -p codex --pretty
```

`diagnose` writes safe provider diagnostics as JSON. `-p` defaults to `all`, which means the enabled providers.

## Serve the local dashboard

```powershell
codexbar serve --port 8080
```

`serve` binds to `127.0.0.1:8080` by default. A non-loopback `--host` needs a dashboard token and `--allow-plain-http`, because the token then crosses the network over cleartext HTTP. Set the token in `CODEXBAR_DASHBOARD_TOKEN` rather than on the command line.

| Route | What it returns |
| --- | --- |
| `/` | The dashboard page. |
| `/health` | Health check. |
| `/usage` | Usage JSON. Without a `provider` query, it follows the enabled providers like a plain `codexbar usage`. |
| `/cost` | Cost JSON. Defaults to Claude. |
| `/dashboard/v1/snapshot` | The stable dashboard-v1 JSON snapshot. |
| `/metrics` | Prometheus text, only with `--metrics`. Otherwise `404`. |
| `/icons/<name>.svg` | Embedded provider icons. No auth. |

`/usage` fetches the selected providers at the same time. Its error rows carry the same `errorKind` and `signInUrl` as `usage --json`.

`--refresh-interval` sets how many seconds the dashboard snapshot is cached. Default 60.

`--identity redacted|full` sets how much account identity the snapshot shows. Without it, each request follows the app's Hide Personal Info setting.

### Provider icons

The dashboard shows each provider's brand icon from the embedded `/icons/` route. The icons are copies of the upstream `ProviderIcon-*.svg` files. #828 added icons for ClawRouter, IBM Bob, Langdock, LithosAI, MuseAI, Synthetic and WorkBuddy. A provider with no embedded icon shows a neutral dot. Icons that use `currentColor` render dark inside the page's `<img>` tags, as upstream does.

### Usage display control

The dashboard page has a "Usage display" control with Follow server, Used and Remaining. Follow server uses `host.usageBarsShowUsed` from the snapshot. The other two override it for this browser only, stored in `localStorage` under `codexbar.dashboard.usageDisplay`. Changing it re-renders the cached snapshot without fetching again. Bar colour always follows consumption, whichever value is shown: 70% used warns and 90% used is full.

### Request timeout

```powershell
codexbar serve --request-timeout 30
```

`--request-timeout <seconds>` limits `/usage` and `/cost`.

- A provider still running at 0.8 of the timeout becomes a row such as `{"provider":"claude","error":"claude usage timed out","errorKind":"timeout"}`. For `/cost` the error is `codex cost refresh timed out`.
- A request that runs past the whole timeout gets `504` with `{"error":"request timed out"}`.
- The fetch behind a timed-out row keeps running. The next request for that provider joins it instead of starting another. At most eight fetches or scans run at once.
- Finished results are not cached, so the first request after a fetch completes starts a new one.

The default, `0`, waits for every provider. Values are capped at 86400. A negative value fails with `--request-timeout must be zero or greater.` Upstream defaults to 30 seconds and serves late results from a response cache. This port has no such cache, so the timeout is opt-in.

### Prometheus metrics

```powershell
$env:CODEXBAR_DASHBOARD_TOKEN = 'replace-with-a-long-random-token'
codexbar serve --metrics --port 8080
curl.exe -H "Authorization: Bearer $env:CODEXBAR_DASHBOARD_TOKEN" http://127.0.0.1:8080/metrics
```

`/metrics` uses the same Host allowlist, Bearer token, snapshot cache and single-flight collection as the dashboard snapshot. A scrape never waits for provider I/O. It returns the last good snapshot while an expired one refreshes in the background, or `codexbar_up 0` until the first collection succeeds.

What the metrics export:

- Collection health for every enabled, known provider. The only provider label is its canonical CLI name, for example `codexbar_provider_up{provider="claude"}`. Disabled providers are absent. One provider's fetch failure doesn't hide the other providers' series.
- Quota values, for Codex only, in fixed `session`, `weekly`, `monthly` and `code_review` metric families. Used and remaining values are ratios from `0` to `1`, with no dynamic window label.
- Local Codex cost estimates, when available.
- Codex OAuth reset credits as `codexbar_reset_credits_available{provider="codex"}` (`0` means used up, `-1` means unavailable or a PAT), and `codexbar_reset_credits_next_expiry_timestamp_seconds{provider="codex"}` when present. Reset-credit results, good or unavailable, are cached for ten minutes.

The metrics leave out unknown, informational, non-finite and dynamic additional-limit values. They never guess a value or use a sentinel. They don't expose account identity, display labels, source names, free-form provider errors or version strings. Alert on provider health, snapshot age and quota values together.

[Prometheus examples](prometheus/README.zh-CN.md) has a Grafana dashboard, scrape settings and alert rules (in Chinese).

## Write a dashboard snapshot once

```powershell
codexbar dashboard --pretty
codexbar dashboard --output C:\temp\codexbar-snapshot.json
```

`dashboard` prints one dashboard-v1 snapshot. `--output` writes it atomically to a file whose folder must already exist. `--timeout` defaults to 30 seconds, and 0 turns it off. `--identity` defaults to `redacted`. `full` shows real account emails, so use it only where the output stays private.

## Manage settings from the CLI

```powershell
codexbar config providers
codexbar config providers --json --pretty
codexbar config enable -p cursor
codexbar config disable cursor --json
printf '%s' $env:OPENROUTER_API_KEY | codexbar config set-api-key -p openrouter --stdin
codexbar config claude-code-credentials status --json
codexbar config preferences export --file C:\temp\codexbar-preferences.json
codexbar config preferences import --file C:\temp\codexbar-preferences.json
codexbar config validate
```

`config` subcommands:

| Subcommand | What it does |
| --- | --- |
| `validate` | Checks the configuration files. |
| `dump` | Prints the configuration. Secrets are redacted. |
| `providers` | Lists providers and whether each is enabled. |
| `enable`, `disable` | Turns a provider on or off and saves the setting. |
| `set-api-key` | Stores a provider API key. `--stdin` reads it from stdin. `--no-enable` stores it without enabling the provider. |
| `path` | Shows the configuration file paths. |
| `preferences export`, `preferences import` | Exports or imports portable preferences. See [CONFIGURATION.md](./CONFIGURATION.md#portable-preferences). |
| `claude-code-credentials` | `allow`, `deny` or `status`. |

`enable`, `disable` and `set-api-key` take the provider as a positional name or with `-p`/`--provider`. `usage -p <id>` is a one-off choice and doesn't change the enabled list.

`config providers`, `enable`, `disable` and `claude-code-credentials` accept `--format text|json`, `--json` and `--pretty`. Their JSON matches upstream:

- `config providers` prints one `{provider, displayName, enabled, defaultEnabled}` object per provider. Retired providers (`kimik2`, `crossmodel`) are listed only while enabled, like in Settings. They still resolve by name, for example `--provider kimik2`.
- `config enable` and `config disable` print `{provider, displayName, enabled, configPath}`.
- `config claude-code-credentials allow|deny|status` prints `{allowed, configPath}`.

`claude-code-credentials` controls whether CodexBar may read and refresh Claude Code's own OAuth credentials (`~/.claude/.credentials.json` or Windows Credential Manager). It is the same choice as the desktop setting, and it is off by default. While it is off, Claude Auto falls back to CLI usage, which has less detail. `allow` and `deny` save the choice. `status` only reports it.

`config preferences import` changes the saved settings. Restart a running CodexBar afterwards.

### Separate config file (`CODEXBAR_CONFIG`)

```powershell
$env:CODEXBAR_CONFIG = 'D:\isolated\codexbar\settings.json'
codexbar config path
codexbar usage --json
```

`CODEXBAR_CONFIG` names the settings file this CLI process uses instead of `%APPDATA%\CodexBar\settings.json`.

- Surrounding whitespace is trimmed, and an empty value is ignored.
- A leading `~` becomes your home folder.
- A relative path is resolved from the current folder.

Upstream keeps provider keys, cookies and token accounts in that one file. This port keeps them in separate stores, so `api_keys.json`, `manual_cookies.json` and `token-accounts.json` are read from and written to the folder of the chosen file. Logs, caches and other app state stay in their default places.

Only the `codexbar` CLI reads the variable. The desktop app ignores it. With the variable set, the CLI also skips two desktop steps that loading settings normally runs: it doesn't sync the start-at-login registry entry, and it doesn't run the tray-default migration. `config path` lists the resulting paths and says when they come from `CODEXBAR_CONFIG`.

## Manage token accounts

```powershell
codexbar account list claude
codexbar account add zai --label Work --token <token>
codexbar account switch zai Work
codexbar account remove zai Work
```

`account switch` and `account remove` take an account label or ID. `--token` takes a token or cookie value, and the value ends up in your shell history. Settings > Providers can add token accounts without that.

## Run hooks

```powershell
codexbar hooks list --json
codexbar hooks enable
codexbar hooks test usage_updated --provider codex --json
codexbar hooks watch --provider codex --json
```

Hook rules live in `hooks.json` next to `settings.json`. `hooks enable` and `hooks disable` flip its master switch. The desktop Settings > Hooks pane edits the same file. See [CONFIGURATION.md](./CONFIGURATION.md#hooks).

`hooks test` runs the matching rules for a sample event. The events are `quota_low`, `quota_reached`, `quota_reset`, `usage_updated`, `provider_unavailable`, `provider_recovered` and `refresh_failed`.

`hooks watch` polls providers and fires hooks on real changes. `--interval` defaults to 300 seconds, with a minimum of 60. `--provider` takes one or more CLI names, comma-separated or repeated. Without it, the enabled providers are polled.

`usage_updated` is opt-in. It fires after a successful refresh and carries the primary and secondary quota usage, the window lengths and the reset times when known. Failed or superseded refreshes don't fire it.

- The desktop app fires it after it publishes a current provider snapshot.
- `hooks watch` fires it right after `provider.fetch_usage` succeeds, without publishing a snapshot.
- Each provider account fires it at most once per ten minutes. The private account key used for that limit is never sent in the hook payload or environment.

## List sessions

```powershell
codexbar sessions
codexbar sessions --json --pretty
codexbar sessions --json-v2 --pretty
codexbar sessions --focus <session-id>
codexbar sessions --ssh-host user@host
```

`--json` prints the legacy v1 format, Codex and Claude only. `--json-v2` adds Pi-family sessions. `--ssh-host` reads a configured SSH host, and you can repeat it or pass a comma-separated list. `--brief` prints one line per session.

## List Codex workspaces

```powershell
codexbar workspaces
codexbar workspaces --json --pretty --days 7
```

`workspaces` lists local Codex project usage. `--days` defaults to 30. `--force` rebuilds the index even when a cached snapshot exists.

## Start CodexBar at login

```powershell
codexbar autostart --status
codexbar autostart --enable
codexbar autostart --disable
```

`--enable` and `--disable` write the Windows start-at-login registry entry.

## Browser cookies

[COOKIES.md](./COOKIES.md) covers browser cookie import. In the desktop app, pick the browser in Settings > Providers. Paste a cookie by hand when DPAPI import fails or under WSL.

## Differences from upstream

Don't copy upstream CLI docs without checking:

- There is no Swift CLI product called `CodexBarCLI`. The release ZIP is named `CodexBarCLI-v<version>-windows-x64.zip`, but it holds `codexbar-cli.exe`.
- There are no macOS Keychain cookie cache flags.
- There is no Homebrew or Linux tarball install path for Windows.
- Card and claude-swap behavior from upstream may be missing or different. Trust `codexbar <command> --help` on this binary.

## Related

- [CONFIGURATION.md](./CONFIGURATION.md)
- [PROVIDERS.md](./PROVIDERS.md)
- [BUILDING.md](./BUILDING.md)
