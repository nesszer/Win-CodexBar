#Requires -Version 5.1
<##
Focused, dependency-free checks for the pure release pipeline helpers.
Run with: powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\release-pipeline.tests.ps1
#>

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $scriptRoot 'release-pipeline-common.ps1')

function Assert-True {
    param([Parameter(Mandatory)][bool]$Condition, [Parameter(Mandatory)][string]$Message)
    if (-not $Condition) { throw "Assertion failed: $Message" }
}

function Assert-Equal {
    param([Parameter(Mandatory)]$Actual, [Parameter(Mandatory)]$Expected, [Parameter(Mandatory)][string]$Message)
    if ($Actual -ne $Expected) { throw "Assertion failed: $Message (actual '$Actual', expected '$Expected')" }
}

function Assert-Throws {
    param([Parameter(Mandatory)][scriptblock]$Block, [Parameter(Mandatory)][string]$Message)
    $thrown = $false
    try { & $Block } catch { $thrown = $true }
    Assert-True $thrown $Message
}
Assert-Equal (Get-NodeMajor 'v24.18.0') 24 'Node 24 major parsing'
Assert-Equal (Assert-NodeMajor 'v24.18.0' 24) 24 'Node 24 requirement'
Assert-Throws { Assert-NodeMajor 'v23.11.0' 24 } 'non-24 Node major rejected by release prerequisite'
Assert-True (-not (Test-NodeVersionAtLeast 'v24.18.0' 'v24.22.1')) 'older Node 24 patch is below the minimum'
Assert-True (Test-NodeVersionAtLeast 'v24.22.1' 'v24.22.1') 'the minimum Node version itself is accepted'
Assert-True (Test-NodeVersionAtLeast 'v24.100.0' 'v24.22.1') 'Node minor versions compare numerically'
Assert-Throws { Assert-NodeVersionAtLeast 'v24.18.0' 'v24.22.1' } 'vulnerable Node 24 patch rejected by release prerequisite'
Assert-Throws { Get-NodeSemVersion 'node24' } 'unparsable Node version rejected'

$prerequisiteText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot 'install-release-prerequisites.ps1')
Assert-True ($prerequisiteText -match '\$requiredNodeMajor\s*=\s*24') 'release prerequisite pins Node major 24'
$packageJson = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot '..\apps\desktop-tauri\package.json') | ConvertFrom-Json
$expectedPnpm = [string]$packageJson.packageManager -replace '^pnpm@', ''
Assert-True ($packageJson.packageManager -match '^pnpm@\d+\.\d+\.\d+$') 'package metadata pins an exact pnpm semver'
Assert-True ($prerequisiteText -match '\$expectedPnpm\s*=') 'release prerequisite derives pnpm from package metadata'
Assert-True ($prerequisiteText -match 'pnpm@\$expectedPnpm') 'release prerequisite activates the derived pnpm version'
Assert-True ($prerequisiteText -notmatch [regex]::Escape("pnpm $expectedPnpm,")) 'release prerequisite does not duplicate the pnpm version in status text'

Assert-Equal (Normalize-GitHubRepository 'https://github.com/nesszer/Win-CodexBar.git') 'nesszer/win-codexbar' 'HTTPS canonical URL'
Assert-Equal (Normalize-GitHubRepository 'git@github.com:nesszer/Win-CodexBar.git') 'nesszer/win-codexbar' 'SSH canonical URL'
Assert-True (Test-CanonicalReleaseTag 'v1.2.3') 'canonical release tag accepted'
Assert-True (-not (Test-CanonicalReleaseTag 'v1.2.3-rc.1')) 'prerelease tag rejected'
Assert-True (-not (Test-CanonicalReleaseTag 'v01.2.3')) 'leading-zero tag rejected'
Assert-Equal (Get-ReleaseVersionFromTag 'v0.48.0') '0.48.0' 'version extraction'
Assert-Throws { Get-ReleaseVersionFromTag 'v0.48.0+build' } 'invalid version extraction fails'

