# Assemble a tester package under dist\, laid out exactly as it must land in the game root.
#
# Everything comes from the repo or from a build output -- nothing is read out of the installed
# game -- so what a tester gets is what is committed. Run scripts\sync_assets.ps1 first if the
# yamls, archives or grip poses have been touched game-side since the last pull.
#
# Usage:
#   pwsh scripts\build_dist.ps1
#   pwsh scripts\build_dist.ps1 -Version 0.1.1 -Zip

param(
    [string]$Version = "0.1.0",
    [string]$BuildDir = "build",
    [switch]$Zip,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$Out      = Join-Path $RepoRoot "dist\CyberpunkVRPort-$Version"

# Folders that exist for development and have no business in a tester's game.
#
# ReloadRecorder is an AUTHORING tool, not a feature: it records the game's own reload
# animation per frame with VRIK off, and every grip pose and wrist placement the reload
# ships was lifted from those takes. A player has nothing to record and nothing to do with
# the takes, and it puts a third onUpdate on a pair of hands that already carry two solvers.
$SkipMods  = @("CyberpunkVRPort_WorldMapDiag", "CyberpunkVRPort_ReloadRecorder")
# *.md too: the notes beside a script are for whoever maintains it, not for a tester's game folder.
$SkipFiles = @("db.sqlite3", "*.log", "*.bak", "*.orig", "*.rar", "*.zip", "*.md")

function Copy-Tree($src, $dst) {
    New-Item -ItemType Directory -Path $dst -Force | Out-Null
    $n = 0
    foreach ($f in (Get-ChildItem -LiteralPath $src -File)) {
        $skip = $false
        foreach ($p in $SkipFiles) { if ($f.Name -like $p) { $skip = $true; break } }
        if ($skip) { continue }
        Copy-Item $f.FullName (Join-Path $dst $f.Name) -Force
        $n++
    }
    foreach ($d in (Get-ChildItem -LiteralPath $src -Directory)) {
        $n += Copy-Tree $d.FullName (Join-Path $dst $d.Name)
    }
    return $n
}

function Need($p, $what) {
    if (-not (Test-Path -LiteralPath $p)) { throw "$what not found: $p" }
    return $p
}

if (Test-Path $Out) {
    if (-not $Force) { Remove-Item $Out -Recurse -Force } else { Remove-Item $Out -Recurse -Force }
}
New-Item -ItemType Directory -Path $Out -Force | Out-Null

$manifest = @()
function Add-File($src, $rel) {
    $dst = Join-Path $Out $rel
    New-Item -ItemType Directory -Path (Split-Path $dst -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $src -Destination $dst -Force
    $script:manifest += [pscustomobject]@{ Path = $rel; Bytes = (Get-Item -LiteralPath $dst).Length }
}

# ---- the plugin -----------------------------------------------------------------------------
# ONE plugin. This used to package CyberpunkVR_Hands from src\red4ext_plugin\ as well, and
# that directory no longer exists: every script-callable native (VRBodyBonePos, VRPalmModelPos,
# SetVRSprintActive, all of them) is compiled into CyberpunkVR_Stereo now, out of src\Natives\.
# Shipping both was also the recipe for the read at 0xFFFFFFFFFFFFFFFF -- two RED4ext plugins
# detouring one address -- so a stale CyberpunkVR_Hands.dll left in red4ext\plugins is worth deleting.
$stereoDll = Need (Join-Path $RepoRoot "$BuildDir\bin\red4ext\plugins\CyberpunkVR_Stereo\Release\CyberpunkVR_Stereo.dll") "CyberpunkVR_Stereo.dll"
Add-File $stereoDll "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_Stereo.dll"

# The sight shaders are loaded by name at PSO-replacement time; without both, the replacement is
# skipped and the only symptom is one line in the log.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\sight_reflex_ps.dxil") "sight PS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_SightPs.dxil"
Add-File (Need (Join-Path $RepoRoot "src\Shaders\sight_reflex_vs.dxil") "sight VS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_SightVs.dxil"

# Installed over the player's own settings ONCE, on a launch that reads first_launch=1 from
# vrport.ini -- 1 means "not installed yet", and FirstLaunch clears it to 0 only after the copy
# succeeds. A timestamped copy of what was there is kept. See INSTALL.txt.
Add-File (Need (Join-Path $RepoRoot "mods\config\UserSettings.json") "UserSettings.json") "red4ext\plugins\CyberpunkVR_Stereo\UserSettings.json"

# ---- captured grip poses, read from beside the exe -------------------------------------------
# ENUMERATED, never listed. This was a hardcoded three (both smoke grips and the lighter) while
# mods\config\ holds fourteen: it also has RestGrip_Left -- the relaxed left hand worn
# while a weapon is out -- and one TwoHandGrip_<weapon> per weapon anyone has recorded a hold for.
# Both absences are silent by design (an uncaptured weapon just has no two-hand hold), so the
# package quietly shipped no two-handed grip at all. Enumerating means a newly captured pose ships
# by existing, without anyone remembering to add it here.
foreach ($g in (Get-ChildItem (Join-Path $RepoRoot "mods\config") -Filter "CyberpunkVR_*.ini" -File | Sort-Object Name)) {
    Add-File $g.FullName "bin\x64\$($g.Name)"
}

# ---- engine-side tuning + the OpenVR shim ------------------------------------------------------
Add-File (Need (Join-Path $RepoRoot "mods\config\vrcam_cpu_tweaks.ini") "vrcam_cpu_tweaks.ini") "engine\config\platform\pc\vrcam_cpu_tweaks.ini"
Add-File (Need (Join-Path $RepoRoot "mods\config\openvr_api.dll") "openvr_api.dll") "bin\x64\openvr_api.dll"

# ---- CET mods, redscript, tweaks --------------------------------------------------------------
foreach ($d in (Get-ChildItem (Join-Path $RepoRoot "mods\cet") -Directory)) {
    if ($SkipMods -contains $d.Name) { continue }
    $n = Copy-Tree $d.FullName (Join-Path $Out "bin\x64\plugins\cyber_engine_tweaks\mods\$($d.Name)")
    $manifest += [pscustomobject]@{ Path = "bin\x64\plugins\cyber_engine_tweaks\mods\$($d.Name)\  ($n files)"; Bytes = 0 }
}
foreach ($d in (Get-ChildItem (Join-Path $RepoRoot "mods\redscript") -Directory)) {
    if ($d.Name -eq "logs" -or $SkipMods -contains $d.Name) { continue }
    $n = Copy-Tree $d.FullName (Join-Path $Out "r6\scripts\$($d.Name)")
    $manifest += [pscustomobject]@{ Path = "r6\scripts\$($d.Name)\  ($n files)"; Bytes = 0 }
}
$tw = Join-Path $RepoRoot "mods\tweaks\vrcigarette"
if (Test-Path $tw) {
    $n = Copy-Tree $tw (Join-Path $Out "r6\tweaks\vrcigarette")
    $manifest += [pscustomobject]@{ Path = "r6\tweaks\vrcigarette\  ($n files)"; Bytes = 0 }
}

# ---- packed archives ---------------------------------------------------------------------------
# ENUMERATED, never listed -- the same rule the grip poses above already follow, and for the same
# reason. The hardcoded three shipped a package with no vrport_mag.archive in it, while that archive
# sat both in mods\archive\ and in the installed game: a file present everywhere except in the
# tester's copy, and nothing said so. Only exact *.archive and *.archive.xl names match, so the
# .bak_*, .holdcomp, .nocollision and .nophys variants beside them stay out by shape rather than by
# anyone remembering to exclude them.
$archiveDir = Join-Path $RepoRoot "mods\archive"
$packed = @(Get-ChildItem -LiteralPath $archiveDir -File |
           Where-Object { $_.Name -like "*.archive" -or $_.Name -like "*.archive.xl" })
if ($packed.Count -eq 0) { Write-Host "[!] no archives in mods\archive -- run sync_assets.ps1 first" }
foreach ($f in $packed) { Add-File $f.FullName "archive\pc\mod\$($f.Name)" }

# ---- HUDitor: the port's setup, on the paths the mod actually uses ----------------------------
# HUD placement is not the port's job -- its own HUD mod was removed on 2026-08-20 because it
# scaled the shared HUD root around screen centre and fought a real editor. What ships instead is
# the port's HUDitor setup: the editor moved off F7 to F11, and a layout tuned in VR.
#
# The binding lives in r6\input\ because that is the only place it works: the game merges
# r6\input\*.xml into r6\cache\inputUserMappings.xml every launch and reads the merged
# result. That merge is RED4ext's input_loader, so without it this file is inert.
#
# persistency.json REPLACES whatever HUDitor layout is installed -- said out loud in INSTALL.txt
# rather than hidden, because for a tester getting the tuned layout is the whole point.
$hud = Join-Path $RepoRoot "mods\config\huditor"
if (Test-Path $hud) {
    Add-File (Need (Join-Path $hud "HUDitor.xml") "HUDitor.xml") "r6\input\HUDitor.xml"
    Add-File (Need (Join-Path $hud "persistency.json") "persistency.json") "bin\x64\plugins\cyber_engine_tweaks\mods\HUDitor\persistency.json"
}

# ---- the scanner's own HUD editor: RIGHT SHIFT ------------------------------------------------
# HUDitor cannot move the scanner. Its movable set is a fixed list of HUD controllers -- minimap,
# tracker, health, hotkeys, the car HUD family -- and the scanner is in none of them; all it does with
# the scanner is slide the minimap out of its way. So CyberpunkVRPort_ScannerHud brings its own
# editor, and this file is the key that opens it.
#
# Same folder and the same input_loader dependency as HUDitor.xml above. A SEPARATE key rather than
# F11, because sharing it would open HUDitor's editor on the same press -- and HUDitor's editor has no
# idea the scanner exists, so the two would fight over one key for different widgets.
$inputDir = Join-Path $RepoRoot "mods\config\input"
if (Test-Path $inputDir) {
    Add-File (Need (Join-Path $inputDir "CyberpunkVRPort_ScannerHud.xml") "CyberpunkVRPort_ScannerHud.xml") "r6\input\CyberpunkVRPort_ScannerHud.xml"
}

# ---- the OpenXR probe is NOT packaged ---------------------------------------------------------
# It stays in tools\xr_probe\ and goes to a tester by hand, when there is something to measure.
# Registering a MACHINE-WIDE OpenXR API layer is not a thing to ship to everyone who installs a
# mod: it is not dropped in a folder, it is written into a registry key, and one left unregistered
# records every VR application on the box. Build it with the xr_probe_layer target and hand over
# that folder when it is actually needed.

# ---- the note a tester actually reads ----------------------------------------------------------
$readme = @"
CyberpunkVRPort $Version
========================

WHAT THIS IS
    A VR mod for Cyberpunk 2077: stereo rendering through OpenXR, 6DoF head tracking, motion
    controllers merged into the game's own gamepad input, VRIK arms, and a set of gameplay mods
    (holsters, physical reload, melee, weapon handling, smoking). HUD placement is HUDitor's job
    now, and this package carries the port's HUDitor setup -- see WHAT LANDS WHERE.

BEFORE YOU INSTALL -- READ THIS ONE
    The first time the plugin starts it REPLACES your Cyberpunk settings with the ones this mod
    was tuned against:

        %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json

    Your own file is copied to UserSettings.pre-vr-<date>-<time>.json in the same folder first,
    and if that copy fails the install is abandoned rather than forced. It happens exactly once:
    afterwards the file is yours and nothing here looks at it again. Everything you change in the
    game's own menus sticks.

    first_launch=1 in bin\x64\vrport.ini means "not installed yet"; the plugin
    clears it to 0 once the copy has succeeded, and never looks again.

    To SKIP it entirely, including on the very first launch: create bin\x64\vrport.ini yourself,
    containing the single line first_launch=0, before you start the game. Creating it afterwards
    is too late -- the plugin writes that file and reads it in the same breath, so a fresh
    install has already been offered the settings by the time you can edit it.

    To ask for the settings later, set first_launch=1 and start the game once.

REQUIREMENTS
    Cyberpunk 2077 2.31 (this build's engine offsets are matched to it)
    RED4ext, Cyber Engine Tweaks, redscript, TweakXL, ArchiveXL, Codeware
    An OpenXR runtime, started BEFORE the game

    HUDitor, plus RED4ext's input_loader -- ONLY if you want HUD placement. This package carries
    the port's HUDitor setup (the editor on F11, and a VR layout), and input_loader is the plugin
    that merges r6\input\*.xml, so without it the F11 binding is inert. Note that
    persistency.json REPLACES any HUDitor layout you already have -- back yours up first if you
    care about it. The port needs neither: with no HUD editor the flat-screen HUD is used
    unchanged, and the port still composites it into the second eye either way.

    Nothing else may proxy dxgi. If bin\x64\dxgi.dll exists (R.E.A.L. VR installs one), move it
    out of the folder -- two VR paths in one process fight over the same engine hooks.

INSTALL
    Extract the contents of this folder into your Cyberpunk 2077 game root -- the folder that
    contains bin\, r6\, red4ext\ and archive\. The paths inside already match.

    Then start your OpenXR runtime, then the game. A small launcher window appears first: pick
    your headset and per-eye render resolution there.

WHAT LANDS WHERE
    red4ext\plugins\CyberpunkVR_Stereo\   the VR plugin, its shaders, the settings template
    bin\x64\CyberpunkVR_*Grip*.ini        captured hand poses for holding a cigarette and lighter
    bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_*\
    r6\scripts\CyberpunkVRPort_*\
    r6\tweaks\vrcigarette\
    archive\pc\mod\                       packed assets + the ArchiveXL manifest
    r6\input\HUDitor.xml               HUDitor's editor moved to F11 (needs input_loader)
    bin\x64\plugins\...\mods\HUDitor\persistency.json   the VR HUD layout -- REPLACES yours

    The player entity assets in cyberpunkvrport.archive carry one render-to-texture camera per
    supported resolution. The launcher offers exactly the ones that exist.

IF SOMETHING IS WRONG
    bin\x64\cyberpunkvrport.log            the plugin's own log, start here
    red4ext\logs\                          script validation errors land here
    bin\x64\plugins\cyber_engine_tweaks\   per-mod CET logs

    Uninstall: run UNINSTALL.bat, in the same folder as this file. It removes every path this
    package added and then offers your own settings back. UNINSTALL.txt is the same thing
    written out by hand, including the two things neither of them can put back for you.

Built from commit $(git -C $RepoRoot rev-parse --short HEAD 2>$null) on $(Get-Date -Format "yyyy-MM-dd").
"@
Set-Content (Join-Path $Out "INSTALL.txt") $readme -Encoding utf8

# ---- the uninstaller, GENERATED from the manifest ----------------------------------------------
# Both the runnable UNINSTALL.bat and the UNINSTALL.txt beside it are derived from what this run
# actually copied. Typed by hand they go stale on the first mod added -- exactly the way the grip
# list and the CET folder list in INSTALL.txt both did, each of which shipped wrong for a while.
#
# The folders are the ones this run copied whole, the loose files are the ones it added into
# folders that are NOT ours, and a file inside a listed folder is left out rather than named twice.
# That last part is why the .bat can use rmdir /s on the folder list without ever aiming it at a
# directory the game or another mod owns.
#
# What neither file can DERIVE is the three things deleting files does not undo -- the replaced
# UserSettings.json, HUDitor's own binding file, and the HUDitor layout. Those are written out by
# hand, and the .bat offers to put the settings back because it is the only one of the three it
# can do safely: the backup is the player's own file and restoring it is a copy, not a guess.
$ownDirs = @("red4ext\plugins\CyberpunkVR_Stereo")
foreach ($e in $manifest) {
    if ($e.Bytes -eq 0 -and $e.Path -match '^(.*?)\\\s+\(\d+ files\)$') { $ownDirs += $Matches[1] }
}
$ownDirs = $ownDirs | Sort-Object -Unique

$loose = @()
foreach ($e in $manifest) {
    if ($e.Bytes -le 0) { continue }
    $inDir = $false
    foreach ($d in $ownDirs) { if ($e.Path.StartsWith($d + "\")) { $inDir = $true; break } }
    if (-not $inDir) { $loose += $e.Path }
}
$loose = $loose | Sort-Object -Unique

$dirLines   = ($ownDirs | ForEach-Object { "    " + $_ + "\" }) -join "`r`n"
$looseLines = ($loose   | ForEach-Object { "    " + $_        }) -join "`r`n"

$uninstall = @"
CyberpunkVRPort $Version -- UNINSTALL
=====================================

Run UNINSTALL.bat in this folder and it does all of part 1, 2 and 3 below for you, then offers to
put your own Cyberpunk settings back. This file is the same thing written out, for anyone who
would rather delete by hand or wants to know what the .bat touches before running it.

Nothing here installs to Windows. There is no installer, no registry key and no service: every
file this mod adds is inside the Cyberpunk 2077 game folder, and deleting them IS the uninstall.
The two lists below are generated from the package itself, so they name exactly what it added.

If you only want to TURN IT OFF for a session you need none of this. Delete
red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_Stereo.dll and the game starts flat, with the CET
and redscript mods idle because the natives they call are gone with it.

1. DELETE THESE FOLDERS -- every one of them is ours, nothing else lives in them

$dirLines

2. DELETE THESE FILES -- these sit in folders that are NOT ours, so delete the file, not the folder

$looseLines

3. AND THESE -- the plugin writes them at runtime, so they are not in the package

    bin\x64\vrport.ini                     every setting you changed in the F10 overlay
    bin\x64\vrport-launcher.ini            the launcher's headset, resolution and DEBUG choice
    bin\x64\cyberpunkvrport.log            the plugin's log

4. THREE THINGS DELETING FILES DOES NOT UNDO -- read this part

    YOUR GAME SETTINGS. On its first launch the plugin replaced

        %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json

    with the one this mod was tuned against, and copied yours to
    UserSettings.pre-vr-<date>-<time>.json in the same folder. Removing the mod does not put it
    back. UNINSTALL.bat offers to; by hand, rename that backup over UserSettings.json yourself
    with the game closed. Skip it and you keep the mod's graphics, comfort and language settings
    for good.

    HUDITOR'S OWN BINDING FILE. r6\input\HUDitor.xml in the list above REPLACED the one HUDitor
    ships, to move its editor off F7 onto F11. Deleting ours leaves HUDitor with no binding file at
    all, so reinstall HUDitor -- or restore its own HUDitor.xml -- if you want the editor back. The
    game rebuilds r6\cache\inputUserMappings.xml from r6\input\*.xml on the next launch, so there
    is nothing to clean up there.

    YOUR HUD LAYOUT. persistency.json in the list above OVERWROTE whatever HUDitor layout you had.
    Deleting it leaves HUDitor to generate a default, which is not the layout you had before
    installing this unless you kept a copy.

5. IF YOU MOVED A dxgi.dll ASIDE FOR THIS MOD

    R.E.A.L. VR and some other VR mods install bin\x64\dxgi.dll, and this port cannot share a
    process with one, so you may have renamed it -- deploy_stereo.ps1 renames it to
    dxgi.dll.disabled-red4ext. Rename it back if you want that mod again.

6. WHAT IS NOT TOUCHED, so you do not go looking for it

    Your saves. Nothing here reads or writes them, and nothing this mod does is persisted into a
    save: no custom quest facts, no items that stop existing, no scripted entities parked in the
    world. A save made with the mod loads without it.

    The other mods this one requires -- RED4ext, Cyber Engine Tweaks, redscript, ArchiveXL,
    TweakXL, Codeware, Visual Holsters, Visible Bullets, Equipment-EX, Nova Optics, Input Loader,
    HUDitor. Each has its own uninstall; the lists above remove only what carries the
    CyberpunkVRPort name.

Generated from commit $(git -C $RepoRoot rev-parse --short HEAD 2>$null) on $(Get-Date -Format "yyyy-MM-dd").
"@
Set-Content (Join-Path $Out "UNINSTALL.txt") $uninstall -Encoding utf8

# ---- and the same lists as a batch file anyone can double-click --------------------------------
# A .bat rather than a .ps1 on purpose: no ExecutionPolicy to explain, no "right-click, Run with
# PowerShell", and the player can read every line of it in Notepad before running it. Written
# ASCII, NOT utf8: Set-Content -Encoding utf8 puts a BOM on it, and cmd.exe reads that BOM as part
# of the first command, so `@echo off` becomes an unknown command and the whole window fills up.
$batDirs  = ($ownDirs | ForEach-Object { 'call :killdir "' + $_ + '"' }) -join "`r`n"
$batFiles = ($loose   | ForEach-Object { 'call :killfile "' + $_ + '"' }) -join "`r`n"

$bat = @"
@echo off
setlocal EnableDelayedExpansion
cd /d "%~dp0"

echo.
echo   CyberpunkVRPort $Version  --  uninstall
echo   =======================================
echo.

rem ---- refuse to run anywhere but the game root ----------------------------------------------
rem Every path below is relative, so in the wrong folder this would delete nothing rather than
rem something -- but saying so beats a run that reports "already gone" 45 times and looks done.
if not exist "bin\x64\Cyberpunk2077.exe" goto :notgameroot

rem ---- and not while the game is running ------------------------------------------------------
rem A locked DLL fails silently enough to leave half a mod behind, and the settings restore below
rem would be overwritten by the running game on exit.
tasklist /fi "imagename eq Cyberpunk2077.exe" 2>nul | find /i "Cyberpunk2077.exe" >nul
if not errorlevel 1 goto :stillrunning

echo   Game folder:  %CD%
echo.
echo   This removes every file CyberpunkVRPort installed -- $($ownDirs.Count) folders, $($loose.Count) loose files,
echo   plus the three the plugin writes at runtime.
echo.
echo   It does NOT restore HUDitor's own binding file or your HUD layout. UNINSTALL.txt,
echo   section 4, says what that means. Your Cyberpunk settings are offered back at the end.
echo.
set "GO="
set /p "GO=  Remove CyberpunkVRPort now? [y/N] "
rem First character only, so y / Y / yes / a stray trailing space all mean yes, and an empty
rem answer (just Enter) does not.
if /i not "!GO:~0,1!"=="y" goto :nothingdone
echo.

set /a GONE=0
set /a ABSENT=0
set /a STUCK=0

$batDirs

$batFiles

call :killfile "bin\x64\vrport.ini"
call :killfile "bin\x64\vrport-launcher.ini"
call :killfile "bin\x64\cyberpunkvrport.log"

echo.
echo   Removed !GONE!, already gone !ABSENT!, could not remove !STUCK!.
if !STUCK! GTR 0 (
  echo.
  echo   Something is holding those files. Close the game and Steam and run this again.
)

rem ---- offer the player's own settings back ---------------------------------------------------
rem The newest backup, because the plugin only ever writes one and only on a first launch, but a
rem reinstall makes a second and the newest is the one from the install being removed.
set "CFG=%LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077"
set "BAK="
for /f "delims=" %%F in ('dir /b /o-d "%CFG%\UserSettings.pre-vr-*.json" 2^>nul') do (
  if not defined BAK set "BAK=%%F"
)
if not defined BAK goto :nobackup

echo.
echo   Your own Cyberpunk settings were saved before this mod replaced them:
echo     !BAK!
set "GO2="
set /p "GO2=  Put them back now? [y/N] "
if /i not "!GO2:~0,1!"=="y" (
  echo   Left alone. It is still there whenever you want it.
  goto :done
)
copy /y "%CFG%\!BAK!" "%CFG%\UserSettings.json" >nul
if errorlevel 1 (
  echo   FAILED. Do it by hand: rename
  echo       %CFG%\!BAK!
  echo       over UserSettings.json
) else (
  echo   Restored. Your graphics, comfort and language settings are yours again.
)
goto :done

:nobackup
echo.
echo   No UserSettings.pre-vr-*.json found in
echo     %CFG%
echo   So either the settings were never replaced, or the backup has been moved.
goto :done

:done
echo.
echo   Done. UNINSTALL.txt is still here if you want to check anything by hand --
echo   delete it, INSTALL.txt and this file whenever you like.
echo.
pause
exit /b 0

:notgameroot
echo   This is not the Cyberpunk 2077 folder:
echo     %CD%
echo.
echo   bin\x64\Cyberpunk2077.exe is not here. UNINSTALL.bat belongs in the game root --
echo   the folder that holds bin\, r6\, red4ext\ and archive\ -- and has to run there.
echo.
pause
exit /b 1

:stillrunning
echo   Cyberpunk 2077 is running.
echo.
echo   Close it first: the plugin DLL is locked while the game is up, and the game
echo   rewrites its settings file on exit, which would undo the restore below.
echo.
pause
exit /b 1

:nothingdone
echo.
echo   Nothing was deleted.
echo.
pause
exit /b 0

rem ---- helpers, past every exit so nothing falls into them ------------------------------------
:killdir
if not exist "%~1\" (
  set /a ABSENT+=1
  exit /b 0
)
rmdir /s /q "%~1" 2>nul
if exist "%~1\" (
  echo   LOCKED      %~1\
  set /a STUCK+=1
) else (
  echo   removed     %~1\
  set /a GONE+=1
)
exit /b 0

:killfile
if not exist "%~1" (
  set /a ABSENT+=1
  exit /b 0
)
del /f /q "%~1" 2>nul
if exist "%~1" (
  echo   LOCKED      %~1
  set /a STUCK+=1
) else (
  echo   removed     %~1
  set /a GONE+=1
)
exit /b 0
"@
Set-Content (Join-Path $Out "UNINSTALL.bat") $bat -Encoding ascii

# ---- report -------------------------------------------------------------------------------------
Write-Host "dist\CyberpunkVRPort-$Version"
foreach ($m in $manifest) {
    if ($m.Bytes -gt 0) { Write-Host ("  {0,-62} {1,10:N0}" -f $m.Path, $m.Bytes) }
    else                { Write-Host ("  {0}" -f $m.Path) }
}
$all = Get-ChildItem $Out -Recurse -File
Write-Host ""
Write-Host ("  {0} files, {1:N0} bytes total" -f $all.Count, ($all | Measure-Object Length -Sum).Sum)

if ($Zip) {
    # Not $zip: PowerShell variable names are case-insensitive, so that would be the -Zip switch.
    $archivePath = "$Out.zip"
    if (Test-Path $archivePath) { Remove-Item $archivePath -Force }
    Compress-Archive -Path (Join-Path $Out "*") -DestinationPath $archivePath
    Write-Host ("  packaged -> {0}  ({1:N0} bytes)" -f $archivePath, (Get-Item $archivePath).Length)
}
