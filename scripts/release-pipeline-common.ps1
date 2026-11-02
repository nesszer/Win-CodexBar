#Requires -Version 5.1
<##
Shared, side-effect-free helpers for the hosted Windows release pipeline.
This file is dot-sourced by the preflight, manifest, publisher, and focused tests.
#>

Set-StrictMode -Version Latest

function Get-NodeMajor {
    param([Parameter(Mandatory)][string]$Version)

    if ($Version -notmatch '^v(\d+)\.') {
        throw "Could not parse Node version '$Version'."
    }
    return [int]$Matches[1]
}

function Assert-NodeMajor {
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][int]$ExpectedMajor
    )

    $actualMajor = Get-NodeMajor $Version
    if ($actualMajor -ne $ExpectedMajor) {
        throw "Node $ExpectedMajor.x is required; found $Version."
    }
    return $actualMajor
}

function Get-NodeSemVersion {
    param([Parameter(Mandatory)][string]$Version)

    if ($Version -notmatch '^v?(\d+)\.(\d+)\.(\d+)') {
        throw "Could not parse Node version '$Version'."
    }
    return [version]::new([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
}

function Test-NodeVersionAtLeast {
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$Minimum
    )

    return (Get-NodeSemVersion $Version) -ge (Get-NodeSemVersion $Minimum)
}

function Assert-NodeVersionAtLeast {
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$Minimum
    )

    if (-not (Test-NodeVersionAtLeast $Version $Minimum)) {
        throw "Node $Minimum or newer is required; found $Version."
    }
}

function Normalize-GitHubRepository {
    param([Parameter(Mandatory)][string]$Url)

    $value = $Url.Trim()
    $value = $value -replace '^git@github\.com:', ''
    $value = $value -replace '^ssh://git@github\.com/', ''
    $value = $value -replace '^https?://github\.com/', ''
    $value = $value.TrimEnd('/')
    $value = $value -replace '\.git$', ''
    return $value.ToLowerInvariant()
}

function Test-CanonicalReleaseTag {
    param([AllowNull()][string]$Tag)

    return -not [string]::IsNullOrWhiteSpace($Tag) -and $Tag -match '^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$'
}

function Get-ReleaseVersionFromTag {
    param([Parameter(Mandatory)][string]$Tag)

    if (-not (Test-CanonicalReleaseTag $Tag)) {
        throw "Tag '$Tag' is not a canonical vX.Y.Z release tag."
    }
    return $Tag.Substring(1)
}

function Get-RequiredReleaseAssets {
    param([Parameter(Mandatory)][string]$Version)

    return @(
        "CodexBar-$Version-Setup.exe",
        "CodexBar-$Version-Setup.exe.sha256",
        "CodexBar-$Version-portable.exe",
        "CodexBar-$Version-portable.exe.sha256",
        "CodexBarCLI-v$Version-windows-x64.zip",
        "CodexBarCLI-v$Version-windows-x64.zip.sha256"
    )
}

function Get-AssetSha256 {
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Missing release asset: $Path"
    }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-SidecarSha256 {
    param([Parameter(Mandatory)][string]$AssetPath)

    $sidecarPath = "$AssetPath.sha256"
    if (-not (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
        throw "Missing SHA-256 sidecar: $sidecarPath"
    }
    $line = Get-Content -LiteralPath $sidecarPath | Select-Object -First 1
    if (-not $line -or $line -notmatch '^([0-9a-fA-F]{64})\s+') {
        throw "Invalid SHA-256 sidecar: $sidecarPath"
    }
    return $Matches[1].ToLowerInvariant()
}

function Assert-AssetMatchesSidecar {
    param([Parameter(Mandatory)][string]$AssetPath)

    $expected = Get-SidecarSha256 $AssetPath
    $actual = Get-AssetSha256 $AssetPath
    if ($actual -ne $expected) {
        throw "SHA-256 sidecar mismatch for $(Split-Path $AssetPath -Leaf): expected $expected, got $actual"
    }
}

function Get-ExpectedReleaseAssetPaths {
    param([Parameter(Mandatory)][string]$AssetsDir, [Parameter(Mandatory)][string]$Version)

    return @(
        Get-RequiredReleaseAssets $Version | ForEach-Object { Join-Path $AssetsDir $_ }
    )
}

function ConvertTo-JsonString {
    param([Parameter(Mandatory)]$Value)

    return ($Value | ConvertTo-Json -Depth 8)
}

# Import-table DLL names of a PE file, read directly so the check needs no
# objdump or dumpbin and cannot pass silently when neither is installed.
function Get-PeImportedDllNames {
    param([Parameter(Mandatory)][string]$Path)

    $bytes = [IO.File]::ReadAllBytes($Path)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x4550) { throw "$Path is not a PE file" }
    $sectionCount = [BitConverter]::ToUInt16($bytes, $peOffset + 6)
    $optionalHeaderSize = [BitConverter]::ToUInt16($bytes, $peOffset + 20)
    $optionalHeader = $peOffset + 24
    $magic = [BitConverter]::ToUInt16($bytes, $optionalHeader)
    $dataDirectories = switch ($magic) {
        0x10b { $optionalHeader + 96 }
        0x20b { $optionalHeader + 112 }
        default { throw "$Path has an unknown optional header magic 0x$('{0:x}' -f $magic)" }
    }
    $importRva = [BitConverter]::ToUInt32($bytes, $dataDirectories + 8)
    if ($importRva -eq 0) { return @() }

    $sectionTable = $optionalHeader + $optionalHeaderSize
    $toOffset = {
        param([uint32]$Rva)
        for ($i = 0; $i -lt $sectionCount; $i++) {
            $section = $sectionTable + 40 * $i
            $virtualAddress = [BitConverter]::ToUInt32($bytes, $section + 12)
            $virtualSize = [Math]::Max([BitConverter]::ToUInt32($bytes, $section + 8), [BitConverter]::ToUInt32($bytes, $section + 16))
            if ($Rva -ge $virtualAddress -and $Rva -lt $virtualAddress + $virtualSize) {
                return [int]($Rva - $virtualAddress + [BitConverter]::ToUInt32($bytes, $section + 20))
            }
        }
        throw "$Path has an import RVA 0x$('{0:x}' -f $Rva) outside every section"
    }

    $names = [Collections.Generic.List[string]]::new()
    for ($descriptor = & $toOffset $importRva; ; $descriptor += 20) {
        $nameRva = [BitConverter]::ToUInt32($bytes, $descriptor + 12)
        if ($nameRva -eq 0) { break }
        $start = & $toOffset $nameRva
        $end = [Array]::IndexOf($bytes, [byte]0, $start)
        if ($end -lt 0) { throw "$Path has an unterminated import name at 0x$('{0:x}' -f $start)" }
        $names.Add([Text.Encoding]::ASCII.GetString($bytes, $start, $end - $start))
    }
    return $names.ToArray()
}

# DLLs that mean the binary needs the Visual C++ Redistributable (or the
# Universal CRT forwarders) at run time instead of a statically linked CRT.
function Get-DynamicCrtImports {
    param([AllowEmptyCollection()][string[]]$DllNames)

    return @($DllNames | Where-Object { $_ -match '^(vcruntime\d+(_\d+)?d?|msvcp\d+(_[a-z0-9]+)?d?|ucrtbased?|api-ms-win-crt-[\w-]+)\.dll$' })
}
