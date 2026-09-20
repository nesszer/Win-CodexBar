# Configuration (Windows)

Windows rewrite of upstream `docs/configuration.md` and `docs/cli-configuration.md`.
Upstream default paths (`~/.config/codexbar/config.json`, `~/.codexbar/config.json`, macOS Keychain layout) are **not** the primary story here.

## Location

On Windows, config lives under the roaming app data directory:

| Store | Typical path |
|-------|----------------|
| Settings | `%AppData%\Roaming\CodexBar\settings.json` |
| Manual cookies | `%AppData%\Roaming\CodexBar\manual_cookies.json` |
| API keys | `%AppData%\Roaming\CodexBar\api_keys.json` |
| Token accounts | `%AppData%\Roaming\CodexBar\token-accounts.json` |

Resolve at runtime:

```powershell
codexbar config path
```

Implementation: `dirs::config_dir()/CodexBar/...` via `Settings::settings_path()` and related helpers in `rust/src/settings/`. Reads/writes go through `secure_file` (can use Windows DPAPI protection for sensitive material).

Desktop UI and CLI share these stores. Prefer the Settings window for day-to-day toggles; use `codexbar config` for scripts/CI.

## What lives where (conceptual)

Aligned with upstream *ideas*, mapped to this port:

- **Enabled providers, theme, refresh, float bar, UI language, metrics, …** → `settings.json`
- **Manual cookie headers** → `manual_cookies.json` (and/or settings fields depending on provider path)
- **API keys** → `api_keys.json` / keyring helpers where used
- **Token accounts** → `token-accounts.json`
- **Browser auto cookies** → extracted at runtime from Chrome/Edge/Brave/Firefox profiles (see [COOKIES.md](./COOKIES.md)); not a substitute for committing secrets into git

Do not commit real `settings.json` / key files into the repo.

## CLI configuration commands

```powershell
codexbar config providers              # list enablement
codexbar config providers --json --pretty
codexbar config enable -p grok
codexbar config disable -p cursor
codexbar config validate
codexbar config dump
codexbar config path

# API key via stdin (example)
printf '%s' $env:OPENROUTER_API_KEY | codexbar config set-api-key -p openrouter --stdin
```

Notes:

- `enable` / `disable` are **persistent** (same idea as upstream).
- `codexbar usage -p <provider>` is a **one-shot** query override; it is not a full substitute for enable/disable.
- If every provider is disabled, bare `usage` may print nothing useful; pass `-p` explicitly to force a provider for that run.

## Settings UI tabs (desktop)

Canonical tab ids (frontend + proof harness whitelist must match):

`general`, `providers`, `notifications`, `menuBar`, `menu`, `usageSpend`, `advanced`, `about`

Unknown ids fall back to General. Legacy ids `display` / `apiKeys` / `cookies` are **not** settings tabs (content lives under other tabs / provider detail).

Proof / automation example:

```powershell
$env:CODEXBAR_PROOF_MODE = "settings:menu"
# then launch the desktop binary
```

## Claude Code accounts

In **Settings → Providers → Claude → Claude Code accounts**, use **Save current
account** to retain an existing CLI login, or **Add account** to sign in to another
Claude subscription in your browser. Adding an account leaves the current CLI
login active. The native Claude Code executable must be installed.

Close running Claude Code CLI sessions, select **Switch**, then reopen the CLI.
The tray's **Claude Code accounts** submenu provides the same actions. Win-CodexBar
saves the outgoing login before switching, including its latest refresh token.
**Remove** forgets the saved copy; it does not log out an active CLI session.

Saved logins are protected with the existing Windows DPAPI storage helper under
`%APPDATA%\CodexBar\claude-accounts\accounts.json`. Sign-in uses a temporary
`CLAUDE_CONFIG_DIR`; successful, failed, cancelled, and timed-out attempts clean up
that directory. Switching updates `claudeAiOauth` in the CLI credentials file and
`oauthAccount` in the CLI configuration, preserving other settings and MCP secrets.
An absolute `CLAUDE_CONFIG_DIR` inherited by Win-CodexBar selects a custom CLI home.
This account feature follows the [documented Windows Claude Code credential
file](https://code.claude.com/docs/en/authentication), `.claude\.credentials.json`.
It does not manage macOS Keychain logins or custom keyring integrations.
On Windows, the isolated sign-in process belongs to a job that terminates it if
Win-CodexBar exits. Startup also removes abandoned UUID sign-in directories;
cleanup skips links and reparse points.

These controls switch **Claude Code CLI**, not Claude Desktop or browser sessions.
Usage monitoring still follows the provider's source settings and the
**Allow reading Claude Code's credentials** toggle. API-key or OAuth-token
environment overrides must be removed before using saved subscription logins.
When credential reading is disabled, active-account status is unknown and every
saved account remains switchable. The list does not open ambient credential or
identity files. Explicitly selecting the already-current account is a no-op.

## Grok accounts

In **Settings → Providers → Grok → Grok accounts**, use **Save current
account** to retain the existing `~/.grok/auth.json` login, or **Add account**
to run `grok login --oauth` in an isolated `GROK_HOME`. Finish that sign-in in
the Firefox profile (or browser) for the second SuperGrok account. Adding an
account leaves the current CLI login active until you **Switch**.

The tray **Grok accounts** submenu provides the same actions. Win-CodexBar
saves the outgoing login before switching. **Remove** forgets the saved copy;
it does not log out the active CLI session. Restart running Grok CLI sessions
after a switch.

Saved logins are protected with the existing Windows DPAPI storage helper under
`%APPDATA%\CodexBar\grok-accounts\accounts.json`. Sign-in uses a temporary
`GROK_HOME`; successful, failed, cancelled, and timed-out attempts clean up
that directory. Switching replaces only `~/.grok/auth.json` (or
`$GROK_HOME/auth.json` when that environment variable is set). Sessions,
skills, and other Grok home files stay in place. `XAI_API_KEY` and
`GROK_OAUTH_TOKEN` are unset for the isolated sign-in so the browser OAuth
flow is used.

## Source mode

CLI `--source` values on this port (see `codexbar usage --help`): `auto`, `web`, `cli`, `oauth`.

Upstream also documents `api` extensively; treat per-provider support as defined by **this** codebase’s provider modules and help text, not by copying upstream tables blindly.

## Hooks

The shared `hooks.json` rules can match `usage_updated` in addition to the
quota and provider-status events. The desktop refresh path emits it after a
successful, current provider publication; `codexbar hooks watch` emits it after
`provider.fetch_usage` succeeds without publishing a provider snapshot. Its
payload can include primary and secondary usage fractions, window durations,
and reset timestamps. A failed or superseded refresh does not produce a
successful-update event.

Repeated `usage_updated` events are limited to one per provider account per ten
minutes in memory. The account discriminator used for that private limit is
never serialized or passed to the hook process. Configure trusted local
executables only; never point hooks at untrusted paths.

## Start at login (Windows)

Desktop start-at-login uses `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `CodexBar` pointing at the desktop executable (managed via settings). CLI also has `codexbar autostart` for boot integration helpers.

## Security

- Do not log cookies, tokens, or API keys (`tracing` only, redacted helpers).
- Manual cookie paste and API keys are secrets — handle like passwords.
- `codexbar serve` on non-loopback without TLS sends bearer tokens in cleartext; require intentional flags/env (see [CLI.md](./CLI.md)).

## Related

- [CLI.md](./CLI.md)
- [COOKIES.md](./COOKIES.md)
- [PROVIDERS.md](./PROVIDERS.md)
- Root [AGENTS.md](../AGENTS.md)
