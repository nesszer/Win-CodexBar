# Dump printable-string context around needles inside a binary (default: codexbar.exe).
# Usage:
#   .\strings-dump.ps1 -Needles 'commandcode','manual_cookies.json' [-Before 200] [-After 300] [-Max 6]
#   .\strings-dump.ps1 -Exe "C:\path\to\codexbar.exe" -Needles 'providers\'
param(
  [string[]]$Needles = @('commandcode'),
  [string]$Exe = "$env:LOCALAPPDATA\Programs\CodexBar\codexbar.exe",
  [int]$Before = 200,
  [int]$After = 300,
  [int]$Max = 6
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Exe)) { throw "binary not found: $Exe" }
$text = [IO.File]::ReadAllText($Exe, [Text.Encoding]::GetEncoding('ISO-8859-1'))

foreach ($needle in $Needles) {
  Write-Output "===== '$needle' ====="
  $idx = 0; $count = 0
  while (($idx = $text.IndexOf($needle, $idx)) -ge 0 -and $count -lt $Max) {
    $start = [Math]::Max(0, $idx - $Before)
    $len = [Math]::Min($Before + $After, $text.Length - $start)
    $snippet = $text.Substring($start, $len) -replace '[^\x20-\x7E]', '.'
    Write-Output "  @$idx  $snippet"
    $idx += $needle.Length; $count++
  }
  if ($count -eq 0) { Write-Output '  (none)' }
}
