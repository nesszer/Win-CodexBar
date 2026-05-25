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
    $pollSeconds = 5

    for ($attempt = 1; $attempt -le 3; $attempt++) {
        if (Test-Path $Destination) {
            Remove-Item -Force $Destination
        }

        Write-Host "Downloading $Name (attempt $attempt)..."
        $job = Start-BitsTransfer `
            -Source $Url `
            -Destination $Destination `
            -Asynchronous `
            -DisplayName "WinCodexBarRust-$Name" `
            -ErrorAction Stop

        try {
            $jobId = $job.JobId
            $elapsed = 0
            while ($elapsed -lt $maxSeconds) {
                Start-Sleep -Seconds $pollSeconds
                $elapsed += $pollSeconds
                $job = Get-BitsTransfer -ErrorAction Stop |
                    Where-Object { $_.JobId -eq $jobId } |
                    Select-Object -First 1
                if (-not $job) {
                    throw "BITS job disappeared while downloading $Name."
                }

                if ($job.JobState -eq "Transferred") {
                    Complete-BitsTransfer -BitsJob $job
                    return
                }

                if ($job.JobState -eq "TransientError") {
                    Resume-BitsTransfer -BitsJob $job -Asynchronous
                } elseif ($job.JobState -eq "Error") {
                    $message = $job.ErrorDescription
                    Remove-BitsTransfer -BitsJob $job -Confirm:$false
                    throw "BITS failed downloading $Name`: $message"
                }

                if (($elapsed % 30) -eq 0) {
                    Write-Host "Still downloading $Name after ${elapsed}s..."
                }
            }

            Remove-BitsTransfer -BitsJob $job -Confirm:$false
            Write-Host "Timed out downloading $Name after ${maxSeconds}s."
        } catch {
            if ($job) {
                Remove-BitsTransfer -BitsJob $job -Confirm:$false -ErrorAction SilentlyContinue
            }
            if ($attempt -eq 3) {
                throw
            }
            Write-Host $_
        }
    }

    throw "Unable to download $Name after 3 attempts."
}

function Install-RustPackage {
    param([string]$Directory)

    $componentList = Join-Path $Directory "components"
    foreach ($component in Get-Content $componentList) {
        $manifest = Join-Path (Join-Path $Directory $component) "manifest.in"
        foreach ($entry in Get-Content $manifest) {
            if ($entry.StartsWith("file:") -or $entry.StartsWith("dir:")) {
                if ($entry.StartsWith("file:")) {
                    $relativePath = $entry.Substring(5)
                } else {
                    $relativePath = $entry.Substring(4)
                }
                $source = Join-Path (Join-Path $Directory $component) $relativePath
                $target = Join-Path $rustRoot $relativePath
                $targetParent = Split-Path -Parent $target
                if ($targetParent -and -not (Test-Path $targetParent)) {
                    New-Item -ItemType Directory -Force $targetParent | Out-Null
                }
                Move-Item -Force $source $target
            }
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

    if (Test-Path $rustRoot) {
        Write-Host "Removing incomplete cached Rust toolchain at $rustRoot..."
        Remove-Item -Recurse -Force $rustRoot
    }
    New-Item -ItemType Directory -Force $rustRoot | Out-Null

    $baseUrl = "https://static.rust-lang.org/dist/$rustDistDate"
    Install-RustArchive `
        -Name "rustc-$rustVersion-$rustHost" `
        -Url "$baseUrl/rustc-$rustVersion-$rustHost.tar.gz" `
        -Checksum "b1101cba184fda0da47658772d04423fdb86cc9ed888cac3b29d0e9f55faec53"
    Install-RustArchive `
        -Name "cargo-$rustVersion-$rustHost" `
        -Url "$baseUrl/cargo-$rustVersion-$rustHost.tar.gz" `
        -Checksum "2d68113a00b98f0dec6d0e8473f82e08cec00c392115933a57dbfe9d3c8b2d8c"
    Install-RustArchive `
        -Name "rust-std-$rustVersion-$rustHost" `
        -Url "$baseUrl/rust-std-$rustVersion-$rustHost.tar.gz" `
        -Checksum "aa56f95b4817f562c0ada0abee3511a802a948303404e8fc872d0371ae0693fc"

    Add-RustPath
    if (-not ((Test-Command "cargo") -and (Test-Command "rustc"))) {
        throw "Missing cargo/rustc after Rust toolchain install."
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
