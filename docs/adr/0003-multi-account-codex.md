# ADR 0003: Multi-account Codex coexistence with ambient provider

Date: 2026-08-06
Status: Accepted. The tray-flyout parts of PR 5/5 were superseded by #813. See "Later changes".

## Context

CodexBar shows exactly one `codex` provider snapshot. The ambient
single-account Codex provider in `rust/src/providers/codex/` produces it. The
provider reads whichever identity is active in `~/.codex` (`auth.json`), calls
`fetch_usage`, and publishes the result through the standard provider pipeline
to the tray, the flyout and Settings.

A stacked series of 5 PRs ports the MIT-licensed Windows core of
[`ademisler/codexcontrol`](https://github.com/ademisler/codexcontrol) (Windows
Python modules) to Rust as `rust/src/codex_accounts/`:

- **PR 1/5, domain core.** The account model (`CodexAccount`,
  `CodexAccountSource::Ambient | ManagedByApp`), ambient vs app-managed
  `CODEX_HOME` discovery (`file_locations`), per-account quota snapshots,
  account stores, login runner, and Codex Desktop (MSIX) session switching.
- **PR 2/5, shell.** Tauri commands exposing discovery/login/switch/remove,
  plus managed-account refresh lanes that fetch and cache a snapshot per
  managed account in parallel.
- **PR 3/5, frontend bridge.** TS types, `tauri.ts` wrappers, i18n keys.
- **PR 4/5, settings panel.** `CodexAccountsSection` in provider settings.
- **PR 5/5, tray menu.** The `CodexAccountsMenu` flyout.

The new module manages *many* accounts, while the provider pipeline still
models *one* `codex` provider. Both must work while the port lands in
reviewable slices.

## Decision

**COEXISTENCE-NOW.** The ambient single-account provider
(`rust/src/providers/codex/`) remains the source of truth for the single
`codex` provider snapshot published through the existing pipeline today.
`rust/src/codex_accounts/` is added as a *parallel* managed-account domain:

- PR 2/5 ships managed-account lanes that fetch/cache one snapshot per managed
  account without disturbing the ambient provider's snapshot.
- PR 4/5 and PR 5/5 add surfaces (settings panel, tray flyout) that read the
  managed-account domain; they present the ambient slot through the same model
  (`CodexAccountSource::Ambient`) without changing what the single `codex`
  provider row shows.
- No existing provider, surface, or persistence path is rewired in this series;
  the ambient provider keeps publishing exactly as before.

**Replacement intent.** This ADR records the intent, but this series does not
carry it out. The ambient provider path retires in a later major release, once
`codex_accounts` reaches feature parity with it. Parity means the managed lanes
match ambient `fetch_usage` behavior *and* local cost scans move to the
managed-home model. Retirement is a separate change with its own review. This
series only prepares for it.

## Consequences

- During the transition both worlds publish snapshots: the ambient provider
  emits the single `codex` provider snapshot, while the codex_accounts lanes
  emit per-managed-account snapshots. They do not overwrite each other.
- **Identity precedence**: the ambient slot's snapshot *is* the `codex`
  provider snapshot (authority for credit bars, notifications, ordering) until
  the ambient path retires. Managed lanes never claim the provider slot; the
  tray/settings account picker is informational for them.
- **Signed/tracked lifetime hook**: the ambient path becomes eligible for
  retirement when (a) codex_accounts lane coverage matches ambient
  `fetch_usage` behavior (same usage windows, credits, and reset semantics per
  account) and (b) local cost scans are migrated to read managed homes. Both
  conditions are checked off under this ADR before the retirement PR merges.
- Review cost stays bounded: each PR is a small, independently testable slice;
  rollback of any surface PR leaves the domain intact.
- Transitional duplication is accepted: two code paths compute "current Codex
  usage" until retirement. The `NOTICE` file carries the upstream
  ademisler/codexcontrol MIT attribution.
- **Managed-home reuse over duplication** (added with the home-reuse fix):
  `materialize_as_managed` reuses a managed home that already holds
  credentials for the account instead of minting a fresh UUID home per
  switch; the first match by `managed_home_key` is the stable reuse target
  and all matches are removal targets. On reuse, the stored `auth.json` is
  refreshed only when the ambient copy is *known* to be at least as recently
  refreshed. Unknown freshness (unreadable JSON, missing `last_refresh`, or
  an unrecognized schema) keeps the incumbent credentials rather than
  clobbering them: copying on unknown freshness is exactly how a stale
  ambient token turned working accounts into dead ones, which is the
  duplicate-home failure mode this policy exists to prevent.

## Later changes

Added 2026-10-12. The decision above is kept as history.

- The enum variant is `CodexAccountSource::ManagedByApp`
  (`rust/src/codex_accounts/models.rs`). Earlier text called it `Managed`.
- The ambient provider has not retired. `rust/src/providers/codex/` still
  publishes the single `codex` snapshot.
- #813 (`feat/mac-card-anatomy`) removed the React `CodexAccountsMenu` and
  `ClaudeAccountsMenu` flyouts from the tray panel. A Codex card now ends with
  a "Switch Account..." or "Add Account..." row that opens Settings >
  Providers, where account switching lives. The tray icon's right-click menu
  still has native "Codex Accounts", "Claude Code accounts" and "Grok
  accounts" submenus (`apps/desktop-tauri/src-tauri/src/tray_accounts.rs`).
