# Keep the game-side assets and the repo copies in step.
#
# Several things live in the game folder and are edited there -- the TweakXL yamls by hand, the
# archive by WolvenKit's packer -- so the repo copy goes stale the moment anything is rebuilt, and
# a stale copy is worse than none: it ships an old cigarette definition with a new archive. This is
# the one command that moves them.
#
# ONE ARCHIVE. Every packed asset of the port is in cyberpunkvrport.archive, with one manifest in
# ArchiveXL's format (cyberpunkvrport.archive.xl in the repo; config\archive.yaml in the plugin's folder,
# where the plugin mounts the archive and applies the manifest itself). The basketball, magazine, monowire-material and
# crouch-animgraph archives, the optics camera catalogue (!vrport_optics_catalog_probe.archive) and
# VRCigarette.archive.xl were merged into it on 2026-10-16, keeping for every path the copy the game
# used to load first (load order is alphabetical and the first archive to hold a path wins). The VR
# menu's two archives (vrport_menu_native, vrport_menu_panels) followed on 2026-10-27. A
# rebuild therefore starts from the INSTALLED archive (extract, change, pack the whole tree), never
# from a WolvenKit project that holds only part of it. -Push removes the retired files.
#
#   pull (default)   game -> repo.  Run it before committing.
#   -Push            repo -> game.  What deploy_stereo.ps1 calls, so a fresh install gets them.
#
# Usage:
#   pwsh scripts\sync_assets.ps1 -GameRoot "C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077"
#   pwsh scripts\sync_assets.ps1 -GameRoot "<path>" -Push

param(
    [Parameter(Mandatory=$true)]
    [string]$GameRoot,
    [switch]$Push,
    [switch]$Quiet
)

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path "$PSScriptRoot\.."

# The plugin's one folder (include/Core/Paths.hpp): its archive, config and grips.
$Plugin = "red4ext\plugins\cyberpunkvrport"

# repo path (relative)                 game path (relative to GameRoot)
$Assets = @(
    @{ Repo = "mods\tweaks\vrcigarette";                Game = "$Plugin\config\tweaks\vrcigarette";     Dir = $true  },
    # The player collision capsule, narrowed from the stock 0.400 m. Not basketball-specific: it is
    # the same capsule that decides what the player can walk past, so it ships with the port.
    @{ Repo = "mods\tweaks\vrport";                     Game = "$Plugin\config\tweaks\vrport";          Dir = $true  },
    @{ Repo = "mods\archive\cyberpunkvrport.archive";   Game = "$Plugin\archive\cyberpunkvrport.archive"; Dir = $false },
    # The prop FACTORY registration. Its absence is silent and total: an item whose entityName is a
    # factory name then resolves to nothing, and AddItemToSlot answers TRUE with an empty slot -- which
    # is exactly how the speedloader stopped reaching the hand without anything reporting an error.
    @{ Repo = "mods\archive\cyberpunkvrport.archive.xl"; Game = "$Plugin\config\archive.yaml";          Dir = $false },
    # The captured grip poses. VRSmokeDumpFingers writes these into the plugin's grips folder, so they
    # only ever exist game-side until something pulls them -- and without them the smoking mod loads,
    # reports its poses resolved, and holds nothing.
    @{ Repo = "mods\config\CyberpunkVR_SmokeGrip_right.ini";  Game = "$Plugin\grips\CyberpunkVR_SmokeGrip_right.ini";  Dir = $false },
    @{ Repo = "mods\config\CyberpunkVR_SmokeGrip_Left.ini";   Game = "$Plugin\grips\CyberpunkVR_SmokeGrip_Left.ini";   Dir = $false },
    @{ Repo = "mods\config\CyberpunkVR_LighterGrip_Left.ini"; Game = "$Plugin\grips\CyberpunkVR_LighterGrip_Left.ini"; Dir = $false },
    # Engine-side CPU tuning the second view needs, and the OpenVR runtime shim SteamVR setups load.
    @{ Repo = "mods\config\vrcam_cpu_tweaks.ini"; Game = "engine\config\platform\pc\vrcam_cpu_tweaks.ini"; Dir = $false },
    @{ Repo = "mods\config\openvr_api.dll";      Game = "bin\x64\openvr_api.dll";                          Dir = $false }
)

