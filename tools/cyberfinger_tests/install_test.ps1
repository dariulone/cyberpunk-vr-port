#Requires -Version 5.1
# Filesystem integration tests, using synthetic packages and games under build/.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$installer = Join-Path $repo 'scripts\install_cyberfinger.ps1'
$fixture = Join-Path $repo ('build\cyberfinger-installer-tests\' + [guid]::NewGuid().ToString('N'))
$package = Join-Path $fixture 'package with spaces'
$plugin = 'red4ext\plugins\cyberpunkvrport'
$utf8 = New-Object Text.UTF8Encoding($false)
$passed = 0
# Only synthetic game roots are installed by this test; do not depend on whether
# the user's actual game is running. Exercise the process guard explicitly below.
$mockGameRunning = $false
function Get-Process {
    [CmdletBinding()] param([string]$Name)
    if ($mockGameRunning -and $Name -eq 'Cyberpunk2077') { [pscustomobject]@{ ProcessName = $Name } }
}
function Put([string]$Root, [string]$Relative, [string]$Text) {
    $path = Join-Path $Root $Relative
    [void][IO.Directory]::CreateDirectory((Split-Path -Parent $path))
    [IO.File]::WriteAllText($path, $Text, $utf8)
}
function Read([string]$Root, [string]$Relative) { [IO.File]::ReadAllText((Join-Path $Root $Relative)) }
function Assert([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Snapshot([string]$Root) {
    return (@(Get-ChildItem -LiteralPath $Root -Recurse -File | Sort-Object FullName | ForEach-Object {
        $_.FullName.Substring($Root.Length) + ':' + (Get-FileHash -LiteralPath $_.FullName).Hash
    }) -join "`n")
}
function Backup-Count {
    $dir = Join-Path $repo 'build\cyberfinger-install-backups'
    if (Test-Path -LiteralPath $dir) { return @(Get-ChildItem -LiteralPath $dir -Directory).Count }
    return 0
}
function Expect-Failure([scriptblock]$Action, [string]$Pattern) {
    $failed = $false
    try { & $Action } catch {
        $failed = $true
        Assert ($_.ToString() -like "*$Pattern*") "Unexpected failure: $_"
    }
    Assert $failed "Expected failure containing: $Pattern"
}
function New-Game([string]$Name) {
    $dir = Join-Path $fixture $Name
    Put $dir 'bin\x64\Cyberpunk2077.exe' 'synthetic exe, never launched'
    Put $dir 'red4ext\RED4ext.dll' 'synthetic loader'
    Put $dir 'bin\x64\dxgi.dll' 'previous proxy'
    Put $dir 'bin\x64\vrport.ini' "; personal preference`r`nxr_left_handed=0`r`n  xr_left_handed = 0`r`nresolution=custom`r`n"
    Put $dir "$plugin\cyberpunkvrport.dll" 'previous plugin'
    Put $dir "$plugin\config\fbt_calibration.ini" 'my fbt calibration'
    Put $dir "$plugin\config\vrik_calibration.ini" 'my vrik calibration'
    Put $dir "$plugin\grips\custom.ini" 'personal grip'
    Put $dir "$plugin\lua\CyberpunkVRPort_WristHud\wrist.cfg" 'personal wrist'
    Put $dir "$plugin\lua\CyberpunkVRPort_Stereo\vrcam.json" '{"component":"vrcam_1440x1600"}'
    Put $dir 'red4ext\plugins\unrelated\other.dll' 'keep unrelated mod'
    return $dir
}
foreach ($rel in @('bin\x64\openvr_api.dll', "$plugin\redscript\scc_lib.dll", "$plugin\redscript\LICENSE.txt",
    "$plugin\archive\cyberpunkvrport.archive", "$plugin\config\archive.yaml",
    "$plugin\config\tweakdb\tweakdbstr.kark", "$plugin\config\tweakdb\usedhashes.kark", "$plugin\config\tweakdb\LICENSE.txt",
    "$plugin\lua\CyberpunkVRPort_Reach\init.lua", "$plugin\shaders\test.dxil", "$plugin\scripts\test.reds",
    "$plugin\config\tweaks\test.yaml", "$plugin\config\input\test.xml")) { Put $package $rel 'fixture payload' }
Put $package "$plugin\cyberpunkvrport.dll" 'MZ fixture cf_inventory cf_weapon cf_scanner cf_heal slime_joints'
Put $package "$plugin\lua\CyberpunkVRPort_Reach\modules\world.lua" 'VRCyberFingerEnabled'
Put $package "$plugin\config\fbt_calibration.ini" 'packaged calibration must not replace personal calibration'
Put $package "$plugin\grips\custom.ini" 'packaged grip'
Put $package "$plugin\lua\CyberpunkVRPort_WristHud\wrist.cfg" 'packaged wrist'
Put $package "$plugin\lua\CyberpunkVRPort_Stereo\vrcam.json" '{"component":"vrcam_2560x2560","virtualCamera":"vrcam_feed_2560x2560","components":["vrcam_2560x2560","vrcam_1440x1600"]}'
Put $package 'bin\x64\winmm.dll' 'not owned by this installer'
Put $package 'INSTALL.txt' 'package documentation is not game payload'

$game = New-Game 'game [test] with spaces'
$before = Snapshot $game
$backupsBefore = Backup-Count
& $installer -GameRoot $game -Package $package -WhatIf
Assert ((Snapshot $game) -eq $before) 'WhatIf changed game files'
Assert ((Backup-Count) -eq $backupsBefore) 'WhatIf created backups'
$passed++

$mockGameRunning = $true
try {
    & $installer -GameRoot $game -Package $package -WhatIf
    Expect-Failure { & $installer -GameRoot $game -Package $package } 'Close Cyberpunk 2077'
    Assert ((Snapshot $game) -eq $before -and (Backup-Count) -eq $backupsBefore) 'Running-game guard or preview changed files'
} finally { $mockGameRunning = $false }
$passed++

# Fresh Windows PowerShell has not autoloaded Get-FileHash yet. Its module can
# inherit WhatIf and suppress path resolution; preview must work in this case.
$savedPreference = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Continue'
    $freshOutput = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installer -ConciseErrors -GameRoot $game -Package $package -WhatIf 2>&1
    $freshExit = $LASTEXITCODE
} finally { $ErrorActionPreference = $savedPreference }
Assert ($freshExit -eq 0) "Fresh-process preview failed: $freshOutput"
Assert ((Snapshot $game) -eq $before -and (Backup-Count) -eq $backupsBefore) 'Fresh-process preview changed files'
$passed++

# A source checkout need not already contain built artifacts. Preview must plan
# preparation, without requiring a CMakeCache or creating the selected build tree.
$freshBuild = Join-Path $fixture 'unconfigured source build'
$backupsBefore = Backup-Count
& $installer -GameRoot $game -BuildDir $freshBuild -WhatIf
Assert (!(Test-Path -LiteralPath $freshBuild)) 'Source preview created a build tree'
Assert ((Snapshot $game) -eq $before -and (Backup-Count) -eq $backupsBefore) 'Source preview wrote files'
$passed++

Put $package "$plugin\cyberpunkvrport.dll" 'stock unpatched DLL'
Expect-Failure { & $installer -GameRoot $game -Package $package } 'lacks CyberFinger'
Assert ((Snapshot $game) -eq $before) 'Rejected package changed files'
Put $package "$plugin\cyberpunkvrport.dll" 'MZ fixture cf_inventory cf_weapon cf_scanner cf_heal slime_joints'
$passed++

$noLoader = Join-Path $fixture 'missing loader'
Put $noLoader 'bin\x64\Cyberpunk2077.exe' 'synthetic exe'
Expect-Failure { & $installer -GameRoot $noLoader -Package $package } 'RED4ext'
Assert (@(Get-ChildItem -LiteralPath $noLoader -Recurse -File).Count -eq 1) 'Missing dependency partially installed files'
$passed++

# The command launcher uses this switch to emit one useful error instead of a
# PowerShell source excerpt/stack. Use a separate process since it exits with 1.
$savedPreference = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Continue'
    $cliOutput = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installer -ConciseErrors -GameRoot $noLoader -Package $package -WhatIf 2>&1
    $cliExit = $LASTEXITCODE
} finally { $ErrorActionPreference = $savedPreference }
$cliText = ($cliOutput | ForEach-Object { $_.ToString() }) -join "`n"
Assert ($cliExit -eq 1 -and $cliText.Contains('RED4ext') -and !$cliText.Contains('CategoryInfo') -and !$cliText.Contains('char:')) 'Concise CLI failure output regressed'
$passed++

& $installer -GameRoot $game -Package $package
$ini = Read $game "$plugin\config\vrport.ini"
Assert ($ini.Contains('resolution=custom') -and $ini.Contains('; personal preference')) 'INI merge lost unrelated settings'
Assert ($ini.Contains('xr_left_handed=1') -and !$ini.Contains('xr_left_handed=0')) 'Left hand or duplicate-key merge failed'
Assert ((Read $game "$plugin\config\cyberfinger.ini").Contains('enabled=1')) 'CyberFinger disabled'
Assert ((Read $game "$plugin\config\fbt_settings.ini").Contains('slime_joints=1')) 'SlimeVR disabled'
Assert ((Read $game "$plugin\config\fbt_calibration.ini") -eq 'my fbt calibration') 'FBT calibration overwritten'
Assert ((Read $game "$plugin\config\vrik_calibration.ini") -eq 'my vrik calibration') 'VRIK calibration overwritten'
Assert ((Read $game "$plugin\grips\custom.ini") -eq 'personal grip') 'Grip overwritten'
Assert ((Read $game "$plugin\lua\CyberpunkVRPort_WristHud\wrist.cfg") -eq 'personal wrist') 'Wrist placement overwritten'
$camera = Read $game "$plugin\lua\CyberpunkVRPort_Stereo\vrcam.json" | ConvertFrom-Json
Assert ($camera.component -eq 'vrcam_1440x1600' -and $camera.components.Count -eq 2) 'Resolution selection or updated catalogue lost'
Assert ((Read $game 'red4ext\plugins\unrelated\other.dll') -eq 'keep unrelated mod') 'Other mod changed'
Assert (!(Test-Path -LiteralPath (Join-Path $game 'bin\x64\dxgi.dll'))) 'Conflicting proxy still active'
Assert (!(Test-Path -LiteralPath (Join-Path $game 'bin\x64\winmm.dll'))) 'Installed a file outside the allowed payload'
$reports = @(Get-ChildItem -LiteralPath (Join-Path $repo 'build\cyberfinger-install-backups') -Filter install.json -Recurse |
    Where-Object { $report = Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
        $report.PSObject.Properties['GameRoot'] -and $report.GameRoot -eq $game })
Assert ($reports.Count -eq 1) 'Expected one backup manifest'
$saved = Split-Path -Parent $reports[0].FullName
Assert ((Read $saved 'before\bin\x64\dxgi.dll') -eq 'previous proxy') 'Proxy backup missing'
Assert ((Read $saved "before\$plugin\cyberpunkvrport.dll") -eq 'previous plugin') 'Plugin backup missing'
$passed++

$installed = Snapshot $game
$backupsBefore = Backup-Count
& $installer -GameRoot $game -Package $package
Assert ((Snapshot $game) -eq $installed -and (Backup-Count) -eq $backupsBefore) 'Identical reinstall should do nothing'
$passed++

& $installer -GameRoot $game -Package $package -WeaponHand Right -NoSlimeVR
Assert ((Read $game "$plugin\config\vrport.ini").Contains('xr_left_handed=0')) 'Right-handed preset failed'
Assert ((Read $game "$plugin\config\fbt_settings.ini").Contains('slime_joints=0')) 'NoSlimeVR preset failed'
$passed++

# Inject one copy failure after writes begin; restore must include existing files
# and remove newly introduced files. This runs only in the test PowerShell process.
$rollbackGame = New-Game 'rollback game'
$rollbackBefore = Snapshot $rollbackGame
$global:CfInstallerFailTarget = Join-Path $rollbackGame "$plugin\cyberpunkvrport.dll"
$global:CfInstallerFailOnce = $true
function global:Copy-Item {
    [CmdletBinding()]param([string]$LiteralPath, [string]$Destination, [switch]$Force)
    if ($global:CfInstallerFailOnce -and $Destination -eq $global:CfInstallerFailTarget -and $LiteralPath -like '*\after\*') {
        $global:CfInstallerFailOnce = $false
        throw 'injected copy failure'
    }
    Microsoft.PowerShell.Management\Copy-Item @PSBoundParameters
}
try { Expect-Failure { & $installer -GameRoot $rollbackGame -Package $package } 'RolledBack' }
finally {
    Remove-Item Function:\Copy-Item
    Remove-Variable CfInstallerFailTarget, CfInstallerFailOnce -Scope Global
}
Assert ((Snapshot $rollbackGame) -eq $rollbackBefore) 'Rollback did not restore original file contents'
$passed++

Expect-Failure { & $installer -GameRoot $game -Package $game } 'must be separate'
Assert ((Read $game 'red4ext\plugins\unrelated\other.dll') -eq 'keep unrelated mod') 'Rejected overlap changed unrelated files'
$passed++

# Exercise the actual release packager's absolute path resolution. Its first
# required file is absent, so it must report that exact path before copying.
$missingBuild = Join-Path $fixture 'absolute build with spaces'
$expectedDll = Join-Path $missingBuild 'bin\red4ext\plugins\cyberpunkvrport\Release\cyberpunkvrport.dll'
$version = '0.0.0-installer-test.' + [guid]::NewGuid().ToString('N')
Expect-Failure { & (Join-Path $repo 'scripts\build_dist.ps1') -BuildDir $missingBuild -Version $version } "not found: $expectedDll"
$passed++
Write-Host "PASS: $passed installer integration scenarios. Fixtures retained under $fixture"
