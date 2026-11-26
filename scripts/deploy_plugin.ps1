# Iteration deploy: replace ONLY the plugin DLL (and the port's loader), preserving the previous DLL and the
# just-tested log outside the game. Never launches or restarts the game.
#
# The Rust build lives in its own folder at the game's root (<game>\cyberpunkvrport), started by its own loader
# (bin\x64\powrprof.dll); an install under red4ext\plugins\cyberpunkvrport is moved there once. -Red4extLayout keeps
# the RED4ext plugin layout (the C++ build).
param(
    [Parameter(Mandatory=$true)][string]$GameRoot,
    [string]$BuildDir = 'build',
    [switch]$Red4extLayout
)
$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$GameRoot = (Resolve-Path -LiteralPath $GameRoot).Path
if (!(Test-Path -LiteralPath (Join-Path $GameRoot 'bin\x64\Cyberpunk2077.exe'))) { throw 'Not a game root' }
if (Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue) { throw 'Close Cyberpunk2077 before deployment' }
$oldFolder = Join-Path $GameRoot 'red4ext\plugins\cyberpunkvrport'
$rootFolder = Join-Path $GameRoot 'cyberpunkvrport'
$changed = @()
if ($Red4extLayout) {
    $relative = 'red4ext\plugins\cyberpunkvrport\cyberpunkvrport.dll'
} else {
    $relative = 'cyberpunkvrport\cyberpunkvrport.dll'
    if (!(Test-Path -LiteralPath $rootFolder) -and (Test-Path -LiteralPath $oldFolder)) {
        # the whole install (settings, archive, shaders, grips, logs) moves to the game's root once
        Move-Item -LiteralPath $oldFolder -Destination $rootFolder
        $changed += 'red4ext\plugins\cyberpunkvrport -> cyberpunkvrport'
    }
}
$release = Join-Path $RepoRoot "$BuildDir\bin\red4ext\plugins\cyberpunkvrport\Release"
$source = Join-Path $release 'cyberpunkvrport.dll'
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
$log = Join-Path (Split-Path $target -Parent) 'log\cyberpunkvrport.log'
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
$changed += $relative
$loaderTarget = Join-Path $GameRoot 'bin\x64\powrprof.dll'
if ($Red4extLayout) {
    # the C++ build is RED4ext's plugin: the port's loader stays out of its way
    if (Test-Path -LiteralPath $loaderTarget) {
        Move-Item -LiteralPath $loaderTarget -Destination (Join-Path $backup 'powrprof.before.dll')
        $changed += 'bin\x64\powrprof.dll (removed)'
    }
} else {
    $loaderSource = Join-Path $release 'powrprof.dll'
    if (!(Test-Path -LiteralPath $loaderSource -PathType Leaf)) { throw "Loader not built: $loaderSource" }
    if (Test-Path -LiteralPath $loaderTarget) { Copy-Item -LiteralPath $loaderTarget -Destination (Join-Path $backup 'powrprof.before.dll') }
    Copy-Item -LiteralPath $loaderSource -Destination $loaderTarget -Force
    if ((Hash $loaderTarget) -ne (Hash $loaderSource)) { throw 'Installed loader mismatch' }
    $changed += 'bin\x64\powrprof.dll'
}
$iniAfter = if (Test-Path -LiteralPath $ini) { Hash $ini } else { $null }
if ($iniBefore -ne $iniAfter) { throw 'Runtime settings changed during deployment' }
$calibrationAfter = if (Test-Path -LiteralPath $calibration) { Hash $calibration } else { $null }
if ($calibrationBefore -ne $calibrationAfter) { throw 'Calibration changed during deployment' }
$report = [pscustomobject]@{
    GameRoot=$GameRoot; Source=$source; Target=$target; Backup=$backup
    PreviousSHA256=$previousHash; SHA256=$builtHash
    Bytes=(Get-Item -LiteralPath $target).Length; ChangedGameFiles=$changed
    SettingsUnchanged=$true; IniSHA256=$iniAfter; GameStarted=$false
    CalibrationSHA256=$calibrationAfter
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $backup 'deployment.json') -Encoding UTF8
$report | ConvertTo-Json -Depth 5
