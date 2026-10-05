param([Parameter(Mandatory = $true)][string]$Census)
$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
$oracle = Join-Path $prepRoot 'local\archive-oracle\bin\Debug\net9.0\archive-oracle.exe'
$inputCensus = Get-Content -LiteralPath $Census -Raw | ConvertFrom-Json
$archive = $inputCensus.archives | Where-Object { $_.scripts.Count -gt 0 } | Sort-Object bytes | Select-Object -First 1
if (-not $archive) { throw 'No archived script available for a mutation test.' }
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$destination = Join-Path $prepRoot "local\negative-archive-$stamp"
[System.IO.Directory]::CreateDirectory($destination) | Out-Null
$control = Join-Path $destination 'control.json'
$altered = Join-Path $destination 'altered.json'
$single = @{ archives = @($archive) }
[System.IO.File]::WriteAllText($control, ($single | ConvertTo-Json -Depth 100), [System.Text.UTF8Encoding]::new($false))
$result = & $oracle $control
if ($LASTEXITCODE -ne 0) { throw 'The unaltered control must pass before a mutation test.' }
[System.IO.File]::WriteAllLines((Join-Path $destination 'control-result.json'), [string[]]$result, [System.Text.UTF8Encoding]::new($false))
$digest = $archive.scripts[0].sha256
$first = if ($digest[0] -eq '0') { '1' } else { '0' }
$archive.scripts[0].sha256 = $first + $digest.Substring(1)
[System.IO.File]::WriteAllText($altered, ($single | ConvertTo-Json -Depth 100), [System.Text.UTF8Encoding]::new($false))
$result = & $oracle $altered
$code = $LASTEXITCODE
$comparison = $result | ConvertFrom-Json
[System.IO.File]::WriteAllLines((Join-Path $destination 'altered-result.json'), [string[]]$result, [System.Text.UTF8Encoding]::new($false))
$valid = ($code -eq 2 -and -not $comparison.passed -and $comparison.comparisons[0].mismatches -eq 1)
$receipt = @{
    passed = $valid
    archive = [System.IO.Path]::GetFileName($archive.file)
    mutation = 'One expected script SHA-256 character; source archive hash, lengths, paths and counts unchanged'
    expected_exit = 2
    actual_exit = $code
    control_sha256 = (Get-FileHash -LiteralPath $control -Algorithm SHA256).Hash.ToLowerInvariant()
    altered_sha256 = (Get-FileHash -LiteralPath $altered -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json
$receiptPath = Join-Path $destination 'receipt.json'
[System.IO.File]::WriteAllText($receiptPath, $receipt, [System.Text.UTF8Encoding]::new($false))
Write-Output "Receipt: $receiptPath"
if (-not $valid) { exit 2 }
