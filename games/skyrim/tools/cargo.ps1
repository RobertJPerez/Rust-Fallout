# Private caches and target; the existing compiler is only read.
$ErrorActionPreference = 'Stop'
$prepRoot = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $prepRoot 'local\cargo-home'
$env:CARGO_TARGET_DIR = Join-Path $prepRoot 'target'
$compilerRoot = 'G:\Rust-Fallout\.tools\rustup\toolchains\1.99.0-x86_64-pc-windows-msvc\bin'
if (-not (Test-Path -LiteralPath (Join-Path $compilerRoot 'cargo.exe'))) {
    throw 'Expected Rust 1.99.0 toolchain is missing; use an installed pinned toolchain.'
}
$env:RUSTC = Join-Path $compilerRoot 'rustc.exe'
$env:RUSTDOC = Join-Path $compilerRoot 'rustdoc.exe'
$env:PATH = "$compilerRoot;$env:PATH"
& (Join-Path $compilerRoot 'cargo.exe') @args
exit $LASTEXITCODE
