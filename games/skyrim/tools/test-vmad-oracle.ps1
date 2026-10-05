param(
    [Parameter(Mandatory = $true)][string]$Plugin,
    [Parameter(Mandatory = $true)][string]$Export
)
$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
$oracle = Join-Path $prepRoot 'local\vmad-oracle\bin\Debug\net9.0\vmad-oracle.exe'
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$destination = Join-Path $prepRoot "local\negative-vmad-$stamp"
[System.IO.Directory]::CreateDirectory($destination) | Out-Null
$control = & $oracle $Plugin $Export
if ($LASTEXITCODE -ne 0) { throw 'The unaltered control must pass before a mutation test.' }
[System.IO.File]::WriteAllLines((Join-Path $destination 'control.json'), [string[]]$control, [System.Text.UTF8Encoding]::new($false))
$altered = Join-Path $destination 'altered.jsonl'
$changed = $false
$writer = [System.IO.StreamWriter]::new($altered, $false, [System.Text.UTF8Encoding]::new($false))
try {
    foreach ($line in [System.IO.File]::ReadLines([System.IO.Path]::GetFullPath($Export))) {
        if (-not $changed -and $line.Contains('"Decoded"')) {
            $row = $line | ConvertFrom-Json
            $fragments = $row.attachment.tail.Decoded.fragments
            if ($fragments.Count -gt 0 -and $fragments[0].function_name.Count -gt 0) {
                $fragments[0].function_name[0] = $fragments[0].function_name[0] -bxor 1
                $writer.WriteLine(($row | ConvertTo-Json -Compress -Depth 100))
                $changed = $true
                continue
            }
        }
        $writer.WriteLine($line)
    }
} finally { $writer.Dispose() }
if (-not $changed) { throw 'No fragment function name found for the mutation test.' }
$result = & $oracle $Plugin $altered
$code = $LASTEXITCODE
$comparison = $result | ConvertFrom-Json
[System.IO.File]::WriteAllLines((Join-Path $destination 'altered-result.json'), [string[]]$result, [System.Text.UTF8Encoding]::new($false))
$valid = ($code -eq 2 -and -not $comparison.passed -and $comparison.mismatches -eq 1 -and $comparison.examples[0].path.EndsWith('.function_name'))
$receipt = @{
    passed = $valid
    mutation = 'Exactly one byte of the first fragment function name; source hash and all counts unchanged'
    expected_exit = 2
    actual_exit = $code
    mismatches = $comparison.mismatches
    original_export_sha256 = (Get-FileHash -LiteralPath $Export -Algorithm SHA256).Hash.ToLowerInvariant()
    altered_export_sha256 = (Get-FileHash -LiteralPath $altered -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json
$receiptPath = Join-Path $destination 'receipt.json'
[System.IO.File]::WriteAllText($receiptPath, $receipt, [System.Text.UTF8Encoding]::new($false))
Write-Output "Receipt: $receiptPath"
if (-not $valid) { exit 2 }
