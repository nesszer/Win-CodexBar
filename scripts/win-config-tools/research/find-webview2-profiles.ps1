Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'webview|chrom' -and $_.CommandLine } |
  ForEach-Object {
    $cl = $_.CommandLine
    $ud = if ($cl -match '--user-data-dir=("([^"]+)"|(\S+))') { $Matches[2] + $Matches[3] } else { '' }
    "{0,-8} {1,-60} {2}" -f $_.ProcessId, $_.Name, $ud
  } | Sort-Object -Unique