if (-not (Test-Path $GameRoot)) { throw "GameRoot not found: $GameRoot" }

function Hash($p) {
    if (-not (Test-Path -LiteralPath $p)) { return $null }
    (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash
}

function CopyFile($src, $dst) {
    if (-not (Test-Path -LiteralPath $src)) { return "missing at source" }
    $before = Hash $dst
    New-Item -ItemType Directory -Path (Split-Path $dst -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $src -Destination $dst -Force
    $after = Hash $dst
    if ($before -eq $after) { return "unchanged" }
    return $(if ($null -eq $before) { "NEW" } else { "updated" })
}

$changed = 0

# The files merged into cyberpunkvrport.archive. Left in a game folder they would load beside it:
# the catalogue's '!' name would even win over the merged copies of the player entities.
# And the archive and manifest themselves, which moved into the plugin's folder: left here the game would mount
# the archive twice and ArchiveXL apply the manifest a second time.
$Retired = @("!vrport_optics_catalog_probe.archive", "VRCigarette.archive.xl", "vrport_basketball.archive",
             "vrport_mag.archive", "vrport_cyberware_visuals.archive", "vrport_physical_crouch_camera.archive",
             "vrport_menu_native.archive", "vrport_menu_panels.archive",
             "cyberpunkvrport.archive", "cyberpunkvrport.archive.xl")
if ($Push) {
    foreach ($name in $Retired) {
        $old = Join-Path $GameRoot "archive\pc\mod\$name"
        if (Test-Path -LiteralPath $old) {
            Remove-Item -LiteralPath $old -Force
            if (-not $Quiet) { Write-Host ("  {0,-44} removed from archive\pc\mod" -f $name) }
            $changed++
        }
    }
}

foreach ($a in $Assets) {
    $repo = Join-Path $RepoRoot $a.Repo
    $game = Join-Path $GameRoot $a.Game
    $src  = $(if ($Push) { $repo } else { $game })
    $dst  = $(if ($Push) { $game } else { $repo })

    if ($a.Dir) {
        if (-not (Test-Path -LiteralPath $src)) {
            if (-not $Quiet) { Write-Host ("  {0,-44} missing at source" -f $a.Repo) }
            continue
        }
        New-Item -ItemType Directory -Path $dst -Force | Out-Null
        # File by file rather than a folder copy: copying a directory ONTO an existing directory
        # nests it instead of merging, and a blind wipe would take anything the other side added.
        $n = 0
        foreach ($f in (Get-ChildItem -LiteralPath $src -File)) {
            $r = CopyFile $f.FullName (Join-Path $dst $f.Name)
            if ($r -ne "unchanged") { $n++ }
        }
        $count = (Get-ChildItem -LiteralPath $dst -File).Count
        if (-not $Quiet) { Write-Host ("  {0,-44} {1} file(s), {2} changed" -f $a.Repo, $count, $n) }
        $changed += $n
    } else {
        $r = CopyFile $src $dst
        if ($r -ne "unchanged") { $changed++ }
        if (-not $Quiet) {
            $size = if (Test-Path -LiteralPath $dst) { "{0:N0} B" -f (Get-Item -LiteralPath $dst).Length } else { "-" }
            Write-Host ("  {0,-44} {1,-12} {2}" -f $a.Repo, $size, $r)
        }
    }
}

if (-not $Quiet) {
    Write-Host ""
    Write-Host ("[ok] {0} -> {1}: {2} item(s) changed" -f
        $(if ($Push) { "repo" } else { "game" }), $(if ($Push) { "game" } else { "repo" }), $changed)
}
