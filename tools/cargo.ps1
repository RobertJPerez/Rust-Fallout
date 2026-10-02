# Keep the development tools local; this does not change the machine's PATH.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $projectRoot '.tools\cargo'
$env:RUSTUP_HOME = Join-Path $projectRoot '.tools\rustup'
$cargoPath = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargoPath)) {
    throw 'Local Rust is missing. See README.md for setup.'
}
& $cargoPath @args
exit $LASTEXITCODE

