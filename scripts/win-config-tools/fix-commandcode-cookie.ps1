$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\secure-file.ps1"

$dir = "$env:APPDATA\CodexBar"
$mcFile = "$dir\manual_cookies.json"
$taFile = "$dir\token-accounts.json"
$mcBackup = "$mcFile.bak"

Stop-CodexBarForEdit
Copy-Item -LiteralPath $mcFile -Destination $mcBackup -Force

# token value comes from token-accounts.json - no secrets in this script
$ta = Read-SecureFile $taFile | ConvertFrom-Json
$rawToken = [string]$ta.providers.commandcode.accounts[0].token
$tokenPrefix = 'Cookie: __Secure-commandcode_prod_.session_token='
$token = if ($rawToken.StartsWith($tokenPrefix)) {
  $rawToken.Substring($tokenPrefix.Length)
} else {
  $rawToken
}
if (-not $token) { throw "no commandcode token in token-accounts.json" }

try {
  $mc = Read-SecureFile $mcFile | ConvertFrom-Json
  $entry = [ordered]@{
    cookie_header = "__Secure-commandcode_prod_.session_token=$token"
    saved_at = (Get-Date -Format 'yyyy-MM-dd HH:mm')
  }
  if ($mc.cookies.PSObject.Properties.Name -contains 'commandcode') {
    $mc.cookies.commandcode = [pscustomobject]$entry
  } else {
    $mc.cookies | Add-Member -NotePropertyName commandcode -NotePropertyValue ([pscustomobject]$entry) -Force
  }
  $plain = $mc | ConvertTo-Json -Depth 6
  Write-SecureFile $mcFile $plain

  $check = (Read-SecureFile $mcFile | ConvertFrom-Json).cookies.commandcode.cookie_header
  if (-not $check.StartsWith('__Secure-commandcode_prod_.session_token=')) { throw "VERIFY FAILED" }
  Write-Output "manual cookie restored: header len $($check.Length)"
  Write-Output ("no-BOM check first bytes: " + (([System.IO.File]::ReadAllBytes($mcFile))[0..2] -join ' '))
}
catch {
  $failure = $_
  try {
    Copy-Item -LiteralPath $mcBackup -Destination $mcFile -Force
  }
  catch {
    throw "Configuration update failed and backup restore failed: $($failure.Exception.Message); restore: $($_.Exception.Message)"
  }
  throw $failure
}
