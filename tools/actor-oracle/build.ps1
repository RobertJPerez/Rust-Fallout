param([Parameter(Mandatory=$true)][string]$BuildDirectory)
$ErrorActionPreference = 'Stop'
$cmakeCommand = Get-Command cmake -ErrorAction SilentlyContinue
if ($cmakeCommand) { $actorCmake = $cmakeCommand.Source } else {
    $actorVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $actorVsInstall = & $actorVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $actorCmake = Join-Path $actorVsInstall 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
}
& $actorCmake -S $PSScriptRoot -B $BuildDirectory -A x64
if ($LASTEXITCODE -ne 0) { throw 'Actor oracle configuration failed' }
& $actorCmake --build $BuildDirectory --config Release --target actor-oracle actor-body-tests --parallel 2
if ($LASTEXITCODE -ne 0) { throw 'Actor oracle build failed' }
& (Join-Path $BuildDirectory 'Release\actor-body-tests.exe')
if ($LASTEXITCODE -ne 0) { throw 'Actor oracle allocation-order checks failed' }
Write-Output (Join-Path $BuildDirectory 'Release\actor-oracle.exe')
