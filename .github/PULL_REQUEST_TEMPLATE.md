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

There is no hosted CI right now. Run the relevant local checks and list the exact commands/results. If a check is not relevant, say why.

- [ ] `powershell.exe -ExecutionPolicy Bypass -NoProfile -File scripts\local-check.ps1`
- [ ] For full pre-release validation: `powershell.exe -ExecutionPolicy Bypass -NoProfile -File scripts\local-check.ps1 -All -Version <version>`
- [ ] For installer/release changes: `powershell.exe -File scripts\windows-release-build.ps1 -Ref <ref> -SmokeInstall`
- [ ] Thermo-nuclear code quality review completed before submitting: https://github.com/cursor/plugins/blob/main/cursor-team-kit/skills/thermo-nuclear-code-quality-review/SKILL.md
- [ ] Other:

## UI / tray proof

For UI, tray, settings, or visual behavior changes, use CUA Driver for visual proof. If CUA Driver cannot be used, explain why and attach equivalent manual proof.

- [ ] Not applicable
- [ ] CUA Driver visual proof attached
- [ ] CUA Driver could not be used; equivalent manual proof and explanation attached

## Notes for reviewers

Call out risky areas, follow-up work, or anything reviewers should focus on.
