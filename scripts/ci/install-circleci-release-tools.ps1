#Requires -Version 5.1

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$env:CARGO_TERM_COLOR = "never"
$env:CARGO_TERM_PROGRESS_WHEN = "never"
$env:NO_COLOR = "1"
trap {
    Write-Host $_
    [Environment]::Exit(1)
}

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$rustVersion = "1.95.0"
$rustDistDate = "2026-04-16"
$rustHost = "x86_64-pc-windows-msvc"
$rustupInitChecksum = "86478e53f769379d7f0ebfa7c9aa97cb76ca92233f79aa2cc0dbee2efaac73c7"
$rustRoot = Join-Path $env:USERPROFILE ".rust-ms\$rustVersion"
$rustBin = Join-Path $rustRoot "bin"

function Test-Command {
    param([string]$Name)

    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Install-ChocoPackages {
    param([string[]]$Packages)

    if ($Packages.Count -eq 0) {
        return
    }

    choco feature enable -n allowGlobalConfirmation
    choco install @Packages -y --no-progress
}

function Add-RustPath {
    if (Test-Path $cargoBin) {
        $env:Path = "$cargoBin;$env:Path"
    }
    if (Test-Path $rustBin) {
        $env:Path = "$rustBin;$env:Path"
    }
}

Add-RustPath

function Get-FileSha256 {
    param([string]$Path)

    return (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
}

function Receive-File {
    param(
        [string]$Name,
        [string]$Url,
        [string]$Destination
    )

    $maxSeconds = 240

    for ($attempt = 1; $attempt -le 3; $attempt++) {
        if (Test-Path $Destination) {
            Remove-Item -Force $Destination
        }

        Write-Host "Downloading $Name (attempt $attempt)..."
        & curl.exe `
            --fail `
            --location `
            --show-error `
            --silent `
            --retry 2 `
            --retry-all-errors `
            --connect-timeout 20 `
            --max-time $maxSeconds `
            --output $Destination `
            $Url
        if ($LASTEXITCODE -eq 0 -and (Test-Path $Destination)) {
            return
        }

        $exitCode = $LASTEXITCODE
        if (Test-Path $Destination) {
            Remove-Item -Force $Destination
        }

        if ($attempt -eq 3) {
            throw "curl failed downloading $Name with exit code $exitCode"
        } else {
            Write-Host "curl failed downloading $Name with exit code $exitCode; retrying..."
        }
    }

    throw "Unable to download $Name after 3 attempts."
}

function Install-RustPackage {
    param([string]$Directory)

    $componentList = Join-Path $Directory "components"
    foreach ($component in Get-Content $componentList) {
        $componentDir = Join-Path $Directory $component
        if (-not (Test-Path $componentDir)) {
            throw "Missing Rust component directory $componentDir"
        }

        Get-ChildItem -Force $componentDir | ForEach-Object {
            Copy-Item -Force -Recurse -Path $_.FullName -Destination $rustRoot
        }
    }
}

function Install-RustArchive {
    param(
        [string]$Name,
        [string]$Url,
        [string]$Checksum
    )

    $downloadDir = Join-Path $env:TEMP "win-codexbar-rust"
    New-Item -ItemType Directory -Force $downloadDir | Out-Null
    $archive = Join-Path $downloadDir "$Name.tar.gz"
    $extractDir = Join-Path $downloadDir "$Name-extracted"

    if (Test-Path $extractDir) {
        Remove-Item -Recurse -Force $extractDir
    }
    New-Item -ItemType Directory -Force $extractDir | Out-Null

    Receive-File -Name $Name -Url $Url -Destination $archive

    $actual = Get-FileSha256 $archive
    if ($actual -ne $Checksum.ToLowerInvariant()) {
        throw "$Name SHA-256 mismatch. Expected $Checksum, got $actual"
    }

    Write-Host "Installing $Name..."
    & tar.exe -xzf $archive -C $extractDir
    if ($LASTEXITCODE -ne 0) {
        throw "tar failed extracting $Name with exit code $LASTEXITCODE"
    }

    $packageDir = Get-ChildItem -Directory $extractDir | Select-Object -First 1
    if (-not $packageDir) {
        throw "Unable to find extracted package directory for $Name"
    }

    Install-RustPackage $packageDir.FullName
}

function Install-RustToolchain {
    Write-Host "Ensuring Rust MSVC toolchain..."
    if ((Test-Command "cargo") -and (Test-Command "rustc")) {
        Write-Host "Rust toolchain already available."
        return
    }

    Write-Host "Installing Rust with verified rustup-init..."
    $downloadDir = Join-Path $env:TEMP "win-codexbar-rustup"
    New-Item -ItemType Directory -Force $downloadDir | Out-Null
    $rustupInit = Join-Path $downloadDir "rustup-init.exe"
    Receive-File `
        -Name "rustup-init-x86_64-pc-windows-msvc" `
        -Url "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" `
        -Destination $rustupInit

    $actual = Get-FileSha256 $rustupInit
    if ($actual -ne $rustupInitChecksum) {
        throw "rustup-init SHA-256 mismatch. Expected $rustupInitChecksum, got $actual"
    }

    $rustupArgs = @(
        "-y",
        "--default-toolchain", "$rustVersion-$rustHost",
        "--profile", "minimal",
        "--no-modify-path"
    )
    $rustupProcess = Start-Process `
        -FilePath $rustupInit `
        -ArgumentList $rustupArgs `
        -NoNewWindow `
        -PassThru

    if (-not $rustupProcess.WaitForExit(240000)) {
        Write-Host "rustup-init exceeded 240s; stopping it and checking whether cargo/rustc were installed..."
        Stop-Process -Id $rustupProcess.Id -Force -ErrorAction SilentlyContinue
    } elseif ($rustupProcess.ExitCode -ne 0) {
        throw "rustup-init failed with exit code $($rustupProcess.ExitCode)"
    }

    Add-RustPath
    if (-not ((Test-Command "cargo") -and (Test-Command "rustc"))) {
        throw "Missing cargo/rustc after rustup-init install."
    }
}

$fullRelease = $env:FULL_WINDOWS_RELEASE -eq "true"
$packages = @()
if (-not (Test-Command "git")) {
    $packages += "git"
}
if (-not (Test-Command "node")) {
    $packages += "nodejs-lts"
}
if ($fullRelease -and -not (Test-Command "gh")) {
    $packages += "gh"
}
if ($fullRelease -and -not (Test-Path (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"))) {
    $packages += "innosetup"
}

Install-ChocoPackages $packages

$env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
    [System.Environment]::GetEnvironmentVariable("Path", "User")
Add-RustPath

Install-RustToolchain

if (Test-Command "rustup") {
    rustup set auto-self-update disable
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Warning: rustup auto-self-update disable failed with exit code $LASTEXITCODE"
    }
} else {
    Write-Host "rustup is not installed; rust-ms provides cargo/rustc directly."
}

$env:CARGO_BUILD_TARGET = "x86_64-pc-windows-msvc"

corepack enable
if ($LASTEXITCODE -ne 0) {
    throw "corepack enable failed with exit code $LASTEXITCODE"
}

corepack prepare pnpm@10.18.1 --activate
if ($LASTEXITCODE -ne 0) {
    throw "corepack prepare failed with exit code $LASTEXITCODE"
}

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
    $vsInstall = & $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
} else {
    $vsInstall = ""
}

if (-not $vsInstall) {
    throw "Missing Visual Studio C++ build tools. Select a CircleCI Windows image with MSVC installed or add a reviewed installer step."
}

git --version
cargo --version
rustc --version
pnpm --version

if ($fullRelease) {
    gh --version
    $iscc = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
    if (-not (Test-Path $iscc)) {
        throw "Inno Setup compiler not found at $iscc"
    }
    Write-Host "Inno Setup compiler: $iscc"
} else {
    Write-Host "Skipping full-release tools for warm Windows build."
}
