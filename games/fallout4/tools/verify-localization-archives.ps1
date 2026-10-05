param(
    [string]$InstallationRoot = 'G:\SteamLibrary\steamapps\common\Fallout 4',
    [string]$CensusPath = 'local\proof-fo4-002\census.json',
    [string]$OutputDirectory = 'local\strings-ba2-oracle-001'
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path -LiteralPath '.').Path
$localRoot = (Resolve-Path -LiteralPath 'local').Path
if ([System.IO.Path]::IsPathRooted($OutputDirectory)) { throw 'Output directory must be a relative path under local/.' }
$outputFullPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory))
if (-not $outputFullPath.StartsWith($localRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Output directory must be inside ignored local/.'
}
if (Test-Path -LiteralPath $outputFullPath) {
    throw 'Output directory already exists; select a fresh local/ path.'
}

$censusFullPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $CensusPath))
$census = Get-Content -LiteralPath $censusFullPath -Raw | ConvertFrom-Json
$censusHash = (Get-FileHash -LiteralPath $censusFullPath -Algorithm SHA256).Hash.ToLowerInvariant()
$installFullPath = (Resolve-Path -LiteralPath $InstallationRoot).Path
$dataRoot = Join-Path $installFullPath 'Data'
$oraclePath = Join-Path $repoRoot 'local\target\debug\fo4-ba2-oracle.exe'
if (-not (Test-Path -LiteralPath $oraclePath -PathType Leaf)) {
    throw 'Build tools/ba2-oracle first with tools/cargo.ps1 and --jobs 1.'
}
$oracleHash = (Get-FileHash -LiteralPath $oraclePath -Algorithm SHA256).Hash.ToLowerInvariant()

$frozenFileByName = @{}
foreach ($file in $census.files) {
    $name = [System.IO.Path]::GetFileName($file.path)
    if ([System.IO.Path]::GetExtension($name) -ieq '.ba2') {
        $frozenFileByName[$name.ToLowerInvariant()] = $file
    }
}
$archiveRows = @($census.archives | Where-Object {
    $_.extensions.strings -gt 0 -or $_.extensions.dlstrings -gt 0 -or $_.extensions.ilstrings -gt 0
} | Sort-Object name)
if ($archiveRows.Count -eq 0) { throw 'The frozen census has no localization-bearing archives.' }

New-Item -ItemType Directory -Path $outputFullPath | Out-Null
$resultsPath = Join-Path $outputFullPath 'results.jsonl'
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$resultsWriter = [System.IO.StreamWriter]::new($resultsPath, $false, $utf8NoBom)
$aggregateEntries = 0
$aggregatePayloads = 0
$aggregateBytes = [UInt64]0
$allResults = [System.Collections.Generic.List[object]]::new()

foreach ($archive in $archiveRows) {
    if ($archive.format -ne 'GNRL') {
        throw "Unexpected localized BA2 format $($archive.format) in $($archive.name)."
    }
    $source = Join-Path $dataRoot $archive.name
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing source archive $source" }
    $frozen = $frozenFileByName[$archive.name.ToLowerInvariant()]
    if ($null -eq $frozen) { throw "Source archive $($archive.name) is absent from the frozen file census." }
    $length = (Get-Item -LiteralPath $source).Length
    $beforeHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($length -ne $frozen.bytes -or $beforeHash -ne $frozen.sha256) {
        throw "Source archive $($archive.name) differs from the frozen census before comparison."
    }

    $oracleOutput = & $oraclePath $source '--all'
    $oracleExit = $LASTEXITCODE
    if ($oracleExit -ne 0) {
        throw "Independent BA2 comparison failed for $($archive.name) with exit ${oracleExit}: $($oracleOutput -join ' ')"
    }
    $comparison = ($oracleOutput -join "`n") | ConvertFrom-Json
    if ($comparison.status -ne 'matched' -or -not $comparison.all_payloads -or $comparison.entries_matched_by_stored_hash_and_name -ne $archive.entries) {
        throw "Independent BA2 comparison was incomplete for $($archive.name)."
    }

    $afterHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($afterHash -ne $beforeHash) { throw "Source archive $($archive.name) changed during comparison." }
    $row = [ordered]@{
        archive = $archive.name
        source_sha256_before = $beforeHash
        source_sha256_after = $afterHash
        source_bytes = $length
        archive_version = $archive.version
        entries = $archive.entries
        localization_tables = [int]($archive.extensions.strings + $archive.extensions.dlstrings + $archive.extensions.ilstrings)
        entries_matched_by_stored_hash_and_name = $comparison.entries_matched_by_stored_hash_and_name
        payloads_compared = $comparison.payloads_compared
        decoded_bytes_compared = $comparison.decoded_bytes_compared
        path_hash_findings = $comparison.path_hash_findings
        all_payloads = $comparison.all_payloads
        comparison_sha256 = $comparison.comparison_sha256
    }
    $jsonLine = $row | ConvertTo-Json -Depth 12 -Compress
    $resultsWriter.WriteLine($jsonLine)
    $allResults.Add($row)
    $aggregateEntries += $comparison.entries_matched_by_stored_hash_and_name
    $aggregatePayloads += $comparison.payloads_compared
    $aggregateBytes += [UInt64]$comparison.decoded_bytes_compared
    Write-Output ("verified {0}: {1} entries, {2} payloads" -f $archive.name, $comparison.entries_matched_by_stored_hash_and_name, $comparison.payloads_compared)
}
$resultsWriter.Dispose()

$resultsHash = (Get-FileHash -LiteralPath $resultsPath -Algorithm SHA256).Hash.ToLowerInvariant()
$manifest = [ordered]@{
    schema = 1
    status = 'matched-all-members-in-physical-localization-archives'
    source_census_sha256 = $censusHash
    ba2_oracle_binary_sha256 = $oracleHash
    ba2_oracle_source_revision = '7b90ce145d5de8e1dda280f10b2b0e0b5aa067f9'
    shared_archive_backend_revision = 'a6ba4c9416e5c7bc211e7a3762420e5bf5c9c049'
    archives = $allResults.Count
    entries_matched_by_stored_hash_and_name = $aggregateEntries
    payloads_compared = $aggregatePayloads
    decoded_bytes_compared = $aggregateBytes
    results_jsonl_sha256 = $resultsHash
    all_payloads = $true
    source_archives_rehashed_before_and_after = $allResults.Count
    runtime_ready = $false
    limits = @(
        'This checks physical BA2 member extraction only; it does not select active archives or establish precedence.',
        'The second BA2 reader does not parse localization table contents; Mutagen separately validates all table keys, offsets and raw values.',
        'Retail archives are read-only; outputs and detailed findings stay under ignored local/.'
    )
}
$manifestPath = Join-Path $outputFullPath 'manifest.json'
$manifestJson = ($manifest | ConvertTo-Json -Depth 16) + "`n"
[System.IO.File]::WriteAllText($manifestPath, $manifestJson, $utf8NoBom)
$manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$completion = [ordered]@{
    schema = 1
    manifest_sha256 = $manifestHash
    results_jsonl_sha256 = $resultsHash
    archives = $allResults.Count
    entries_matched_by_stored_hash_and_name = $aggregateEntries
    payloads_compared = $aggregatePayloads
    decoded_bytes_compared = $aggregateBytes
    complete = $true
    runtime_ready = $false
}
$completionJson = ($completion | ConvertTo-Json -Depth 8) + "`n"
[System.IO.File]::WriteAllText((Join-Path $outputFullPath 'complete.json'), $completionJson, $utf8NoBom)
$completion | ConvertTo-Json -Depth 8
