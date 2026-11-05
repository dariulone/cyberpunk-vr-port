param(
    [Parameter(Mandatory=$true)][ValidateRange(0,2147483647)][int]$GameProcessId,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9-]+$')][string]$Label,
    [Parameter(Mandatory=$true)][ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$Previous,
    [string]$GameRoot='C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'
)
$ErrorActionPreference='Stop'
if ([IO.Path]::GetFullPath($GameRoot).TrimEnd('\') -ne 'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077') {
    throw 'The live test helpers currently target this local Steam installation only'
}
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ready=Join-Path $PSScriptRoot 'run_launch_tests.py'
$exe=Join-Path $GameRoot 'bin\x64\Cyberpunk2077.exe'
$dll=Join-Path $GameRoot 'red4ext\plugins\cyberpunkvrport\cyberpunkvrport.dll'
$built=Join-Path $repo 'build\bin\red4ext\plugins\cyberpunkvrport\Release\cyberpunkvrport.dll'
if (-not (Test-Path -LiteralPath $ready)) { throw 'Stereo readiness helper missing' }
if (Get-Process -Name qrenderdoc -ErrorAction SilentlyContinue) { throw 'Close RenderDoc first' }
if ((Get-FileHash -LiteralPath $dll).Hash -ne $Previous) { throw 'Installed DLL changed before restart' }
$expected=(Get-FileHash -LiteralPath $built).Hash
if ($GameProcessId -gt 0) {
    $game=Get-Process -Id $GameProcessId -ErrorAction Stop
    if ($game.Path -ne $exe) { throw 'PID does not match the game executable' }
    $null=$game.CloseMainWindow()
    if (-not $game.WaitForExit(5000)) {
        Stop-Process -Id $GameProcessId -Force
        if (-not $game.WaitForExit(10000)) { throw 'Game did not exit; deployment cancelled' }
    }
}
& (Join-Path $PSScriptRoot 'deploy_probe.ps1') -Label $Label -Previous $Previous -GameRoot $GameRoot
if ((Get-FileHash -LiteralPath $dll).Hash -ne $expected) { throw 'Deployment hash verification failed' }
& py -3.13 -B $ready --out (Join-Path $repo ('build\single-pass-20260930\'+$Label))
if ($LASTEXITCODE -ne 0) { throw 'Stereo launch tests failed; inspect summary.json and crash reports' }
