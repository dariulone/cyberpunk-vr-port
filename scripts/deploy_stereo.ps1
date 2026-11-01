# Install the VR port: the RED4ext plugin, its shaders, and the CET mod that owns the VRCAM
# component switch.
#
# Reversible with revert_stereo.ps1.
#
# Build first:
#   cmake --build build --config Release --target cyberpunkvrport_stereo
#
# Usage:
#   pwsh scripts\deploy_stereo.ps1 -GameRoot "C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077"

param(
    [Parameter(Mandatory=$true)]
    [string]$GameRoot,
    [string]$BuildDir = "build"
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path "$PSScriptRoot\.."
$BinX64   = Join-Path $GameRoot "bin\x64"
$Dxgi     = Join-Path $BinX64 "dxgi.dll"
$Stash    = Join-Path $BinX64 "dxgi.dll.disabled_by_stereo"

# The CMake TARGET and the shipped NAME differ, and conflating them is why this script used to
# install to a folder RED4ext then ignored: the target is cyberpunkvrport_stereo, the artefact is
# cyberpunkvrport.dll, and RED4ext loads <plugin folder>\<anything>.dll.
$Target   = "cyberpunkvrport_stereo"
$ModName  = "CyberpunkVR_Stereo"
$Dll      = Join-Path $RepoRoot "$BuildDir\bin\red4ext\plugins\$ModName\Release\$ModName.dll"
$PlugDir  = Join-Path $GameRoot "red4ext\plugins\$ModName"
$Shaders  = Join-Path $RepoRoot "src\Shaders"
$CETName  = "CyberpunkVRPort_Stereo"
$CETMods  = Join-Path $BinX64 "plugins\cyber_engine_tweaks\mods\$CETName"
$CETSrc   = Join-Path $RepoRoot "mods\cet\$CETName"

if (-not (Test-Path $GameRoot)) { throw "GameRoot not found: $GameRoot" }
if (-not (Test-Path $Dll))      { throw "Plugin not built: $Dll  (cmake --build $BuildDir --config Release --target $Target)" }

$proc = Get-Process Cyberpunk2077 -ErrorAction SilentlyContinue
if ($proc) { throw "Cyberpunk2077.exe is running -- close it first, the DLL is locked." }

# 1. Nothing else may proxy dxgi while this plugin runs. Two VR
#    paths in one process fight over the same engine hooks.
if (Test-Path $Dxgi) {
    if (Test-Path $Stash) { Remove-Item $Stash -Force }
    Move-Item $Dxgi $Stash -Force
    Write-Host "[+] Moved bin\x64\dxgi.dll to $Stash (this build does not proxy dxgi)"
} else {
    Write-Host "[=] No bin\x64\dxgi.dll present (already clean)."
}

# 2. The plugin, plus the compiled shaders it loads by name at PSO-replacement time. Both have to
#    be present: the sight replacement is skipped entirely when either blob is missing, and the
#    only symptom is one line in the log saying so.
New-Item -ItemType Directory -Path $PlugDir -Force | Out-Null
Copy-Item $Dll (Join-Path $PlugDir "$ModName.dll") -Force
$sha = (Get-FileHash $Dll -Algorithm SHA256).Hash.Substring(0,16)
Write-Host "[+] Installed $ModName.dll to $PlugDir  (sha $sha)"

Copy-Item (Join-Path $Shaders "sight_reflex_ps.dxil") (Join-Path $PlugDir "CyberpunkVR_SightPs.dxil") -Force
Copy-Item (Join-Path $Shaders "sight_reflex_vs.dxil") (Join-Path $PlugDir "CyberpunkVR_SightVs.dxil") -Force
Copy-Item (Join-Path $Shaders "sight_reflex_ps_scope.dxil") (Join-Path $PlugDir "CyberpunkVR_SightPsScope.dxil") -Force
foreach ($variant in @('649567EB5A039A01', '372EEC30C414B67A')) {
    Copy-Item -LiteralPath (Join-Path $Shaders "scope_glass_$variant.dxil") -Destination (Join-Path $PlugDir "CyberpunkVR_ScopeGlass_$variant.dxil") -Force
}
Copy-Item -LiteralPath (Join-Path $Shaders "lens_velocity_ps.dxil") -Destination (Join-Path $PlugDir "CyberpunkVR_LensVelocityPs.dxil") -Force
Copy-Item -LiteralPath (Join-Path $Shaders "lens_mask_ps.dxil") -Destination (Join-Path $PlugDir "CyberpunkVR_LensMaskPs.dxil") -Force
Copy-Item -LiteralPath (Join-Path $Shaders "sight_mask_ps.dxil") -Destination (Join-Path $PlugDir "CyberpunkVR_SightMaskPs.dxil") -Force
Copy-Item -LiteralPath (Join-Path $Shaders "lens_source_resolve_cs.dxil") -Destination (Join-Path $PlugDir "CyberpunkVR_LensSourceResolveCs.dxil") -Force
Copy-Item -LiteralPath (Join-Path $Shaders "camera_source_resolve_cs.dxil") -Destination (Join-Path $PlugDir "CyberpunkVR_CameraSourceResolveCs.dxil") -Force
Copy-Item (Join-Path $Shaders "vision_highlight_cs.dxil") (Join-Path $PlugDir "CyberpunkVR_VisionCs.dxil") -Force
# Volumetric-fog twins for the asymmetric projection (tools/asym_tests/asym_fog_shaders.py): loaded by
# the engine shader's hash from asym\. Without them the fog ignores the projection's shift.
# Each twin folder is emptied first: twins are keyed by the game build's shader hashes, and an earlier
# build's left behind would still count as loaded.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "asym") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "asym") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "asym_fog\*.dxil") -Destination (Join-Path $PlugDir "asym") -Force
Write-Host "[+] Installed the volumetric-fog shift twins (asym\*.dxil)"
# Screen-space reflection twins (tools/asym_tests/asym_ssr_shaders.py), same mechanism, own folder and switch:
# without them the reflections are traced from points the shift moved away from their pixels.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "asym_ssr") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "asym_ssr") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "asym_ssr\*.dxil") -Destination (Join-Path $PlugDir "asym_ssr") -Force
Write-Host "[+] Installed the screen-space reflection shift twins (asym_ssr\*.dxil)"
# Cone-AO twins (tools/asym_tests/asym_ao_shaders.py): they read the projection centre the plugin writes into
# the AO block's padding (Stereo/Grading.cpp), switch CyberpunkVR_AsymAoPatch.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "asym_ao") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "asym_ao") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "asym_ao\*.dxil") -Destination (Join-Path $PlugDir "asym_ao") -Force
Write-Host "[+] Installed the cone-AO shift twins (asym_ao\*.dxil)"
# Shadow-mask twin (tools/asym_tests/asym_shadow_shaders.py): the two eyes' sun contact shadows as one wide display.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "asym_shadow") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "asym_shadow") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "asym_shadow\*.dxil") -Destination (Join-Path $PlugDir "asym_shadow") -Force
Write-Host "[+] Installed the shadow-mask wide-merge twin (asym_shadow\*.dxil)"
# Material view-direction twins (tools/asym_tests/view_dir_shaders.py): parallax screens/holograms take the pixel's ray.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "view_dir") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "view_dir") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "view_dir\*.dxil") -Destination (Join-Path $PlugDir "view_dir") -Force
Write-Host "[+] Installed the material view-direction twins (view_dir\*.dxil)"
# Particle-facing vertex twins (tools/asym_tests/particle_facing_shaders.py): sprites turned to the eye position
# instead of laid out in the screen plane, switch CyberpunkVR_ParticleFacing.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "asym_particles") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "asym_particles") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "asym_particles\*.dxil") -Destination (Join-Path $PlugDir "asym_particles") -Force
Write-Host "[+] Installed the particle-facing twins (asym_particles\*.dxil)"
# Auto-exposure twins (tools/asym_tests/asym_expo_shaders.py): the meter centred on the view instead of the
# middle of each eye's screen, slower adaptation; switch CyberpunkVR_ExposureCentre.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "asym_expo") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "asym_expo") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "asym_expo\*.dxil") -Destination (Join-Path $PlugDir "asym_expo") -Force
Write-Host "[+] Installed the auto-exposure twins (asym_expo\*.dxil)"
# Screen-effect twins (tools/asym_tests/screen_fx_shaders.py): the Sandevistan keeps its tint and grid without the
# radial smear and block tearing; switch CyberpunkVR_ScreenFxPatch.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "screen_fx") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "screen_fx") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "screen_fx\*.dxil") -Destination (Join-Path $PlugDir "screen_fx") -Force
Write-Host "[+] Installed the screen-effect twins (screen_fx\*.dxil)"
# Engine-stereo merges (tools/asym_tests/stereo_ssr_merge_shaders.py): the ambient pass takes the reflections an eye
# lost at its inner edge from the other eye's half of the frame; switch CyberpunkVR_StereoMergePatch.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "stereo_merge") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "stereo_merge") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "stereo_merge\*.dxil") -Destination (Join-Path $PlugDir "stereo_merge") -Force
Write-Host "[+] Installed the engine-stereo merge twins (stereo_merge\*.dxil)"
# Engine-stereo eye-of-pixel twins (tools/asym_tests/stereo_eye_shaders.py): the volumetric clouds' fog lookup takes
# the pixel's eye instead of the dispatch's camera; switch CyberpunkVR_StereoEyePatch.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "stereo_eye") -Force | Out-Null
Get-ChildItem -LiteralPath (Join-Path $PlugDir "stereo_eye") -Filter *.dxil | Remove-Item -Force
Copy-Item -Path (Join-Path $Shaders "stereo_eye\*.dxil") -Destination (Join-Path $PlugDir "stereo_eye") -Force
Write-Host "[+] Installed the engine-stereo eye twins (stereo_eye\*.dxil)"
# First stereo bloom downsample: normalized HDR weights keep thin emissives from dominating bloom.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "stereo_bloom") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Shaders "stereo_bloom\83848f96b80814d5.dxil") -Destination (Join-Path $PlugDir "stereo_bloom") -Force
Write-Host "[+] Installed the stereo bloom prefilter (stereo_bloom\83848f96b80814d5.dxil)"
# Shared particle lighting must retain particles visible to either eye.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "stereo_particle_lighting") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Shaders "stereo_particle_lighting\e4c84ae0e5616f91.dxil") -Destination (Join-Path $PlugDir "stereo_particle_lighting") -Force
Write-Host "[+] Installed the stereo particle-lighting allocator (stereo_particle_lighting\e4c84ae0e5616f91.dxil)"
# Clustered-light bounds use each eye's scale, matching its asymmetric projection.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "stereo_light_cull") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Shaders "stereo_light_cull\37851ec24d58e673.dxil") -Destination (Join-Path $PlugDir "stereo_light_cull") -Force
Write-Host "[+] Installed the stereo light-culling twin (stereo_light_cull\37851ec24d58e673.dxil)"
# Distant lights use eye-local HiZ coordinates and visibility in either eye.
New-Item -ItemType Directory -Path (Join-Path $PlugDir "stereo_distant_light") -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $Shaders "stereo_distant_light\6a1544708bf0067e.dxil") -Destination (Join-Path $PlugDir "stereo_distant_light") -Force
Write-Host "[+] Installed the stereo distant-light culler (stereo_distant_light\6a1544708bf0067e.dxil)"
Write-Host "[+] Installed the sight shaders (CyberpunkVR_SightPs.dxil / _SightVs.dxil / _SightPsScope.dxil)"
Write-Host "[+] Installed the outline shader (CyberpunkVR_VisionCs.dxil)"

