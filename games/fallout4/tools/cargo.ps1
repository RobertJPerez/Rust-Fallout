# Use the installed compiler, with private Cargo caches and build output.
$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $prepRoot 'local\cargo-home'
$env:CARGO_TARGET_DIR = Join-Path $prepRoot 'local\target'
$compilerRoot = 'G:\Rust-Fallout\.tools\rustup\toolchains\1.99.0-x86_64-pc-windows-msvc\bin'
$env:RUSTC = Join-Path $compilerRoot 'rustc.exe'
$env:RUSTDOC = Join-Path $compilerRoot 'rustdoc.exe'
$env:PATH = "$compilerRoot;$env:PATH"
& (Join-Path $compilerRoot 'cargo.exe') @args
exit $LASTEXITCODE
