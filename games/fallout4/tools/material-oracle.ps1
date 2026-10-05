param(
    [Parameter(Mandatory=$true)][string]$ExtractionDirectory,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$checkout = Join-Path $repoRoot 'local\research\Material-Editor'
$expected = '21411873f17454a2442d6785d533499e14c63adb'
$actual = (& git -C $checkout rev-parse HEAD).Trim()
if ($actual -ne $expected) { throw "Material reference revision mismatch: $actual" }
$dirty = & git -C $checkout status --porcelain
if ($LASTEXITCODE -ne 0 -or $dirty) { throw 'Material reference checkout is not clean' }
$materialLib = Join-Path $checkout 'MaterialLib'
$project = Join-Path $repoRoot 'tools\material-oracle\MaterialOracle.csproj'
$buildOutput = Join-Path $repoRoot 'local\material-oracle-bin'
$objOutput = Join-Path $repoRoot 'local\material-oracle-obj'
$cliHome = Join-Path $repoRoot 'local\dotnet-home'
$packages = Join-Path $repoRoot 'local\nuget'
$env:DOTNET_CLI_HOME = $cliHome
$env:NUGET_PACKAGES = $packages
$localRoot = (Resolve-Path -LiteralPath (Join-Path $repoRoot 'local')).Path.TrimEnd('\') + '\'
$evidencePath = [System.IO.Path]::GetFullPath($EvidenceDirectory)
if (-not $evidencePath.StartsWith($localRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Material reference output must remain under ignored local/.'
}
if (Test-Path -LiteralPath $evidencePath) {
    throw 'Evidence directory already exists; choose a new ignored local/ directory.'
}
$null = New-Item -ItemType Directory -Path $evidencePath
$out = Join-Path $evidencePath 'material-reference.jsonl'
$summary = Join-Path $evidencePath 'complete.json'
if ((Test-Path -LiteralPath $out) -or (Test-Path -LiteralPath $summary)) {
    throw 'Evidence output already exists; choose a new directory.'
}
& dotnet build $project --configuration Release --output $buildOutput `
    "-p:BaseIntermediateOutputPath=$objOutput\" "-p:MaterialLibRoot=$materialLib"
if ($LASTEXITCODE -ne 0) { throw "dotnet build failed: $LASTEXITCODE" }
$app = Join-Path $buildOutput 'MaterialOracle.dll'
& dotnet $app (Resolve-Path -LiteralPath $ExtractionDirectory).Path $out $summary
if ($LASTEXITCODE -notin @(0, 2)) { throw "material oracle failed: $LASTEXITCODE" }
exit $LASTEXITCODE
