#Requires -Version 5.1
<#
.SYNOPSIS
Install the CyberFinger-patched VR port, with backups and merged preferences.
.DESCRIPTION
Defaults to the GOG game folder and a left-handed weapon with eight-role SlimeVR.
Use -Package for an extracted, complete patched build_dist package. Otherwise
prepares dependencies, builds this checkout, and packages it with build_dist.ps1.
-Build remains accepted; source installs always build. -WhatIf writes nothing.
Missing HUD settings use the captured CyberFinger preset. -ResetHudDefaults
also reapplies that preset to existing HUD settings; other preferences survive.
SteamVR drivers and RED4ext are installed separately. Never launches the game.
.EXAMPLE
.\install_cyberfinger.cmd -Package .\dist\CyberpunkVRPort-0.2.3-cyberfinger -WhatIf
.EXAMPLE
.\install_cyberfinger.cmd -BuildDir build -Build
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$GameRoot = 'C:\Program Files (x86)\GOG Galaxy\Games\Cyberpunk 2077',
    [string]$BuildDir = 'build\cyberfinger-release',
    [string]$Package,
    [switch]$Build,
    [ValidateSet('Left', 'Right')][string]$WeaponHand = 'Left',
    [switch]$NoSlimeVR,
    [switch]$ResetHudDefaults,
    [Parameter(DontShow = $true)][switch]$ConciseErrors
)

