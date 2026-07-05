# Win-CodexBar local checks

Win-CodexBar does not currently use hosted CI/CD. Run checks locally before PRs
and releases.

## PR checks

Run the smallest check that covers your change:

```powershell
powershell.exe -ExecutionPolicy Bypass -NoProfile -File scripts\local-check.ps1
powershell.exe -ExecutionPolicy Bypass -NoProfile -File scripts\local-check.ps1 -Format -Clippy
powershell.exe -ExecutionPolicy Bypass -NoProfile -File scripts\local-check.ps1 -All -Version 0.38.2
```

## Release checks

The canonical Windows release path uses the release scripts:

```powershell
powershell.exe -File scripts\release-doctor.ps1 -Version 0.38.2
powershell.exe -File scripts\windows-release-build.ps1 -Ref v0.38.2 -SmokeInstall
```

Use the version/ref being released.

## Release flow

1. Tag the release, for example `v0.38.2`.
2. Run `scripts\release-doctor.ps1`.
3. Run `scripts\windows-release-build.ps1 -SmokeInstall`.
4. Upload the verified installer, portable exe, and SHA-256 sidecars to GitHub Releases.
5. Submit the Winget manifest update after the GitHub installer URL is stable.