# A SPARE .DLL IN A PLUGIN FOLDER IS A SECOND COPY OF THE PLUGIN, NOT A BACKUP.
#
# RED4ext loads every .dll under red4ext\plugins, by file, not by folder name. Someone (me) left
# CyberpunkVR_Hands.pre-smoke-20260801-130436.dll beside the real one and RED4ext dutifully said
#
#     Loading plugin from '...\CyberpunkVR_Hands\CyberpunkVR_Hands.pre-smoke-20260801-130436.dll'
#     CyberpunkVR_Hands (version: 1.0.0, author(s): VRPort) has been loaded
#     6 plugin(s) loaded
#
# Two builds of the same plugin, both registering the same natives and both installing pattern-scan
# detours over the same addresses -- the second writing its jump over the first's trampoline. That
# is a crash waiting for a launch to land on, and the launches it landed on read inaccessible data
# at 0xFFFFFFFFFFFFFFFF with no stack.
#
# Only OUR folders are touched, and nothing is deleted: strays move one directory out of the tree
# RED4ext scans, which is the whole difference between a backup and a second plugin.
$strayDest = Join-Path $GameRoot "_vrport_disabled"
foreach ($ourDir in @("CyberpunkVR_Stereo", "CyberpunkVR_Hands")) {
    $dir = Join-Path $GameRoot "red4ext\plugins\$ourDir"
    if (-not (Test-Path $dir)) { continue }
    $strays = Get-ChildItem $dir -Filter *.dll -File -ErrorAction SilentlyContinue |
              Where-Object { $_.Name -ne "$ourDir.dll" }
    foreach ($s in $strays) {
        if (-not (Test-Path $strayDest)) { New-Item -ItemType Directory -Path $strayDest -Force | Out-Null }
        Move-Item -LiteralPath $s.FullName -Destination $strayDest -Force
        Write-Host "[!] $ourDir\$($s.Name) was a SECOND copy of the plugin -- RED4ext loads every"
        Write-Host "    .dll in there. Moved to _vrport_disabled\ (not deleted)."
    }
}

