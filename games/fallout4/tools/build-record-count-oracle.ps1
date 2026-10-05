$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$researchRoot = Join-Path $projectRoot 'local/research/Mutagen'
$revision = (& git -C $researchRoot rev-parse HEAD).Trim()
if ($revision -ne '4f533562ee0c70347d47c1979d5464d42b06ee6b') { throw 'Wrong Mutagen revision' }
if (& git -C $researchRoot status --porcelain --untracked-files=no) { throw 'Modified Mutagen source' }
$env:NUGET_PACKAGES = Join-Path $projectRoot 'local/nuget'
$env:DOTNET_CLI_HOME = Join-Path $projectRoot 'local/dotnet-home'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$artifactPath = Join-Path $projectRoot 'local/dotnet-artifacts/record-count-oracle'
$project = Join-Path $PSScriptRoot 'record-count-oracle/record-count-oracle.csproj'
& dotnet build $project -c Release --artifacts-path $artifactPath -m:1 -p:GeneratePackageOnBuild=false -v:minimal
exit $LASTEXITCODE
