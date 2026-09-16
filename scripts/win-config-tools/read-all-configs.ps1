$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security

function Read-SecureFile($path) {
  $json = Get-Content -Raw -Encoding UTF8 $path | ConvertFrom-Json
  $bytes = [Convert]::FromBase64String($json.payload)
  $plain = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, 'CurrentUser')
  [Text.Encoding]::UTF8.GetString($plain)
}

Write-Output '===== settings.json ====='
Read-SecureFile "$env:APPDATA\CodexBar\settings.json"
Write-Output ''
Write-Output '===== api_keys.json ====='
Read-SecureFile "$env:APPDATA\CodexBar\api_keys.json"
Write-Output ''
Write-Output '===== manual_cookies.json ====='
Read-SecureFile "$env:APPDATA\CodexBar\manual_cookies.json"
