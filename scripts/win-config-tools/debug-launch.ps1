$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\secure-file.ps1"

Stop-CodexBarForEdit

$dir = "$env:APPDATA\CodexBar"
$taFile = "$dir\token-accounts.json"
$mcFile = "$dir\manual_cookies.json"

$taBackup = "$taFile.bak"
$mcBackup = "$mcFile.bak"
Copy-Item -LiteralPath $taFile -Destination $taBackup -Force
Copy-Item -LiteralPath $mcFile -Destination $mcBackup -Force

try {
  # 1. revert token to bare value
  $ta = Read-SecureFile $taFile | ConvertFrom-Json
  $acct = $ta.providers.commandcode.accounts[0]
  $pfx = 'Cookie: __Secure-commandcode_prod_.session_token='
  if ($acct.token.StartsWith($pfx)) {
    $acct.token = $acct.token.Substring($pfx.Length)
    Write-SecureFile $taFile ($ta | ConvertTo-Json -Depth 6)
    Write-Output "token reverted to bare form"
  }

  # 2. re-add full-form manual cookie
  $mc = Read-SecureFile $mcFile | ConvertFrom-Json
  $entry = [ordered]@{
    cookie_header = ("__Secure-commandcode_prod_.session_token=" + $acct.token)
    saved_at = (Get-Date -Format 'yyyy-MM-dd HH:mm')
  }
  if ($mc.cookies.PSObject.Properties.Name -contains 'commandcode') {
    $mc.cookies.commandcode = [pscustomobject]$entry
  } else {
    $mc.cookies | Add-Member -NotePropertyName commandcode -NotePropertyValue ([pscustomobject]$entry) -Force
  }
  Write-SecureFile $mcFile ($mc | ConvertTo-Json -Depth 6)
  $hdr = (Read-SecureFile $mcFile | ConvertFrom-Json).cookies.commandcode.cookie_header
  Write-Output "manual cookie restored: header len $($hdr.Length)"
}
catch {
  $failure = $_
  $restoreErrors = @()
  foreach ($backup in @(
      @{ Source = $taBackup; Target = $taFile },
      @{ Source = $mcBackup; Target = $mcFile }
    )) {
    try {
      Copy-Item -LiteralPath $backup.Source -Destination $backup.Target -Force
    }
    catch {
      $restoreErrors += "$($backup.Target): $($_.Exception.Message)"
    }
  }
  if ($restoreErrors.Count -gt 0) {
    throw "Configuration update failed and backup restore failed ($($restoreErrors -join '; ')): $($failure.Exception.Message)"
  }
  throw $failure
}

# 3. preserve the old log before starting a clean capture
$log = "$dir\logs\codexbar-desktop.log"
if (Test-Path -LiteralPath $log) {
  $archive = "$log.pre-debug-$([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))-$([guid]::NewGuid().ToString('N')).log"
  Move-Item -LiteralPath $log -Destination $archive
  Write-Output "previous log preserved at $archive"
}

# 4. launch with debug logging
$env:RUST_LOG = 'debug'
Start-Process -FilePath "$env:LOCALAPPDATA\Programs\CodexBar\codexbar.exe" -WorkingDirectory "$env:LOCALAPPDATA\Programs\CodexBar"
Write-Output "app launched with RUST_LOG=debug"
