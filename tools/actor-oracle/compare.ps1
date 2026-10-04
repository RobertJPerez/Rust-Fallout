param(
    [Parameter(Mandatory=$true)][string]$Fallout,
    [Parameter(Mandatory=$true)][string]$Oracle,
    [Parameter(Mandatory=$true)][string]$Install,
    [Parameter(Mandatory=$true)][string]$LoadOrder,
    [Parameter(Mandatory=$true)][string]$RunDirectory,
    [switch]$AllowSourceFindings,
    [switch]$IncludeAssociations,
    [switch]$IncludeClasses,
    [switch]$IncludeFactions,
    [switch]$IncludePlacements,
    [switch]$IncludeRaces,
    [switch]$IncludePackages,
    [switch]$IncludePackageDependencies,
    [switch]$IncludeDependencies,
    [string[]]$DependencyRoots = @(),
    [string]$DependencyOracle,
    [string]$PackageDependencyOracle,
    [string]$TeamDirectory,
    [string]$SessionId,
    [switch]$SkipReordered
)
$ErrorActionPreference = 'Stop'
if ($IncludePackageDependencies -and -not $IncludePackages) {
    throw 'Package dependencies require IncludePackages'
}
if ($DependencyRoots.Count -gt 64 -or ($DependencyRoots.Count -gt 0 -and -not $IncludeDependencies)) {
    throw 'Dependency roots require IncludeDependencies and at most 64 roots'
}
$actorInstall = (Resolve-Path -LiteralPath $Install).Path.TrimEnd('\')
$actorRun = [IO.Path]::GetFullPath($RunDirectory).TrimEnd('\')
if ($actorRun.Equals($actorInstall, [StringComparison]::OrdinalIgnoreCase) -or
    $actorRun.StartsWith($actorInstall + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Actor evidence must be outside the installation'
}
if (Test-Path -LiteralPath $actorRun) { throw 'Use a new actor evidence directory' }
$null = New-Item -ItemType Directory -Path $actorRun
$actorCache = Join-Path $actorRun 'index-cache'
$null = New-Item -ItemType Directory -Path $actorCache
$actorParsedOrder = Get-Content -LiteralPath $LoadOrder -Raw | ConvertFrom-Json
$actorNames = @(foreach ($actorName in $actorParsedOrder) { [string]$actorName })
$actorFallout = (Resolve-Path -LiteralPath $Fallout).Path
$actorOracle = (Resolve-Path -LiteralPath $Oracle).Path
$actorUtf8 = New-Object System.Text.UTF8Encoding($false)
$actorCommands = New-Object 'System.Collections.Generic.List[object]'
$actorDependencySha = $null
$actorPythonSha = $null
$actorPackageSha = $null
$actorPackageCompanionSha = $null
if ($IncludeDependencies) {
    if (-not $DependencyOracle) { $DependencyOracle = Join-Path $PSScriptRoot 'dependencies.py' }
    $actorDependencyOracle = (Resolve-Path -LiteralPath $DependencyOracle).Path
    $actorDependencySha = (Get-FileHash -LiteralPath $actorDependencyOracle).Hash.ToLowerInvariant()
}
if ($IncludePackageDependencies) {
    if (-not $PackageDependencyOracle) { $PackageDependencyOracle = Join-Path $PSScriptRoot 'package_dependencies.py' }
    $actorPackageOracle = (Resolve-Path -LiteralPath $PackageDependencyOracle).Path
    $actorPackageCompanion = (Resolve-Path -LiteralPath (Join-Path (Split-Path -Parent $actorPackageOracle) 'dependencies.py')).Path
    $actorPackageSha = (Get-FileHash -LiteralPath $actorPackageOracle).Hash.ToLowerInvariant()
    $actorPackageCompanionSha = (Get-FileHash -LiteralPath $actorPackageCompanion).Hash.ToLowerInvariant()
}
if ($IncludeDependencies -or $IncludePackageDependencies) {
    $actorPython = (& py -3 -c 'import sys; print(sys.executable)').Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Independent dependency reader requires Python 3' }
    $actorPythonSha = (Get-FileHash -LiteralPath $actorPython).Hash.ToLowerInvariant()
}

# Optional current-team guard. Standalone comparisons do not require mailboxes.
# Each child is polled, so STOP does not wait for an entire source comparison.
function Test-ActorTeam {
    if (-not $TeamDirectory) { return }
    $actorControl = Get-Content -LiteralPath (Join-Path (Split-Path -Parent $TeamDirectory) 'team\control.json') -Raw | ConvertFrom-Json
    $actorAssignment = Get-Content -LiteralPath (Join-Path $TeamDirectory 'actors.assignment.json') -Raw | ConvertFrom-Json
    $actorLease = Get-Content -LiteralPath (Join-Path $TeamDirectory 'leases\actors.json') -Raw | ConvertFrom-Json
    if (-not $SessionId -or $actorLease.session_uuid -ne $SessionId -or
        $actorControl.mode -ne 'active' -or $actorControl.stop_requested -or
        $actorControl.generation -ne $actorAssignment.generation -or
        $actorControl.run_id -ne $actorAssignment.run_id -or
        $actorLease.run_id -ne $actorControl.run_id -or
        $actorAssignment.state -ne 'active' -or -not $actorAssignment.implementation_authorized) {
        throw 'Actor comparison team authorization changed'
    }
    foreach ($actorMailbox in Get-ChildItem -LiteralPath $TeamDirectory -Filter '*.outbox.jsonl') {
        foreach ($actorLine in Get-Content -LiteralPath $actorMailbox.FullName) {
            if ($actorLine.Trim()) {
                $actorRow = $actorLine | ConvertFrom-Json
                if ($actorRow.run_id -eq $actorControl.run_id -and $actorRow.type -eq 'stop_requested') {
                    throw 'Actor comparison observed current-run STOP'
                }
            }
        }
    }
}
Test-ActorTeam

function Wait-ActorProcess([Diagnostics.Process]$Process) {
    try {
        while (-not $Process.WaitForExit(1000)) { Test-ActorTeam }
        Test-ActorTeam
    } catch {
        if (-not $Process.HasExited) { & taskkill.exe /PID $Process.Id /T /F | Out-Null; $Process.WaitForExit() }
        throw
    }
}

function Get-ActorSourceSnapshot {
    $actorSourceRoot = (git rev-parse --show-toplevel).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Actor comparison requires a Git worktree' }
    $actorHead = (git rev-parse HEAD).Trim()
    $actorPaths = [string[]]@(git ls-files --cached --others --exclude-standard)
    if ($LASTEXITCODE -ne 0) { throw 'Actor source enumeration failed' }
    [Array]::Sort($actorPaths, [StringComparer]::Ordinal)
    $actorFiles = New-Object 'System.Collections.Generic.List[object]'
    foreach ($actorPath in $actorPaths) {
        $actorSourceFile = Join-Path $actorSourceRoot $actorPath
        $actorInfo = Get-Item -LiteralPath $actorSourceFile
        $actorFiles.Add([ordered]@{path=$actorPath; bytes=$actorInfo.Length; sha256=(Get-FileHash -LiteralPath $actorSourceFile).Hash.ToLowerInvariant()})
    }
    $actorManifest = ConvertTo-Json -InputObject $actorFiles.ToArray() -Depth 5 -Compress
    $actorDigest = [Security.Cryptography.SHA256]::Create()
    try {
        $actorDigestBytes = $actorDigest.ComputeHash([Text.Encoding]::UTF8.GetBytes($actorManifest))
        $actorManifestSha = -join ($actorDigestBytes | ForEach-Object { $_.ToString('x2') })
    } finally { $actorDigest.Dispose() }
    return [ordered]@{head_revision=$actorHead; working_tree_status=@(git status --porcelain=v1 --untracked-files=all); file_count=$actorFiles.Count; manifest_sha256=$actorManifestSha; files=$actorFiles.ToArray()}
}
# Capture the actual dirty source and executables before any comparison. A
# worker receipt is meaningful only while these exact inputs stay unchanged.
$actorInitialSource = Get-ActorSourceSnapshot
$actorInitialEngineSha = (Get-FileHash -LiteralPath $actorFallout).Hash.ToLowerInvariant()
$actorInitialOracleSha = (Get-FileHash -LiteralPath $actorOracle).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $actorRun 'source-start.json'), (ConvertTo-Json -InputObject $actorInitialSource -Depth 8), $actorUtf8)

function Write-ActorOrder([string]$Path, [object[]]$Names) {
    $actorBuffer = New-Object IO.MemoryStream
    $actorWriter = New-Object IO.BinaryWriter($actorBuffer)
    $actorWriter.Write([Text.Encoding]::ASCII.GetBytes('FRORDER1'))
    $actorWriter.Write([uint16]$Names.Count)
    foreach ($actorName in $Names) {
        $actorBytes = [Text.Encoding]::UTF8.GetBytes([string]$actorName)
        $actorWriter.Write([uint16]$actorBytes.Length)
        $actorWriter.Write($actorBytes)
    }
    $actorWriter.Flush()
    [IO.File]::WriteAllBytes($Path, $actorBuffer.ToArray())
    $actorWriter.Dispose()
}
function Invoke-SupplementOracle([string]$OrderJson, [string]$Output, [string]$Log, [bool]$Package) {
    $actorSuffix = $(if ($Package) { '.package-dependencies' } else { '.dependencies' })
    $actorHelper = $(if ($Package) { $actorPackageOracle } else { $actorDependencyOracle })
    $actorDependencyOutput = $Output + $actorSuffix + '.json'
    $actorDependencyArguments = @($actorHelper,'--data',(Join-Path $actorInstall 'Data'),
        '--load-order',$OrderJson,'--base-report',$Output,'--output',$actorDependencyOutput)
    if ($Package) { $actorDependencyArguments += @('--executable',(Join-Path $actorInstall 'FalloutNV.exe')) }
    else { foreach ($actorRoot in $DependencyRoots) { $actorDependencyArguments += @('--root',$actorRoot) } }
    if ($TeamDirectory) { $actorDependencyArguments += @('--team-directory',$TeamDirectory,'--session-id',$SessionId) }
    Test-ActorTeam
    $actorStart = New-Object Diagnostics.ProcessStartInfo
    $actorStart.FileName = $actorPython
    $actorStart.Arguments = (@(foreach ($actorArgument in $actorDependencyArguments) {
        '"' + ([string]$actorArgument -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"'
    }) -join ' ')
    $actorStart.UseShellExecute = $false
    $actorStart.CreateNoWindow = $true
    $actorStart.EnvironmentVariables['PYTHONDONTWRITEBYTECODE'] = '1'
    $actorStart.RedirectStandardOutput = $true
    $actorStart.RedirectStandardError = $true
    $actorProcess = New-Object Diagnostics.Process
    $actorProcess.StartInfo = $actorStart
    try {
        $null = $actorProcess.Start()
        $actorErrorTask = $actorProcess.StandardError.ReadToEndAsync()
        $actorOutputTask = $actorProcess.StandardOutput.ReadToEndAsync()
        Wait-ActorProcess $actorProcess
        [IO.File]::WriteAllText($Log + $actorSuffix + '.log', $actorErrorTask.GetAwaiter().GetResult(), $actorUtf8)
        [IO.File]::WriteAllText($Log + $actorSuffix + '.stdout', $actorOutputTask.GetAwaiter().GetResult(), $actorUtf8)
        if ($actorProcess.ExitCode -ne 0) { throw 'Independent dependency reader failed; see its log' }
    } finally { $actorProcess.Dispose() }
    Move-Item -LiteralPath $Output -Destination ($Output + $(if ($Package) { '.package-base.json' } else { '.base.json' }))
    Move-Item -LiteralPath $actorDependencyOutput -Destination $Output
    $actorCommands.Add(@($actorPython) + $actorDependencyArguments)
}
function Invoke-ActorOracle([string]$Order, [string]$OrderJson, [string]$Output, [string]$Log) {
    $actorStart = New-Object Diagnostics.ProcessStartInfo
    $actorStart.FileName = $actorOracle
    $actorStart.Arguments = '"' + (Join-Path $actorInstall 'Data') + '" "' + $Order + '"'
    if ($IncludeAssociations) { $actorStart.Arguments += ' --include-associations' }
    if ($IncludeClasses) { $actorStart.Arguments += ' --include-classes' }
    if ($IncludeFactions) { $actorStart.Arguments += ' --include-factions' }
    if ($IncludePlacements) { $actorStart.Arguments += ' --include-placements' }
    if ($IncludeRaces) { $actorStart.Arguments += ' --include-races' }
    if ($IncludePackages) { $actorStart.Arguments += ' --include-packages' }
    $actorStart.UseShellExecute = $false
    $actorStart.CreateNoWindow = $true
    $actorStart.RedirectStandardOutput = $true
    $actorStart.RedirectStandardError = $true
    $actorProcess = New-Object Diagnostics.Process
    $actorProcess.StartInfo = $actorStart
    $actorOutputStream = [IO.File]::Open($Output, [IO.FileMode]::CreateNew)
    try {
        $null = $actorProcess.Start()
        $actorErrorTask = $actorProcess.StandardError.ReadToEndAsync()
        $actorCopyTask = $actorProcess.StandardOutput.BaseStream.CopyToAsync($actorOutputStream)
        Wait-ActorProcess $actorProcess
        $null = $actorCopyTask.GetAwaiter().GetResult()
        [IO.File]::WriteAllText($Log, $actorErrorTask.GetAwaiter().GetResult(), $actorUtf8)
        if ($actorProcess.ExitCode -ne 0) { throw 'Independent actor reader failed; see its log' }
    } finally { $actorOutputStream.Dispose(); $actorProcess.Dispose() }
    $actorOracleArguments = @($actorOracle, (Join-Path $actorInstall 'Data'), $Order)
    if ($IncludeAssociations) { $actorOracleArguments += '--include-associations' }
    if ($IncludeClasses) { $actorOracleArguments += '--include-classes' }
    if ($IncludeFactions) { $actorOracleArguments += '--include-factions' }
    if ($IncludePlacements) { $actorOracleArguments += '--include-placements' }
    if ($IncludeRaces) { $actorOracleArguments += '--include-races' }
    if ($IncludePackages) { $actorOracleArguments += '--include-packages' }
    $actorCommands.Add($actorOracleArguments)
    if ($IncludeDependencies) { Invoke-SupplementOracle $OrderJson $Output $Log $false }
    if ($IncludePackageDependencies) { Invoke-SupplementOracle $OrderJson $Output $Log $true }
}
$actorOrderJson = Join-Path $actorRun 'order.json'
[IO.File]::WriteAllText($actorOrderJson, (ConvertTo-Json -InputObject $actorNames), $actorUtf8)
$actorOrderBinary = Join-Path $actorRun 'order.bin'
Write-ActorOrder $actorOrderBinary $actorNames
$actorOracleJson = Join-Path $actorRun 'oracle.json'
Invoke-ActorOracle $actorOrderBinary $actorOrderJson $actorOracleJson (Join-Path $actorRun 'oracle.log')
$actorPhases = New-Object 'System.Collections.Generic.List[object]'
foreach ($actorPhase in @('cold','warm','reordered')) {
    if ($actorPhase -eq 'reordered') {
        if ($SkipReordered -or $actorNames.Count -lt 3) { continue }
        $actorSwapped = @($actorNames)
        $actorLast = $actorSwapped.Count - 1
        $actorTemporaryName = $actorSwapped[$actorLast]
        $actorSwapped[$actorLast] = $actorSwapped[$actorLast - 1]
        $actorSwapped[$actorLast - 1] = $actorTemporaryName
        # Reordering is only valid if these last plugins are independent. The
        # production loader and oracle both reject a violated master order.
        $actorOrderJson = Join-Path $actorRun 'reordered-order.json'
        [IO.File]::WriteAllText($actorOrderJson, (ConvertTo-Json -InputObject $actorSwapped), $actorUtf8)
        $actorOrderBinary = Join-Path $actorRun 'reordered-order.bin'
        Write-ActorOrder $actorOrderBinary $actorSwapped
        $actorOracleJson = Join-Path $actorRun 'reordered-oracle.json'
        Invoke-ActorOracle $actorOrderBinary $actorOrderJson $actorOracleJson (Join-Path $actorRun 'reordered-oracle.log')
    }
    $actorOutput = Join-Path $actorRun ($actorPhase + '.json')
    $actorArguments = @('actor-sources','--install',$actorInstall,'--load-order',$actorOrderJson,
        '--index-cache',$actorCache,'--compare-oracle',$actorOracleJson,'--output',$actorOutput)
    if ($IncludeAssociations) { $actorArguments += '--include-associations' }
    if ($IncludeClasses) { $actorArguments += '--include-classes' }
    if ($IncludeFactions) { $actorArguments += '--include-factions' }
    if ($IncludePlacements) { $actorArguments += '--include-placements' }
    if ($IncludeRaces) { $actorArguments += '--include-races' }
    if ($IncludePackages) { $actorArguments += '--include-packages' }
    if ($IncludePackageDependencies) { $actorArguments += '--include-package-dependencies' }
    if ($IncludeDependencies) { $actorArguments += '--include-dependencies' }
    foreach ($actorRoot in $DependencyRoots) { $actorArguments += @('--dependency-root',$actorRoot) }
    Test-ActorTeam
    $actorStart = New-Object Diagnostics.ProcessStartInfo
    $actorStart.FileName = $actorFallout
    # Arguments here are fixed option names and absolute paths. Escape the Windows
    # trailing-backslash/quote cases rather than using shell command composition.
    $actorQuoted = @(foreach ($actorArgument in $actorArguments) {
        '"' + ([string]$actorArgument -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"'
    })
    $actorStart.Arguments = $actorQuoted -join ' '
    $actorStart.UseShellExecute = $false
    $actorStart.CreateNoWindow = $true
    $actorStart.RedirectStandardOutput = $true
    $actorStart.RedirectStandardError = $true
    $actorProcess = New-Object Diagnostics.Process
    $actorProcess.StartInfo = $actorStart
    try {
        $null = $actorProcess.Start()
        $actorErrorTask = $actorProcess.StandardError.ReadToEndAsync()
        $actorOutputTask = $actorProcess.StandardOutput.ReadToEndAsync()
        Wait-ActorProcess $actorProcess
        [IO.File]::WriteAllText((Join-Path $actorRun ($actorPhase + '.log')), $actorErrorTask.GetAwaiter().GetResult(), $actorUtf8)
        [IO.File]::WriteAllText((Join-Path $actorRun ($actorPhase + '.stdout')), $actorOutputTask.GetAwaiter().GetResult(), $actorUtf8)
        $actorExit = $actorProcess.ExitCode
    } finally { $actorProcess.Dispose() }
    if ($actorExit -ne 0 -and -not ($AllowSourceFindings -and $actorExit -eq 1 -and (Test-Path -LiteralPath $actorOutput))) {
        throw "Actor $actorPhase comparison failed; see its log"
    }
    $actorCommands.Add(@($actorFallout) + $actorArguments)
    $actorPhases.Add([ordered]@{name=$actorPhase; exit_code=$actorExit; rust_report_sha256=(Get-FileHash -LiteralPath $actorOutput).Hash.ToLowerInvariant(); oracle_report_sha256=(Get-FileHash -LiteralPath $actorOracleJson).Hash.ToLowerInvariant()})
}
$actorFinalSource = Get-ActorSourceSnapshot
Test-ActorTeam
$actorFinalEngineSha = (Get-FileHash -LiteralPath $actorFallout).Hash.ToLowerInvariant()
$actorFinalOracleSha = (Get-FileHash -LiteralPath $actorOracle).Hash.ToLowerInvariant()
if ($actorInitialSource.head_revision -ne $actorFinalSource.head_revision -or
    $actorInitialSource.manifest_sha256 -ne $actorFinalSource.manifest_sha256 -or
    (ConvertTo-Json -InputObject $actorInitialSource.working_tree_status -Compress) -ne (ConvertTo-Json -InputObject $actorFinalSource.working_tree_status -Compress) -or
    $actorInitialEngineSha -ne $actorFinalEngineSha -or $actorInitialOracleSha -ne $actorFinalOracleSha) {
    throw 'Actor source, HEAD, dirty state or executable changed during comparison; no completed worker receipt published'
}
if (($IncludeDependencies -or $IncludePackageDependencies) -and $actorPythonSha -ne (Get-FileHash -LiteralPath $actorPython).Hash.ToLowerInvariant()) {
    throw 'Independent Python binary changed during comparison'
}
if ($IncludeDependencies -and $actorDependencySha -ne (Get-FileHash -LiteralPath $actorDependencyOracle).Hash.ToLowerInvariant()) {
    throw 'Independent dependency script or Python binary changed during comparison'
}
if ($IncludePackageDependencies -and ($actorPackageSha -ne (Get-FileHash -LiteralPath $actorPackageOracle).Hash.ToLowerInvariant() -or
    $actorPackageCompanionSha -ne (Get-FileHash -LiteralPath $actorPackageCompanion).Hash.ToLowerInvariant())) {
    throw 'Independent package dependency reader or raw source companion changed during comparison'
}
[IO.File]::WriteAllText((Join-Path $actorRun 'source-finish.json'), (ConvertTo-Json -InputObject $actorFinalSource -Depth 8), $actorUtf8)
$actorReceipt = [ordered]@{
    schema_version=1
    task_id= $(if ($IncludePackageDependencies) { 'ACT-07B-package-dependencies' } elseif ($IncludeDependencies) { 'ACT-07-dependencies' } elseif ($IncludePackages) { 'ACT-06-PACK' } elseif ($IncludeRaces) { 'ACT-05-RACE' } elseif ($IncludePlacements) { 'ACT-03-core-extras' } elseif ($IncludeFactions) { 'ACT-05-FACT' } elseif ($IncludeClasses) { 'ACT-05-CLAS' } elseif ($IncludeAssociations) { 'ACT-02' } else { 'ACT-01' })
    scope='Private worker source-field comparisons; not an integrated checkpoint or retail acceptance receipt'
    started_source_revision=$actorInitialSource.head_revision
    source_snapshot_sha256=$actorInitialSource.manifest_sha256
    source_and_binaries_unchanged=$true
    engine_binary_sha256=$actorInitialEngineSha
    oracle_binary_sha256=$actorInitialOracleSha
    dependency_script_sha256=$actorDependencySha
    dependency_python_sha256=$actorPythonSha
    package_dependency_script_sha256=$actorPackageSha
    package_dependency_companion_sha256=$actorPackageCompanionSha
    phases=$actorPhases.ToArray()
    commands=$actorCommands.ToArray()
    retail_parity_accepted=$false
}
[IO.File]::WriteAllText((Join-Path $actorRun 'worker-comparison.json'), (ConvertTo-Json -InputObject $actorReceipt -Depth 10), $actorUtf8)
Write-Output (Join-Path $actorRun 'worker-comparison.json')
