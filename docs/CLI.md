# CodexBar CLI (Windows)

Windows rewrite of upstream `docs/cli.md` for the **`codexbar`** binary built from `rust/`.
Upstream install paths (`/Applications`, Homebrew, Sparkle-bundled Helpers) do **not** apply.

## Install / build

```powershell
# From repo root
cargo build -p codexbar --release
# Binary: target\release\codexbar.exe  (or target\<triple>\release\ under some setups)

cargo run -p codexbar -- --help
```

Release installers may place CLI next to the desktop app; for development, run the cargo-built `codexbar.exe` or put it on `PATH` yourself. There is no “Preferences → Install CLI” symlink flow like macOS.

## Configuration

CLI and desktop app share the same Windows config directory (see [CONFIGURATION.md](./CONFIGURATION.md)):

- Settings: `%AppData%\Roaming\CodexBar\settings.json`
- Manual cookies / API keys / token accounts: sibling files under that folder

```powershell
codexbar config path
codexbar config validate
codexbar config dump
```

Set `CODEXBAR_CONFIG` to run the CLI against another settings file; see [Separate config file](#separate-config-file-codexbar_config).

## Commands (current)

Top-level (from `codexbar --help`):

| Command | Purpose |
|---------|---------|
| `usage` | Print usage from the enabled providers, or the ones passed with `-p` |
| `cost` | Local token cost usage (Claude + Codex session scans; no web required for those) |
| `guard` | Gate automation on remaining quota for one provider |
| `diagnose` | Export safe provider diagnostics as JSON |
| `sessions` | List or focus local / SSH agent sessions |
| `serve` | HTTP JSON/dashboard server with optional Prometheus metrics |
| `dashboard` | Emit a one-shot dashboard snapshot (JSON to stdout or `--output` file) |
| `autostart` | Manage Windows boot auto-start |
| `account` | Token accounts for providers |
| `config` | validate / dump / providers / enable / disable / set-api-key / path / claude-code-credentials |
| `hooks` | List, enable, disable, test, or watch external hooks |
| `workspaces` | List local Codex project/workspace usage |

### Usage

```powershell
codexbar usage
codexbar usage -p claude -f json --pretty
codexbar usage -p all --status
codexbar usage --source auto   # auto | web | cli | oauth
codexbar usage --brief
```

These flags belong to `usage`: `-p/--provider`, `-f/--format` (`text`, `json` or `toon`), `--json`, `--pretty`, `--status`, `--all-accounts`, `--account`, `--no-credits`, `--no-color`, `--source`, `--web-timeout`, `--brief`. The root command takes only `-v/--verbose`, `--json-output` (JSON logs on stderr) and `--no-color`; `codexbar` without a subcommand prints a hint and exits with a usage error.

Without `-p`, `usage` follows the providers enabled in settings, like upstream: exactly Codex and Claude are fetched as `both`, one enabled provider is a single fetch, and any other set keeps the display order. With nothing enabled, JSON and TOON print an empty list, and text explains how to enable a provider (`codexbar config enable <provider>`) or pass `--provider`. An explicit `-p` (a provider, `both` or `all`) never reads the enabled list. `--account` needs exactly one provider, including when the enabled list resolves to several. `cost` keeps its Claude default.

**Error rows.** In JSON and TOON output a provider that fails becomes a row with `provider`, the human-readable `error`, and `errorKind`, so scripts can react without parsing the text:

| `errorKind` | Meaning |
|---|---|
| `needsAuthentication` | Credentials are missing or were rejected; sign in again or add a key. |
| `expiredSession` | The credentials worked but the session or token has expired. |
| `localRuntimeOffline` | A local runtime the provider reads is not running, for example the Claude CLI is not installed. |
| `browserSignInRequired` | Only a browser sign-in can bring usage back. The row adds `signInUrl`, the page to open. |
| `timeout` | The fetch ran out of time. |
| `unknown` | Anything else, including failures before the provider runs (account selection, stored keys). |

The first three use the same names as the desktop's provider states. Claude reports `browserSignInRequired` (with `signInUrl` `https://claude.ai/login`) when its OAuth usage endpoint is rate limited, no claude.ai browser session was found, and the CLI probe failed too. Until the rate limit lifts, signing in to claude.ai in a browser is the only way back, and the `error` text says so.

```json
{
  "provider": "claude",
  "error": "Claude usage failed from all configured sources. ... Sign in at https://claude.ai/login in your browser, then refresh.",
  "errorKind": "browserSignInRequired",
  "signInUrl": "https://claude.ai/login"
}
```

### Cost

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

Claude/Codex costs come from local session logs. Antigravity reads supported local token history; known models receive API list-price estimates from the bundled price table or the models.dev pricing catalog, and unknown models stay unpriced rather than becoming a false `$0`. Antigravity's Gemini 3.1 Pro aliases (`gemini-pro-default`, `gemini-pro-agent`, and the `gemini-3.1-pro` effort tiers) price as `gemini-3.1-pro-preview`, the only catalogued Gemini 3.1 Pro row; the alias is provider-local and recorded model names stay unchanged. These estimates are not Antigravity charges or credit deductions. Other providers may differ; do not assume upstream Cursor dashboard cost behavior unless implemented in this tree.

Antigravity history that stopped short is never shown as exact. A scan whose decoded rows are trustworthy reports `tokensAreLowerBound` / `costIsLowerBound` (text: "at least N"); a scan cut off by a hard limit, or one whose sources contradict each other, is withheld and publishes no total. A later partial read does not replace an earlier complete read of the same window and roots within one process. Reading the history never waits on the network. When a recorded model has no known public price, the desktop Usage & Spend view and `serve /cost` start one bounded models.dev pricing refresh in the background and a later read picks up the new prices; `codexbar cost --provider antigravity --refresh` waits for that refresh and rescans, while a plain `codexbar cost` starts no download. The request carries no account identity or usage data, and a failure leaves the affected models unpriced. Empty or absent history never starts a pricing download.
**Reporting period.** `--period month-to-date|all` selects the calendar month so far (midnight on the first of the month, in the machine's local zone, through today) or all available local history. `--days N` (1..=365) always selects a rolling window and wins over `--period`. With neither flag the saved `cost_reporting_period` in `settings.json` applies (`rolling:30`, `month-to-date` or `all`; 30 days when unset). Text headings show the period label (`Last 30 days`, `Month to date`, `All`, `Today`). JSON payloads add `reportingPeriod` (the raw value), `historyLabel`, and a `totals` object (`inputTokens`, `outputTokens`, `cachedTokens`, `reasoningTokens`, `totalTokens`, `totalCost`) for the selected window, and `days_scanned` is the resolved day count; the existing `cost`, `tokens`, and `sessions_count` fields keep their meaning. `totalTokens` follows each provider's token rule: Claude and Pi add cache reads and writes to input + output, while Codex input already includes cached input, so its total is input + output. `totals` is `null` when a scan found nothing without establishing a known zero.

`--remote` and `--summary-only` keep the 1..=365 day protocol: month to date is sent as its current day count, and All is rejected unless you pass an explicit `--days N`. `--group-by session` lists the most recent 365 days at most when All is selected. All reads every available local log, including logs older than a year; missing or deleted logs cannot be recovered and incomplete scans stay marked incomplete.

`serve` `/cost` reads the saved period on every request, so `days_scanned`, `totals`, the scanner windows and the `daily` chart rows follow it (`daily` covers at most the latest 365 days for All). The dashboard cost line, the dashboard snapshot, and the Prometheus gauge `codexbar_cost_last_30_days_usd` keep their 30-day meaning.

Claude/Codex costs come from local session logs. Antigravity exposes local **token history only** through `cost`; dollar cost remains unknown rather than becoming a false `$0`. Other providers may differ; do not assume upstream Cursor dashboard cost behavior unless implemented in this tree.

`--remote` adds one separate native Codex report fetched through non-interactive SSH; overlapping local and remote histories are never combined. `--summary-only` emits the versioned, path-free JSON contract used by the remote comparison and accepts only `--provider codex --format json`. Both modes reject project/session grouping and other provider selections.

`--group-by project` lists Codex local projects from the workspace index (text only; other providers print a not-available line and JSON output is unchanged). When the local cost scan has incomplete coverage, the report shows the same `Coverage: partial (history catch-up in progress)` notice as the default Codex text report, including when no project rows are available. Hide personal information replaces project names and drops paths.

**Identified-request subtotal (experimental).** `codexbar cost --provider codex --identified-only --format json --days 30` exports a versioned native request ledger (`schemaVersion` 1) over one inclusive UTC window. `--identified-end <Unix milliseconds>` pins the end (default: now); the start is UTC midnight `days - 1` days earlier, and saved periods and time zones do not change it. Add `--remote <ssh-host>` to return per-source `reports` plus an **identified-request subtotal** for this machine and that host; the remote host needs a CLI that supports `--identified-only`.

- Only owned native `token_usage_record` events with thread and response IDs contribute. Legacy `token_count` usage without a matching identified request, and unidentified usage, is excluded and counted in `excludedCount`; the subtotal is not complete account usage. Foreign-provider routes (for example `deepseek/...`) are not Codex usage.
- Identical copies count once. Contradictory copies of one identity are excluded for the whole merge, even after a later matching copy. Identities are length-framed SHA-256 hashes, so rows carry timestamps and token categories without conversation text, model names, paths, or raw session IDs.
- Combined prices are unavailable (`costUSD: null`); no dollar estimate is made. Exports are limited to 50,000 records and conflict IDs and 32 MiB of remote output; exceeding a limit rejects that source instead of truncating it. An unavailable source keeps the successful source data, marks the result `partial`, and exits nonzero. Readable empty history is available; malformed lines, dropped usage lines, or files that change during the scan mark the data partial.
- Requires explicit `--provider codex` and JSON, and `--days` 1..=365 (30 by default). It cannot be combined with `--summary-only`, `--period`, or `--group-by`. The separate-host `--remote` summary without `--identified-only` is unchanged.

Codex local-history scans use a 60-second scanner-side debounce for ordinary disk-cache reads. This is separate from the desktop provider refresh setting. With Adaptive refresh off, **Manual** (`refresh_interval_secs = 0`) disables the recurring desktop refresh timer, but it does not forbid startup/stale-aware reads, explicit refreshes, or pending Codex catch-up scans. Low Power Mode floors recurring automatic refreshes to 30 minutes; explicit/manual work remains immediate.

### Guard

```powershell
codexbar guard -p claude --min-remaining 10 --window session
codexbar guard -p codex --json --pretty --fail-open
```

Exit codes (stable intent): `0` ok, `1` below threshold, usage errors for bad args, unavailable when quota cannot be checked (`--fail-open` turns unavailable into `0`).

### Serve

```powershell
codexbar serve --port 8080
# Non-loopback binds need a dashboard token and --allow-plain-http (cleartext bearer).
# Prefer: $env:CODEXBAR_DASHBOARD_TOKEN = '...'
```

Typical endpoints: `/health`, `/usage`, `/cost`, and `/dashboard/v1/snapshot`. Loopback default keeps local use simple; treat non-loopback as a threat-model choice because the token for protected requests crosses the network over HTTP.

The dashboard page at `/` has a "Usage display" control (Follow server, Used, Remaining). Follow server uses `host.usageBarsShowUsed` from the snapshot; the other two override it for this browser only, stored in `localStorage` under `codexbar.dashboard.usageDisplay`. Changing it re-renders the cached snapshot without refetching. Bar colour always follows consumption (70% used warns, 90% used is full), whichever value is shown.

`/usage` without a `provider` query (or with an empty one) follows the enabled providers, like a plain `codexbar usage`; `/cost` still defaults to Claude. `/usage` fetches the selected providers concurrently, and its error rows carry the same `errorKind` (and `signInUrl`) as `usage --json`.

```powershell
codexbar serve --request-timeout 30
```

`--request-timeout <seconds>` bounds `/usage` and `/cost`. A provider still running after 0.8 of the timeout becomes a row such as `{"provider":"claude","error":"claude usage timed out","errorKind":"timeout"}` (`/cost`: `codex cost refresh timed out`), and a request that outlives the whole timeout answers `504` with `{"error":"request timed out"}`. The fetch behind a timed-out row keeps running: the next request for that provider joins it instead of starting another, and at most eight fetches or scans run at once. Finished results are not cached, so the first request after a fetch completes starts a new one. The default `0` waits for every provider. Upstream defaults to 30 seconds and serves the late result from a response cache; this port has no such cache, so the timeout is opt-in. Values are capped at 86400, and negative values are rejected with `--request-timeout must be zero or greater.`

Pass `--metrics` to enable the Prometheus text endpoint at `/metrics`; it returns `404` when the flag is absent. The endpoint uses the same Host allowlist, Bearer token, snapshot cache, and single-flight collection as the dashboard snapshot. A scrape never waits for provider I/O: it returns the last successful snapshot while an expired value refreshes in the background, or `codexbar_up 0` until the first collection succeeds.

```powershell
$env:CODEXBAR_DASHBOARD_TOKEN = 'replace-with-a-long-random-token'
codexbar serve --metrics --port 8080
curl.exe -H "Authorization: Bearer $env:CODEXBAR_DASHBOARD_TOKEN" http://127.0.0.1:8080/metrics
```

The metrics contract exports collection health for every enabled, known provider. The only provider label is its bounded canonical CLI slug, for example `codexbar_provider_up{provider="claude"}`; disabled providers are absent, and an ordinary provider fetch failure does not suppress healthy provider series. Quota semantics are currently exported only for Codex through fixed `session`, `weekly`, `monthly`, and `code_review` metric families. Used and remaining values are ratios from `0` to `1`, with no dynamic window label. Available local Codex cost estimates are also exported.

Codex OAuth reset credits are exposed as `codexbar_reset_credits_available{provider="codex"}` (`0` exhausted, `-1` unavailable or PAT) and, when present, `codexbar_reset_credits_next_expiry_timestamp_seconds{provider="codex"}`. Successful and unavailable reset-credit observations are cached for ten minutes. See [Prometheus examples](prometheus/README.zh-CN.md) for a Chinese Grafana dashboard, scrape settings, and alert rules.

Unknown, informational, non-finite, and dynamic additional-limit values are omitted instead of being inferred or replaced with sentinels. Account identity, display labels, source names, free-form provider errors, and version strings are not exposed. Consumers should alert on provider health, snapshot staleness, and quota values together.

### Config

```powershell
codexbar config providers
codexbar config providers --json --pretty
codexbar config enable -p cursor
codexbar config disable cursor --json
printf '%s' $env:OPENROUTER_API_KEY | codexbar config set-api-key -p openrouter --stdin
codexbar config claude-code-credentials status --json
codexbar config validate
```

`enable` / `disable` persist settings. `usage -p <id>` is a one-shot override and does not by itself toggle enabled state the same way. `enable`, `disable` and `set-api-key` take the provider either as a positional name or as `-p/--provider`.

`config providers`, `enable`, `disable` and `claude-code-credentials` accept `--format text|json`, `--json` and `--pretty`. Their JSON follows upstream:

- `config providers`: one `{provider, displayName, enabled, defaultEnabled}` object per provider. The text lines are unchanged. Retired providers (`kimik2`, `crossmodel`) are listed only while enabled, like the Settings UI; they still resolve by name (for example `--provider kimik2`).
- `config enable` / `config disable`: `{provider, displayName, enabled, configPath}`.
- `config claude-code-credentials allow|deny|status`: `{allowed, configPath}`.

`claude-code-credentials` controls whether CodexBar may read, and refresh, Claude Code's own OAuth credentials (`~/.claude/.credentials.json` or Windows Credential Manager). It is the same choice as the desktop setting and is off by default. While it is off, Claude Auto falls back to reduced-fidelity CLI usage. `allow` and `deny` save the choice, and `status` only reports it.

#### Separate config file (`CODEXBAR_CONFIG`)

```powershell
$env:CODEXBAR_CONFIG = 'D:\isolated\codexbar\settings.json'
codexbar config path
codexbar usage --json
```

`CODEXBAR_CONFIG` names the settings file this CLI process uses instead of `%AppData%\Roaming\CodexBar\settings.json`. Surrounding whitespace is trimmed, an empty value is ignored, a leading `~` becomes your home directory, and a relative path is resolved from the current directory. Upstream keeps provider keys, cookies and token accounts in that one file; this port keeps them in separate stores, so `api_keys.json`, `manual_cookies.json` and `token-accounts.json` are read from and written to the same folder as the chosen file. Logs, caches and other app state stay in their default locations.

Only the `codexbar` CLI reads the variable; the desktop app ignores it. A CLI run with the variable set also skips the desktop integrations that loading settings normally performs: it does not sync the start-at-login registry entry or run the tray-default migration. `config path` lists the resulting paths and notes when they come from `CODEXBAR_CONFIG`.

### Hooks

```powershell
codexbar hooks list --json
codexbar hooks test usage_updated --provider codex --json
codexbar hooks watch --provider codex --json
```

The opt-in `usage_updated` event is emitted after a successful refresh and
contains the primary and secondary quota usage, window durations, and reset
timestamps when available. Failed or superseded refreshes do not emit it.
The desktop refresh path emits it after publishing a current provider
snapshot; `hooks watch` emits it directly after `provider.fetch_usage`
succeeds, without publishing a snapshot. Repeated events for the same
provider account are limited to one per ten minutes; the private account
discriminator used for that limit is never sent to the hook payload or
environment.

### Sessions

```powershell
codexbar sessions
codexbar sessions --json --pretty
codexbar sessions --focus <session-id>
codexbar sessions --ssh-host user@host
```

### Cache / cookies

Browser cookie import for the app is documented in [COOKIES.md](./COOKIES.md). Prefer Settings → Providers → browser picker on Windows. Manual cookie paste is supported when DPAPI import fails or under WSL.

## Upstream differences (do not copy blindly)

- No Commander/Swift CLI product name `CodexBarCLI`
- No macOS Keychain cookie cache flags as primary docs
- No Homebrew Linux tarball install story as the default Windows path
- Cards / claude-swap–specific CLI behavior from upstream docs may be absent or different — trust `codexbar <cmd> --help` on this binary

## Related

- [CONFIGURATION.md](./CONFIGURATION.md)
- [PROVIDERS.md](./PROVIDERS.md)
- [BUILDING.md](./BUILDING.md)
