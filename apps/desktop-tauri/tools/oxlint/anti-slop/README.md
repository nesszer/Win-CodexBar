# Vendored anti-slop rules

This directory contains the project-owned subset of the generic rules from
[dmmulroy/anti-slop](https://github.com/dmmulroy/anti-slop). The upstream
project is intended to be vendored, so the source is kept here for review and
controlled updates rather than installed as an opaque package.

## Provenance

- Upstream repository: `https://github.com/dmmulroy/anti-slop`
- Imported revision: `c44ef22ca116d0ba62a3ff663a0bd13a3f3fa40b`
- Imported on: `2026-09-13`
- License: MIT; see [LICENSE](./LICENSE)
- Local changes: the entry point exposes only the two baseline-safe generic
  rules selected for this repository. The selected rules and their shared
  helpers are copied from the upstream revision; the upstream Effect rules and
  deferred generic rules are intentionally not included.

The Oxlint and `@oxlint/plugins` packages are pinned to the matching exact
version `1.78.0` in the frontend package. Keep those versions together when
updating this vendor directory.

## Enabled rules

The initial blocking set is deliberately small and baseline-safe:

- `anti-slop/no-object-parameters`
- `anti-slop/no-widen-then-assert`

The other four candidate rules were exercised against the current frontend
before being deferred: they reported existing findings in type assertions,
explicit dictionary annotations, conditional object construction, and test
fixtures. They can be reconsidered individually after those findings receive
an owner and a design decision; this gate does not hide them with blanket
suppression.

Run the focused upstream rule tests from `apps/desktop-tauri` with:

```text
pnpm run test:anti-slop
```
