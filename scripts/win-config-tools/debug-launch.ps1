$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security

Get-Process -Name codexbar, codexbar-desktop -ErrorAction SilentlyContinue |
  ForEach-Object { Write-Output "killing $($_.ProcessName)"; Stop-Process -Id $_.Id -Force }
Start-Sleep -Seconds 2

$dir = "$env:APPDATA\CodexBar"
$taFile = "$dir\token-accounts.json"
$mcFile = "$dir\manual_cookies.json"

function Read-SecureFile($path) {
  $json = Get-Content -Raw -Encoding UTF8 $path | ConvertFrom-Json
  $bytes = [Convert]::FromBase64String($json.payload)
  [Text.Encoding]::UTF8.GetString([System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, 'CurrentUser'))
}
function Write-SecureFile($path, $plainJson) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($plainJson)
  $enc = [System.Security.Cryptography.ProtectedData]::Protect($bytes, $null, 'CurrentUser')
  $wrapper = @{
    format = 'codexbar.secure-file'
    version = 1
    protection = 'windows-dpapi-user'
    payload = [Convert]::ToBase64String($enc)
  } | ConvertTo-Json
  [System.IO.File]::WriteAllText($path, $wrapper, (New-Object Text.UTF8Encoding($false)))
}

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
Write-Output "manual cookie: $($hdr.Substring(0, 40))... (len $($hdr.Length))"

# 3. truncate old log for clean capture
$log = "$dir\logs\codexbar-desktop.log"
if (Test-Path $log) { Clear-Content $log }

# 4. launch with debug logging
$env:RUST_LOG = 'debug'
Start-Process -FilePath "$env:LOCALAPPDATA\Programs\CodexBar\codexbar.exe" -WorkingDirectory "$env:LOCALAPPDATA\Programs\CodexBar"
Write-Output "app launched with RUST_LOG=debug"
