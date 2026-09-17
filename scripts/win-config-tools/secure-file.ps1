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
      $replacementBackup = "$Path.$([guid]::NewGuid().ToString('N')).replace-bak"
      try {
        [System.IO.File]::Replace($tempPath, $Path, $replacementBackup)
      }
      finally {
        if ([System.IO.File]::Exists($replacementBackup)) {
          [System.IO.File]::Delete($replacementBackup)
        }
      }
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

function Stop-CodexBarForEdit([switch]$Force) {
  $running = Get-CodexBarProcesses
  foreach ($process in $running) {
    Write-Output "requesting graceful exit for $($process.ProcessName)"
    if (-not $process.CloseMainWindow()) {
      Write-Output "$($process.ProcessName) did not accept a close request"
    }
  }

  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  do {
    Start-Sleep -Milliseconds 250
    $remaining = Get-CodexBarProcesses
  } while ($remaining.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)

  if ($remaining.Count -gt 0 -and -not $Force) {
    $names = (($remaining | Select-Object -ExpandProperty ProcessName -Unique) -join ', ')
    throw "Refusing to modify configuration while CodexBar processes remain active: $names. Close them and retry."
  }

  if ($remaining.Count -gt 0) {
    foreach ($process in $remaining) {
      Write-Output "force-stopping $($process.ProcessName)"
      Stop-Process -Id $process.Id -Force
    }
    Start-Sleep -Milliseconds 250
    $remaining = Get-CodexBarProcesses
    if ($remaining.Count -gt 0) {
      $names = (($remaining | Select-Object -ExpandProperty ProcessName -Unique) -join ', ')
      throw "CodexBar processes remain after force-stop: $names"
    }
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

function Get-CodexBarProcesses {
  @(Get-Process -Name @('codexbar', 'codexbar-desktop', 'codexbar-desktop-tauri') -ErrorAction SilentlyContinue)
}

function Get-CodexBarDesktopPath {
  if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    return $null
  }
  $installDir = Join-Path $env:LOCALAPPDATA 'Programs\CodexBar'
  foreach ($name in @('codexbar-desktop-tauri.exe', 'codexbar.exe', 'codexbar-desktop.exe')) {
    $candidate = Join-Path $installDir $name
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
      return (Resolve-Path -LiteralPath $candidate).Path
    }
  }
  return $null
}
