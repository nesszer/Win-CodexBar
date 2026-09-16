$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security
function Read-SecureFile($path) {
  $json = Get-Content -Raw -Encoding UTF8 $path | ConvertFrom-Json
  $bytes = [Convert]::FromBase64String($json.payload)
  $plain = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, 'CurrentUser')
  [Text.Encoding]::UTF8.GetString($plain)
}
$s = Read-SecureFile "$env:APPDATA\CodexBar\settings.json" | ConvertFrom-Json
Write-Output "provider_configs:"
$s.provider_configs | ConvertTo-Json -Depth 5
Write-Output ""
Write-Output "enabled_providers: $($s.enabled_providers -join ', ')"