# The game settings this port was tuned against. The plugin merges their values into the player's
# UserSettings.json once (preserving personal settings), with first_launch=1 in vrport.ini, and
# keeps a timestamped copy of what was there. Putting the file here rather than installing it from
# this script is deliberate: the decision belongs to the flag, not to whoever ran the deploy.
$Settings = Join-Path $RepoRoot "mods\config\UserSettings.json"
if (Test-Path $Settings) {
    Copy-Item $Settings (Join-Path $PlugDir "UserSettings.json") -Force
    Write-Host "[+] Staged the VR game settings (applied on the next launch with first_launch=1)"
}

# 3. CET mod. Preserve bridge/ so a live session's notes survive a re-deploy, and carry the chosen
#    resolution PICK across -- but not the whole vrcam.json. Keeping the installed file wholesale
#    is how the catalogue goes stale: the repo copy grows a resolution, the deploy puts the old
#    file back, and the launcher never offers the new entry.
# Named files, not the whole folder: Copy-Item of a directory ONTO an existing directory nests it
# rather than merging (bridge\<backup name>\...), and a blanket restore also drags back whatever
# the folder used to hold -- which for this one is a pile of dead IPC files.
$BridgeKeep = @("vrcam_enable.txt", "vrcam_active.txt")
$BridgeBak = Join-Path $env:TEMP "${CETName}_bridge_bak"
Remove-Item $BridgeBak -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $BridgeBak -Force | Out-Null
foreach ($f in $BridgeKeep) {
    $src = Join-Path $CETMods "bridge\$f"
    if (Test-Path $src) { Copy-Item $src (Join-Path $BridgeBak $f) -Force }
}
$PickComponent = $null
$PickCamera    = $null
$InstalledCfg = Join-Path $CETMods "vrcam.json"
if (Test-Path $InstalledCfg) {
    $body = Get-Content $InstalledCfg -Raw
    if ($body -match '"component"\s*:\s*"([^"]+)"')     { $PickComponent = $Matches[1] }
    if ($body -match '"virtualCamera"\s*:\s*"([^"]+)"') { $PickCamera    = $Matches[1] }
}
Remove-Item $CETMods -Recurse -Force -ErrorAction SilentlyContinue
Copy-Item $CETSrc $CETMods -Recurse -Force
New-Item -ItemType Directory -Path (Join-Path $CETMods "bridge") -Force | Out-Null
foreach ($f in $BridgeKeep) {
    $src = Join-Path $BridgeBak $f
    if (Test-Path $src) { Copy-Item $src (Join-Path $CETMods "bridge\$f") -Force }
}
Remove-Item $BridgeBak -Recurse -Force -ErrorAction SilentlyContinue
if ($PickComponent) {
    $cfg = Get-Content $InstalledCfg -Raw
    # Only if the pick still exists in the fresh catalogue -- a resolution can be retired.
    if ($cfg -match [regex]::Escape('"' + $PickComponent + '"')) {
        $cfg = $cfg -replace '("component"\s*:\s*")[^"]+(")', ('${1}' + $PickComponent + '${2}')
        if ($PickCamera) {
            $cfg = $cfg -replace '("virtualCamera"\s*:\s*")[^"]+(")', ('${1}' + $PickCamera + '${2}')
        }
        Set-Content $InstalledCfg $cfg -Encoding utf8 -NoNewline
        Write-Host "[=] Carried the resolution pick across: $PickComponent"
    } else {
        Write-Host "[!] Previous pick $PickComponent is not in the new catalogue -- left at the default"
    }
}
Write-Host "[+] Installed the CET mod to $CETMods"

