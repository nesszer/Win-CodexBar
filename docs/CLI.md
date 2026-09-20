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

## Commands (current)

Top-level (from `codexbar --help`):

| Command | Purpose |
|---------|---------|
| `usage` | Print usage from enabled providers (default-style workflow; also global `-p` / `-f`) |
| `cost` | Local token cost usage (Claude + Codex session scans; no web required for those) |
| `guard` | Gate automation on remaining quota for one provider |
| `diagnose` | Export safe provider diagnostics as JSON |
| `sessions` | List or focus local / SSH agent sessions |
| `serve` | HTTP JSON/dashboard server with optional Prometheus metrics |
| `autostart` | Manage Windows boot auto-start |
| `account` | Token accounts for providers |
| `config` | validate / dump / providers / enable / disable / set-api-key / path |
| `hooks` | List, enable, disable, test, or watch external hooks |

### Usage

```powershell
codexbar usage
codexbar usage -p claude -f json --pretty
codexbar usage -p all --status
codexbar usage --source auto   # auto | web | cli | oauth
codexbar usage --brief
```

Global-style flags (also on root help): `-p/--provider`, `-f/--format`, `--json`, `--pretty`, `--status`, `--all-accounts`, `--account`, `--no-credits`, `--source`, `--web-timeout`, `--brief`.

### Cost

```powershell
codexbar cost
codexbar cost -p codex -f json --pretty
codexbar cost -p codex --remote user@mac-host
codexbar cost -p codex --format json --summary-only --provider-native-only --days 30
```

Claude/Codex costs come from local session logs. Antigravity exposes local **token history only** through `cost`; dollar cost remains unknown rather than becoming a false `$0`. Other providers may differ; do not assume upstream Cursor dashboard cost behavior unless implemented in this tree.

`--remote` adds one separate native Codex report fetched through non-interactive SSH; overlapping local and remote histories are never combined. `--summary-only` emits the versioned, path-free JSON contract used by the remote comparison and accepts only `--provider codex --format json`. Both modes reject session grouping and other provider selections.

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

Pass `--metrics` to enable the Prometheus text endpoint at `/metrics`; it returns `404` when the flag is absent. The endpoint uses the same Host allowlist, Bearer token, snapshot cache, and single-flight collection as the dashboard snapshot. A scrape never waits for provider I/O: it returns the last successful snapshot while an expired value refreshes in the background, or `codexbar_up 0` until the first collection succeeds.

```powershell
$env:CODEXBAR_DASHBOARD_TOKEN = 'replace-with-a-long-random-token'
codexbar serve --metrics --port 8080
curl.exe -H "Authorization: Bearer $env:CODEXBAR_DASHBOARD_TOKEN" http://127.0.0.1:8080/metrics
```

The metrics contract exports collection health for every enabled, known provider. The only provider label is its bounded canonical CLI slug, for example `codexbar_provider_up{provider="claude"}`; disabled providers are absent, and an ordinary provider fetch failure does not suppress healthy provider series. Quota semantics are currently exported only for Codex through fixed `session`, `weekly`, `monthly`, and `code_review` metric families. Used and remaining values are ratios from `0` to `1`, with no dynamic window label. Available local Codex cost estimates are also exported.

Unknown, informational, non-finite, and dynamic additional-limit values are omitted instead of being inferred or replaced with sentinels. Account identity, display labels, source names, free-form provider errors, and version strings are not exposed. Consumers should alert on provider health, snapshot staleness, and quota values together.

### Config

```powershell
codexbar config providers
codexbar config enable -p cursor
codexbar config disable -p cursor
printf '%s' $env:OPENROUTER_API_KEY | codexbar config set-api-key -p openrouter --stdin
codexbar config validate
```

`enable` / `disable` persist settings. `usage -p <id>` is a one-shot override and does not by itself toggle enabled state the same way.

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
