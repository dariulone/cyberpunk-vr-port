param(
    [ValidateSet('alone', 'dump-with-port', 'pretend-unknown')][string]$Mode = 'alone',
    [int]$TimeoutSeconds = 240,
    # dump-with-port: drive the left stick through the port's XInput path before closing (stick_session.py)
    [switch]$StickTest,
    # dump-with-port: walk and squat the simulator's head (roomscale_session.py) before closing
    [switch]$RoomscaleTest,
    # dump-with-port: the player's solve's probes at rest, squatting and leaning (vrik_session.py)
    [switch]$VrikTest,
    # dump-with-port: Home posted to the simulator's window, the recenter base captured again (reset_view_session.py)
    [switch]$ResetViewTest,
    # dump-with-port: vrik_calibration.ini saved by the port right after its load comes out byte for byte (calibration_session.py)
    [switch]$CalibrationTest,
    # dump-with-port: CyberpunkVR_* exports written before the tests, NAME=VALUE each (set_export.py)
    [string[]]$SetExports = @(),
    # dump-with-port: both eyes' composited frame saved into the run's folder (shot_session.py)
    [switch]$ShotTest,
    # dump-with-port: CyberpunkVR_* exports read after the tests, NAME or NAME:FMT each (read_export.py)
    [string[]]$ReadExports = @(),
    # dump-with-port: the pause menu opened and closed with Esc, the submission counted over it (menu_session.py)
    [switch]$MenuTest,
    # dump-with-port: the port's natives called through CET twice in gameplay (native_probe\CyberpunkVRPort_NativeProbe,
    # installed into the port's Lua mods for this run only), each result one "[native-probe]" line of scripting.log
    [switch]$NativeTest,
    # dump-with-port: a scoped handgun given and drawn through CET in gameplay (scope_probe\CyberpunkVRPort_ScopeProbe,
    # installed into the port's Lua mods for this run only), then the hands (scope_session.py: the aim, the scanner
    # gesture) and ScopeWaitSeconds for the engine's and the port's pipelines; the "[scope-probe]" lines of
    # scripting.log are the result
    [switch]$ScopeTest,
    # the run's own saves moved aside afterwards (a probe that gives items starts the game's autosave), as -ScopeTest does
    [switch]$SavesAside,
    [int]$ScopeWaitSeconds = 20,
    # dump-with-port: extra options for the QuickBoot launch (launch_to_save.py), NAME=VALUE each for --NAME VALUE
    # (e.g. family-manifest=<path>; a leading -- would read as a parameter of this script)
    [string[]]$LaunchArgs = @(),
    # dump-with-port: before closing, wait (at most WaitForPortSeconds) for a line of the port's log matching this
    # regex (a development tour's last line, e.g. "\[VRMenu\] tour done")
    [string]$WaitForPortLine = '',
    [int]$WaitForPortSeconds = 300
)
# Through -File a list arrives as one string: split it on commas (and drop quotes) either way.
$SetExports = @($SetExports | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim(" '`"") } | Where-Object { $_ })
$ReadExports = @($ReadExports | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim(" '`"") } | Where-Object { $_ })
$LaunchArgs = @($LaunchArgs | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim(" '`"") } | Where-Object { $_ })
$LaunchArgs = @($LaunchArgs | ForEach-Object { $name, $value = $_ -split '=', 2; "--$name"; if ($null -ne $value) { $value } })
# In-game acceptance runs of the Rust migration (docs/rust-migration/). One launch per run, closed with WM_CLOSE;
# everything installed for the run is taken out again and the port's DLL is checked to be the one found.
#   alone           cvr_rust_probe.dll in red4ext\plugins\cvr_rust_probe, the port's DLL renamed aside for this launch
#                   only: RED4ext loads the Rust probe and nothing else (stage 1b)
#   dump-with-port  the port loaded as usual (a save loaded through the QuickBoot harness and the OpenXR simulator), the
#                   probe with CVR_PROBE_DUMP_ONLY=1 (registers nothing, hooks nothing, only writes log\rtti_natives.json):
#                   the port's natives as the running game's RTTI has them
#   pretend-unknown the port alone with CVR_PRETEND_UNKNOWN_BUILD=1: it must register its natives, install nothing, and the
#                   game must run flat (stage 2)
$ErrorActionPreference = 'Stop'
$game = 'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path   # this checkout
$plugins = Join-Path $game 'red4ext\plugins'
$probeDir = Join-Path $plugins 'cvr_rust_probe'
# the port's folder: at the game's root with its own loader (the Rust build), else RED4ext's plugin folder
$portRoot = Join-Path $game 'cyberpunkvrport'
if (!(Test-Path -LiteralPath (Join-Path $portRoot 'cyberpunkvrport.dll'))) { $portRoot = Join-Path $plugins 'cyberpunkvrport' }
$portDll = Join-Path $portRoot 'cyberpunkvrport.dll'
$portLog = Join-Path $portRoot 'log\cyberpunkvrport.log'
$portAside = "$portDll.rustprobe-off"
$nativeProbeDir = Join-Path $portRoot 'lua\CyberpunkVRPort_NativeProbe'
$scopeProbeDir = Join-Path $portRoot 'lua\CyberpunkVRPort_ScopeProbe'
$scriptingLog = Join-Path $portRoot 'log\lua\scripting.log'
$out = Join-Path $repo "build\rust\probe-runs\$(Get-Date -Format 'yyyyMMdd-HHmmss')-$Mode"
New-Item -ItemType Directory -Force $out | Out-Null
if (Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue) { throw 'The game is running' }
if (Test-Path -LiteralPath $probeDir) { throw "A probe is already installed: $probeDir" }
$portHash = (Get-FileHash -LiteralPath $portDll -Algorithm SHA256).Hash
$useProbe = $Mode -ne 'pretend-unknown'

$reports = 'C:\Users\lone33\AppData\Local\REDEngine\ReportQueue'
$reportsBefore = @(Get-ChildItem $reports -Directory | ForEach-Object Name)
$result = [ordered]@{ mode = $Mode; out = $out; portSha256 = $portHash.ToLowerInvariant() }
try {
    if ($useProbe) {
        New-Item -ItemType Directory -Force (Join-Path $probeDir 'scripts'), (Join-Path $probeDir 'redscript') | Out-Null
        Copy-Item -LiteralPath (Join-Path $repo 'rust\target\release\cvr_rust_probe.dll') -Destination $probeDir
        Copy-Item -LiteralPath (Join-Path $repo 'rust\target\release\cvr_rust_probe.pdb') -Destination $probeDir
    }
    if ($Mode -eq 'alone') {
        Copy-Item -LiteralPath (Join-Path $repo 'rust\crates\cvr-probe\scripts\cvr_rust_probe.reds') -Destination (Join-Path $probeDir 'scripts')
        $scc = Join-Path $repo 'build\patchday\sdk25-build\bin\red4ext\plugins\cyberpunkvrport\Release\redscript'
        Copy-Item -LiteralPath (Join-Path $scc 'scc_lib.dll'), (Join-Path $scc 'LICENSE.txt') -Destination (Join-Path $probeDir 'redscript')
        Move-Item -LiteralPath $portDll -Destination $portAside
    }
    $scriptingStart = if (Test-Path -LiteralPath $scriptingLog) { (Get-Item -LiteralPath $scriptingLog).Length } else { 0 }
    # -ScopeTest: the saves there before the launch (the items the probe gives start the game's autosave)
    $savesDir = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'Saved Games\CD Projekt Red\Cyberpunk 2077'
    $savesBefore = if (($ScopeTest -or $SavesAside) -and (Test-Path -LiteralPath $savesDir)) { @((Get-ChildItem -LiteralPath $savesDir -Directory).Name) } else { @() }
    if ($NativeTest -and $Mode -eq 'dump-with-port') {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'native_probe\CyberpunkVRPort_NativeProbe') -Destination $nativeProbeDir -Recurse
    }
    if ($ScopeTest -and $Mode -eq 'dump-with-port') {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'scope_probe\CyberpunkVRPort_ScopeProbe') -Destination $scopeProbeDir -Recurse
    }
    # The Rust build never starts the built-in Lua host; the natives probe is a Lua mod and asks for it (a development
    # switch the C++ build ignores).
    $env:CVR_LUA = if (($NativeTest -or $ScopeTest) -and $Mode -eq 'dump-with-port') { '1' } else { $null }
    $exe = Join-Path $game 'bin\x64\Cyberpunk2077.exe'
    $env:CVR_PROBE_DUMP_ONLY = if ($Mode -eq 'dump-with-port') { '1' } else { $null }
    $env:CVR_PRETEND_UNKNOWN_BUILD = if ($Mode -eq 'pretend-unknown') { '1' } else { $null }
    if ($Mode -eq 'dump-with-port') {
        # The port's own launcher dialog and loading screens: the QuickBoot harness gets through them to a save.
        $launch = & python (Join-Path $repo 'tools\quickboot_tests\launch_to_save.py') --timeout 420 @LaunchArgs 2>&1
        $launch | Set-Content -LiteralPath (Join-Path $out 'launch.txt')
        $proc = Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $proc) { throw "The game did not start: $($launch | Select-Object -Last 3)" }
    } else {
        $proc = Start-Process -FilePath $exe -ArgumentList '-skipStartScreen' -WorkingDirectory (Split-Path $exe) -PassThru
    }
    $env:CVR_PROBE_DUMP_ONLY = $null
    $env:CVR_PRETEND_UNKNOWN_BUILD = $null
    $result.pid = $proc.Id
    $log = if ($useProbe) { Join-Path $probeDir 'log\cvr_rust_probe.log' } else { $portLog }
    $dump = Join-Path $probeDir 'log\rtti_natives.json'
    $wanted = switch ($Mode) {
        'alone' { 'CVRRustProbe\(42\) -> 43' }
        'dump-with-port' { 'native global functions ->' }
        'pretend-unknown' { 'NOT the same build' }
    }
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $seen = $false
    while ((Get-Date) -lt $deadline -and -not $proc.HasExited) {
        if ((Test-Path -LiteralPath $log) -and (Select-String -LiteralPath $log -Pattern $wanted -Quiet)) { $seen = $true; break }
        Start-Sleep -Seconds 2
    }
    $result.reached = $seen
    if ($Mode -eq 'pretend-unknown' -and $seen) {
        # Flat: the game gets to its main menu on its own; give it the time and see that it is still running.
        Start-Sleep -Seconds 60
        $result.stillRunningAfter60s = -not $proc.HasExited
    }
    if ($SetExports.Count -gt 0 -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.setExports = (& python (Join-Path $PSScriptRoot 'set_export.py') @SetExports 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($StickTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.stick = (& python (Join-Path $PSScriptRoot 'stick_session.py') 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($RoomscaleTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.roomscale = (& python (Join-Path $PSScriptRoot 'roomscale_session.py') 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($VrikTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.vrik = (& python (Join-Path $PSScriptRoot 'vrik_session.py') 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($ResetViewTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.resetView = (& python (Join-Path $PSScriptRoot 'reset_view_session.py') $portLog 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($CalibrationTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $calibrationIni = Join-Path $portRoot 'config\vrik_calibration.ini'
        $result.calibration = (& python (Join-Path $PSScriptRoot 'calibration_session.py') $calibrationIni 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($ShotTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.shot = (& python (Join-Path $PSScriptRoot 'shot_session.py') $out 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($MenuTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.menu = (& python (Join-Path $PSScriptRoot 'menu_session.py') 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($ReadExports.Count -gt 0 -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $result.exports = (& python (Join-Path $PSScriptRoot 'read_export.py') @ReadExports 2>&1 | Select-Object -Last 1) -join ' '
    }
    if ($NativeTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        # Two passes, each asked for through go.txt beside the mod once the tests above have the game running; the lines
        # written since this launch are the result.
        $lines = @()
        foreach ($nativePass in 1, 2) {
            [System.IO.File]::WriteAllText((Join-Path $nativeProbeDir 'go.txt'), "$nativePass")
            $nativeDeadline = (Get-Date).AddSeconds(30)
            while ((Get-Date) -lt $nativeDeadline -and -not $proc.HasExited) {
                $stream = [System.IO.FileStream]::new($scriptingLog, 'Open', 'Read', 'ReadWrite')
                try {
                    $stream.Seek($scriptingStart, 'Begin') | Out-Null
                    $reader = [System.IO.StreamReader]::new($stream)
                    $lines = @($reader.ReadToEnd() -split "`r?`n" | Where-Object { $_.Contains('[native-probe]') })
                }
                finally { $stream.Dispose() }
                if ($lines | Where-Object { $_.EndsWith("[native-probe] pass $nativePass done") }) { break }
                Start-Sleep -Seconds 1
            }
            Start-Sleep -Seconds 2
        }
        $result.natives = @($lines | ForEach-Object { $_.Substring($_.IndexOf('[native-probe]') + 15) })
        $lines | Set-Content -LiteralPath (Join-Path $out 'natives.txt') -Encoding utf8
    }
    if ($ScopeTest -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        [System.IO.File]::WriteAllText((Join-Path $scopeProbeDir 'go.txt'), '1')
        $scopeLines = @()
        $scopeDeadline = (Get-Date).AddSeconds(60)
        while ((Get-Date) -lt $scopeDeadline -and -not $proc.HasExited) {
            $stream = [System.IO.FileStream]::new($scriptingLog, 'Open', 'Read', 'ReadWrite')
            try {
                $stream.Seek($scriptingStart, 'Begin') | Out-Null
                $reader = [System.IO.StreamReader]::new($stream)
                $scopeLines = @($reader.ReadToEnd() -split "`r?`n" | Where-Object { $_.Contains('[scope-probe]') })
            }
            finally { $stream.Dispose() }
            if ($scopeLines | Where-Object { $_.EndsWith('[scope-probe] pass 1 done') }) { break }
            Start-Sleep -Seconds 1
        }
        if (-not $proc.HasExited) {
            # the hands: the aim with the scoped rifle, then the scanner gesture (scope_session.py)
            $result.scopeHands = (& python (Join-Path $PSScriptRoot 'scope_session.py') 2>&1 | Select-Object -Last 1) -join ' '
        }
        Start-Sleep -Seconds $ScopeWaitSeconds
        $result.scope = @($scopeLines | ForEach-Object { $_.Substring($_.IndexOf('[scope-probe]') + 14) })
    }
    if ($WaitForPortLine -and $Mode -eq 'dump-with-port' -and -not $proc.HasExited) {
        $portDeadline = (Get-Date).AddSeconds($WaitForPortSeconds)
        $result.portLineSeen = $false
        while ((Get-Date) -lt $portDeadline -and -not $proc.HasExited) {
            if ((Test-Path -LiteralPath $portLog) -and (Select-String -LiteralPath $portLog -Pattern $WaitForPortLine -Quiet)) { $result.portLineSeen = $true; break }
            Start-Sleep -Seconds 2
        }
    }
    $result.exitedEarly = $proc.HasExited
    Start-Sleep -Seconds 5
    $result.close = (& (Join-Path $PSScriptRoot 'close_game.ps1') -TimeoutSeconds 60) -join ' '
    if (($ScopeTest -or $SavesAside) -and (Test-Path -LiteralPath $savesDir)) {
        # The run's saves out of the save folder, into the run's own (kept, not deleted): QuickBoot's Continue loads the
        # newest save, and the next run (or the player) must find the one from before the probe gave its items.
        $made = @(Get-ChildItem -LiteralPath $savesDir -Directory | Where-Object { $savesBefore -notcontains $_.Name })
        if ($made.Count -gt 0) {
            Start-Sleep -Seconds 3
            $aside = Join-Path $out 'saves-aside'
            New-Item -ItemType Directory -Force -Path $aside | Out-Null
            foreach ($d in $made) { Move-Item -LiteralPath $d.FullName -Destination (Join-Path $aside $d.Name) }
        }
        $result.savesAside = @($made | ForEach-Object { $_.Name })
    }
    Start-Sleep -Seconds 3
    if ($useProbe -and (Test-Path -LiteralPath $log)) { Copy-Item -LiteralPath $log -Destination $out }
    if (Test-Path -LiteralPath $dump) { Copy-Item -LiteralPath $dump -Destination $out }
    if ($Mode -ne 'alone' -and (Test-Path -LiteralPath $portLog)) { Copy-Item -LiteralPath $portLog -Destination $out }
    $red4extLog = Get-ChildItem (Join-Path $game 'red4ext\logs') -File | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($red4extLog) { Copy-Item -LiteralPath $red4extLog.FullName -Destination (Join-Path $out 'red4ext.log') }
    $new = @(Get-ChildItem $reports -Directory | Where-Object { $reportsBefore -notcontains $_.Name })
    $result.crashReports = @($new | ForEach-Object Name)
    foreach ($r in $new) { $st = Join-Path $r.FullName 'stacktrace.txt'; if (Test-Path $st) { Copy-Item $st (Join-Path $out "$($r.Name)-stacktrace.txt") } }
    Get-Process CrashReporter -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null; if (-not $_.WaitForExit(3000)) { $_.Kill() } }
}
finally {
    $env:CVR_PROBE_DUMP_ONLY = $null
    $env:CVR_PRETEND_UNKNOWN_BUILD = $null
    if (Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue) { & (Join-Path $PSScriptRoot 'close_game.ps1') -TimeoutSeconds 30 | Out-Null }
    # A game still exiting (a crash at exit writes its report first) holds the probe's log open: wait for it and
    # retry the cleanup, so a slow exit cannot cost the run its result.json.
    $exitWait = (Get-Date).AddSeconds(60)
    while ((Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue) -and (Get-Date) -lt $exitWait) { Start-Sleep -Milliseconds 500 }
    $removeRetrying = {
        param([string]$Path)
        for ($try = 1; (Test-Path -LiteralPath $Path) -and $try -le 20; ++$try) {
            try { Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop } catch { Start-Sleep -Milliseconds 500 }
        }
    }
    if (Test-Path -LiteralPath $portAside) { Move-Item -LiteralPath $portAside -Destination $portDll }
    & $removeRetrying $probeDir
    & $removeRetrying $nativeProbeDir
    & $removeRetrying $scopeProbeDir
    $result.portRestored = ((Get-FileHash -LiteralPath $portDll -Algorithm SHA256).Hash -eq $portHash)
    $result.probeRemoved = -not (Test-Path -LiteralPath $probeDir)
    $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $out 'result.json') -Encoding utf8
    $result | ConvertTo-Json -Depth 4
}
