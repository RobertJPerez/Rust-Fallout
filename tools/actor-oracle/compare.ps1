param(
    [Parameter(Mandatory=$true)][string]$Fallout,
    [Parameter(Mandatory=$true)][string]$Oracle,
    [Parameter(Mandatory=$true)][string]$Install,
    [Parameter(Mandatory=$true)][string]$LoadOrder,
    [Parameter(Mandatory=$true)][string]$RunDirectory,
    [switch]$AllowSourceFindings,
    [switch]$SkipReordered
)
$ErrorActionPreference = 'Stop'
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
function Invoke-ActorOracle([string]$Order, [string]$Output, [string]$Log) {
    $actorStart = New-Object Diagnostics.ProcessStartInfo
    $actorStart.FileName = $actorOracle
    $actorStart.Arguments = '"' + (Join-Path $actorInstall 'Data') + '" "' + $Order + '"'
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
        $null = $actorCopyTask.GetAwaiter().GetResult()
        $actorProcess.WaitForExit()
        [IO.File]::WriteAllText($Log, $actorErrorTask.GetAwaiter().GetResult(), $actorUtf8)
        if ($actorProcess.ExitCode -ne 0) { throw 'Independent actor reader failed; see its log' }
    } finally { $actorOutputStream.Dispose(); $actorProcess.Dispose() }
    $actorCommands.Add(@($actorOracle, (Join-Path $actorInstall 'Data'), $Order))
}
$actorOrderJson = Join-Path $actorRun 'order.json'
[IO.File]::WriteAllText($actorOrderJson, (ConvertTo-Json -InputObject $actorNames), $actorUtf8)
$actorOrderBinary = Join-Path $actorRun 'order.bin'
Write-ActorOrder $actorOrderBinary $actorNames
$actorOracleJson = Join-Path $actorRun 'oracle.json'
Invoke-ActorOracle $actorOrderBinary $actorOracleJson (Join-Path $actorRun 'oracle.log')
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
        Invoke-ActorOracle $actorOrderBinary $actorOracleJson (Join-Path $actorRun 'reordered-oracle.log')
    }
    $actorOutput = Join-Path $actorRun ($actorPhase + '.json')
    $actorArguments = @('actor-sources','--install',$actorInstall,'--load-order',$actorOrderJson,
        '--index-cache',$actorCache,'--compare-oracle',$actorOracleJson,'--output',$actorOutput)
    # Windows PowerShell presents native stderr (including the CLI's normal
    # "Wrote ..." notice) as an error record. Exit status determines success.
    $actorPriorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $actorFallout @actorArguments 2> (Join-Path $actorRun ($actorPhase + '.log'))
        $actorExit = $LASTEXITCODE
    } finally { $ErrorActionPreference = $actorPriorPreference }
    if ($actorExit -ne 0 -and -not ($AllowSourceFindings -and $actorExit -eq 1 -and (Test-Path -LiteralPath $actorOutput))) {
        throw "Actor $actorPhase comparison failed; see its log"
    }
    $actorCommands.Add(@($actorFallout) + $actorArguments)
    $actorPhases.Add([ordered]@{name=$actorPhase; exit_code=$actorExit; rust_report_sha256=(Get-FileHash -LiteralPath $actorOutput).Hash.ToLowerInvariant(); oracle_report_sha256=(Get-FileHash -LiteralPath $actorOracleJson).Hash.ToLowerInvariant()})
}
$actorReceipt = [ordered]@{
    schema_version=1
    task_id='ACT-01'
    scope='Private worker source-field comparisons; not an integrated checkpoint or retail acceptance receipt'
    started_source_revision=(git rev-parse HEAD).Trim()
    engine_binary_sha256=(Get-FileHash -LiteralPath $actorFallout).Hash.ToLowerInvariant()
    oracle_binary_sha256=(Get-FileHash -LiteralPath $actorOracle).Hash.ToLowerInvariant()
    phases=$actorPhases.ToArray()
    commands=$actorCommands.ToArray()
    retail_parity_accepted=$false
}
[IO.File]::WriteAllText((Join-Path $actorRun 'worker-comparison.json'), (ConvertTo-Json -InputObject $actorReceipt -Depth 10), $actorUtf8)
Write-Output (Join-Path $actorRun 'worker-comparison.json')
