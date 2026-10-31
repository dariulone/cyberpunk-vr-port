# Assemble a tester package under dist\, laid out exactly as it must land in the game root.
#
# Everything comes from the repo or from a build output -- nothing is read out of the installed
# game -- so what a tester gets is what is committed. Run scripts\sync_assets.ps1 first if the
# yamls, archives or grip poses have been touched game-side since the last pull.
#
# Usage:
#   pwsh scripts\build_dist.ps1
#   pwsh scripts\build_dist.ps1 -Version 0.1.9 -Zip

param(
    [ValidatePattern('^\d+\.\d+\.\d+(?:-[A-Za-z0-9][A-Za-z0-9.-]*)?$')]
    [string]$Version = "0.1.9",
    # The game 2.5 build tree (RED4ext.SDK game-2.5); build\ is the old 2.31 tree and its DLL is stale.
    [string]$BuildDir = "build\patchday\sdk25-build",
    [switch]$Zip,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$DistRoot = [IO.Path]::GetFullPath((Join-Path $RepoRoot "dist"))
$Out      = [IO.Path]::GetFullPath((Join-Path $DistRoot "CyberpunkVRPort-$Version"))
if ((Split-Path $Out -Parent) -ne $DistRoot) { throw "Package path must be directly inside dist" }

# Folders that exist for development and have no business in a tester's game.
#
# ReloadRecorder is an AUTHORING tool, not a feature: it records the game's own reload
# animation per frame with VRIK off, and every grip pose and wrist placement the reload
# ships was lifted from those takes. A player has nothing to record and nothing to do with
# the takes, and it puts a third onUpdate on a pair of hands that already carry two solvers.
$SkipMods  = @("CyberpunkVRPort_WorldMapDiag", "CyberpunkVRPort_ReloadRecorder", "CyberpunkVRPort_QuickBoot")
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
    if (-not $Force) { throw "Package already exists: $Out. Use -Force to rebuild it." }
    $resolvedOut = (Resolve-Path -LiteralPath $Out).Path
    if ((Split-Path $resolvedOut -Parent) -ne $DistRoot -or
        ((Get-Item -LiteralPath $Out).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Refusing to replace a package outside dist or a linked directory"
    }
    Remove-Item -LiteralPath $resolvedOut -Recurse -Force
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
# Private V557 and loader must travel with the plugin; do not copy flight logs.
$ofxrDir=Join-Path (Split-Path $stereoDll -Parent) 'ofxr-v557'
foreach($file in @('CyberpunkVR_OFXR.dll','openxr_loader.dll','ofxr-v557.json','ofxr_bridge.ini','LICENSE-OFXR.txt','LICENSE-OpenXR.txt','THIRD_PARTY-OFXR.md','README.md')) {
    Add-File (Need (Join-Path $ofxrDir $file) "Private V557 $file") ("red4ext\plugins\CyberpunkVR_Stereo\ofxr-v557\"+$file)
}
foreach($license in Get-ChildItem -LiteralPath (Need (Join-Path $ofxrDir 'licenses') 'OFXR licenses') -File) {
    Add-File $license.FullName ("red4ext\plugins\CyberpunkVR_Stereo\ofxr-v557\licenses\"+$license.Name)
}
Add-File (Join-Path $RepoRoot 'docs\OFXR-V557-INTEGRATION.md') 'docs\OFXR-V557-INTEGRATION.md'


# The sight shaders are loaded by name at PSO-replacement time; without both, the replacement is
# skipped and the only symptom is one line in the log.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\sight_reflex_ps.dxil") "sight PS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_SightPs.dxil"
Add-File (Need (Join-Path $RepoRoot "src\Shaders\sight_reflex_vs.dxil") "sight VS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_SightVs.dxil"
Add-File (Need (Join-Path $RepoRoot "src\Shaders\sight_reflex_ps_scope.dxil") "sight PS scope variant") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_SightPsScope.dxil"
foreach ($variant in @('649567EB5A039A01', '372EEC30C414B67A')) {
    Add-File (Need (Join-Path $RepoRoot "src\Shaders\scope_glass_$variant.dxil") "scope glass PS $variant") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_ScopeGlass_$variant.dxil"
}
Add-File (Need (Join-Path $RepoRoot "src\Shaders\lens_velocity_ps.dxil") "lens velocity PS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_LensVelocityPs.dxil"
Add-File (Need (Join-Path $RepoRoot "src\Shaders\lens_mask_ps.dxil") "lens history mask PS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_LensMaskPs.dxil"
Add-File (Need (Join-Path $RepoRoot "src\Shaders\sight_mask_ps.dxil") "camera surface history mask PS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_SightMaskPs.dxil"
Add-File (Need (Join-Path $RepoRoot "src\Shaders\lens_source_resolve_cs.dxil") "lens source resolve CS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_LensSourceResolveCs.dxil"
Add-File (Need (Join-Path $RepoRoot "src\Shaders\camera_source_resolve_cs.dxil") "camera source resolve CS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_CameraSourceResolveCs.dxil"
# The outline shader. Without it the port still runs and the engine draws its own thin line, so it
# is not fatal to omit -- which is exactly why it has to be listed: a missing file here looks like a
# feature that was never built.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\vision_highlight_cs.dxil") "outline CS") "red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_VisionCs.dxil"
# The volumetric-fog twins for the asymmetric projection, by engine shader hash. Missing ones leave the
# fog ignoring the projection's shift -- shafts in the wrong place, blocky haze -- so all three are required.
foreach ($hash in @('3bec9b299e5c0610', '601954c7c7c00b49', 'b098a198a7565e1d', 'f60d19af775ca034')) {
    Add-File (Need (Join-Path $RepoRoot "src\Shaders\asym_fog\$hash.dxil") "volumetric fog shift twin $hash") "red4ext\plugins\CyberpunkVR_Stereo\asym\$hash.dxil"
}
# The screen-space and planar reflection twins, same reason: without them reflections are traced from the
# wrong points. One per SSR quality's trace (tools/asym_tests/asym_ssr_shaders.py); the traces and the clear of
# the tiles without coarse hits (c36da...) also merge the two eyes' reflections (Render/SsrWideMerge.cpp).
foreach ($hash in @('842ab722bfe403cd', 'b86f8871454997e6', 'c827b32cf54f25fb', '8661cc86756e2ecc', 'acaedc11005a92d0', '5e01aaacc7c617d2', 'e5741046a837b9cc')) {
    Add-File (Need (Join-Path $RepoRoot "src\Shaders\asym_ssr\$hash.dxil") "SSR shift twin $hash") "red4ext\plugins\CyberpunkVR_Stereo\asym_ssr\$hash.dxil"
}
# The shadow-mask twin: merges the two eyes' sun contact shadows as one wide display (Render/SsrWideMerge.cpp).
Add-File (Need (Join-Path $RepoRoot "src\Shaders\asym_shadow\753b3bed8df87d53.dxil") "shadow-mask wide-merge twin") "red4ext\plugins\CyberpunkVR_Stereo\asym_shadow\753b3bed8df87d53.dxil"
# The material view-direction twins (tools/asym_tests/view_dir_shaders.py): screens and holograms whose parallax
# layers and facing terms used the camera's forward axis take the pixel's own view ray (Render/HiddenAreaShaders.cpp).
Get-ChildItem (Join-Path $RepoRoot "src\Shaders\view_dir") -Filter *.dxil | ForEach-Object { Add-File $_.FullName "red4ext\plugins\CyberpunkVR_Stereo\view_dir\$($_.Name)" }
# The cone-AO twins: without them the occlusion in corners differs between the eyes; the traces also merge the
# two eyes' AO at their inner edges (Render/SsrWideMerge.cpp, kConeAo).
foreach ($hash in @('33f28d9a0e7a8352', '1cbc87916ec8e03c', '1381eb9e608485d7', '059246fd5e64ea98')) {
    Add-File (Need (Join-Path $RepoRoot "src\Shaders\asym_ao\$hash.dxil") "cone-AO shift twin $hash") "red4ext\plugins\CyberpunkVR_Stereo\asym_ao\$hash.dxil"
}
# The particle-facing vertex twins, one per particle vertex shader in the game's cache that lays sprites out
# along the camera's axes (tools/asym_tests/particle_facing_shaders.py): without them smoke and steam turn
# with the head. Too many to list; the count is checked instead.
$particleTwins = @(Get-ChildItem -LiteralPath (Need (Join-Path $RepoRoot "src\Shaders\asym_particles") "particle-facing twins") -Filter *.dxil)
if ($particleTwins.Count -lt 106) { throw "particle-facing twins: $($particleTwins.Count) of 106 in src\Shaders\asym_particles" }
foreach ($twin in $particleTwins) {
    Add-File $twin.FullName "red4ext\plugins\CyberpunkVR_Stereo\asym_particles\$($twin.Name)"
}
# The auto-exposure twins (tools/asym_tests/asym_expo_shaders.py): the luminance image metered around the view
# centre, and the slower adaptation.
foreach ($name in @("1a6a7c32da1959d6.dxil", "d1b593e255bfcb11.dxil")) {
    Add-File (Need (Join-Path $RepoRoot "src\Shaders\asym_expo\$name") "auto-exposure twin $name") "red4ext\plugins\CyberpunkVR_Stereo\asym_expo\$name"
}
# The screen-effect twin (tools/asym_tests/screen_fx_shaders.py): the Sandevistan's look without its radial smear
# and block tearing around each eye's screen middle.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\screen_fx\6924aa01ad878186.dxil") "Sandevistan screen-effect twin") "red4ext\plugins\CyberpunkVR_Stereo\screen_fx\6924aa01ad878186.dxil"
# The engine-stereo merge twins (tools/asym_tests/stereo_ssr_merge_shaders.py): under 2.5 engine stereo the ambient
# pass (all its permutations) takes the reflections an eye lost at its inner edge from the other eye's half of the frame.
foreach ($hash in @('19fe7d5422b1206e', '056eafbcc8ee2a87', '2188c71b84b45ded', '68de2bc2f8971207', '6a766457f5744bdd',
                    '710492da1c504db5', 'b39515454d8975e2', 'e3e6c1d2d6391955')) {
    Add-File (Need (Join-Path $RepoRoot "src\Shaders\stereo_merge\$hash.dxil") "engine-stereo SSR merge twin $hash") "red4ext\plugins\CyberpunkVR_Stereo\stereo_merge\$hash.dxil"
}
# The engine-stereo eye twins (tools/asym_tests/stereo_eye_shaders.py): the volumetric clouds (both qualities of the
# march) look up the fog in front of a cloud in the pixel's own eye, not the dispatch camera's.
foreach ($hash in @('8a6c46c528ce5160', 'c944329b340f33c2')) {
    Add-File (Need (Join-Path $RepoRoot "src\Shaders\stereo_eye\$hash.dxil") "engine-stereo eye twin $hash") "red4ext\plugins\CyberpunkVR_Stereo\stereo_eye\$hash.dxil"
}
# First stereo bloom downsample; enabled separately from the other stereo patches.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\stereo_bloom\83848f96b80814d5.dxil") "stereo bloom prefilter") "red4ext\plugins\CyberpunkVR_Stereo\stereo_bloom\83848f96b80814d5.dxil"
# Shared particle-lighting allocation: visibility in either eye is sufficient.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\stereo_particle_lighting\e4c84ae0e5616f91.dxil") "stereo particle-lighting allocator") "red4ext\plugins\CyberpunkVR_Stereo\stereo_particle_lighting\e4c84ae0e5616f91.dxil"
# Clustered-light bounds agree with the per-eye projection used by their planes.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\stereo_light_cull\37851ec24d58e673.dxil") "stereo light-culling twin") "red4ext\plugins\CyberpunkVR_Stereo\stereo_light_cull\37851ec24d58e673.dxil"
# Distant lights query each eye's HiZ rectangle with its own projection.
Add-File (Need (Join-Path $RepoRoot "src\Shaders\stereo_distant_light\6a1544708bf0067e.dxil") "stereo distant-light culler") "red4ext\plugins\CyberpunkVR_Stereo\stereo_distant_light\6a1544708bf0067e.dxil"

# Merged into the player's settings ONCE, preserving personal preferences, when first_launch=1
# in vrport.ini. FirstLaunch clears it to 0 only after the merge succeeds. A timestamped backup
# of the original file is kept. See INSTALL.txt.
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
    # Only our installed modules belong in the package/uninstaller, never a
    # shared script cache, authoring folder or an empty retired module.
    if ($d.Name -notlike "CyberpunkVRPort_*" -or $SkipMods -contains $d.Name -or
        -not (Get-ChildItem -LiteralPath $d.FullName -Recurse -File -Filter '*.reds' | Select-Object -First 1)) { continue }
    $n = Copy-Tree $d.FullName (Join-Path $Out "r6\scripts\$($d.Name)")
    $manifest += [pscustomobject]@{ Path = "r6\scripts\$($d.Name)\  ($n files)"; Bytes = 0 }
}
$tweaksRoot = Join-Path $RepoRoot "mods\tweaks"
foreach ($tw in (Get-ChildItem -LiteralPath $tweaksRoot -Directory)) {
    $n = Copy-Tree $tw.FullName (Join-Path $Out "r6\tweaks\$($tw.Name)")
    $manifest += [pscustomobject]@{ Path = "r6\tweaks\$($tw.Name)\  ($n files)"; Bytes = 0 }
}
foreach ($tw in (Get-ChildItem -LiteralPath $tweaksRoot -File)) {
    if ($tw.Extension -in @('.yaml', '.yml', '.tweak')) {
        Add-File $tw.FullName "r6\tweaks\$($tw.Name)"
    }
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

# ---- scanner HUD editor binding ---------------------------------------------------------------
# Input Loader merges this file into the game's input mappings on startup.
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
    (holsters, physical reload, melee, weapon handling, smoking). HUD placement is handled by
    the VR port.

BEFORE YOU INSTALL -- READ THIS ONE
    The first time the plugin starts it MERGES the VR preset into your Cyberpunk settings:

        %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json

    Graphics, all input bindings/controls, HUD and comfort values come from the preset. Your
    language, audio, game difficulty, telemetry, cloud-save preference and gallery favorites
    are preserved. Groups and settings absent from the preset are retained too.
    Your original file is backed up as UserSettings.pre-vr-<date>-<time>-<id>.json in the same
    folder before atomic installation. Invalid JSON or a failed backup leaves it unchanged.
    It happens exactly once:
    afterwards the file is yours and nothing here looks at it again. Everything you change in the
    game's own menus sticks.

    first_launch=1 in bin\x64\vrport.ini means "not installed yet"; the plugin
    clears it to 0 once the merge has succeeded, and never looks again.

    To SKIP it entirely, including on the very first launch: create bin\x64\vrport.ini yourself,
    containing the single line first_launch=0, before you start the game. Creating it afterwards
    is too late -- the plugin writes that file and reads it in the same breath, so a fresh
    install has already been offered the settings by the time you can edit it.

    To ask for the settings later, set first_launch=1 and start the game once.

REQUIREMENTS
    Cyberpunk 2077 2.5 (this build's engine offsets are matched to it; 2.31 will not work)
    The 2.5 releases of the frameworks, or newer:
        RED4ext 1.31.0, Cyber Engine Tweaks 1.38.0, redscript 0.5.32,
        ArchiveXL 1.28.0, TweakXL 1.12.0, Codeware 1.21.0
    Older ones fail on 2.5: an ArchiveXL before 1.28 loads no item factories (items in the hands
    stay invisible), a redscript before 0.5.32 cannot read the 2.5 script cache.
    An OpenXR runtime, started BEFORE the game

    Input Loader is required for the scanner HUD editor's Right Shift binding.

    Nothing else may proxy dxgi. If another VR mod installed bin\x64\dxgi.dll, move it
    out of the folder -- two VR paths in one process fight over the same engine hooks.

INSTALL
    Extract the contents of this folder into your Cyberpunk 2077 game root -- the folder that
    contains bin\, r6\, red4ext\ and archive\. The paths inside already match.

    When upgrading, remove r6\scripts\CyberpunkVRPort_LootUi\vrport_loot_ui.reds.
    Its old loot scaling and tooltip reparenting are replaced by the texture HUD.
    Also remove r6\scripts\CyberpunkVRPort_SettingsGuard\vrport_settings_guard.reds
    to restore the native cascade shadow settings in the game menu.
    Upgrading from 0.1.7 or older: delete archive\pc\mod\VRCigarette.archive.xl,
    archive\pc\mod\vrport_basketball.archive and archive\pc\mod\vrport_mag.archive.
    Their assets are inside cyberpunkvrport.archive now; the old files would duplicate them.
    The same for archive\pc\mod\vrport_menu_native.archive and vrport_menu_panels.archive
    from a development build: the VR menu's assets are inside cyberpunkvrport.archive too.
    QuickBoot is not included. If you previously installed a development build with it,
    remove bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_QuickBoot to keep
    normal startup and save selection. Existing launcher preferences are preserved;
    show_launcher defaults to 1 only when no saved preference overrides it.

    Install More Occluders (Scripted v0.3) separately from its author's page:
    https://www.nexusmods.com/cyberpunk2077/mods/33136
    It is not included in this archive.

    Then start your OpenXR runtime, then the game. A small launcher window appears first: pick
    your headset and per-eye render resolution there.

WHAT LANDS WHERE
    red4ext\plugins\CyberpunkVR_Stereo\   the VR plugin, its shaders, the settings template
    bin\x64\CyberpunkVR_*Grip*.ini        captured hand poses for holding a cigarette and lighter
    bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_*\
    r6\scripts\CyberpunkVRPort_*\
    r6\tweaks\vrcigarette\
    archive\pc\mod\                       packed assets + the ArchiveXL manifest
    r6\input\CyberpunkVRPort_ScannerHud.xml   scanner editor binding (Input Loader)

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

# ---- the changelog, beside INSTALL.txt --------------------------------------------------------
# NOT added through Add-File, and that is the point: the manifest drives the generated uninstaller,
# so anything registered there is a path this package puts INTO the game and later deletes from it.
# This is documentation that lives inside the zip, like INSTALL.txt itself.
$changelog = Join-Path $RepoRoot "CHANGELOG.md"
if (Test-Path $changelog) {
    $text = (Get-Content -LiteralPath $changelog -Raw)
    Set-Content (Join-Path $Out "CHANGELOG.txt") $text -Encoding utf8
    Write-Host ("  {0,-64}{1,10:N0}" -f "CHANGELOG.txt", (Get-Item (Join-Path $Out "CHANGELOG.txt")).Length)
} else {
    Write-Host "[!] CHANGELOG.md not found -- the package ships without one"
}

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
# Restoring the first-launch UserSettings.json backup is separate from deleting
# the package files. The generated uninstaller offers to restore that backup.
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

4. RESTORE YOUR GAME SETTINGS IF NEEDED

    YOUR GAME SETTINGS. On its first launch the plugin merged the VR preset into

        %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json

    while preserving personal preferences, and copied the original to
    UserSettings.pre-vr-<date>-<time>-<id>.json in the same folder. Removing the mod does not put it
    back. UNINSTALL.bat offers to; by hand, rename that backup over UserSettings.json yourself
    with the game closed. Skip it and you keep the applied VR graphics, input and comfort settings
    for good.

5. IF YOU MOVED A dxgi.dll ASIDE FOR THIS MOD

    Some other VR mods install bin\x64\dxgi.dll, and this port cannot share a
    process with one, so you may have renamed it -- deploy_stereo.ps1 renames it to
    dxgi.dll.disabled-red4ext. Rename it back if you want that mod again.

6. WHAT IS NOT TOUCHED, so you do not go looking for it

    Your saves. Nothing here reads or writes them, and nothing this mod does is persisted into a
    save: no custom quest facts, no items that stop existing, no scripted entities parked in the
    world. A save made with the mod loads without it.

    The other mods this one requires -- RED4ext, Cyber Engine Tweaks, redscript, ArchiveXL,
    TweakXL, Codeware, Visual Holsters, Equipment-EX, Nova Optics, Input Loader.
    Each has its own uninstall; the lists above remove only what carries the
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
echo   See UNINSTALL.txt for the complete removal and settings-restoration instructions.
echo   Your Cyberpunk settings are offered back at the end.
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
echo   Your own Cyberpunk settings were saved before this mod applied its VR preset:
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
echo   So either the VR preset was never applied, or the backup has been moved.
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
if ($all.FullName -match '[\\/]CyberpunkVRPort_QuickBoot[\\/]') {
    throw "QuickBoot must not be included in a release package"
}
if (Test-Path -LiteralPath (Join-Path $Out 'r6\scripts\cache')) {
    throw "Shared script cache must not be owned by the release package"
}
if (Test-Path -LiteralPath (Join-Path $Out 'r6\scripts\MoreOccluders.reds')) {
    throw "More Occluders must be installed separately from its author's page"
}
Write-Host ""
Write-Host ("  {0} files, {1:N0} bytes total" -f $all.Count, ($all | Measure-Object Length -Sum).Sum)

if ($Zip) {
    # Not $zip: PowerShell variable names are case-insensitive, so that would be the -Zip switch.
    $archivePath = "$Out.zip"
    if (Test-Path $archivePath) { Remove-Item $archivePath -Force }
    Compress-Archive -Path (Join-Path $Out "*") -DestinationPath $archivePath
    Write-Host ("  packaged -> {0}  ({1:N0} bytes)" -f $archivePath, (Get-Item $archivePath).Length)
}
