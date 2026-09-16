$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security

function Read-SecureFile($path) {
  $json = Get-Content -Raw -Encoding UTF8 $path | ConvertFrom-Json
  $bytes = [Convert]::FromBase64String($json.payload)
  $plain = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, 'CurrentUser')
  [Text.Encoding]::UTF8.GetString($plain)
}

Write-Output '===== token-accounts.json ====='
Read-SecureFile "$env:APPDATA\CodexBar\token-accounts.json"
