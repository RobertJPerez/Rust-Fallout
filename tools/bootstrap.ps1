# Install the pinned toolchain into this checkout without changing the user's PATH.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$toolRoot = Join-Path $projectRoot '.tools'
New-Item -ItemType Directory -Force -Path $toolRoot | Out-Null
$env:CARGO_HOME = Join-Path $toolRoot 'cargo'
$env:RUSTUP_HOME = Join-Path $toolRoot 'rustup'
$rustup = Join-Path $env:CARGO_HOME 'bin\rustup.exe'
if (-not (Test-Path -LiteralPath $rustup)) {
    $installer = Join-Path $toolRoot 'rustup-init.exe'
    Invoke-WebRequest -UseBasicParsing -Uri 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile $installer
    & $installer -y --no-modify-path --profile minimal --default-toolchain none
    if ($LASTEXITCODE -ne 0) { throw 'rustup installation failed' }
}
& $rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy
if ($LASTEXITCODE -ne 0) { throw 'Rust toolchain installation failed' }
Write-Output 'Rust is ready. Use tools\cargo.ps1 for builds in this checkout.'
