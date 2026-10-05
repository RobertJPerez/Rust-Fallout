param([Parameter(Mandatory = $true)][string]$Census)
$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
$inputCensus = Get-Content -LiteralPath $Census -Raw | ConvertFrom-Json
$binary = Join-Path $prepRoot 'target\debug\skyrim-prep.exe'
$oracle = Join-Path $prepRoot 'local\vmad-oracle\bin\Debug\net9.0\vmad-oracle.exe'
if (-not (Test-Path -LiteralPath $binary) -or -not (Test-Path -LiteralPath $oracle)) {
    throw 'Build the Rust application and the offline vmad-oracle first; see README.md.'
}
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$destination = Join-Path $prepRoot "local\bindings-$stamp"
[System.IO.Directory]::CreateDirectory($destination) | Out-Null
$results = @()
$passed = $true
foreach ($plugin in $inputCensus.plugins) {
    $name = [string]$plugin.file
    if (-not $name -or $name.IndexOfAny([char[]]'\/:') -ge 0 -or $name -eq '.' -or $name -eq '..') { throw 'Unsafe census filename' }
    $source = [System.IO.Path]::Combine([string]$inputCensus.data, $name)
    $export = Join-Path $destination "$name.jsonl"
    $comparison = Join-Path $destination "$name.oracle.json"
    Write-Output "Exporting and independently comparing $name"
    & $binary export-bindings --plugin $source --output $export
    if ($LASTEXITCODE -ne 0) { throw "Binding export failed: $name" }
    $header = Get-Content -LiteralPath $export -TotalCount 1 | ConvertFrom-Json
    if ($header.sha256 -ne $plugin.sha256) { throw "Source changed since census: $name" }
    $json = & $oracle $source $export
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0 -and $exitCode -ne 2) { throw "Independent reader failed: $name" }
    [System.IO.File]::WriteAllLines($comparison, [string[]]$json, [System.Text.UTF8Encoding]::new($false))
    $result = Get-Content -LiteralPath $comparison -Raw | ConvertFrom-Json
    $passed = $passed -and $result.passed -and ($exitCode -eq 0)
    $results += @{
        file = $name
        source_sha256 = $header.sha256
        export = $export
        export_sha256 = (Get-FileHash -LiteralPath $export -Algorithm SHA256).Hash.ToLowerInvariant()
        comparison = $comparison
        comparison_sha256 = (Get-FileHash -LiteralPath $comparison -Algorithm SHA256).Hash.ToLowerInvariant()
        passed = $result.passed
        bindings = $result.rust_bindings
        matched_bindings = $result.matched_bindings
        decoded_tails = $result.decoded_tails
        comparisons = $result.comparisons
        mismatches = $result.mismatches
        exit_code = $exitCode
    }
}
if ($results.Count -eq 0) { throw 'No plugins in census' }
$receipt = @{
    schema_version = 1
    observed_utc = [DateTime]::UtcNow.ToString('o')
    census_sha256 = (Get-FileHash -LiteralPath $Census -Algorithm SHA256).Hash.ToLowerInvariant()
    inspector_sha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    oracle_assembly_sha256 = (Get-FileHash -LiteralPath (Join-Path (Split-Path $oracle -Parent) 'vmad-oracle.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
    passed = $passed
    plugins = $results
} | ConvertTo-Json -Depth 8
$receiptPath = Join-Path $destination 'receipt.json'
[System.IO.File]::WriteAllText($receiptPath, $receipt, [System.Text.UTF8Encoding]::new($false))
Write-Output "Receipt: $receiptPath"
if (-not $passed) { exit 2 }