$assetNames = Get-RequiredReleaseAssets '0.48.0'
Assert-Equal $assetNames.Count 6 'exactly six release asset names including sidecars'
Assert-Equal $assetNames[0] 'CodexBar-0.48.0-Setup.exe' 'installer name'
Assert-Equal $assetNames[3] 'CodexBar-0.48.0-portable.exe.sha256' 'portable sidecar name'
Assert-Equal $assetNames[4] 'CodexBarCLI-v0.48.0-windows-x64.zip' 'CLI archive name'
Assert-Equal $assetNames[5] 'CodexBarCLI-v0.48.0-windows-x64.zip.sha256' 'CLI sidecar name'

$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('win-codexbar-release-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
try {
    $asset = Join-Path $testRoot 'CodexBar-0.48.0-Setup.exe'
    [IO.File]::WriteAllText($asset, 'deterministic fixture')
    $hash = Get-AssetSha256 $asset
    [IO.File]::WriteAllText("$asset.sha256", "$hash  $(Split-Path $asset -Leaf)`n")
    Assert-Equal (Get-SidecarSha256 $asset) $hash 'sidecar parser'
    Assert-AssetMatchesSidecar $asset
    [IO.File]::WriteAllText("$asset.sha256", ('0' * 64) + "  bad`n")
    Assert-Throws { Assert-AssetMatchesSidecar $asset } 'sidecar mismatch fails'
} finally {
    if ([IO.Directory]::Exists($testRoot)) { [IO.Directory]::Delete($testRoot, $true) }
}

$builderText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot 'windows-release-build.ps1')
$legacySwitch = 'Upload' + 'Release'
$clobberFlag = '--' + 'clobber'
Assert-True ($builderText -notmatch $legacySwitch) 'legacy upload parameter removed'
Assert-True ($builderText -notmatch $clobberFlag) 'builder has no clobber upload path'
$publisherText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot 'publish-github-release.ps1')
Assert-True ($publisherText -notmatch $clobberFlag) 'publisher has no clobber flag'
Assert-True ($publisherText -match '\[Parameter\(Mandatory\)\]\[string\]\$Sha') 'publisher requires an explicit immutable SHA'
Assert-True ($publisherText -notmatch 'CIRCLE_SHA1|RELEASE_SHA') 'publisher has no CircleCI SHA dependency'

$artifactConfigText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot '..\.signpath\artifact-configuration.xml')
Assert-True ($artifactConfigText -match '<pe-file path="CodexBar-\$\{version\}-Setup\.exe"') 'SignPath config signs installer'
Assert-True ($artifactConfigText -match '<pe-file path="CodexBar-\$\{version\}-portable\.exe"') 'SignPath config signs portable executable'
Assert-True ($artifactConfigText -match '<zip-file path="CodexBarCLI-v\$\{version\}-windows-x64\.zip"') 'SignPath config opens the CLI ZIP'
Assert-True ($artifactConfigText -match '<pe-file path="codexbar-cli\.exe"') 'SignPath config signs nested CLI executable'

$finalizerText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot 'finalize-signed-release.ps1')
Assert-True ($finalizerText -match 'Get-AuthenticodeSignature') 'finalizer verifies Authenticode'
Assert-True ($finalizerText -match 'ExpectedSignerThumbprint') 'finalizer can enforce the issued signer certificate'
Assert-True ($finalizerText -match 'Expand-Archive') 'finalizer inspects nested CLI archive'
Assert-True ($finalizerText -match 'codexbar-cli\.exe at its root') 'finalizer enforces the configured CLI archive path'
Assert-True ($finalizerText -match 'CLI ZIP must contain exactly one file') 'finalizer rejects extra CLI archive files'
Assert-True ($finalizerText -match 'Invoke-ManifestEmission -AssetsDir \$finalAssetsDir') 'finalizer emits manifest from signed-only assets'
Assert-True ($finalizerText -notmatch '\$BuildOutputDir|keeping unsigned|fallback') 'finalizer has no unsigned fallback'
Assert-True ($publisherText -match 'unexpected nested files') 'publisher rejects nested final-bundle files'

