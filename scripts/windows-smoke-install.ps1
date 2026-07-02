param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,

    [string]$ExpectedVersion = "",

    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\CodexBar",

    [switch]$LeaveInstalled
)

$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "[smoke] $Message"
}

function Assert-Path {
    param(
        [string]$Path,
        [string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "Missing $Label at $Path"
    }
}

$isWindowsHost = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
    [System.Runtime.InteropServices.OSPlatform]::Windows
)
if (-not $isWindowsHost) {
    throw "This smoke test must run on Windows."
}

$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
if ([IO.Path]::GetExtension($installer).ToLowerInvariant() -ne ".exe") {
    throw "Expected an Inno Setup .exe installer, got: $installer"
}

Write-Step "installer: $installer"
$installerHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
Write-Step "installer sha256: $installerHash"

$signature = Get-AuthenticodeSignature -FilePath $installer
if ($signature.Status -eq "Valid") {
    Write-Step "installer signature: valid ($($signature.SignerCertificate.Subject))"
} else {
    Write-Step "installer signature: $($signature.Status)"
}

foreach ($name in @("codexbar", "codexbar-desktop", "codexbar-desktop-tauri")) {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
}

$logDir = Join-Path $env:TEMP "codexbar-installer-smoke"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$installLog = Join-Path $logDir "install.log"

Write-Step "running silent install"
$installArgs = @(
    "/VERYSILENT",
    "/SUPPRESSMSGBOXES",
    "/NORESTART",
    "/LOG=`"$installLog`""
)
$install = Start-Process -FilePath $installer -ArgumentList $installArgs -Wait -PassThru
if ($install.ExitCode -notin @(0, 3010)) {
    throw "Installer exited with $($install.ExitCode). Log: $installLog"
}

$desktopExe = Join-Path $InstallDir "codexbar.exe"
$cliExe = Join-Path $InstallDir "codexbar-cli.exe"
$legacyDesktopExe = Join-Path $InstallDir "codexbar-desktop.exe"
$icon = Join-Path $InstallDir "icon.ico"
Assert-Path -Path $desktopExe -Label "installed desktop executable"
Assert-Path -Path $cliExe -Label "installed CLI executable"
Assert-Path -Path $legacyDesktopExe -Label "installed desktop compatibility executable"
Assert-Path -Path $icon -Label "icon"

$desktopHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $desktopExe).Hash.ToLowerInvariant()
Write-Step "installed codexbar.exe sha256: $desktopHash"
$cliHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $cliExe).Hash.ToLowerInvariant()
Write-Step "installed codexbar-cli.exe sha256: $cliHash"
$legacyDesktopHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $legacyDesktopExe).Hash.ToLowerInvariant()
Write-Step "installed codexbar-desktop.exe sha256: $legacyDesktopHash"

$verifyExecutablesScript = Join-Path (Split-Path -Parent $PSScriptRoot) "scripts\verify-windows-executables.ps1"
if (-not (Test-Path -LiteralPath $verifyExecutablesScript)) {
    throw "Executable verification script not found: $verifyExecutablesScript"
}
& $verifyExecutablesScript `
    -DesktopExe $desktopExe `
    -CliExe $cliExe `
    -LegacyDesktopExe $legacyDesktopExe `
    -CheckCliStdout

if ($ExpectedVersion) {
    $versionOutput = (& $cliExe --version) -join "`n"
    if ($LASTEXITCODE -ne 0) {
        throw "codexbar-cli.exe --version exited with $LASTEXITCODE"
    }
    if ($versionOutput -notmatch [regex]::Escape($ExpectedVersion)) {
        throw "Expected codexbar-cli.exe --version to mention $ExpectedVersion, got: $versionOutput"
    }
    Write-Step "CLI version output: $versionOutput"
}

$helpOutput = (& $cliExe --help) -join "`n"
if ($LASTEXITCODE -ne 0) {
    throw "codexbar-cli.exe --help exited with $LASTEXITCODE"
}
if ($helpOutput -notmatch "Usage:" -or $helpOutput -notmatch "diagnose") {
    throw "codexbar-cli.exe --help did not print CLI help."
}
Write-Step "CLI help output: ok"

$uninstallKeys = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCodexBar_is1",
    "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\WinCodexBar_is1",
    "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\WinCodexBar_is1"
)
$uninstallEntry = $null
foreach ($key in $uninstallKeys) {
    if (Test-Path $key) {
        $uninstallEntry = Get-ItemProperty $key
        break
    }
}
if ($null -eq $uninstallEntry) {
    throw "Missing WinCodexBar uninstall registry entry."
}

Write-Step "registry display name: $($uninstallEntry.DisplayName)"
if ($ExpectedVersion -and $uninstallEntry.DisplayVersion -ne $ExpectedVersion) {
    throw "Expected DisplayVersion $ExpectedVersion, got $($uninstallEntry.DisplayVersion)"
}

$startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$shortcutCandidates = @(
    (Join-Path $startMenu "CodexBar.lnk"),
    (Join-Path $startMenu "CodexBar\CodexBar.lnk")
)
$shortcut = $shortcutCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $shortcut) {
    throw "Missing Start Menu shortcut. Checked: $($shortcutCandidates -join ', ')"
}
Write-Step "Start Menu shortcut: $shortcut"

if (-not $LeaveInstalled) {
    $uninstallLog = Join-Path $logDir "uninstall.log"
    $uninstallCommand = [string]$uninstallEntry.UninstallString
    if (-not $uninstallCommand) {
        throw "UninstallString is empty."
    }

    $uninstaller = $uninstallCommand.Trim('"')
    Write-Step "running silent uninstall"
    $uninstallArgs = @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/LOG=`"$uninstallLog`""
    )
    $uninstall = Start-Process -FilePath $uninstaller -ArgumentList $uninstallArgs -Wait -PassThru
    if ($uninstall.ExitCode -notin @(0, 3010)) {
        throw "Uninstaller exited with $($uninstall.ExitCode). Log: $uninstallLog"
    }
    foreach ($leftover in @($desktopExe, $cliExe, $legacyDesktopExe)) {
        if (Test-Path -LiteralPath $leftover) {
            throw "Executable still exists after uninstall: $leftover"
        }
    }
}

Write-Step "ok"
