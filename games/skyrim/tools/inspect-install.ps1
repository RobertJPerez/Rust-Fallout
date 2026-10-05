param([string]$Install)
$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
if (-not $Install) {
    $libraries = [System.Collections.Generic.List[string]]::new()
    $libraries.Add('G:\SteamLibrary')
    $steam = Get-ItemProperty -LiteralPath 'HKCU:\Software\Valve\Steam' -ErrorAction SilentlyContinue
    if ($steam -and $steam.SteamPath) {
        $libraries.Add($steam.SteamPath)
        $vdf = Join-Path $steam.SteamPath 'steamapps\libraryfolders.vdf'
        if (Test-Path -LiteralPath $vdf) {
            $text = Get-Content -LiteralPath $vdf -Raw
            foreach ($match in [regex]::Matches($text, '"path"\s+"([^"]+)"')) {
                $libraries.Add($match.Groups[1].Value.Replace('\\', '\'))
            }
        }
    }
    foreach ($library in ($libraries | Select-Object -Unique)) {
        $manifest = Join-Path $library 'steamapps\appmanifest_489830.acf'
        if (-not (Test-Path -LiteralPath $manifest)) { continue }
        $text = Get-Content -LiteralPath $manifest -Raw
        $match = [regex]::Match($text, '"installdir"\s+"([^"]+)"')
        if ($match.Success) {
            $Install = Join-Path (Join-Path $library 'steamapps\common') $match.Groups[1].Value
            break
        }
    }
}
if (-not $Install) { throw 'Skyrim installation not found; pass -Install with its directory once installation finishes.' }
$Install = [System.IO.Path]::GetFullPath($Install)
$steamApps = Split-Path -Parent (Split-Path -Parent $Install)
$manifest = Join-Path $steamApps 'appmanifest_489830.acf'
$manifestInfo = $null
if (Test-Path -LiteralPath $manifest) {
    $text = Get-Content -LiteralPath $manifest -Raw
    $state = [regex]::Match($text, '"StateFlags"\s+"(\d+)"')
    if (-not $state.Success -or $state.Groups[1].Value -ne '4') {
        throw 'Steam still reports Skyrim installation/update activity. Retry when it finishes; no game files were opened.'
    }
    $manifestInfo = @{
        path = $manifest
        sha256 = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash.ToLowerInvariant()
        build_id = [regex]::Match($text, '"buildid"\s+"(\d+)"').Groups[1].Value
        state_flags = 4
    }
}
$exe = Join-Path $Install 'SkyrimSE.exe'
$data = Join-Path $Install 'Data'
if (-not (Test-Path -LiteralPath $exe) -or -not (Test-Path -LiteralPath $data)) {
    throw "Installation is incomplete at $Install; SkyrimSE.exe and Data are required."
}
$binary = Join-Path $prepRoot 'target\debug\skyrim-prep.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw 'Build skyrim-prep first using tools/cargo.ps1 build --locked --jobs 1.' }
$local = Join-Path $prepRoot 'local'
[System.IO.Directory]::CreateDirectory($local) | Out-Null
$stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$output = Join-Path $local "census-$stamp.json"
$version = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion
& $binary inspect --data $data --runtime-version $version --output $output
$result = $LASTEXITCODE
$receipt = @{
    observed_utc = [DateTime]::UtcNow.ToString('o')
    installation = $Install
    runtime_version = $version
    steam_manifest = $manifestInfo
    census = $output
    exit_code = $result
} | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $local "install-$stamp.json"), $receipt, [System.Text.UTF8Encoding]::new($false))
Write-Output "Census: $output"
exit $result
