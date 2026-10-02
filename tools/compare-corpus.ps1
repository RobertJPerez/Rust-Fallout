param(
    [string]$Install = 'G:\SteamLibrary\steamapps\common\Fallout New Vegas'
)
# Every run gets a new folder. Decode failures remain failures in the saved report.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $cargo = Join-Path $PSScriptRoot 'cargo.ps1'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $cargo build --release --locked -p archive-compare
    if ($LASTEXITCODE -ne 0) { throw 'Archive oracle build failed' }
    & powershell -NoProfile -ExecutionPolicy Bypass -File $cargo build --release --locked --manifest-path tools\plugin-oracle\Cargo.toml
    if ($LASTEXITCODE -ne 0) { throw 'Plugin oracle build failed' }
    $runRoot = Join-Path $projectRoot ('local\comparison-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $runRoot | Out-Null
    $archiveReports = @()
    $failures = 0
    foreach ($archive in (Get-ChildItem -LiteralPath (Join-Path $Install 'Data') -File -Filter '*.bsa' | Sort-Object Name)) {
        Write-Host ('Comparing ' + $archive.Name)
        $json = & .\target\release\archive-compare.exe $archive.FullName --all
        if ($LASTEXITCODE -ne 0) { $failures += 1 }
        if (-not $json) { throw ('Comparison produced no report: ' + $archive.Name) }
        $archiveReports += ($json | ConvertFrom-Json)
    }
    ConvertTo-Json -InputObject $archiveReports -Depth 12 | Set-Content -Encoding UTF8 (Join-Path $runRoot 'archive-comparison.json')
    $pluginReport = & .\tools\plugin-oracle\target\release\plugin-oracle.exe (Join-Path $Install 'Data')
    if ($LASTEXITCODE -ne 0) { throw 'Plugin oracle failed' }
    $pluginReport | Set-Content -Encoding UTF8 (Join-Path $runRoot 'plugin-oracle.json')
    Write-Output ('Reports: ' + $runRoot)
    if ($failures -gt 0) { throw ('Archive comparisons with failures: ' + $failures) }
} finally {
    Pop-Location
}
