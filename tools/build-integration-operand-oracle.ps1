param([Parameter(Mandatory=$true)][string]$BuildDirectory)
$ErrorActionPreference = 'Stop'
$integrationCmakeCommand = Get-Command cmake -ErrorAction SilentlyContinue
if ($integrationCmakeCommand) {
    $integrationCmake = $integrationCmakeCommand.Source
} else {
    $integrationVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $integrationVsInstall = & $integrationVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $integrationCmake = Join-Path $integrationVsInstall 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
}
$integrationOperandSource = Join-Path $PSScriptRoot 'operand-oracle'
& $integrationCmake -S $integrationOperandSource -B $BuildDirectory -A x64
if ($LASTEXITCODE -ne 0) { throw 'Integration operand oracle configuration failed' }
& $integrationCmake --build $BuildDirectory --config Release --target operand-oracle --parallel 2
if ($LASTEXITCODE -ne 0) { throw 'Integration operand oracle build failed' }
Write-Output (Join-Path $BuildDirectory 'Release\operand-oracle.exe')
