param(
    [string]$BinaryDir = (Join-Path $PSScriptRoot '..\..\build\resource-profiler\Release'),
    [string]$OutputRoot = (Join-Path $PSScriptRoot '..\..\build\resource-profiler\verification')
)
$ErrorActionPreference = 'Stop'
$BinaryDir = [IO.Path]::GetFullPath($BinaryDir)
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$verifyRoot = Join-Path $OutputRoot ('run-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($verifyRoot) | Out-Null
$profilerExe = Join-Path $BinaryDir 'CyberpunkVRProfiler.exe'
$probeExe = Join-Path $BinaryDir 'profiler_probe.exe'
$profilerProcess = $null
$probeProcess = $null
try {
    # Start-Process does not preserve argv boundaries: quote path arguments here.
    $verifyArguments = '--duration 12 --interval-ms 250 --threads all --target profiler_probe.exe --output "' + $verifyRoot + '"'
    $profilerProcess = Start-Process -FilePath $profilerExe -ArgumentList $verifyArguments -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $verifyRoot 'profiler.stdout.txt') -RedirectStandardError (Join-Path $verifyRoot 'profiler.stderr.txt')
    Start-Sleep -Milliseconds 1200
    $probeJson = Join-Path $verifyRoot 'probe.json'
    $probeProcess = Start-Process -FilePath $probeExe -WindowStyle Hidden -PassThru -RedirectStandardOutput $probeJson -RedirectStandardError (Join-Path $verifyRoot 'probe.stderr.txt')
    if (-not $probeProcess.WaitForExit(20000)) { throw 'Probe timed out' }
    if (-not $profilerProcess.WaitForExit(20000)) { throw 'Profiler timed out' }
    if ($probeProcess.ExitCode -ne 0 -or $profilerProcess.ExitCode -ne 0) { throw 'Workload or profiler failed' }
    $probeResult = [IO.File]::ReadAllText($probeJson) | ConvertFrom-Json
    $session = Get-ChildItem -LiteralPath $verifyRoot -Directory | Select-Object -First 1
    if (-not $session) { throw 'No output session' }
    $processRows = @(Import-Csv -LiteralPath (Join-Path $session.FullName 'processes.csv') | Where-Object pid -eq $probeResult.pid)
    $threadRows = @(Import-Csv -LiteralPath (Join-Path $session.FullName 'threads.csv') | Where-Object pid -eq $probeResult.pid)
    $busyRows = @($threadRows | Where-Object tid -eq $probeResult.busy_tid)
    $idleRows = @($threadRows | Where-Object tid -eq $probeResult.idle_tid)
    if ($processRows.Count -lt 15 -or $busyRows.Count -lt 15 -or $idleRows.Count -lt 15) { throw 'Missing dynamic process/thread samples' }
    if ($processRows[0].cpu_machine_pct -ne '' -or $busyRows[0].cpu_one_core_pct -ne '') { throw 'First observation incorrectly presented as CPU zero' }
    $invariant = [Globalization.CultureInfo]::InvariantCulture
    function Read-Number([string]$Value) { [double]::Parse($Value, $invariant) }
    $busyPeak = ($busyRows | Where-Object cpu_one_core_pct -ne '' | ForEach-Object { Read-Number $_.cpu_one_core_pct } | Measure-Object -Maximum).Maximum
    $idlePeak = ($idleRows | Where-Object cpu_one_core_pct -ne '' | ForEach-Object { Read-Number $_.cpu_one_core_pct } | Measure-Object -Maximum).Maximum
    if ($busyPeak -lt 40 -or $busyPeak -gt 115 -or $idlePeak -gt 10) { throw "Unexpected thread percentages: busy=$busyPeak idle=$idlePeak" }
    if (-not ($busyRows | Where-Object thread_name -eq 'Profiler test: busy, "core"')) { throw 'Thread names/CSV escaping failed' }
    $threadSummary = Import-Csv -LiteralPath (Join-Path $session.FullName 'thread_summary.csv') | Where-Object { $_.pid -eq $probeResult.pid -and $_.tid -eq $probeResult.busy_tid }
    $observedCpu = Read-Number $threadSummary.cpu_observed_s
    $actualCpu = $probeResult.busy_cpu_100ns / 10000000.0
    if ([Math]::Abs($actualCpu - $observedCpu) -gt 1.2) { throw "CPU counter mismatch: profiler=$observedCpu GetThreadTimes=$actualCpu" }
    if ($probeResult.gpu_textures -eq 4) {
        $vram = @($processRows | Where-Object vram_status -eq 'ok')
        $gpuRows = @($processRows | Where-Object gpu_status -eq 'ok')
        if (-not $vram.Count -or -not $gpuRows.Count) { throw 'Dynamically created GPU process was not discovered' }
        $vramPeak = ($vram | ForEach-Object { Read-Number $_.vram_dedicated_mib } | Measure-Object -Maximum).Maximum
        $sharedPeak = ($processRows | Where-Object shared_status -eq 'ok' | ForEach-Object { Read-Number $_.gpu_shared_mib } | Measure-Object -Maximum).Maximum
        $gpuPeak = ($gpuRows | ForEach-Object { Read-Number $_.gpu_busiest_engine_pct } | Measure-Object -Maximum).Maximum
        if (($vramPeak + $sharedPeak) -lt 240 -or $gpuPeak -le 0) { throw "GPU allocation/activity missing: dedicated=$vramPeak shared=$sharedPeak busy=$gpuPeak" }
        Write-Output "GPU verified: dedicated peak $vramPeak MiB, shared peak $sharedPeak MiB, busiest engine $gpuPeak%"
    } else { Write-Output 'GPU allocation check skipped: no suitable hardware D3D11 device' }
    $logText = [IO.File]::ReadAllText((Join-Path $session.FullName 'session.log'))
    if (-not $logText.Contains('TARGET_PRESENT') -or -not $logText.Contains('TARGET_EXITED') -or -not $logText.Contains('STOP normal')) { throw 'Lifecycle/finalization records missing' }
    if (([IO.File]::ReadAllText((Join-Path $session.FullName 'summary.txt'))).IndexOf('| FINAL', [StringComparison]::Ordinal) -lt 0) { throw 'Final summary missing' }
    Write-Output "CPU verified: busy peak $busyPeak%, idle peak $idlePeak%; observed $observedCpu s / GetThreadTimes $actualCpu s"
    Write-Output "Integration verification passed: $($session.FullName)"
} finally {
    foreach ($ownedProcess in @($probeProcess, $profilerProcess)) {
        if ($null -ne $ownedProcess) {
            if (-not $ownedProcess.HasExited) { $ownedProcess.Kill(); $ownedProcess.WaitForExit() }
            $ownedProcess.Dispose()
        }
    }
}
