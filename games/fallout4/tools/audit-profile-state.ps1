[CmdletBinding()]
param(
    [string]$InstallRoot = 'G:\SteamLibrary\steamapps\common\Fallout 4',
    [string]$VortexRoot = (Join-Path $env:APPDATA 'Vortex\fallout4'),
    [string]$OutputDirectory = 'local/profile-recheck-NNN'
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$localRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'local'))
if ([IO.Path]::IsPathRooted($OutputDirectory) -or
    $OutputDirectory.Split([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) -contains '..') {
    throw 'OutputDirectory must be a new relative path under local/.'
}
$outputPath = [IO.Path]::GetFullPath((Join-Path $repoRoot $OutputDirectory))
if (-not $outputPath.StartsWith($localRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'OutputDirectory must be under local/.'
}
if (Test-Path -LiteralPath $outputPath) {
    throw 'OutputDirectory already exists; choose a fresh local/ path.'
}
$installPath = (Resolve-Path -LiteralPath $InstallRoot).Path
$appDataFallout4 = Join-Path $env:LOCALAPPDATA 'Fallout4'
$vortexProfiles = Join-Path $VortexRoot 'profiles'
$rows = [Collections.Generic.List[object]]::new()

function Add-FileObservation([string]$Role, [string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $rows.Add([ordered]@{ role = $Role; present = $false })
        return
    }

    $before = Get-Item -LiteralPath $Path -Force
    $row = [ordered]@{
        role = $Role
        present = $true
        regular_file = (($before.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0)
        bytes = $before.Length
        modified_utc = $before.LastWriteTimeUtc.ToString('o')
    }
    if ($row.regular_file) {
        $row.sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
        $after = Get-Item -LiteralPath $Path -Force
        $row.unchanged_during_hash = ($before.Length -eq $after.Length -and
            $before.LastWriteTimeUtc -eq $after.LastWriteTimeUtc)
    } else {
        $row.unchanged_during_hash = $null
    }
    $rows.Add($row)
}

Add-FileObservation 'global_plugins' (Join-Path $appDataFallout4 'plugins.txt')
Add-FileObservation 'global_loadorder' (Join-Path $appDataFallout4 'loadorder.txt')

$profileDirectories = @()
if (Test-Path -LiteralPath $vortexProfiles -PathType Container) {
    $profileDirectories = @(Get-ChildItem -LiteralPath $vortexProfiles -Directory -Force | Sort-Object Name)
}
for ($index = 0; $index -lt $profileDirectories.Count; $index++) {
    $number = $index + 1
    $profilePath = $profileDirectories[$index].FullName
    Add-FileObservation ("vortex_profile_{0:D3}_plugins" -f $number) (Join-Path $profilePath 'plugins.txt')
    Add-FileObservation ("vortex_profile_{0:D3}_loadorder" -f $number) (Join-Path $profilePath 'loadorder.txt')
}

$snapshotDirectory = Join-Path $VortexRoot 'snapshots'
$snapshots = @()
if (Test-Path -LiteralPath $snapshotDirectory -PathType Container) {
    $snapshots = @(Get-ChildItem -LiteralPath $snapshotDirectory -File -Filter 'snapshot.json' -Force | Sort-Object Name)
}
if ($snapshots.Count -eq 0) {
    Add-FileObservation 'vortex_snapshot' (Join-Path $snapshotDirectory 'snapshot.json')
} else {
    for ($index = 0; $index -lt $snapshots.Count; $index++) {
        Add-FileObservation ("vortex_snapshot_{0:D3}" -f ($index + 1)) $snapshots[$index].FullName
    }
}

$dataPath = Join-Path $installPath 'Data'
$manifestPath = Join-Path $dataPath 'vortex.deployment.json'
$historicalMarkerPath = Join-Path $dataPath 'BCR\__folder_managed_by_vortex'
$vortexRunning = @(Get-Process -Name Vortex -ErrorAction SilentlyContinue).Count -gt 0
$manifest = [ordered]@{
    schema = 1
    status = 'metadata-only-current-profile-recheck'
    captured_utc = [DateTimeOffset]::UtcNow.ToString('o')
    installation_root = $installPath
    vortex_profile_directories = $profileDirectories.Count
    vortex_process_running = $vortexRunning
    deployment_manifest_present = (Test-Path -LiteralPath $manifestPath -PathType Leaf)
    historical_marker_present = (Test-Path -LiteralPath $historicalMarkerPath)
    files = $rows
    runtime_ready = $false
    limits = @(
        'Only names, regular-file metadata, timestamps and SHA-256 digests are captured; profile contents and mod names are not published.',
        'A process snapshot and profile/deployment file presence do not establish which profile or virtual filesystem the game used.',
        'This command does not inspect plugin activation semantics, winner order, archive precedence, settings or gameplay.'
    )
}

New-Item -ItemType Directory -Path $outputPath | Out-Null
$manifestPathOut = Join-Path $outputPath 'manifest.json'
$json = ConvertTo-Json -InputObject $manifest -Depth 8
[IO.File]::WriteAllText($manifestPathOut, $json + [Environment]::NewLine, (New-Object Text.UTF8Encoding($false)))
$manifestHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $manifestPathOut).Hash.ToLowerInvariant()
$complete = [ordered]@{ schema = 1; status = 'complete'; manifest_sha256 = $manifestHash; runtime_ready = $false }
$completeJson = ConvertTo-Json -InputObject $complete -Compress
[IO.File]::WriteAllText((Join-Path $outputPath 'complete.json'), $completeJson + [Environment]::NewLine, (New-Object Text.UTF8Encoding($false)))
Write-Output "profile_directories=$($profileDirectories.Count); files_observed=$($rows.Count); runtime_ready=false; manifest_sha256=$manifestHash"
