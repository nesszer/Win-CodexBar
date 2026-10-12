# ADR 0002: CI budget modes

Date: 2026-07-24
Status: Accepted. ADR 0005 replaced the PR check gate description, and the runner and release facts below are out of date. See "Later changes".

## Context

Win-CodexBar and its sister repo, `linear-cli`, share a Blacksmith CI minute
budget. We need one coarse control to keep Win-CodexBar within its 60% share
and to stop runaway spend quickly. Per-workflow switches are too fine-grained
for an emergency stop.

## Decision

Add a repository variable, `CI_BUDGET_MODE`, with three modes: `normal`,
`thin` and `off`. Unset means `normal`.

- **PR check** (`pr-check.yml`) runs when the mode is unset, `normal` or
  `thin`, and skips only when it is `off`.
  Gate: `if: vars.CI_BUDGET_MODE != 'off'`
- **Interaction guard** (`interaction-guard.yml`) uses the same gate. It keeps
  its existing `blacksmith-2vcpu-ubuntu-2404` runner.
- **Release** is not affected. Releases stay local in every mode.

`thin` does nothing for Win-CodexBar's PR check, because the check is already
a single job with no cross-OS matrix to trim. The mode exists for parity with
the sister repo, and as a setting to use if a second job is added.

## Consequences

- One variable controls both workflows. `off` stops both at once.
- Operators set `CI_BUDGET_MODE` in repo Settings > Variables, not in code.
- The glossary in `CONTEXT.md` is the shared definition for both repos.

## Later changes

Added 2026-10-12. The decision above is kept as history.

- ADR 0005 (2026-08-30) moved the automatic PR check to CircleCI. The CircleCI
  `pr-check` job reads `CI_BUDGET_MODE` from CircleCI project environment
  variables and halts when it is `off`. `pr-check.yml` is now manual dispatch
  only and keeps the `vars.CI_BUDGET_MODE != 'off'` gate.
- The interaction guard moved to the GitHub-hosted `ubuntu-latest` runner in
  #416 (`Optimize hosted CI usage`). It still uses the same budget gate.
- Releases are no longer local. ADR 0004 records that
  `.github/workflows/release.yml` builds them on GitHub Actions. That workflow
  does not read `CI_BUDGET_MODE`.
