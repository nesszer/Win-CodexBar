$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security

$dir = "$env:APPDATA\CodexBar"
$mcFile = "$dir\manual_cookies.json"
$taFile = "$dir\token-accounts.json"

function Read-SecureFile($path) {
  $json = Get-Content -Raw -Encoding UTF8 $path | ConvertFrom-Json
  $bytes = [Convert]::FromBase64String($json.payload)
  $plain = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, 'CurrentUser')
  [Text.Encoding]::UTF8.GetString($plain)
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

# token value comes from token-accounts.json - no secrets in this script
$ta = Read-SecureFile $taFile | ConvertFrom-Json
$token = $ta.providers.commandcode.accounts[0].token
if (-not $token) { throw "no commandcode token in token-accounts.json" }

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
Write-Output "manual cookie restored: header len $($check.Length), starts with $($check.Substring(0,45))..."
Write-Output ("no-BOM check first bytes: " + (([System.IO.File]::ReadAllBytes($mcFile))[0..2] -join ' '))
