param(
    [string]$InstallationRoot = 'G:\SteamLibrary\steamapps\common\Fallout 4',
    [string]$CensusPath = 'local\proof-fo4-002\census.json',
    [string]$OutputDirectory = 'local\recursive-data-001'
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path -LiteralPath '.').Path
$localRoot = (Resolve-Path -LiteralPath 'local').Path
if ([System.IO.Path]::IsPathRooted($OutputDirectory)) { throw 'Output directory must be a relative path under local/.' }
$outputFullPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory))
if (-not $outputFullPath.StartsWith($localRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Output directory must be inside ignored local/.'
}
if (Test-Path -LiteralPath $outputFullPath) { throw 'Output directory already exists; select a fresh local/ path.' }

$installFullPath = (Resolve-Path -LiteralPath $InstallationRoot).Path
$dataRoot = Join-Path $installFullPath 'Data'
if (-not (Test-Path -LiteralPath $dataRoot -PathType Container)) { throw 'Installation has no Data directory.' }
$censusFullPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $CensusPath))
$census = Get-Content -LiteralPath $censusFullPath -Raw | ConvertFrom-Json
$priorDataFiles = @{}
foreach ($file in $census.files) {
    if ($file.path.StartsWith('Data/', [System.StringComparison]::OrdinalIgnoreCase)) {
        $relative = $file.path.Substring(5).Replace('/', '\')
        $priorDataFiles[$relative.ToLowerInvariant()] = $file
    }
}

$sourceFiles = @(Get-ChildItem -LiteralPath $dataRoot -File -Force -Recurse | Sort-Object FullName)
if ($sourceFiles.Count -eq 0) { throw 'No recursive Data files were found.' }
if (@(Get-ChildItem -LiteralPath $dataRoot -Directory -Force -Recurse | Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint }).Count -ne 0) {
    throw 'A Data directory is a reparse point; the recursive inventory does not follow links.'
}
if (@($sourceFiles | Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint }).Count -ne 0) {
    throw 'A Data file is a reparse point; the recursive inventory does not follow links.'
}

New-Item -ItemType Directory -Path $outputFullPath | Out-Null
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$filesPath = Join-Path $outputFullPath 'files.jsonl'
$writer = [System.IO.StreamWriter]::new($filesPath, $false, $utf8NoBom)
$rows = [System.Collections.Generic.List[object]]::new()
$currentKeys = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$newRecursiveFiles = 0
$priorFilesMatched = 0

for ($index = 0; $index -lt $sourceFiles.Count; $index++) {
    $file = $sourceFiles[$index]
    $relative = $file.FullName.Substring($dataRoot.Length).TrimStart('\', '/')
    [void]$currentKeys.Add($relative)
    $baseline = $priorDataFiles[$relative.ToLowerInvariant()]
    $beforeLength = $file.Length
    $beforeHash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $oldCensusMatch = $null -ne $baseline -and $baseline.bytes -eq $beforeLength -and $baseline.sha256 -eq $beforeHash
    if ($null -ne $baseline) {
        if (-not $oldCensusMatch) { throw "Prior top-level census fingerprint changed for Data/$relative." }
        $priorFilesMatched++
    } else {
        $newRecursiveFiles++
    }
    $rows.Add([ordered]@{
        relative_path = $relative.Replace('\', '/')
        bytes = $beforeLength
        sha256_before = $beforeHash
        sha256_after = $null
        matched_prior_top_level_census = $oldCensusMatch
        nested = $relative.Contains('\')
    })
    if ((($index + 1) % 10) -eq 0) { Write-Output ("fingerprinted {0}/{1} Data files" -f ($index + 1), $sourceFiles.Count) }
}

$missingPrior = @($priorDataFiles.Keys | Where-Object { -not $currentKeys.Contains($_) })
if ($missingPrior.Count -ne 0) { throw "Recursive Data inventory is missing $($missingPrior.Count) files from the frozen top-level census." }
if ($priorFilesMatched -ne $priorDataFiles.Count) { throw 'Not every frozen top-level Data file was matched.' }

$sourceFilesAfter = @(Get-ChildItem -LiteralPath $dataRoot -File -Force -Recurse | Sort-Object FullName)
if ($sourceFilesAfter.Count -ne $sourceFiles.Count) { throw 'Data file count changed during recursive inventory.' }
for ($index = 0; $index -lt $sourceFiles.Count; $index++) {
    if ($sourceFilesAfter[$index].FullName -cne $sourceFiles[$index].FullName) {
        throw 'Data file paths changed during recursive inventory.'
    }
}

for ($index = 0; $index -lt $sourceFiles.Count; $index++) {
    $file = Get-Item -LiteralPath $sourceFiles[$index].FullName
    $afterHash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($file.Length -ne $rows[$index].bytes -or $afterHash -ne $rows[$index].sha256_before) {
        throw "Data file changed during recursive inventory: $($rows[$index].relative_path)."
    }
    $rows[$index].sha256_after = $afterHash
    $writer.WriteLine(($rows[$index] | ConvertTo-Json -Depth 5 -Compress))
    if ((($index + 1) % 10) -eq 0) { Write-Output ("rehashed {0}/{1} Data files" -f ($index + 1), $sourceFiles.Count) }
}
$writer.Dispose()
Write-Output ("fingerprint ledger written: {0} Data files; {1} prior census files matched; {2} new recursive files" -f $rows.Count, $priorFilesMatched, $newRecursiveFiles)
Write-Output 'Run tools/finalize_recursive_data.py to validate the ledger, recheck nested files and write the completion marker.'
