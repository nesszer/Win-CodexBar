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

`usage` fetches up to three providers at the same time. Rows and text sections come out in the order the providers were selected, whichever fetch finishes first. Diagnostics logged on each provider's fetch are held and printed to stderr in selection order after all fetches finish, not streamed live. Log lines from background tasks a provider spawns are not captured and are not reordered. One provider's failure becomes its own error row and does not stop the others (upstream v0.76.0 #4501).

Which providers `usage` fetches without `-p`:

- If exactly Codex and Claude are enabled, it fetches them as `both`.
- If one provider is enabled, it fetches that one.
- Any other set is fetched in display order.
- If nothing is enabled, JSON and TOON print an empty list. Text output explains how to enable a provider (`codexbar config enable <provider>`) or pass `--provider`.

An explicit `-p` never reads the enabled list. `--account` needs exactly one provider, even when the enabled list has several.

### Claude and claude-swap in `usage`

When the claude-swap integration is enabled (Settings > Providers > Claude) and `usage --provider claude` runs in Auto, the answer comes from the active claude-swap slot, the one the adapter marks active (upstream 0.79 #4631). Other browser or CLI logins on the machine don't supply the answer. This holds only when all of these are true:

- The integration is on and an executable path is set.
- `--source` is `auto` (or omitted), and the configured Claude usage source is also `auto`.
- No `--account` and no `--all-accounts`.

The output is one row for the active slot with `source` `claude-swap` (or `tcr`) and the slot's `account` object, including `usageFetchedAt` when the adapter reported a live measurement. Text shows the same slot's `Measured <time>` line. A live measurement is used when the row's status is `ok` or `unavailable`. Any other status uses the row's last-good usage and shows its capture time and the status diagnostic. The inactive slots are never used.

If the adapter fails, or the active slot has no usable usage, `usage` runs the ordinary Claude sources. The source label then gets a fallback suffix such as `web (claude-swap fallback)`, and the identity is the one from that fallback source. Another swap slot is never substituted. Explicit `--source`, `--account` or `--all-accounts` and a configured non-auto source skip this route entirely.

Windows difference: the active-slot row uses the claude-swap account shape that `--all-accounts` already emits (`account`, `error`, `historicalUsage`), not the ordinary `usage` snapshot, so there is no top-level `usage` key for that row. Error rows from a failed fallback fetch do not carry the suffix.

### App usage cache fallback

When **Settings > Advanced > Share usage with the CLI** is on, the desktop app keeps `%APPDATA%\CodexBar\cli-usage-cache.json` up to date (see [CONFIGURATION.md](./CONFIGURATION.md#advanced)). `usage` shows that copy instead of an error row only when all of these hold (upstream v0.77 #4554):

- Sharing is on in `settings.json`. If `settings.json` itself can't be decrypted in this session, the presence of the cache file counts as opted in, because the app only writes it while sharing is on.
- The fetch is for the default account: `--source auto`, no `--account` and no `--all-accounts`. `CODEXBAR_CONFIG` turns the fallback off.
- A DPAPI decrypt failed earlier in the same run (stored credentials or browser cookies), and the provider then failed with a missing-credentials or missing-cookies error. Timeouts, network errors, expired sessions and other failures still print the error row.
- The cached entry for that provider is at most one hour old and has at least one real limit window.

The JSON row uses `source` `codexbar-app` and adds `ageSeconds`. Text output adds the note "Cached by the CodexBar app 5m ago (credentials could not be decrypted in this session)."; `--brief` appends it in parentheses.

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
codexbar cost -p codex --group-by project
codexbar cost -p codex --identified-only --format json --days 30 --remote user@build-host
codexbar cost --period month-to-date --json
codexbar cost --period all --json
codexbar cost --days 7 --json
```

`cost` reads local logs and makes no web or CLI calls for Claude and Codex. `-p` takes `codex`, `claude`, `pi`, `muse`, `antigravity`, `cursor`, `gemini`, `copilot`, `all` or `both`. The default is Claude. `-f` takes `text` or `json`.

Claude and Codex costs come from local session logs. Other providers may work differently. Don't assume upstream Cursor dashboard cost behavior unless this tree implements it.

### OpenCode Go cost

`codexbar cost --provider opencodego` reads the recorded `opencode-go` history from the local OpenCode database. It honors the selected window (`--days`, `--period`, or the saved period). Costs are the recorded `cost` values. Tokens are never used to derive a cost.

- Text output shows the total, the window token totals, and each model's cost followed by that model's token counts.
- A row whose token counts are missing, malformed, negative or overflowing has *unknown* tokens, not zero. A row that records a total but omits input or output keeps that exact total, but its input and output stay unknown. Text shows `Tokens:   unknown (N rows lacked complete token counts)`. The affected model shows `tokens unknown (...)`. Costs are still reported.
- JSON: `totals` token fields and `tokens` are `null` for an unknown window. `unknownTokenRowCount` appears only when a window has such rows. `byModelTokens` maps each model to its `inputTokens`, `outputTokens`, `cachedTokens`, `reasoningTokens` and `totalTokens`, with `null` for models that have unknown rows. `spendContract` token totals are `null` for an unknown window; its per-model rows still carry the usable partial counts (see `docs/PROVIDERS.md`).

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

`--remote <host>` adds one separate native Codex report, fetched over non-interactive SSH. Local and remote histories are never merged. `--summary-only` prints the versioned, path-free JSON contract that the remote comparison uses. It accepts only `--provider codex --format json`. Both modes reject project or session grouping and other providers. The remote host can run this CLI or upstream macOS CodexBar; both print the same `--summary-only` array. When the remote report says its history coverage is incomplete, its totals are dropped and the report is shown as partial.

`--provider-native-only` leaves out pi/OMP agent session mirrors. This Windows build has no such mirrors, so the flag is accepted but changes nothing here.

`--group-by project` lists Codex local projects from the workspace index. It prints text only; other providers print a not-available line, and JSON output is unchanged. When the local cost scan has incomplete coverage, the report shows the same `Coverage: partial (history catch-up in progress)` notice as the default Codex text report, even when no project rows are available. Hide personal information replaces project names and drops paths.

### Identified-request subtotal (experimental)

`codexbar cost --provider codex --identified-only --format json --days 30` exports a versioned native request ledger (`schemaVersion` 1) for one inclusive UTC window. `--identified-end <Unix milliseconds>` sets the end (default: now). The start is UTC midnight `days - 1` days earlier, and saved periods and time zones don't change it. Add `--remote <ssh-host>` to get per-source `reports` plus an **identified-request subtotal** for this machine and that host. The remote host needs a CLI that supports `--identified-only`.

- Only owned native `token_usage_record` events with thread and response IDs count. Legacy `token_count` usage without a matching identified request, and unidentified usage, is excluded and counted in `excludedCount`. The subtotal is not complete account usage. Foreign-provider routes (for example `deepseek/...`) are not Codex usage.
- Identical copies count once. Contradictory copies of one identity are excluded for the whole merge, even after a later matching copy. Identities are length-framed SHA-256 hashes, so rows carry timestamps and token categories without conversation text, model names, paths or raw session IDs.
- Combined prices are unavailable (`costUSD: null`), so no dollar estimate is made. Exports are limited to 50,000 records and conflict IDs and 32 MiB of remote output. A source that goes over a limit is rejected, not truncated. An unavailable source keeps the data from the sources that worked, marks the result `partial` and exits nonzero. Readable empty history is valid. Malformed lines, dropped usage lines or files that change during the scan mark the data partial.
- It requires `--provider codex` and JSON, and `--days` from 1 to 365 (30 by default). It can't be combined with `--summary-only`, `--period` or `--group-by`. The separate-host `--remote` summary without `--identified-only` is unchanged.

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

The desktop app can run the same server for you from Settings > Advanced > HTTP server, without a console window. See [CONFIGURATION.md](./CONFIGURATION.md#advanced).

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

`--all-accounts` adds an `accounts` list to the dashboard snapshot for each provider that has configured token accounts. Each account is fetched with its own credential, through the same source routing the desktop app uses when that account is selected; it never falls back to another login on the machine. The provider's top-level row shows the active account, so that account is fetched only once. Account ids have the form `token:<uuid>`. Claude keeps its existing account section. `/usage` doesn't change, and `/metrics` keeps one series per provider, taken from the top-level row.

How much identity the expanded snapshot shows depends on `--identity`:

| `--identity` | Account labels | Emails and other identity | Error text |
| --- | --- | --- | --- |
| not set | `Account 1`, `Account 2` ... | hidden | `Account usage unavailable`, `Account list unavailable` |
| `redacted` | `Account 1`, `Account 2` ... | redacted, as without `--all-accounts` | `Account usage unavailable`, `Account list unavailable` |
| `full` | the labels you gave the accounts | shown | the real error |

These rules cover Claude's account rows too. If an account's fetch fails before it produces a row, the provider reports `Account list incomplete`.

Providers are fetched at the same time, and so are their account lists, all under the build's 75-second deadline. Within one provider, up to three accounts are fetched at once, and a slow account doesn't hold back the ones queued after it. Neuralwatt is the exception: its quota endpoint allows one request per second, so its accounts are fetched one after another with a one-second pause between them. An account that is still waiting when the deadline passes becomes a timeout row, and every timeout row reports the 75-second build budget. Claude's account rows are not under the shared deadline: each Claude account gets its own 75-second timeout.

```powershell
codexbar serve --all-accounts
codexbar serve --all-accounts --identity full
```

Windows differences: upstream also lists visible Codex managed profiles; Windows lists only configured token accounts. Upstream fetches each provider's accounts one at a time; Windows fetches up to three at once, unless the provider needs a pause between accounts.

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
codexbar config set-source --provider codex --source cli
printf '%s' $env:OPENROUTER_API_KEY | codexbar config set-api-key -p openrouter --stdin
Get-Clipboard | codexbar config set-cookie -p cursor --stdin
codexbar config clear-credential -p cursor --cookie --json
codexbar config claude-code-credentials status --json
codexbar config login --provider copilot
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
| `set-source` | Saves a provider's usage source (`--source auto\|web\|cli\|oauth`) without changing whether it is enabled. `auto` returns to the default. |
| `set-api-key` | Stores a provider API key. `--stdin` reads it from stdin. `--no-enable` stores it without enabling the provider. |
| `set-cookie` | Stores a manual cookie header (`--cookie <header>` or `--stdin`) and selects the Manual cookie source. `--no-enable` stores it without enabling the provider. Only providers with a cookie lane are accepted. The header is trimmed, must be one line, and is never printed. |
| `clear-credential` | Removes the stored API key and manual cookie for a provider. `--api-key` or `--cookie` narrows it to one. Enablement, cookie and usage sources, and token accounts stay as they are, so Manual stays selected after a cookie is cleared. Nothing is written when nothing was stored. |
| `login` | Signs in to GitHub Copilot with GitHub's device-code flow, or to Grok and Kiro through their own CLIs. Other providers are rejected. See [Sign in from the CLI](#sign-in-from-the-cli). |
| `path` | Shows the configuration file paths. |
| `preferences export`, `preferences import` | Exports or imports portable preferences. See [CONFIGURATION.md](./CONFIGURATION.md#portable-preferences). |
| `claude-code-credentials` | `allow`, `deny` or `status`. |

`enable`, `disable`, `set-source`, `set-api-key`, `set-cookie` and `clear-credential` take the provider as a positional name or with `-p`/`--provider`. `usage -p <id>` is a one-off choice and doesn't change the enabled list.

`config providers`, `enable`, `disable`, `set-source`, `set-cookie`, `clear-credential` and `claude-code-credentials` accept `--format text|json`, `--json` and `--pretty`. Their JSON matches upstream:

- `config providers` prints one `{provider, displayName, enabled, defaultEnabled}` object per provider. Retired providers (`kimik2`, `crossmodel`) are listed only while enabled, like in Settings. They still resolve by name, for example `--provider kimik2`.
- `config enable` and `config disable` print `{provider, displayName, enabled, configPath}`.
- `config set-source` prints `{provider, displayName, enabled, source, configPath}`.
- `config set-cookie` prints `{provider, enabled, configPath}`.
- `config clear-credential` prints `{provider, cleared, configPath}`, where `cleared` lists `apiKey` and/or `cookieHeader`.
- `config claude-code-credentials allow|deny|status` prints `{allowed, configPath}`.

`set-source` writes the same per-provider usage source as the Settings > Providers source picker. It checks the source against what that provider supports, so `codexbar config set-source codex --source web` fails with `Source web is not supported for codex. Supported sources: auto, cli, oauth.` Upstream also accepts `api`. This port has no separate API source; API-key providers use `oauth` for their token lane.

`claude-code-credentials` controls whether CodexBar may read and refresh Claude Code's own OAuth credentials (`~/.claude/.credentials.json` or Windows Credential Manager). It is the same choice as the desktop setting, and it is off by default. While it is off, Claude Auto falls back to CLI usage, which has less detail. `allow` and `deny` save the choice. `status` only reports it.

`config preferences import` changes the saved settings. Restart a running CodexBar afterwards.

### Sign in from the CLI

```powershell
codexbar config login --provider copilot
codexbar config login --provider grok
codexbar config login --provider kiro --json
```

`config login` supports three providers: `copilot`, `grok` and `kiro`. The output and exit codes are the same for all three. Each provider's own sign-in differs, as described below.

#### GitHub Copilot

`config login --provider copilot` starts GitHub's device-code flow. It prints the verification URL and a one-time code, then waits while you approve the code in your browser. The code expires after the lifetime GitHub returns, and the command stops when it does.

Text output is two lines and then a confirmation:

```text
https://github.com/login/device
ABCD-1234
Connected
```

The token is never printed. Once you approve the code, the account is saved the same way the desktop app's Copilot sign-in saves it. The account is matched by GitHub identity, so signing in again updates the existing account instead of adding a second one. Its label is the GitHub login, with the plan in brackets when GitHub reports one.

A successful login also turns the Copilot provider on in settings, as the macOS app does. The desktop app's sign-in does not change the enabled state, so enable Copilot there yourself if you use that path.

#### Grok

`config login --provider grok` runs `grok login --oauth` from Grok Build, the same sign-in the desktop app's Grok account add uses. Grok opens the browser itself, so the CLI prints only `Waiting for the browser sign-in to finish.` (in JSON, an `authorization_pending` line with no URL). Grok's own output is not shown. The sign-in waits up to 5 minutes.

Once the sign-in finishes, the account is saved to CodexBar's Grok accounts the same way the desktop app saves it. Then the Grok provider is turned on. If the Grok CLI is not installed, the command exits with `8`. Set `CODEXBAR_GROK_CLI_PATH` to a `grok` executable to use that file; the variable is used only when it names an existing file.

#### Kiro

`config login --provider kiro` runs `kiro-cli login`. The command first prints `Waiting for the browser sign-in to finish.` (in JSON, an `authorization_pending` line with no URL). kiro-cli then prints a sign-in link and opens the browser. The link is printed as soon as the CLI shows it: on its own line in text mode, or as a second `authorization_pending` line with `verificationURI` in JSON mode. The sign-in waits up to 120 seconds, the same as the desktop sign-in.

kiro-cli keeps the credential itself. CodexBar does not copy it into its own store. A zero exit code therefore means kiro-cli reported a successful sign-in. Then the Kiro provider is turned on. If kiro-cli is not installed, the command exits with `8`. `CODEXBAR_KIRO_CLI_PATH` selects a specific `kiro-cli` executable, as it does for usage.

#### JSON events

`--json` prints one JSON object per line, using upstream's field names:

| Line | Shape |
| --- | --- |
| Code issued (Copilot) | `{"status":"authorization_pending","verificationURI":"https://github.com/login/device","userCode":"ABCD-1234","expiresIn":900}` |
| Kiro sign-in link | `{"status":"authorization_pending","verificationURI":"<link printed by kiro-cli>"}` |
| Waiting in the browser (Grok; also the first line for Kiro) | `{"status":"authorization_pending"}` |
| Approved and saved | `{"status":"authorized"}`. This line is written only after the credential is saved and the provider is enabled. |
| Cancelled (Ctrl-C) | `{"status":"cancelled"}` |
| Failed | `{"status":"failed","reason":"denied","error":"GitHub device login failed: Access denied by user"}`. `reason` is one of `denied`, `expired`, `cancelled`, `missing_binary` or `error`. The exit code table below maps each reason to its exit code. |

Text mode prints the URL and code for Copilot. Grok prints only `Waiting for the browser sign-in to finish.` Kiro prints that waiting line, then the sign-in URL once kiro-cli shows it. On success every provider prints `Connected`. Errors go to stderr as `Error: <message>`. Ctrl-C ends the login with `Error: Login cancelled.` and exit code `7`.

### Exit codes

| Exit code | Meaning | Final `--json` line |
| --- | --- | --- |
| `0` | Signed in. The credential is saved (Copilot and Grok) or kiro-cli reported success (Kiro), and the provider is enabled. | `{"status":"authorized"}` |
| `1` | Any other failure: network error, provider server error, or a settings or store write error. Usually nothing is saved. If only the settings write that enables the provider fails, the sign-in already finished and the error says so: `Signed in and saved the account, but could not enable Copilot` (or `Grok`), or `Signed in, but could not enable Kiro`. Running the login again updates that account. | `{"status":"failed","reason":"error",...}` |
| `5` | The user denied the request on GitHub (`access_denied`). Copilot only. Nothing is saved. | `{"status":"failed","reason":"denied",...}` |
| `6` | The sign-in expired before approval: the GitHub device code (`expired_token`), or the Grok or Kiro sign-in window ran out. Nothing is saved. | `{"status":"failed","reason":"expired",...}` |
| `7` | Stopped with Ctrl-C before the credential was saved. Nothing is saved. | `{"status":"cancelled"}` |
| `8` | The provider's CLI is not installed: `grok` (Grok) or `kiro-cli` (Kiro). Copilot never uses this code. Nothing is saved. | `{"status":"failed","reason":"missing_binary",...}` |
| `64` | Invalid arguments, including an unsupported `--provider`. No network call is made. | None; the error goes to stderr |

Only exit code `0` guarantees both that the sign-in finished and that the provider is enabled. For Kiro, kiro-cli keeps the credential, so `0` means kiro-cli reported a successful sign-in. Codes `5`, `6`, `7`, `8` and `64` mean nothing was saved; code `1` can follow a finished sign-in, as described above.

Polling follows the interval GitHub returns. Each `slow_down` response adds 5 seconds for the rest of the login. GitHub's `access_denied` and `expired_token` responses end the login with the friendly errors above, and nothing is saved.

Other providers fail before any network call: `Login is supported for copilot, grok and kiro. Use --provider copilot, grok or kiro.`

The Copilot login always signs in on github.com. This port has no GitHub Enterprise host option for the login flow, and the desktop app's sign-in behaves the same way.

For tests and proof runs, `CODEXBAR_GITHUB_BASE_URL` replaces both GitHub roots with one origin. The device-code requests (`/login/device/code`, `/login/oauth/access_token`) and the identity and usage requests (`/user`, `/copilot_internal/user`) all go there. Only `https://` URLs, or loopback `http://` URLs (`127.0.0.1`, `localhost`, `[::1]`), with no path, query or credentials are accepted. Anything else is ignored with a warning, and the default GitHub roots are used. The variable also applies to the desktop app's Copilot sign-in and usage fetches, so unset it for normal use.

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
codexbar hooks watch --provider both
```

Hook rules live in `hooks.json` next to `settings.json`. `hooks enable` and `hooks disable` flip its master switch. The desktop Settings > Hooks pane edits the same file. See [CONFIGURATION.md](./CONFIGURATION.md#hooks).

`hooks test` runs the matching rules for a sample event. The events are `quota_low`, `quota_reached`, `quota_reset`, `usage_updated`, `provider_unavailable`, `provider_recovered` and `refresh_failed`.

`hooks watch` polls providers and fires hooks on real changes. `--interval` defaults to 300 seconds, with a minimum of 60. `--provider` takes one or more CLI names, comma-separated or repeated. `both` means Codex and Claude, and `all` means every provider; they can be mixed with names, and duplicates are dropped. Without `--provider`, the enabled providers are polled.

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

The CLI always runs its remote discovery: each `--ssh-host` and any online Tailscale peer (standalone `tailscale.exe` on `PATH`). It does not read the desktop **Agent sessions** switch or **Local sessions only** setting, so those settings do not change CLI output. Use the desktop app's settings to limit remote discovery there.

Each SSH host runs `sh -lc '<chain>'` in BatchMode, where the chain tries `codexbar sessions --json-v2`, then `codexbar sessions --json`, then the same two commands with the bundled macOS CLI (`/Applications/CodexBar.app/Contents/Helpers/CodexBarCLI`). Two remote outputs are decoded:

- Upstream macOS CodexBar prints a flat JSON array of sessions with `cwd`, `projectName`, `startedAt` and `lastActivityAt` at the top level of each row.
- This Windows CLI prints `{"sessions": [...], "errors": [...]}`, where each row nests `workspace`, `activity` and `focusTarget`.

Every remote session is shown under the SSH host you configured, whatever host name the remote payload carries. Entries in the remote `errors` list are shown as that host's error, and its sessions are still listed. Output that is not JSON, or a row with a missing or unknown provider, source or state, fails that host with a message to update CodexBar on the remote host.

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
