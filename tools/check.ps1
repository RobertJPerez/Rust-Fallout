# Fast, asset-free checks. Retail comparisons are separate because they read gigabytes.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $cargo = Join-Path $PSScriptRoot 'cargo.ps1'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
    & powershell -NoProfile -ExecutionPolicy Bypass -File $cargo test --workspace --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    & powershell -NoProfile -ExecutionPolicy Bypass -File $cargo clippy --workspace --all-targets --locked -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
} finally {
    Pop-Location
}
