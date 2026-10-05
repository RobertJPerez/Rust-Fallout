param(
    [string]$SourceDirectory = "local/research/Caprica",
    [string]$BuildDirectory = "local/caprica-build-001",
    [string]$CacheDirectory = "local/caprica-deps-001"
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
function Resolve-WorkspacePath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) { return [System.IO.Path]::GetFullPath($Path) }
    return [System.IO.Path]::GetFullPath((Join-Path $root $Path))
}

$source = Resolve-WorkspacePath $SourceDirectory
$build = Resolve-WorkspacePath $BuildDirectory
$cache = Resolve-WorkspacePath $CacheDirectory
$expectedRevision = "e4dee0860914d75e770d3f9ab374f7aba474b701"
if (-not (Test-Path -LiteralPath (Join-Path $source ".git"))) {
    throw "Pinned Caprica checkout is missing: $source"
}
$actualRevision = (& git -C $source rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $actualRevision -ne $expectedRevision) {
    throw "Caprica must be checked out at $expectedRevision (found $actualRevision)."
}
$dirty = (& git -C $source status --porcelain)
if ($LASTEXITCODE -ne 0 -or $dirty) {
    throw "Keep the pinned Caprica research checkout clean; use the local CMake compatibility hook."
}

$vcpkgRoot = $env:VCPKG_ROOT
if (-not $vcpkgRoot -or -not (Test-Path -LiteralPath (Join-Path $vcpkgRoot "scripts/buildsystems/vcpkg.cmake"))) {
    throw "Set VCPKG_ROOT in an x64 Visual Studio developer shell before building the offline reference tool."
}
$cmake = Get-Command cmake -ErrorAction Stop
$ninja = Get-Command ninja -ErrorAction Stop
$installed = Join-Path $cache "installed"
$downloads = Join-Path $cache "downloads"
$registries = Join-Path $cache "registries"
$binaryCache = Join-Path $cache "binary-cache"
New-Item -ItemType Directory -Force -Path $installed, $downloads, $registries, $binaryCache | Out-Null

$env:VCPKG_DOWNLOADS = $downloads
$env:X_VCPKG_REGISTRIES_CACHE = $registries
$env:VCPKG_DEFAULT_BINARY_CACHE = $binaryCache
$env:VCPKG_MAX_CONCURRENCY = "1"
$env:VCPKG_DISABLE_METRICS = "1"
$env:CMAKE_BUILD_PARALLEL_LEVEL = "1"

$compatibilityHook = Join-Path $root "tools/cmake/caprica-pugixml-compat.cmake"
& $cmake.Source -S $source -B $build -G Ninja `
    "-DCMAKE_MAKE_PROGRAM=$($ninja.Source)" `
    "-DVCPKG_INSTALLED_DIR=$installed" `
    -DVCPKG_TARGET_TRIPLET=x64-windows `
    -DCMAKE_BUILD_TYPE=Release `
    "-DCMAKE_PROJECT_Caprica_INCLUDE=$compatibilityHook"
if ($LASTEXITCODE -ne 0) { throw "Caprica CMake configure failed with exit code $LASTEXITCODE." }

& $cmake.Source --build $build --config Release --parallel 1
if ($LASTEXITCODE -ne 0) { throw "Caprica build failed with exit code $LASTEXITCODE." }
