## Summary

Describe what changed and why.

## Related issue

Fixes #

## Affected areas

Check every area this PR changes or could affect:

- [ ] Tray panel
- [ ] Settings UI
- [ ] Config file / settings persistence
- [ ] CLI
- [ ] Provider-specific behavior
- [ ] Installer / release packaging
- [ ] Startup / background behavior
- [ ] Documentation
- [ ] Other:

## Validation

List the exact commands you ran and their results. If a check is not relevant, say why.

- [ ] `cargo test --manifest-path rust\Cargo.toml`
- [ ] `cargo test --manifest-path apps\desktop-tauri\src-tauri\Cargo.toml`
- [ ] `cargo fmt --all`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `pnpm --dir apps\desktop-tauri test`
- [ ] `pnpm --dir apps\desktop-tauri run build`
- [ ] Thermo-nuclear code quality review completed before submitting: https://github.com/cursor/plugins/blob/main/cursor-team-kit/skills/thermo-nuclear-code-quality-review/SKILL.md
- [ ] Other:

## UI / tray proof

For UI, tray, settings, or visual behavior changes, use CUA Driver for visual proof. If CUA Driver cannot be used, explain why and attach equivalent manual proof.

- [ ] Not applicable
- [ ] CUA Driver visual proof attached
- [ ] CUA Driver could not be used; equivalent manual proof and explanation attached

## Notes for reviewers

Call out risky areas, follow-up work, or anything reviewers should focus on.
