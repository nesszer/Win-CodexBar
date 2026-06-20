#Requires -Version 5.1
<#
.SYNOPSIS
    Build and run the CodexBar Tauri desktop shell for Windows.

.DESCRIPTION
    Checks that build prerequisites are installed, then builds the Tauri desktop
    shell through the Tauri CLI and launches it.

.PARAMETER Release
    Build in release mode (optimised). Default is debug.

.PARAMETER SkipBuild
    Skip the build step and run the last built binary.

.PARAMETER Verbose
    Enable debug logging via RUST_LOG for the Tauri desktop shell.

.EXAMPLE
    .\dev.ps1                  # debug build + run
    .\dev.ps1 -Release         # release build + run
    .\dev.ps1 -SkipBuild       # run last build
    .\dev.ps1 -Verbose         # debug build + run with verbose logging
#>

param(
    [switch]$Release,
    [switch]$SkipBuild,
    [switch]$Verbose
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = $PSScriptRoot
$TauriFrontendDir = Join-Path $RepoRoot "apps\desktop-tauri"
$TargetDir = Join-Path $RepoRoot "target"
$DesktopBinaryName = "codexbar-desktop-tauri.exe"

function Get-RustHostTriple {
    if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
        return $null
    }

    $versionDetails = rustc -vV 2>$null
    $hostLine = $versionDetails | Where-Object { $_ -like 'host:*' } | Select-Object -First 1
    if (-not $hostLine) {
        return $null
    }

    return ($hostLine -replace '^host:\s*', '').Trim()
}

function Get-DesktopBinaryCandidates {
    param(
        [string]$Profile,
        [string]$ConfiguredTarget
    )

    $candidates = @(
        (Join-Path $TargetDir "$Profile\$DesktopBinaryName")
    )

    if ($ConfiguredTarget) {
        $candidates += Join-Path $TargetDir "$ConfiguredTarget\$Profile\$DesktopBinaryName"
    }

    $candidates += @(
        (Join-Path $TargetDir "x86_64-pc-windows-msvc\$Profile\$DesktopBinaryName"),
        (Join-Path $TargetDir "x86_64-pc-windows-gnu\$Profile\$DesktopBinaryName")
    )

    return $candidates | Select-Object -Unique
}

# ── Ensure known tool paths are in current session PATH ─────────────────────

$knownPaths = @("$env:USERPROFILE\.cargo\bin", "C:\mingw64\bin")
foreach ($p in $knownPaths) {
    if ((Test-Path $p) -and ($env:PATH -notlike "*$p*")) {
        $env:PATH = "$p;$env:PATH"
    }
}

# ── Check prerequisites ─────────────────────────────────────────────────────

$hasCargo = [bool](Get-Command cargo -ErrorAction SilentlyContinue)
$rustHostTriple = Get-RustHostTriple
$needsDlltool = $rustHostTriple -like '*-windows-gnu'
$hasDlltool = [bool](Get-Command dlltool -ErrorAction SilentlyContinue)

if (-not $hasCargo -or ($needsDlltool -and -not $hasDlltool)) {
    $missing = @()
    if (-not $hasCargo)   { $missing += "cargo (Rust)" }
    if ($needsDlltool -and -not $hasDlltool) { $missing += "dlltool (MinGW-w64)" }
    Write-Host "Missing prerequisites: $($missing -join ', ')" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Install the required tools, restart your terminal, then rerun .\dev.ps1." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Required:" -ForegroundColor Cyan
    Write-Host "  - Rust/Cargo: https://rustup.rs/ or 'mise install rust@stable'" -ForegroundColor Cyan
    Write-Host "  - Node.js + pnpm: https://nodejs.org/ and 'corepack enable pnpm', or mise-managed node/pnpm" -ForegroundColor Cyan
    Write-Host "  - Microsoft Visual Studio Build Tools with the Desktop development with C++ workload" -ForegroundColor Cyan
    if ($needsDlltool) {
        Write-Host ""
        Write-Host "This shell is using a GNU Rust target, which also needs MinGW-w64 dlltool." -ForegroundColor Yellow
        Write-Host "Install MinGW-w64 or switch Rust to the MSVC target: rustup default stable-x86_64-pc-windows-msvc" -ForegroundColor Yellow
    }
    exit 1
}

$pnpmCommand = Get-Command pnpm.cmd -ErrorAction SilentlyContinue
if (-not $pnpmCommand) {
    Write-Host "ERROR: pnpm not found." -ForegroundColor Red
    Write-Host "Install pnpm to build apps/desktop-tauri before running this script." -ForegroundColor Yellow
    exit 1
}

# ── Build ────────────────────────────────────────────────────────────────────

if (-not $SkipBuild) {
    Push-Location $TauriFrontendDir
    try {
        if ($Release) {
            Write-Host "Building CodexBar Desktop (release, no bundle)..." -ForegroundColor Cyan
            & $pnpmCommand.Source run tauri:build
        } else {
            Write-Host "Building CodexBar Desktop (debug, no bundle)..." -ForegroundColor Cyan
            & $pnpmCommand.Source run tauri:build:debug
        }
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } finally {
        Pop-Location
    }
}

# ── Run ──────────────────────────────────────────────────────────────────────

# Binary may be under target/<profile> or target/<triple>/<profile>
$profile = if ($Release) { "release" } else { "debug" }
$cargoConfigPath = Join-Path $RepoRoot ".cargo\config.toml"
$configuredTarget = $null

if ($env:CARGO_BUILD_TARGET) {
    $configuredTarget = $env:CARGO_BUILD_TARGET
} elseif (Test-Path $cargoConfigPath) {
    $targetLine = Get-Content $cargoConfigPath | Where-Object { $_ -match '^\s*target\s*=\s*"([^"]+)"' } | Select-Object -First 1
    if ($targetLine -and $targetLine -match '^\s*target\s*=\s*"([^"]+)"') {
        $configuredTarget = $Matches[1]
    }
}

$candidates = Get-DesktopBinaryCandidates -Profile $profile -ConfiguredTarget $configuredTarget

$binary = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $binary) {
    Write-Host "ERROR: Binary not found. Searched:" -ForegroundColor Red
    $candidates | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
    Write-Host "Run without -SkipBuild to build first." -ForegroundColor Yellow
    exit 1
}

if ($Verbose) {
    if (-not $env:RUST_LOG) {
        $env:RUST_LOG = "debug"
    }
    Write-Host "Verbose logging enabled via RUST_LOG=$env:RUST_LOG" -ForegroundColor Cyan
}
if (-not $env:TAURI_DEV) {
    $env:TAURI_DEV = "0"
}

Write-Host ""
Write-Host "Starting CodexBar Desktop..." -ForegroundColor Green
& $binary
