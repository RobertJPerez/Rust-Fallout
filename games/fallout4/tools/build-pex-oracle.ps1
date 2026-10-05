$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
$refRoot = Join-Path $prepRoot 'local\research\Champollion'
$revision = & git -C $refRoot rev-parse HEAD
if ($revision -ne 'bc961a0bdfb4831f8240e6dacee0818b4bf81e00') { throw 'Champollion revision mismatch' }
if ((& git -C $refRoot status --porcelain)) { throw 'Champollion checkout is dirty' }
$vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vsRoot = & $vswhere -latest -property installationPath
$cmake = Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$buildRoot = Join-Path $prepRoot 'local\pex-oracle-build'
& $cmake -S (Join-Path $PSScriptRoot 'pex-oracle') -B $buildRoot -A x64 "-DCHAMPOLLION_SOURCE=$refRoot"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $cmake --build $buildRoot --config Release --parallel 1
exit $LASTEXITCODE