$releaseWorkflowText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot '..\.github\workflows\release.yml')
Assert-True ($releaseWorkflowText -match 'signpath/github-action-submit-signing-request@v3') 'production workflow uses current SignPath action'
Assert-True ($releaseWorkflowText -match 'actions/upload-artifact@v4') 'production workflow uploads a GitHub artifact'
Assert-True ($releaseWorkflowText -match 'signing-policy-slug: release-signing') 'production workflow pins release-signing'
Assert-True ($releaseWorkflowText -match 'CodexBarCLI-v\$version-windows-x64\.zip') 'production workflow includes the CLI ZIP'
Assert-True ($releaseWorkflowText -match 'finalize-signed-release\.ps1') 'production workflow runs signed finalization'
Assert-True ($releaseWorkflowText -match 'actions/download-artifact@v4') 'production workflow transfers only the verified bundle to publisher'
Assert-True ($releaseWorkflowText -match 'contents: write') 'production publisher has release write permission'
Assert-True ($releaseWorkflowText -match 'SIGNPATH_RELEASE_CERT_THUMBPRINT') 'production workflow requires the issued certificate thumbprint'
Assert-True ($releaseWorkflowText -match [regex]::Escape('ref: ${{ needs.sign.outputs.sha }}')) 'publisher checks out the immutable signed SHA'
Assert-True ($releaseWorkflowText -notmatch 'trigger-circleci|if:.*SIGNPATH_API_TOKEN') 'production workflow cannot skip SignPath or delegate to CircleCI'

$testWorkflowText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot '..\.github\workflows\signpath-test.yml')
Assert-True ($testWorkflowText -match 'workflow_dispatch') 'SignPath test is manually dispatched'
Assert-True ($testWorkflowText -match 'signing-policy-slug: test-signing') 'SignPath test pins test-signing'
Assert-True ($testWorkflowText -match 'source_ref:') 'SignPath test builds a reviewed workflow source ref'
Assert-True ($testWorkflowText -match 'AllowTagSourceMismatch') 'SignPath test separates version label from source commit'
Assert-True ($testWorkflowText -notmatch 'gh release create|Publish draft') 'SignPath test cannot publish a release'

$circleConfigText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot '..\.circleci\config.yml')
Assert-True ($circleConfigText -notmatch 'release-build|release-publish|release-approval') 'CircleCI has no tag release jobs'
Assert-True ($circleConfigText -notmatch '(?ms)^  release:\s*$') 'CircleCI has no tag release workflow'

# #761: release binaries link the C runtime statically and the verifier rejects dynamic CRT imports.
Assert-True ($builderText -match [regex]::Escape('-C target-feature=+crt-static')) 'release build links the C runtime statically'
$verifierText = Get-Content -Raw -LiteralPath (Join-Path $scriptRoot 'verify-windows-executables.ps1')
Assert-True ($verifierText -match 'Get-DynamicCrtImports \(Get-PeImportedDllNames') 'executable verifier rejects dynamic CRT imports'
Assert-Equal (@(Get-DynamicCrtImports @('KERNEL32.dll', 'VCRUNTIME140.dll', 'vcruntime140_1.dll', 'MSVCP140.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'ucrtbase.dll', 'WS2_32.dll', 'api-ms-win-core-synch-l1-2-0.dll', 'msvcrt.dll', 'VCRUNTIME140D.dll', 'ucrtbased.dll')) -join ',') 'VCRUNTIME140.dll,vcruntime140_1.dll,MSVCP140.dll,api-ms-win-crt-runtime-l1-1-0.dll,ucrtbase.dll,VCRUNTIME140D.dll,ucrtbased.dll' 'dynamic CRT import classification'
Assert-Equal (@(Get-DynamicCrtImports @()).Count) 0 'no imports means no dynamic CRT'
$systemExe = Join-Path $env:SystemRoot 'System32\where.exe'
$systemImports = @(Get-PeImportedDllNames $systemExe)
Assert-True ($systemImports.Count -gt 0 -and ($systemImports | Where-Object { $_ -ieq 'KERNEL32.dll' -or $_ -like 'api-ms-win-core-*' })) "PE import reader lists the imports of $systemExe"
Assert-Throws { Get-PeImportedDllNames (Join-Path $scriptRoot 'release-pipeline-common.ps1') } 'PE import reader rejects a non-PE file'

Write-Host 'Release pipeline focused tests passed.'
