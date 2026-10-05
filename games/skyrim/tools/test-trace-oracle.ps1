param([Parameter(Mandatory = $true)][string]$Trace)
$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
$oracle = Join-Path $prepRoot 'local\trace-oracle\bin\Debug\net9.0\trace-oracle.exe'
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$destination = Join-Path $prepRoot "local\negative-trace-$stamp"
[System.IO.Directory]::CreateDirectory($destination) | Out-Null
$control = & $oracle $Trace
if ($LASTEXITCODE -ne 0) { throw 'The unaltered trace must pass before mutation.' }
[System.IO.File]::WriteAllLines((Join-Path $destination 'control-result.json'), [string[]]$control, [System.Text.UTF8Encoding]::new($false))
$inputTrace = Get-Content -LiteralPath $Trace -Raw | ConvertFrom-Json
if ($inputTrace.direct_references.Count -lt 1) { throw 'No direct reference to mutate.' }
$inputTrace.direct_references[0].target_raw = $inputTrace.direct_references[0].target_raw -bxor 1
$altered = Join-Path $destination 'altered.json'
[System.IO.File]::WriteAllText($altered, ($inputTrace | ConvertTo-Json -Depth 100), [System.Text.UTF8Encoding]::new($false))
$result = & $oracle $altered
$code = $LASTEXITCODE
[System.IO.File]::WriteAllLines((Join-Path $destination 'altered-result.json'), [string[]]$result, [System.Text.UTF8Encoding]::new($false))
$comparison = $result | ConvertFrom-Json
# One changed edge becomes a missing original plus an unexpected replacement.
$valid = ($code -eq 2 -and -not $comparison.passed -and $comparison.mismatches -eq 2 -and $comparison.expected_edges -eq $comparison.independent_edges)
$receipt = @{
    passed = $valid
    mutation = 'One direct target FormID bit, with unchanged record and edge counts'
    expected_exit = 2
    actual_exit = $code
    mismatches = $comparison.mismatches
    original_trace_sha256 = (Get-FileHash -LiteralPath $Trace -Algorithm SHA256).Hash.ToLowerInvariant()
    altered_trace_sha256 = (Get-FileHash -LiteralPath $altered -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json
$receiptPath = Join-Path $destination 'receipt.json'
[System.IO.File]::WriteAllText($receiptPath, $receipt, [System.Text.UTF8Encoding]::new($false))
Write-Output "Receipt: $receiptPath"
if (-not $valid) { exit 2 }