# The other CET mods that ship from this repo. Only the ones whose folder is already installed are
# refreshed -- this script is not a mod installer, it just stops the copies drifting apart, which
# is how the smoking bridge ended up reading two slots that had been reassigned underneath it.
# Weapon, HUD and the rest joined the list once the DEBUG gate went in: a bridge left on the old
# copy keeps logging per frame no matter what the launcher checkbox says, and "I turned it off and
# it still writes 5 MB" is a bug report nobody can act on.
foreach ($name in @("CyberpunkVRPort_Smoking", "CyberpunkVRPort_Holster", "CyberpunkVRPort_Weapon",
                    "CyberpunkVRPort_HUD", "CyberpunkVRPort_VRIK", "CyberpunkVRPort_Crosshair",
                    "CyberpunkVRPort_WorldMap", "CyberpunkVRPort_Basketball",
                    "CyberpunkVRPort_HandCollision", "CyberpunkVRPort_Driving")) {
    $src = Join-Path $RepoRoot "mods\cet\$name"
    $dst = Join-Path $BinX64 "plugins\cyber_engine_tweaks\mods\$name"
    if ((Test-Path $src) -and (Test-Path $dst)) {
        Get-ChildItem $src -File | ForEach-Object { Copy-Item $_.FullName (Join-Path $dst $_.Name) -Force }
        Write-Host "[+] Refreshed the $name CET mod"
    }
}

# 4. The TweakXL yamls and the packed archives. They are edited game-side (by hand, and by
#    WolvenKit's packer) but the repo copy is what a fresh install has to get, so the deploy pushes
#    them out. Pull them back with: pwsh scripts\sync_assets.ps1 -GameRoot <path>
$Sync = Join-Path $PSScriptRoot "sync_assets.ps1"
if (Test-Path $Sync) {
    Write-Host "[+] Game assets (TweakXL yamls, archives):"
    & $Sync -GameRoot $GameRoot -Push
}

Write-Host ""
Write-Host "[ok] deployed."
Write-Host "     Log: $BinX64\cyberpunkvrport.log"
Write-Host "     Revert with: pwsh scripts\revert_stereo.ps1 -GameRoot <path>"
