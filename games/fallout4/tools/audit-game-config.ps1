param(
    [string]$ProfileDirectory = '',
    [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path -LiteralPath '.').Path
$localRoot = (Resolve-Path -LiteralPath 'local').Path
function Get-ByteSha256([byte[]]$Bytes) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($algorithm.ComputeHash($Bytes)).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose() }
}

if ([string]::IsNullOrWhiteSpace($ProfileDirectory)) {
    $documents = [Environment]::GetFolderPath([Environment+SpecialFolder]::MyDocuments)
    if ([string]::IsNullOrWhiteSpace($documents)) { throw 'Windows did not provide a Documents known-folder path.' }
    $ProfileDirectory = Join-Path $documents 'My Games\Fallout4'
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) { throw 'Supply a fresh output directory under local/.' }
if ([System.IO.Path]::IsPathRooted($OutputDirectory)) { throw 'Output directory must be relative to the repository.' }
$outputFullPath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory))
if (-not $outputFullPath.StartsWith($localRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Output directory must be inside ignored local/.'
}
if (Test-Path -LiteralPath $outputFullPath) { throw 'Output directory already exists; select a fresh local/ path.' }

$profileFullPath = (Resolve-Path -LiteralPath $ProfileDirectory).Path
if (-not (Test-Path -LiteralPath $profileFullPath -PathType Container)) { throw 'The Fallout 4 profile directory is not a directory.' }
$allowedNames = @('Fallout4.ini', 'Fallout4Prefs.ini', 'Fallout4Custom.ini')
$sourceFiles = @(
    foreach ($name in $allowedNames) {
        $candidate = Join-Path $profileFullPath $name
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { Get-Item -LiteralPath $candidate -Force }
    }
)
if ($sourceFiles.Count -eq 0) { throw 'No allow-listed Fallout 4 INI files were found in the profile directory.' }

New-Item -ItemType Directory -Path $outputFullPath | Out-Null
$inputDir = Join-Path $outputFullPath 'inputs'
New-Item -ItemType Directory -Path $inputDir | Out-Null
$entries = [System.Collections.Generic.List[object]]::new()
foreach ($source in $sourceFiles) {
    if ($source.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw "Refusing to read an INI reparse point: $($source.Name)." }
    if ($source.Length -gt 1048576) { throw "INI exceeds the one-MiB observation limit: $($source.Name)." }
    $before = Get-Item -LiteralPath $source.FullName -Force
    $bytes = [System.IO.File]::ReadAllBytes($source.FullName)
    $sourceHash = Get-ByteSha256 $bytes
    $after = Get-Item -LiteralPath $source.FullName -Force
    if ($before.Length -ne $after.Length -or $before.Length -ne $bytes.Length -or $before.LastWriteTimeUtc -ne $after.LastWriteTimeUtc) {
        throw "INI changed while being read: $($source.Name)."
    }
    $secondHash = (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sourceHash -ne $secondHash) { throw "INI content changed during fingerprinting: $($source.Name)." }

    $copyPath = Join-Path $inputDir $source.Name
    [System.IO.File]::WriteAllBytes($copyPath, $bytes)
    $copyHash = (Get-FileHash -LiteralPath $copyPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $finalSourceHash = (Get-FileHash -LiteralPath $source.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($copyHash -ne $sourceHash -or $finalSourceHash -ne $sourceHash) {
        throw "Preserved copy or source recheck failed for $($source.Name)."
    }
    $entries.Add([ordered]@{
        name = $source.Name
        source_path = $source.FullName
        bytes = $bytes.Length
        source_sha256 = $sourceHash
        preserved_copy = "inputs/$($source.Name)"
        preserved_copy_sha256 = $copyHash
        modified_utc = $after.LastWriteTimeUtc.ToString('o')
        source_fingerprint_stable = $true
    })
}

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$manifest = [ordered]@{
    schema = 1
    status = 'verified-bounded-fallout4-ini-inputs-not-effective-runtime-configuration'
    profile_directory = $profileFullPath
    profile_source = 'Windows Documents known folder / My Games / Fallout4'
    allow_list = $allowedNames
    files = $entries
    source_and_copy_hashes_match = $true
    configuration_values_reported = $false
    source_files_modified = $false
    runtime_ready = $false
    limits = @(
        'The selected INI files are a configuration candidate; their presence does not prove which process or profile loaded them.',
        'This does not inspect Fallout4Prefs.ini semantics, executable command-line overrides, virtual deployment, plugins, saves or gameplay.',
        'Exact source paths, hashes and preserved user settings remain in ignored local evidence only.'
    )
}
$manifestPath = Join-Path $outputFullPath 'manifest.json'
[System.IO.File]::WriteAllText($manifestPath, (($manifest | ConvertTo-Json -Depth 8) + "`n"), $utf8NoBom)
$manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$complete = [ordered]@{
    schema = 1
    manifest_sha256 = $manifestHash
    files = $entries.Count
    all_source_and_copy_hashes_match = $true
    complete = $true
    runtime_ready = $false
}
[System.IO.File]::WriteAllText((Join-Path $outputFullPath 'complete.json'), (($complete | ConvertTo-Json -Depth 5) + "`n"), $utf8NoBom)
$complete | ConvertTo-Json -Depth 5
