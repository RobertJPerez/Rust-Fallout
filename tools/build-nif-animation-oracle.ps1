# Separate offline GPL oracle. Research inputs remain read-only.
param(
    [string]$NiflySource = 'G:\Rust-Fallout\.research\nifly',
    [string]$BuildDirectory
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $BuildDirectory) { $BuildDirectory = Join-Path $projectRoot 'local\nif-animation-oracle-build' }
$revision = 'cca0a770094bb962fb28ea1fec5ea903e68fda8e'
$actual = & git -C $NiflySource rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual -ne $revision) { throw 'nifly revision differs from the source pin' }
$dirty = & git -C $NiflySource status --porcelain --untracked-files=no
if ($LASTEXITCODE -ne 0 -or $dirty) { throw 'nifly tracked source files are modified' }
$cmakeCommand = Get-Command cmake -ErrorAction SilentlyContinue
if ($cmakeCommand) { $cmake = $cmakeCommand.Source } else {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $vsInstall = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $cmake = Join-Path $vsInstall 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
}
& $cmake -S (Join-Path $PSScriptRoot 'nif-animation-oracle') -B $BuildDirectory -A x64 "-DNIFLY_SOURCE_DIR=$NiflySource"
if ($LASTEXITCODE -ne 0) { throw 'Animation oracle configuration failed' }
& $cmake --build $BuildDirectory --config Release --target nif-animation-oracle --parallel 2
if ($LASTEXITCODE -ne 0) { throw 'Animation oracle build failed' }
Write-Output (Join-Path $BuildDirectory 'Release\nif-animation-oracle.exe')
