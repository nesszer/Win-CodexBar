$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\secure-file.ps1"
$s = Read-SecureFile "$env:APPDATA\CodexBar\settings.json" | ConvertFrom-Json
Write-Output "provider_configs:"
ConvertTo-Json (ConvertTo-RedactedObject $s.provider_configs) -Depth 10
Write-Output ""
Write-Output "enabled_providers: $($s.enabled_providers -join ', ')"