try {
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\')
$Plugin = 'red4ext\plugins\cyberpunkvrport'
$Utf8 = New-Object Text.UTF8Encoding($false)

function Full-Path([string]$Path) {
    if (![IO.Path]::IsPathRooted($Path)) { $Path = Join-Path $RepoRoot $Path }
    return [IO.Path]::GetFullPath($Path).TrimEnd('\')
}
function Assert-NoLink([string]$Path) {
    # Check existing parents too, before any traversal or write through a junction.
    $p = $Path
    while ($p) {
        if (Test-Path -LiteralPath $p) {
            if ((Get-Item -LiteralPath $p -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Linked paths are not supported by this installer: $p"
            }
        }
        $p = Split-Path -Parent $p
    }
}
function Under-Root([string]$Root, [string]$Relative) {
    if ([IO.Path]::IsPathRooted($Relative)) { throw "Expected a relative path: $Relative" }
    $p = [IO.Path]::GetFullPath((Join-Path $Root $Relative))
    if (!$p.StartsWith($Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path escapes its root: $Relative"
    }
    Assert-NoLink $p
    return $p
}
function Files-In([string]$Path) {
    Assert-NoLink $Path
    foreach ($item in (Get-ChildItem -LiteralPath $Path -Force)) {
        Assert-NoLink $item.FullName
        if ($item.PSIsContainer) { Files-In $item.FullName } else { $item }
    }
}
function File-Hash([string]$Path) {
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        # Windows PowerShell 5.1 can autoload Get-FileHash under the caller's
        # WhatIf preference and suppress its own path resolution. Hash directly:
        # this is read-only in both preview and install, including a fresh shell.
        $stream = [IO.File]::OpenRead($Path)
        $sha = [Security.Cryptography.SHA256]::Create()
        try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') }
        finally { $sha.Dispose(); $stream.Dispose() }
    }
    if (Test-Path -LiteralPath $Path) { throw "Expected a file, found a directory: $Path" }
    return $null
}
function Bytes-Hash([byte[]]$Bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($Bytes)).Replace('-', '') }
    finally { $sha.Dispose() }
}
function Ensure-Parent([string]$Path) {
    Assert-NoLink $Path
    [void][IO.Directory]::CreateDirectory((Split-Path -Parent $Path))
}
function Merge-Ini([string]$Text, [System.Collections.IDictionary]$Values, [switch]$MissingOnly) {
    foreach ($key in $Values.Keys) {
        $pattern = '(?m)^[ \t]*' + [regex]::Escape($key) + '[ \t]*=[^\r\n]*'
        $line = $key + '=' + $Values[$key]
        if ([regex]::IsMatch($Text, $pattern)) {
            if ($MissingOnly) { continue }
            # Replace every duplicate, since some of the native readers take the last.
            $Text = [regex]::Replace($Text, $pattern, $line)
        } else {
            if ($Text.Length -and !$Text.EndsWith("`n")) { $Text += "`r`n" }
            $Text += $line + "`r`n"
        }
    }
    return $Text
}

$GameRoot = Full-Path $GameRoot
Assert-NoLink $GameRoot
$missing = New-Object 'System.Collections.Generic.List[string]'
$exe = Under-Root $GameRoot 'bin\x64\Cyberpunk2077.exe'
if (!(Test-Path -LiteralPath $exe -PathType Leaf)) { $missing.Add("Game executable: $exe") }
$red4ext = Under-Root $GameRoot 'red4ext\RED4ext.dll'
if (!(Test-Path -LiteralPath $red4ext -PathType Leaf)) { $missing.Add('RED4ext 1.31.0 or newer (install separately).') }
else {
    $redVersionText = (Get-Item -LiteralPath $red4ext).VersionInfo.ProductVersion
    if ($redVersionText -match '^(\d+\.\d+\.\d+)') {
        if ([version]$Matches[1] -lt [version]'1.31.0') { $missing.Add("RED4ext 1.31.0 or newer; found $redVersionText.") }
    }
}
# A preview only reads files and can prepare a review while the player is in-game.
if (!$WhatIfPreference -and (Get-Process -Name Cyberpunk2077 -ErrorAction SilentlyContinue)) { throw 'Close Cyberpunk 2077 before installing.' }
if (Test-Path -LiteralPath $exe -PathType Leaf) {
    $version = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
    if ($version -and $version -notmatch '^2\.5(?:\D|$)') { $missing.Add("Game 2.5 required by this checkout; found $version.") }
}
if ($Package -and $Build) { throw 'Use -Package for a prepared package, or -Build for the local build tree.' }
if (!$Package) {
    $BuildDir = Full-Path $BuildDir
    Assert-NoLink $BuildDir
    foreach ($rel in @('mods\archive\cyberpunkvrport.archive.xl', 'mods\config\openvr_api.dll')) {
        if (!(Test-Path -LiteralPath (Under-Root $RepoRoot $rel) -PathType Leaf)) { $missing.Add("Source asset: $rel") }
    }
    foreach ($tool in @('cmake', 'git')) {
        if (!(Get-Command $tool -ErrorAction SilentlyContinue)) { $missing.Add("$tool on PATH (needed to build this source checkout).") }
    }
}
if ($missing.Count) { throw ("Installation prerequisites are missing; no files changed:`n - " + ($missing -join "`n - ")) }

if (!$Package) {
    if (!$PSCmdlet.ShouldProcess($BuildDir, 'Prepare matching assets and dependencies, configure/build the patched DLL, and assemble its package')) { return }
    & (Join-Path $PSScriptRoot 'prepare_cyberfinger.ps1') -BuildDir $BuildDir
    $projectText = [IO.File]::ReadAllText((Join-Path $RepoRoot 'CMakeLists.txt'))
    if ($projectText -notmatch 'project\(CyberpunkVRPort VERSION ([0-9.]+)') { throw 'Cannot read project version.' }
    $packageVersion = $Matches[1] + '-cyberfinger.' + [guid]::NewGuid().ToString('N')
    # Reuse the release packager's asset list, not the obsolete deploy_stereo layout.
    & (Join-Path $PSScriptRoot 'build_dist.ps1') -BuildDir $BuildDir -Version $packageVersion
    $Package = Join-Path $RepoRoot "dist\CyberpunkVRPort-$packageVersion"
}
$Package = Full-Path $Package
Assert-NoLink $Package
if (!(Test-Path -LiteralPath $Package -PathType Container)) { throw "Extracted package not found: $Package" }
if ($Package -eq $GameRoot -or $Package.StartsWith($GameRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
    $GameRoot.StartsWith($Package + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Package and game folders must be separate.' }

$required = @('bin\x64\openvr_api.dll', "$Plugin\cyberpunkvrport.dll", "$Plugin\redscript\scc_lib.dll",
    "$Plugin\redscript\LICENSE.txt", "$Plugin\archive\cyberpunkvrport.archive", "$Plugin\config\archive.yaml",
    "$Plugin\config\tweakdb\tweakdbstr.kark", "$Plugin\config\tweakdb\usedhashes.kark", "$Plugin\config\tweakdb\LICENSE.txt",
    "$Plugin\lua\CyberpunkVRPort_Reach\init.lua", "$Plugin\lua\CyberpunkVRPort_Reach\modules\world.lua")
foreach ($rel in $required) {
    if (!(Test-Path -LiteralPath (Under-Root $Package $rel) -PathType Leaf)) { throw "Incomplete package; missing $rel. No game files changed." }
}
foreach ($subdir in @('shaders', 'scripts', 'grips', 'config\tweaks', 'config\input')) {
    $path = Under-Root $Package "$Plugin\$subdir"
    if (!(Test-Path -LiteralPath $path -PathType Container) -or !(@(Files-In $path).Count)) {
        throw "Incomplete package; empty or absent $Plugin\$subdir. No game files changed."
    }
}
$dllText = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Under-Root $Package "$Plugin\cyberpunkvrport.dll")))
foreach ($marker in @('cf_inventory', 'cf_weapon', 'cf_scanner', 'cf_heal', 'slime_joints')) {
    if (!$dllText.Contains($marker)) { throw "Package DLL lacks CyberFinger/SlimeVR action '$marker'. Rebuild this patched checkout." }
}
$dllText = $null
$world = [IO.File]::ReadAllText((Under-Root $Package "$Plugin\lua\CyberpunkVRPort_Reach\modules\world.lua"))
if (!$world.Contains('VRCyberFingerEnabled')) { throw 'Package contains the unpatched Reach script. Repackage this checkout.' }

$hudPreset = Under-Root $Package "$Plugin\config\cyberfinger-hud.ini.example"
if (!(Test-Path -LiteralPath $hudPreset -PathType Leaf)) {
    $hudPreset = Under-Root $RepoRoot 'mods\config\cyberfinger-hud.ini.example'
}
$hudDefaults = [ordered]@{}
foreach ($line in [IO.File]::ReadAllLines($hudPreset)) {
    if ($line -match '^\s*(?:;.*)?$') { continue }
    if ($line -notmatch '^((?:xr_hud_|xr_interaction_|hud_element_)[a-z_]+|xr_loot_follow_deg)=([-+0-9.eE, \t]+)$') {
        throw "Invalid HUD preset entry: $line"
    }
    if ($hudDefaults.Contains($Matches[1])) { throw "Duplicate HUD preset key: $($Matches[1])" }
    $hudDefaults[$Matches[1]] = $Matches[2]
}
if (!$hudDefaults.Count) { throw 'HUD preset is empty' }

$plan = [ordered]@{}
function Plan-File([string]$Relative, [string]$Source, [byte[]]$Bytes, [bool]$Remove = $false) {
    $target = Under-Root $GameRoot $Relative
    $before = File-Hash $target
    $after = if ($Remove) { $null } elseif ($Source) { File-Hash $Source } else { Bytes-Hash $Bytes }
    if ($before -eq $after) { return }
    $script:plan[$Relative] = [pscustomobject]@{
        Path = $Relative; Source = $Source; Bytes = $Bytes; Remove = $Remove; Before = $before; After = $after
    }
}
foreach ($file in @(Files-In (Under-Root $Package $Plugin)) + @(Get-Item -LiteralPath (Under-Root $Package 'bin\x64\openvr_api.dll'))) {
    $rel = $file.FullName.Substring($Package.Length + 1)
    if ($rel -like "$Plugin\log\*" -or $rel -like '*\bridge\*') { continue }
    if ($rel -in @("$Plugin\config\cyberfinger.ini", "$Plugin\config\vrport.ini", "$Plugin\config\fbt_settings.ini")) { continue }
    $target = Under-Root $GameRoot $rel
    # Personal calibration, grip captures, launcher settings and wrist placement survive updates.
    if ((Test-Path -LiteralPath $target -PathType Leaf) -and
        ($rel -like "$Plugin\config\*.ini" -or $rel -like "$Plugin\grips\*.ini" -or $rel -like '*\wrist.cfg')) { continue }
    if ($rel -eq "$Plugin\lua\CyberpunkVRPort_Stereo\vrcam.json" -and (Test-Path -LiteralPath $target -PathType Leaf)) {
        $old = [IO.File]::ReadAllText($target) | ConvertFrom-Json
        $fresh = [IO.File]::ReadAllText($file.FullName) | ConvertFrom-Json
        if ($old.PSObject.Properties['component'] -and $fresh.PSObject.Properties['components'] -and
            $fresh.components -contains $old.component) {
            $fresh.component = $old.component
            if ($fresh.PSObject.Properties['virtualCamera']) {
                $fresh.virtualCamera = $old.component -replace '^vrcam_', 'vrcam_feed_'
            }
            Plan-File $rel '' ($Utf8.GetBytes(($fresh | ConvertTo-Json -Depth 20)))
            continue
        }
    }
    Plan-File $rel $file.FullName $null
}

$slime = if ($NoSlimeVR) { '0' } else { '1' }
$left = if ($WeaponHand -eq 'Left') { '1' } else { '0' }
$settings = [ordered]@{
    'cyberfinger.ini' = [ordered]@{ enabled = '1' }
    'vrport.ini' = [ordered]@{
        xr_input_actions = '1'; xr_xinput_hook = '1'; xr_finger_bones = '1'; xr_grip_from_fingers = '1'
        xr_vive_trackers = $slime; xr_left_handed = $left; xr_left_handed_bindings = '0'
    }
    'fbt_settings.ini' = [ordered]@{ legs = $slime; spine = $slime; arms = $slime; slime_joints = $slime }
}
foreach ($name in $settings.Keys) {
    $rel = "$Plugin\config\$name"
    $existing = Under-Root $GameRoot $rel
    # Seed from the old location before creating the new vrport.ini, so the
    # plugin's later migration cannot lose existing preferences.
    if ($name -eq 'vrport.ini' -and !(Test-Path -LiteralPath $existing)) {
        $existing = Under-Root $GameRoot 'bin\x64\vrport.ini'
    }
    $text = if (Test-Path -LiteralPath $existing -PathType Leaf) { [IO.File]::ReadAllText($existing) } else { '' }
    if ($name -eq 'vrport.ini') {
        $text = Merge-Ini $text $hudDefaults -MissingOnly:(!$ResetHudDefaults)
    }
    $bytes = $Utf8.GetBytes((Merge-Ini $text $settings[$name]))
    Plan-File $rel '' $bytes
}

# Only conflicting loaders are disabled here. The plugin handles its own legacy
# Lua/redscript/archive migration on launch. Backups never live in a DLL scan path.
Plan-File 'bin\x64\dxgi.dll' '' $null $true
foreach ($name in @('CyberpunkVR_Hands', 'CyberpunkVR_Stereo')) {
    $dir = Under-Root $GameRoot "red4ext\plugins\$name"
    if (Test-Path -LiteralPath $dir -PathType Container) {
        foreach ($file in @(Files-In $dir | Where-Object Extension -eq '.dll')) {
            Plan-File $file.FullName.Substring($GameRoot.Length + 1) '' $null $true
        }
    }
}
Plan-File "$Plugin\CyberpunkVR_Stereo.dll" '' $null $true
Write-Host "Package: $Package"
Write-Host "Game: $GameRoot; weapon hand: $WeaponHand; SlimeVR: $(!$NoSlimeVR)"
foreach ($entry in $plan.Values | Where-Object Remove) { Write-Host "Back up and disable: $($entry.Path)" }
Write-Host "Planned file changes: $($plan.Count). Existing calibration and unrelated INI keys are preserved."
if (!$plan.Count) { Write-Host 'Already installed with these settings.'; return }
if (!$PSCmdlet.ShouldProcess($GameRoot, "Install CyberFinger mod and settings ($($plan.Count) file changes), with verified backups")) { return }

$backup = Under-Root $RepoRoot ('build\cyberfinger-install-backups\' + [guid]::NewGuid().ToString('N'))
if ($backup.StartsWith($GameRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Backups must be outside the game folder.' }
[void][IO.Directory]::CreateDirectory($backup)
$report = [ordered]@{ Status = 'Prepared'; GameRoot = $GameRoot; Package = $Package; WeaponHand = $WeaponHand
    SlimeVR = !$NoSlimeVR; Files = @($plan.Values | Select-Object Path, Remove, Before, After) }
function Save-Report {
    [IO.File]::WriteAllText((Under-Root $backup 'install.json'), ($report | ConvertTo-Json -Depth 5), $Utf8)
}
# Finish and verify ALL backups and staged replacements before changing the game.
foreach ($entry in $plan.Values) {
    $target = Under-Root $GameRoot $entry.Path
    if ($entry.Before) {
        $saved = Under-Root $backup ('before\' + $entry.Path)
        Ensure-Parent $saved
        Copy-Item -LiteralPath $target -Destination $saved
        if ((File-Hash $saved) -ne $entry.Before) { throw "Backup mismatch: $target" }
    }
    if (!$entry.Remove) {
        $staged = Under-Root $backup ('after\' + $entry.Path)
        Ensure-Parent $staged
        if ($entry.Source) { Copy-Item -LiteralPath $entry.Source -Destination $staged }
        else { [IO.File]::WriteAllBytes($staged, $entry.Bytes) }
        if ((File-Hash $staged) -ne $entry.After) { throw "Staging mismatch: $($entry.Path)" }
    }
}
foreach ($entry in $plan.Values) {
    if ((File-Hash (Under-Root $GameRoot $entry.Path)) -ne $entry.Before) { throw "File changed during preparation: $($entry.Path)" }
}
if (Get-Process -Name Cyberpunk2077 -ErrorAction SilentlyContinue) { throw 'Game started during preparation; installation stopped.' }
Save-Report
$written = New-Object 'System.Collections.Generic.List[object]'
try {
    foreach ($entry in $plan.Values) {
        $target = Under-Root $GameRoot $entry.Path
        $written.Add($entry)
        if ($entry.Remove) { Remove-Item -LiteralPath $target -Force }
        else {
            Ensure-Parent $target
            Copy-Item -LiteralPath (Under-Root $backup ('after\' + $entry.Path)) -Destination $target -Force
        }
        if ((File-Hash $target) -ne $entry.After) { throw "Installed hash mismatch: $($entry.Path)" }
    }
    $report.Status = 'Complete'
    Save-Report
} catch {
    $failure = $_
    $rollbackErrors = @()
    for ($i = $written.Count - 1; $i -ge 0; $i--) {
        $entry = $written[$i]
        try {
            $target = Under-Root $GameRoot $entry.Path
            if ($entry.Before) { Copy-Item -LiteralPath (Under-Root $backup ('before\' + $entry.Path)) -Destination $target -Force }
            elseif (Test-Path -LiteralPath $target -PathType Leaf) { Remove-Item -LiteralPath $target -Force }
            if ((File-Hash $target) -ne $entry.Before) { throw "Rollback hash mismatch: $($entry.Path)" }
        } catch { $rollbackErrors += $_.ToString() }
    }
    $report.Status = if ($rollbackErrors.Count) { 'RollbackFailed' } else { 'RolledBack' }
    Save-Report
    throw "Install failed: $failure. Status: $($report.Status). Backups: $backup. $($rollbackErrors -join '; ')"
}
Write-Host "Installed. Backup and file manifest: $backup"
Write-Host 'Select the Cyberpunk binding after installing your SteamVR driver update. Calibrate FBT in-game before using it.'
} catch {
    if ($ConciseErrors) {
        [Console]::Error.WriteLine("Installation stopped: " + $_.Exception.Message)
        exit 1
    }
    throw
}
