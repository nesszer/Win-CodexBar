# ADR 0004: GitHub Actions release and CircleCI validation boundary

Date: 2026-09-16
Status: Accepted. This version replaces the release part of the first ADR 0004
(dated 2026-08-14, #285), which put tag releases on CircleCI. That first version
superseded the local-release decision in ADR 0001.

## Context

Win-CodexBar needs reproducible hosted Windows checks and a release path whose
build origin SignPath can verify. SignPath's GitHub trusted-build integration
must see the build, the uploaded signing artifact and the jobs before signing
run as GitHub Actions work on GitHub-hosted runners. CircleCI stays for
ordinary PR and protected-branch checks, but it cannot produce the artifact
that SignPath signs.

Validation and build steps that don't need GitHub release write access must
not get release credentials. v0.60.3 was released before this change and is
immutable.

## Decision

CircleCI's .circleci/config.yml contains the pr-check validation workflow only.
Its canonical local-check CI slice remains the primary hosted Windows gate.
CircleCI has no tag release jobs, approval job, or release publisher.

GitHub Actions owns the canonical tag release in
.github/workflows/release.yml. The workflow accepts only canonical vX.Y.Z tags,
checks out the exact tag SHA, runs release preflight, and builds on a
GitHub-hosted Windows runner. It creates a signing input with exactly the
installer, portable executable, and CLI ZIP, uploads that input as a GitHub
Actions artifact, and submits it to the pinned SignPath configuration and
release-signing policy.

The workflow waits for SignPath, verifies Authenticode on the two top-level
executables and codexbar-cli.exe at the root of the nested CLI ZIP, computes
sidecars from signed bytes, and validates the exact six-asset final bundle.
Only the verified bundle is passed to a separate publisher job with contents
write permission. The publisher creates or updates a draft release and never
replaces a divergent asset or finalizes a release.

The manual SignPath Test workflow uses test-signing, builds a reviewed source
ref with a matching version label, and retains its verified output as an
Actions artifact. It never writes a GitHub Release.

## Trust boundaries

- CircleCI validation has no GitHub release-write credential.
- The GitHub signing job has read-only repository permission plus artifact
  read access; it receives only the SignPath API token needed for submission.
- The GitHub publisher job is the only job with contents write permission.
- SignPath trusted-build and origin verification cover the GitHub workflow and
  its artifact lineage.
- Repository administrators protect main and the canonical vX.Y.Z tag
  namespace and manually publish or roll back draft releases.

## Consequences

- There is one release build lineage and one signing artifact lineage.
- CircleCI Windows credits continue to cover PR and protected-branch checks.
- GitHub-hosted Windows is used for release builds because SignPath requires
  that trusted provenance.
- A signing, verification, or publication mismatch fails closed. Re-running
  the publisher is safe for exact matching assets, while a different digest
  is a hard failure.
- The first production-signed release is the next normal version after
  SignPath certificate and policy onboarding is complete.

## Compiled-output caching

CircleCI may continue to cache Cargo registry data, Cargo targets, and the
pnpm store for its validation job. The GitHub Actions release job may use its
own GitHub-hosted cache strategy, but the final release assets are always
built fresh from the frozen tag SHA and then signed. A cached output is never
accepted as the final signed artifact without the full post-sign verification
and manifest generation steps.

## Later changes

Added 2026-10-12. No change to the decision.

- The build step in `release.yml` runs `scripts/circleci-release-build.ps1`.
  The script name comes from the CircleCI era. GitHub Actions runs it on
  `windows-latest`.
- The six release assets are `CodexBar-<version>-Setup.exe`,
  `CodexBar-<version>-portable.exe` and `CodexBarCLI-v<version>-windows-x64.zip`,
  each with a `.sha256` file.
