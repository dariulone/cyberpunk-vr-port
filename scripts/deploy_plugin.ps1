# Iteration deploy: replace ONLY the plugin DLL, preserving the previous DLL and
# the just-tested log outside the game. Never launches or restarts the game.
param(
    [Parameter(Mandatory=$true)][string]$GameRoot,
    [string]$BuildDir = 'build'
)
$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$GameRoot = (Resolve-Path -LiteralPath $GameRoot).Path
if (!(Test-Path -LiteralPath (Join-Path $GameRoot 'bin\x64\Cyberpunk2077.exe'))) { throw 'Not a game root' }
if (Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue) { throw 'Close Cyberpunk2077 before deployment' }
$relative = 'red4ext\plugins\cyberpunkvrport\cyberpunkvrport.dll'
$source = Join-Path $RepoRoot "$BuildDir\bin\red4ext\plugins\cyberpunkvrport\Release\cyberpunkvrport.dll"
$target = Join-Path $GameRoot $relative
if (!(Test-Path -LiteralPath $source -PathType Leaf)) { throw "Plugin not built: $source" }
if (!(Test-Path -LiteralPath (Split-Path $target -Parent))) { throw 'Install the full mod first' }
$backupParent = Join-Path $RepoRoot 'build'
if (!(Test-Path -LiteralPath $backupParent)) { throw 'Missing backup parent' }
$backup = Join-Path $backupParent ('roomscale-plugin-deploy-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
if (Test-Path -LiteralPath $backup) { throw "Backup already exists: $backup" }
New-Item -ItemType Directory -Path $backup | Out-Null
function Hash([string]$path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
$previousHash = $null
if (Test-Path -LiteralPath $target) {
    $previousHash = Hash $target
    $before = Join-Path $backup 'CyberpunkVR_Stereo.before.dll'
    Copy-Item -LiteralPath $target -Destination $before
    if ((Hash $before) -ne $previousHash) { throw 'DLL backup mismatch' }
}
$log = Join-Path $GameRoot 'red4ext\plugins\cyberpunkvrport\log\cyberpunkvrport.log'
if (Test-Path -LiteralPath $log) { Copy-Item -LiteralPath $log -Destination (Join-Path $backup 'previous-run.log') }
$ini = Join-Path $GameRoot 'bin\x64\vrport.ini'
$iniBefore = if (Test-Path -LiteralPath $ini) { Hash $ini } else { $null }
$calibration = Join-Path (Split-Path $target -Parent) 'vrik_calibration.ini'
$calibrationBefore = if (Test-Path -LiteralPath $calibration) { Hash $calibration } else { $null }
if ($calibrationBefore) {
    $calibrationBackup = Join-Path $backup 'vrik_calibration.before.ini'
    Copy-Item -LiteralPath $calibration -Destination $calibrationBackup
    if ((Hash $calibrationBackup) -ne $calibrationBefore) { throw 'Calibration backup mismatch' }
}
$builtHash = Hash $source
Copy-Item -LiteralPath $source -Destination $target -Force
if ((Hash $target) -ne $builtHash) { throw 'Installed DLL mismatch' }
$iniAfter = if (Test-Path -LiteralPath $ini) { Hash $ini } else { $null }
if ($iniBefore -ne $iniAfter) { throw 'Runtime settings changed during deployment' }
$calibrationAfter = if (Test-Path -LiteralPath $calibration) { Hash $calibration } else { $null }
if ($calibrationBefore -ne $calibrationAfter) { throw 'Calibration changed during deployment' }
$report = [pscustomobject]@{
    GameRoot=$GameRoot; Source=$source; Target=$target; Backup=$backup
    PreviousSHA256=$previousHash; SHA256=$builtHash
    Bytes=(Get-Item -LiteralPath $target).Length; ChangedGameFiles=@($relative)
    SettingsUnchanged=$true; IniSHA256=$iniAfter; GameStarted=$false
    CalibrationSHA256=$calibrationAfter
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $backup 'deployment.json') -Encoding UTF8
$report | ConvertTo-Json -Depth 5
