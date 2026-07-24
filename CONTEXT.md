# Shared CI Budget glossary

This glossary is shared with the sister repo (`linear-cli`) so operators can
reason about CI spend across both projects with one vocabulary.

## Blacksmith Pool

The shared Blacksmith free-tier minute pool that both Win-CodexBar and the
sister repo (`linear-cli`) draw from. All `blacksmith-*` runners bill against
this one pool. The intent-share (60%) and $0 spend alert are defined against
the combined draw on this pool, not per repo.

## CI budget mode

A repository variable `CI_BUDGET_MODE` controls how much CI runs per change.
It is intentionally coarse: `normal`, `thin`, or `off`.

| Mode   | PR check | Interaction guard | Release                          |
|--------|----------|--------------------|----------------------------------|
| normal | runs     | runs               | local (Win-CodexBar only)        |
| thin   | runs     | runs               | local (Win-CodexBar only)        |
| off    | **skip** | **skip**           | local (Win-CodexBar only)        |

> Note: Win-CodexBar's Release is **always local** (no release workflow on
> Blacksmith). The sister repo `linear-cli` has a Blacksmith dispatch-only
> release workflow, so its Release column differs by design. Budget mode does
> not gate Win-CodexBar's local release — it is operator-driven regardless of
> mode.

- `normal` — default. Unset is treated as `normal`.
- `thin` — Win-CodexBar's PR check survives `thin`: it is the single Windows
  Blacksmith job and stays within the 60% intent share, so it still runs. The
  sister repo (`linear-cli`) skips its PR check entirely under `thin`
  (`if: mode != 'off' && mode != 'thin'`); its default PR check is already a
  single Linux-only job, so there is no matrix to trim — `thin` simply drops
  the whole job.
- `off` — emergency stop. Both the PR check and the interaction guard skip.
  Use this when the bill approaches the `$0 spend` alert threshold or when a
  runaway workflow is burning minutes.

Set it in **Settings → Secrets and variables → Actions → Variables**, not in
code. The workflows read it as `vars.CI_BUDGET_MODE`.

## Intent share (60/30/10)

The Blacksmith Pool minutes intent is divided: roughly **60% Win-CodexBar**,
**30% linear-cli**, and **10% buffer**. This is an intent allocation of the
pool's minutes, not a measure of time spent in `normal`/`thin`/`off` modes.
Win-CodexBar's single Blacksmith Windows PR check is the only recurring
Windows job; release builds stay local. If you add a second recurring job
here, reassess the 60% share before merging.

## Blacksmith billing note

On the Blacksmith free tier, **Windows minutes bill at 2x** (one Windows
minute consumes two free-tier minutes). `blacksmith-4vcpu-windows-2025` is a
Windows Server 2025 runner with VS Build Tools available, so Rust + Tauri
builds work without extra setup. The PR check is a thin slice (fmt + clippy +
test) and never runs `tauri:build` release, installer packaging, smoke
install, or upload.

## Local check slice

The PR check mirrors `scripts/local-check.ps1`'s default slice only:
`cargo fmt --check`, `cargo clippy -D warnings` on both crates, `cargo test`
on both crates, and the frontend `pnpm test` / `tsc --noEmit` (via
`pnpm run build`). It deliberately excludes `tauri:build` release, the
installer, smoke install, and release upload — those stay on the local
Windows release path.
