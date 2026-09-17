$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\secure-file.ps1"

Write-Output '===== token-accounts.json ====='
ConvertTo-RedactedJson (Read-SecureFile "$env:APPDATA\CodexBar\token-accounts.json")
