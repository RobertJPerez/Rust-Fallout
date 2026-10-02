# This GPL tool is separate from the Rust runtime. It reads only local cache files.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$checkout = Join-Path $projectRoot '.research\nifly'
$revision = 'cca0a770094bb962fb28ea1fec5ea903e68fda8e'
if (-not (Test-Path -LiteralPath $checkout)) {
    & git clone --filter=blob:none --no-checkout https://github.com/ousnius/nifly $checkout
    if ($LASTEXITCODE -ne 0) { throw 'nifly checkout failed' }
    & git -C $checkout checkout $revision
    if ($LASTEXITCODE -ne 0) { throw 'nifly revision checkout failed' }
}
$actual = & git -C $checkout rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual -ne $revision) { throw 'nifly revision does not match sources.lock.json' }
$cmakeCommand = Get-Command cmake -ErrorAction SilentlyContinue
if ($cmakeCommand) {
    $cmake = $cmakeCommand.Source
} else {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $vsInstall = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $cmake = Join-Path $vsInstall 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
}
$build = Join-Path $projectRoot 'local\nif-oracle-build'
& $cmake -S (Join-Path $PSScriptRoot 'nif-oracle') -B $build -A x64 "-DNIFLY_SOURCE_DIR=$checkout"
if ($LASTEXITCODE -ne 0) { throw 'NIF oracle configuration failed' }
& $cmake --build $build --config Release --target nif-oracle --parallel 8
if ($LASTEXITCODE -ne 0) { throw 'NIF oracle build failed' }
Write-Output (Join-Path $build 'Release\nif-oracle.exe')
