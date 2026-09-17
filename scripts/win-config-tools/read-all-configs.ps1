$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\secure-file.ps1"

Write-Output '===== settings.json ====='
ConvertTo-RedactedJson (Read-SecureFile "$env:APPDATA\CodexBar\settings.json")
Write-Output ''
Write-Output '===== api_keys.json ====='
ConvertTo-RedactedJson (Read-SecureFile "$env:APPDATA\CodexBar\api_keys.json")
Write-Output ''
Write-Output '===== manual_cookies.json ====='
ConvertTo-RedactedJson (Read-SecureFile "$env:APPDATA\CodexBar\manual_cookies.json")
