$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security

function Read-SecureFile([string]$Path) {
  $raw = [System.IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8)
  $json = $raw | ConvertFrom-Json
  if ($json.format -ne 'codexbar.secure-file') {
    return $raw
  }
  if (-not $json.payload) {
    throw "Secure file is missing its protected payload: $Path"
  }
  $bytes = [Convert]::FromBase64String($json.payload)
  $plain = [System.Security.Cryptography.ProtectedData]::Unprotect($bytes, $null, 'CurrentUser')
  [Text.Encoding]::UTF8.GetString($plain)
}

function Write-SecureFile([string]$Path, [string]$PlainJson) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($PlainJson)
  $enc = [System.Security.Cryptography.ProtectedData]::Protect($bytes, $null, 'CurrentUser')
  $wrapper = @{
    format = 'codexbar.secure-file'
    version = 1
    protection = 'windows-dpapi-user'
    payload = [Convert]::ToBase64String($enc)
  } | ConvertTo-Json
  $tempPath = "$Path.$([guid]::NewGuid().ToString('N')).tmp"
  try {
    [System.IO.File]::WriteAllText($tempPath, $wrapper, (New-Object Text.UTF8Encoding($false)))
    if ([System.IO.File]::Exists($Path)) {
      [System.IO.File]::Move($tempPath, $Path, $true)
    } else {
      [System.IO.File]::Move($tempPath, $Path)
    }
  }
  finally {
    if ([System.IO.File]::Exists($tempPath)) {
      [System.IO.File]::Delete($tempPath)
    }
  }
}

function Stop-CodexBarForEdit {
  $running = @(Get-Process -Name codexbar, codexbar-desktop -ErrorAction SilentlyContinue)
  foreach ($process in $running) {
    Write-Output "stopping $($process.ProcessName)"
    Stop-Process -Id $process.Id -Force
  }
  if ($running.Count -gt 0) {
    Start-Sleep -Seconds 2
  }
  $remaining = @(Get-Process -Name codexbar, codexbar-desktop -ErrorAction SilentlyContinue)
  if ($remaining.Count -gt 0) {
    $names = (($remaining | Select-Object -ExpandProperty ProcessName -Unique) -join ', ')
    throw "Refusing to modify configuration while CodexBar processes remain active: $names"
  }
}

function ConvertTo-RedactedObject($Value) {
  if ($null -eq $Value) {
    return $null
  }
  if ($Value -is [string] -or $Value.GetType().IsValueType) {
    return $Value
  }
  if ($Value -is [System.Collections.IEnumerable]) {
    $items = @($Value | ForEach-Object { ConvertTo-RedactedObject $_ })
    return ,$items
  }

  $redacted = [ordered]@{}
  foreach ($property in $Value.PSObject.Properties) {
    if ($property.Name -match '(?i)(token|api.?key|secret|cookie|password|authorization|refresh.?token|access.?token)') {
      $redacted[$property.Name] = '<redacted>'
    } else {
      $redacted[$property.Name] = ConvertTo-RedactedObject $property.Value
    }
  }
  [pscustomobject]$redacted
}

function ConvertTo-RedactedJson([string]$PlainJson) {
  ConvertTo-Json (ConvertTo-RedactedObject ($PlainJson | ConvertFrom-Json)) -Depth 20
}
