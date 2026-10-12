# Shared CI budget glossary

This glossary defines the CI budget terms that Win-CodexBar shares with its sister repo, `linear-cli`. Use it to reason about CI spend across both projects.

Trigger rules, caches, fork PRs and the release workflow are in [`.github/CI.md`](.github/CI.md). The budget modes are defined here and in [ADR 0002](docs/adr/0002-ci-budget-modes.md), because `.github/CI.md` does not cover them.

## Win-CodexBar compute roles

- **CircleCI Windows** runs the hosted PR and `main` check (`pr-check`). It builds no releases.
- **GitHub Actions Windows** (`windows-latest`) builds, signs and drafts tagged releases in `.github/workflows/release.yml`.
- **Blacksmith Windows** runs `.github/workflows/pr-check.yml` by manual dispatch only, as a reserve or second opinion.
- **GitHub-hosted Ubuntu** (`ubuntu-latest`) runs the interaction guard for authors who are not owners, members or collaborators.

CircleCI and the Blacksmith reserve run the same command, `scripts/local-check.ps1 -Slice ci`.

## Blacksmith pool

Win-CodexBar shares its Blacksmith allowance with `linear-cli`. Win-CodexBar uses Blacksmith only as reserve capacity. Don't run Blacksmith beside a healthy CircleCI run.

The original split between the repos was about **60% Win-CodexBar, 30% linear-cli and 10% buffer**. That split is budget history. It is not a reason to run duplicate checks.

## CI budget mode

`CI_BUDGET_MODE` is the emergency stop for hosted checks. It takes `normal`, `thin` or `off`. Unset or empty means `normal`.

| Mode | CircleCI PR check | Interaction guard | Blacksmith reserve | Release |
| --- | --- | --- | --- | --- |
| `normal` | runs when in scope | runs when needed | manual only | GitHub Actions on a `v*.*.*` tag |
| `thin` | runs when in scope | runs when needed | manual only | GitHub Actions on a `v*.*.*` tag |
| `off` | skips | skips | skips | GitHub Actions on a `v*.*.*` tag |

`thin` behaves like `normal`. The code checks only for `off`, and the PR check is a single job with nothing to trim. The value exists for parity with `linear-cli`.

Porting micro PRs that target a `port/upstream-*` branch save compute in a different way: CircleCI skips them. See `docs/PORTING.md`.

Set the variable in each CI service's settings, not in source code:

- CircleCI: Project Settings > Environment Variables > `CI_BUDGET_MODE`. The `pr-check` gate reads it.
- GitHub Actions: Settings > Secrets and variables > Actions > Variables. `pr-check.yml` and `interaction-guard.yml` read it.

`release.yml` doesn't read `CI_BUDGET_MODE`, so budget mode never stops a tagged release.

## Local check slice

The hosted Windows check runs this command from the repo root:

```powershell
.\scripts\local-check.ps1 -Slice ci
```

It runs these steps in order:

1. `scripts/circleci-pr.tests.ps1` (CI gate tests)
2. `cargo fmt --all --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace`
5. `pnpm install --frozen-lockfile`
6. `pnpm run lint` (oxlint)
7. `pnpm run check:anti-slop`
8. `pnpm run test:anti-slop`
9. `pnpm test`
10. `pnpm run build`
11. `node --test .github/scripts/interaction-guard.test.mjs`

The slice doesn't build the installer or release assets, and it doesn't prove UI behavior.

For tray, settings, float bar and other UI changes, CI is not enough. Rebuild on Windows and prove the change in the real window with CUA, or attach equivalent manual proof. `AGENTS.md` and `docs/WINDOWS_PROOF.md` describe the proof.
