# CyberFinger / SlimeVR experimental patch

The control specification is [README_CYBRFNGR.md](../README_CYBRFNGR.md);
build/install and live verification are in the [session TODO](cyberfinger-session-todo.md).
The native CF UI bridge leases map, quest-log and apartment-frame contexts from
their visible game controllers. Menu release sends Back without leaking the
both-Menu chord; map Grip uses a foreground-window right mouse button, and map
zoom/Tab and journal Tab use the corresponding native gamepad actions.
The apartment frame uses the full native menu image at 55 degrees and latches
its pose until close or tracking-origin recenter. Prompts/loot reveal the HUD
without claiming conversation navigation.

World interactions follow the hand opposite the movement stick, computed from
the existing handedness/bindings settings. Trigger/Grip supply Choice1/Choice2
(X/Y) after the port's weapon-wheel/reload remaps. Their input remains claimed
until release when the prompt closes. Computers have separate ownership of
the zoomed world widget: the existing native world-hit hook replaces gaze with
the hand ray through the mesh/UV picker. A miss cannot click a retained gaze
target. The same hand scrolls; Grip returns within the computer and Menu emits
StopDeviceControl. The keypad path is unchanged for non-CF controllers.

This checkout contains an experimental patch, release packages and an installer
for the Cyberpunk port. Companion binding assets have also been
added to `D:\build\CyberFinger_SteamVRFusion`; its driver binary and bridge are
unchanged. ResoFBTAutoSetup is a reference; mod updates are installed separately
with backups. Features still awaiting headset verification are called out below.

## What already existed

- CyberFinger Fusion publishes hand/controller poses and a hand skeleton to
  SteamVR. Its fallback profile emulates Index inputs. Buttons still work when
  there is no hand pose, which must not be mistaken for valid hand tracking.
- The port reads `XR_EXT_hand_tracking` for finger curl/spread and can use a fist
  as grip. Motion-controlled hands, holsters, magazine grabs, two-hand support,
  ladders, wheel grabs and nearby world interactions already exist.
- The original FBT integration reads waist and both feet, with in-game
  calibration. It estimates knees, elbows and the torso from those inputs.

## What this patch changes

### Eye-tracked foveated rendering

In **F10 > General > Variable Rate Shading**, immediately below **Fixed Foveated
Rendering**, enable **Eye-tracked foveated rendering**. It defaults to off;
enabling it selects 2 x 2 peripheral shading if shading was off. The center
width/height controls set the full-quality area. Fixed center offsets are ignored
while gaze tracking is enabled. Switch peripheral shading to Off to disable VRS
entirely. Turning off eye tracking restores the existing fixed VRS behavior,
which is available only in legacy stereo.

This uses D3D12 Tier 2 variable-rate shading, including on AMD, independently of
the NVIDIA-only fixed DLSS path. It supports both legacy per-eye targets and
native stereo targets. Only recognized opaque scene passes are affected;
HUD, transparent materials and unrecognized passes keep their normal shading.
It reduces material shading work, not geometry, ray-tracing or streaming costs.

Quest Pro needs eye tracking enabled/calibrated and Steam Link's **Share eye
tracking data to other apps on this PC** enabled. The mod uses
`XR_EXT_eye_gaze_interaction`; it needs no OSC listener or additional eye-tracking
mod. [Valve documents this OpenXR passthrough](https://store.steampowered.com/news/posts/?appgroupname=SteamVR&appids=250820&enddate=1728412551&feed=steam_community_announcements).
The runtime's capability and current gaze status appear in the overlay. Missing
capability, denied sharing, lost focus/tracking, stale samples or unsupported
projection use full quality. The feature currently requires the port's default
asymmetric projection. It projects the combined gaze direction into each rendered
eye window; the extension supplies no per-eye convergence depth.

Shading maps are immutable, cached on a 1/16-image grid, and retained through GPU
completion. The full-quality region includes a guard for grid rounding. A newly
requested map uses full quality until its upload completes; no old gaze map is
substituted. The cache is bounded to 128 maps. This avoids rewriting a texture
that recorded or in-flight command lists still reference. Gains and the best
center size depend on the scene and require headset measurement.

`config/vrport.ini`: `xr_vrs_eye_tracked=0/1`, with existing `xr_vrs=0/1/2`
(Off/2 x 2/4 x 4), `xr_vrs_width`, and `xr_vrs_height`. No gaze coordinates are
written to disk. Debug logs report capability/status changes, and the existing
VRS diagnostic export includes freshness and draw counts.

Validation: `tools/vrs_tests` exercises settings persistence, gaze projection,
tracking/focus loss, stale/invalid input, extension setup/teardown, GPU shading
reduction, full-quality fallback, stereo map readback and eye viewports. The
Release plugin builds. SteamVR on this machine advertises the extension and has
sharing enabled; the current Quest Pro / Steam Link game log confirms live gaze
reception (tracking state 2). Visual quality and performance testing remain pending.

Live testing exposed a 512-command-list registry cap that stopped VRS after
loading gameplay. The fix attaches tracking state to each command list's lifetime,
uses weak registry/cache references, and prunes expired entries. GPU regression
testing covers 640 live lists and releasing their states. Check **increases** in
the exported VRS draw counters over a timed interval; a nonzero cumulative total
can belong entirely to an earlier scene. `tracked_lists` and `state_failures` in
the diagnostic report expose the new ownership path. The on/off measurement tool
is `tools/vrs_tests/live_ab.py`; it restores `xr_vrs` after testing and rejects an
on sample with no new VRS draws. See the session TODO for installation/test status.

### Headset, lobby and controls

The startup headset list groups **Meta Quest Pro** with the other Meta headsets
at the top (saved as `hmd_type=19`, independently of its position in the list).
For Steam Link, select the **SteamVR** runtime and start with **2080 x 2104 (Balanced)**.
Quest Pro uses conservative render sizes shared with the existing Quest 2 assets;
they are not panel-resolution or exact lens-aspect presets. OpenXR supplies the
actual view angles and IPD. Sizes without packed VRCAM assets appear only with
engine stereo enabled. Existing headset IDs and saved selections are unchanged.

The lobby composition check recognizes the shorter grading layout observed on
game 2.5 with an RX 7900 XT. Its unused tail varies between zeros and stack data;
checking it caused a flat SBS panel, then flickering between immersive VR and a
flat panel as `WorldShown` expired. The shorter layout is accepted only inside
the known DrawHUD rendering node, with the weight, target-size, and grading
checks retained. Other nodes still require the original full grading layout.
Captured zero and changing-tail frames are covered by
`tools/lobby_composition_tests`. CyberFinger pointing works in the headset;
verification of the final flicker correction is pending.

`config/cyberfinger.ini` with `enabled=1` opts into the profile at startup. With it
absent or disabled, the original input behavior is retained.

Finger tracking supplies grab independently of the physical grip button. Open
the hand once, then close at least three fingers to grab a nearby target. The
existing finger filter and fist hysteresis are retained. A release must persist
for 100 ms; invalid hand pose/joints can hold an existing grab for at most 250 ms.
After a longer loss, open the hand before another grab. Missing skeletons produce
no grab rather than silently reusing the physical utility button.

World interaction is restricted to the existing 32 cm palm radius, including
when the game's sticky loot prompt names a different object. Remote pull is
disabled in this profile. Holsters, reload parts, ladders and other physical
interactions retain their own target distances and ownership rules. This adapts
the deliberate close-fingers/nearby-target behavior; it does not load the
Resonite ProximityGrab DLL or implement its precision pinch-point grabs.

Four additional OpenXR actions expose inventory, weapon selection, scanner and quick heal
to SteamVR's binding editor. These are custom app bindings, not a new OpenXR
interaction profile. There are no guessed CyberFinger paths in the Index profile.
These named actions retain their meaning in every handedness mode; only physical
button mappings follow the selected mirroring option.

FBT now reads eight roles through `XR_HTCX_vive_tracker_interaction`: waist,
feet, chest, knees and elbows. The `slime_joints` option enables the extra five
targets. Knees and elbows steer the existing IK bending planes while preserving
hand/foot endpoints and orientation. The chest divides the spine solve into a
lower and an upper chain, with the headset still the head target. An unreachable
chest target falls back to the original spine solve. Missing optional trackers
fall back individually. Existing cutscene/vehicle/ladder/swimming suspensions
still apply. Chest orientation participates in the solve, but is not a hard
constraint: reaching the head target takes priority.

## Recommended controls and SteamVR binding

Use SteamVR as the OpenXR runtime. The CyberFinger driver checkout now ships
**CyberFinger CyberpunkVRPort** bindings and registers the expected direct/GOG
executable key and Steam app 1091500. See its `docs/cyberpunk-vr-port.md` for
packaging, custom launch aliases and the optional pink-pause layout. The actual
GOG OpenXR launch key observed on this machine is
`system.generated.openxr.cyberpunkvrport.cyberpunk2077.exe`; it is now registered
in the driver sources too. Without that alias SteamVR used an automatic Index
mapping, omitting the four dedicated CyberFinger actions. Install the updated
binding assets and select the CyberFinger layout for this application. A personal
saved binding takes priority over a driver default. The bindings require this
patch with `enabled=1`.
They preserve both grip/aim poses and haptics; finger joints come independently
from `XR_EXT_hand_tracking`, not invented OpenVR skeleton actions.

If hands move but fingers do not, check the mod log for
`[finger-grip] left/right hand: finger data from hand tracking`. Extension
availability alone does not prove joints are arriving. In an earlier run the
driver log showed live 31-bone skeletons but the game logged no valid finger
samples. After the user uninstalled Ultraleap and restarted SteamVR, the game
logs valid finger samples for both hands; visible animation/grabs remain to test.
An Ultraleap implicit OpenXR layer was also observed during a runtime
probe; interference is a hypothesis, not yet a confirmed cause. After closing
the game, `launch_cyberfinger_native_hands.cmd` starts the GOG executable with
Ultraleap bypassed for that process only, using its
[documented disable variable](https://docs.ultraleap.com/openxr/configuration/disabling-the-layer/index.html).
It does not change the registry, SteamVR settings or other apps. Compare finger
tracking with an ordinary launch before treating this as the fix.

For snap turning, open **F10 > Controls > Turning (right stick)**, enable
**Snap turn**, choose **Snap angle**, and click **Save**. The file setting is
`xr_snap_turn=1`; the inspected configuration had it set to 0.

When editing manually, edit the **CyberpunkVRPort** application's binding. The action
labels below are those created by the port; SteamVR may display per-hand action
variants. Bind each input to its physical hand for both weapon modes. The results
below assume **Same as right-handed** when playing left-handed. The stick rows
describe movement with the gun holstered. See the
[illustrated controls](../README_CYBRFNGR.md#buttons-at-a-glance) for each context.

| Physical control | Action to bind | Result |
|---|---|---|
| Left stick axes | Thumbstick, left | Move; full forward does not start sprint |
| Right stick axes | Thumbstick, right | Turn; up dash, down crouch |
| Left stick click | Thumbstick Click, left | Click to sprint until the stick is released; hold remains a D-pad modifier |
| Right stick click | Primary Button (A/X), right | Jump/confirm; hold supports charge jump |
| Left trigger | Trigger, left | With a gun on foot or driving, aim for a right-handed weapon or fire for a left-handed weapon. While driving unarmed, sound the horn for RH or draw for LH |
| Right trigger | Trigger, right | With a gun on foot or driving, fire for a right-handed weapon or aim for a left-handed weapon. While driving unarmed, draw for RH or sound the horn for LH |
| Left physical grip (`/input/grip/click`) | CyberFinger Scanner (`cf_scanner`) | Toggle scanner in exploration, X in native menus, context action in VR menus/F10. In right-handed melee, toggle fists/weapon. |
| Right physical grip (`/input/grip/click`) | Thumbstick Click, right | With a firearm on foot, tap to crouch or hold 0.65 s for slide release/hammer. While driving, cycle camera view. In left-handed melee, toggle fists/weapon. |
| Left Menu button (`/input/b`) | CyberFinger Weapon Wheel (`cf_weapon`) | Tap next weapon, hold weapon wheel |
| Right Menu button (`/input/b`) | CyberFinger Inventory (`cf_inventory`) | Tap inventory; hold 0.65 s for OS cyberware |
| Left black wrist (`/input/a`) | Primary Button (A/X), left | Quick reload for a right-handed weapon, eject for a left-handed weapon |
| Right black wrist (`/input/a`) | Secondary Button (B/Y), right | Eject for a right-handed weapon, quick reload for a left-handed weapon |
| Left black wrist, hold (`/input/a_hold`) | Menu Button | Pause with either weapon hand |
| Right black wrist, hold (`/input/a_hold`) | CyberFinger Right Black Hold (`cf_heal`) | Heal with either weapon hand |
| Left pink wrist (`/input/pink`) | No game action; existing compositor binding | SteamVR dashboard |
| Right pink wrist (`/input/pink`) | CyberFinger VR Menu (`cf_settings`) | Open/close world-space VR menu; F10 remains keyboard-only |
| Close/open fingers near a target | No SteamVR gesture binding | Grab/release |

Use the **raw** physical grip click, not `/input/grab`: the latter has CyberFinger's
tap-to-hold behavior. Remove the old source assignments when replacing them, so
right stick click does not also emit R3 and Menu does not also emit B. Leave
pinch/fist gestures unassigned in SteamVR; the port evaluates the joints itself.

Tap **right pink wrist** to open/close the world-space VR menu. Hold **both
Menu buttons** for half a second to toggle the larger HUD; release both before
repeating. Left-stick + right-Grip is retired. The old
fist/cyberware chord is disabled for active CyberFinger controllers; use right
Menu hold instead. In original Cyberpunk menus, including the wardrobe, either
hand uses Trigger for A/select, Menu for B/Back on release, and physical Grab for
X (held when requested). Ray selection uses a mouse click instead of sending a
second A activation. Both-Menu suppresses B. The world-space VR menu and F10 use
Menu for Back/cancel/close and Grip for context actions, with hold-Grip on F10's
title handle still moving the panel. The raw left/right Menu state is preserved
before gameplay tap/hold conversion and published in `VRMenuPointer`'s optional
CyberFinger metadata. Closing fingers remains a
nearby-object gesture, not a menu click. These context changes require the
matching plugin; the v2 raw SteamVR bindings are unchanged. Black-wrist raw
actions are resolved by weapon hand before merging as named game actions, so
optional face-button/stick mirroring cannot turn eject into weapon switch or
reload into jump. Physical left black hold pauses and right black hold heals. The right stick is the analog accelerator/brake in the driver seat. Hold the physical left Grip button for 0.4 seconds to exit a vehicle with either weapon hand.
See [the quick reference](../README_CYBRFNGR.md) for context-specific controls.

The default leaves left pink for the SteamVR dashboard through its compositor
binding. Older optional `*_pink_pause.json` variants pair with the driver's
`vrcompositor_cyberfinger_black_hold.json`, but do not implement this new wrist
layout. Use the default v2 binding for the controls above. Neither optional
layout is registered by default. The
bridge's **Right pink button** option must be **SteamVR**, so it reaches the app
instead of being consumed by mic mute/FluxAction. Black wrist short presses are
reported on release by this driver; holding them emits the separate `a_hold`
input, resolved by weapon hand by the plugin. No firmware change or rotary dial is
needed: “dial” here means the existing Resonite Menu button. Left and right Menu
are separate physical inputs and can be rebound independently in SteamVR.

## Weapon hand

Keep both triggers bound to their physical hands in SteamVR. The port reads
**Controls > Left-handed controls** from `config/vrport.ini` and assigns the
gun, support hand, reload and wrist shortcuts in game.

| Weapon hand | F10 setting | Firearm controls on foot |
|---|---|---|
| Right | Left-handed controls off (`xr_left_handed=0`) | Right Trigger fires, left aims and left stick moves. |
| Left | Left-handed controls on (`xr_left_handed=1`), **Same as right-handed** (`xr_left_handed_bindings=0`) | Left Trigger fires, right aims and right stick moves with the gun drawn. Holster the gun to return movement to the left stick. |

The installer seeds the left-handed setting. Both modes use the same on-foot
Menu shortcuts and driver-seat pedals.

If desired, `xr_left_handed_bindings=1` also mirrors ordinary face buttons, and
`2` also mirrors sticks/clicks so movement is on the right stick. Those modes
change native pad mappings. Black wrist taps follow the weapon hand. Holds use fixed physical sides. Dedicated `cf_weapon`,
`cf_inventory`, `cf_scanner` and `cf_heal` actions keep their named functions.

## Configuration

### Installer

Run `install_cyberfinger.cmd` from this checkout. It uses Windows PowerShell 5.1,
detects GOG and Steam game folders, and asks you to choose when it finds several
copies. Use `-GameRoot` to select a folder explicitly. The default preset enables
CyberFinger and eight-role SlimeVR with a left-handed weapon. Install SteamVR
drivers and RED4ext separately.

With a complete **patched** release package already extracted:

```powershell
.\install_cyberfinger.cmd -Package .\dist\CyberpunkVRPort-0.2.3-cyberfinger -WhatIf
.\install_cyberfinger.cmd -Package .\dist\CyberpunkVRPort-0.2.3-cyberfinger
```

Without `-Package`, the installer prepares and builds this checkout automatically
in `build\cyberfinger-release`, then assembles its package with
`scripts/build_dist.ps1`. Git, CMake 3.24+ and the Visual Studio 2022 C++ tools
with a Windows SDK must be installed. The first run downloads pinned dependencies
and takes longer; later runs reuse them. RED4ext 1.31.0 or newer must already be
installed in the game.

```powershell
# Prepare, build, package and install:
.\install_cyberfinger.cmd
# Prepare/build only, without changing game files:
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\prepare_cyberfinger.ps1
# A separate build directory can also be selected (inside this checkout):
.\install_cyberfinger.cmd -BuildDir build\my-cyberfinger-release
```

Preparation downloads the original [0.2.3 release](https://github.com/dariulone/cyberpunk-vr-port/releases/tag/0.2.3)
with its pinned SHA-256 checksum, checks its archive manifest against this
checkout, and supplies the missing packed asset archive and matching redscript
compiler. It builds the patched `cyberpunkvrport.dll` from source; it never uses
the release's stock game plugin. This avoids needing Rust/Cargo for the bundled
script compiler. Downloads are cached under `build/cyberfinger-bootstrap`.
The preparation script uses game-2.5 SDK revision
`a34f0ad0a9e4f37cbe1fe001e4ff68c2949d0665` from `wopss/RED4ext.SDK`, the upstream
used by this repository's patch-day tools. It keeps that checkout in the build
cache; the original 2.31 SDK submodule is not repointed.
The original Rust build remains available when configuring CMake without
`CVR_REDSCRIPT_COMPILER_DIR`.

`-Build` remains accepted for existing commands; source installs always build.
A source patch ZIP is not an installable `-Package`, and the focused test harness
is not the game DLL. Missing C++ tools or failed builds stop before any game
files change. The `.cmd` launcher prints a concise failure message.

Optional arguments: `-GameRoot "D:\Games\Cyberpunk 2077"`, `-WeaponHand Right`,
and `-NoSlimeVR`. The latter turns off the tracker/FBT preset. `-WhatIf` performs
read-only validation and previews the install; it also skips building and
packaging. Close the game before installing. Windows may require an elevated
terminal to write to Program Files.

The installer checks for the patched native action names and Reach script,
copies the current plugin layout and OpenVR shim, and merges the three INI
files. Existing calibration, captured grips, wrist placement and unrelated INI
keys are preserved; the selected render resolution is carried into the new
catalogue if still supported. First-launch game-setting behavior is left to the
mod's existing configuration.

Replaced files and conflicting loaders (`bin/x64/dxgi.dll` and the old VR plugin
DLLs) are backed up under `build/cyberfinger-install-backups/<id>/before`, outside
the game's DLL search paths. `install.json` lists paths and before/after hashes.
The installer verifies all backups before writing, checks installed hashes and
restores changed files if installation fails. Retain that backup directory for
manual recovery; a rollback can leave empty directories. It never launches the
game or installs a SteamVR driver. Identical reinstalls do nothing.

The PowerShell filesystem tests use synthetic packages and game folders entirely
inside this checkout:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\cyberfinger_tests\install_test.ps1
```

### Manual configuration

After building the plugin, copy `mods/config/cyberfinger.ini.example` to the
plugin's `config/cyberfinger.ini`. The companion
`mods/config/cyberfinger-vrport.ini.example` contains the **left-handed preset**.
Merge its keys below into the existing
`config/vrport.ini` (do not replace the whole file):

```ini
xr_input_actions=1
xr_xinput_hook=1
xr_finger_bones=1
xr_grip_from_fingers=1
xr_vive_trackers=1
xr_left_handed=1
xr_left_handed_bindings=0
```

The CyberFinger profile uses its bounded finger-grip policy even if the ordinary
grip-from-fingers toggle is later changed. Restart with `enabled=0` to return to
the original controller-grip policy. `xr_finger_bones=1` also enables visible
finger animation; the profile itself requests the tracking extension.

Enable legs, spine, arms and **SlimeVR chest, knees and elbows** in the F10 FBT
panel, or merge this into `config/fbt_settings.ini`:

```ini
legs=1
spine=1
arms=1
slime_joints=1
```

Optional targets also work without CyberFinger input enabled. Their settings
are saved by the existing FBT panel. Chest requires spine, and elbows require
arms; these in turn require legs and an in-game calibration.

## Your eight SlimeVR sensors

ResoFBTAutoSetup documents the physical arrangement as waist, chest, two thighs,
two ankles and two upper arms. SlimeVR turns those IMUs into **joint poses**; the
Cyberpunk port should receive those computed poses, not raw sensor rotations.

| SlimeVR output | SteamVR tracker role | Port use |
|---|---|---|
| `human://WAIST` | Waist | Hips |
| `human://LEFT_FOOT`, `human://RIGHT_FOOT` | Left Foot, Right Foot | Feet |
| `human://CHEST` | Chest | Torso target |
| `human://LEFT_KNEE`, `human://RIGHT_KNEE` | Left Knee, Right Knee | Knee direction |
| `human://LEFT_ELBOW`, `human://RIGHT_ELBOW` | Left Elbow, Right Elbow | Elbow direction |

Enable these computed outputs in SlimeVR and assign/check their SteamVR roles.
The port reads roles, not tracker connection order. The role-based transport is
the existing [OpenXR tracker extension](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_HTCX_vive_tracker_interaction.html).
Physical sensor locations such as upper arm and thigh are not the SteamVR roles
to assign here. ResoFBTAutoSetup is Resonite-specific; it is not loaded or used to
change SlimeVR settings by this patch.

Use SlimeVR's own mounting/yaw resets first. Then load a save and run the port's
existing three-step full-body calibration, with the chest tracker active during
step 2. The chest offset is captured in that same body frame. Knees and elbows
are already anatomical joints, so they provide bending directions without
strap offsets. If the chest was absent at calibration, it remains on the default
spine solve until recalibration; knees/elbows can join later. Do not use
`slime_joints=1` with physical Vive tracker positions as if they were anatomical
knee/elbow joints.

## Build and validation

The main build uses the repository's existing CMake and packaging workflow.
New `.cpp` files are picked up by its source glob. Ship the rebuilt native plugin
and the updated `CyberpunkVRPort_Reach/modules/world.lua` together. The profile
example is intentionally not enabled in ordinary release packages.

The focused harness uses the repository-pinned IK and RED4ext SDK plus OpenXR
headers. With submodules initialized and the usual dependency directory:

```powershell
cmake -S tools/cyberfinger_tests -B build/cyberfinger-tests -G "Visual Studio 17 2022" -A x64
cmake --build build/cyberfinger-tests --config Release --parallel 1
ctest --test-dir build/cyberfinger-tests -C Release --output-on-failure
```

Alternatively pass `CVR_FBT_IK_DIR`, `OPENXR_INCLUDE_DIR`,
`RED4EXT_INCLUDE_DIR`, and `IMGUI_INCLUDE_DIR` for separately downloaded sources.
Pass `LUA_SOURCE_DIR` pointing to official Lua 5.4 sources (the directory with
`lua.c`) to include the offline Lua test. The harness exercises
tracking loss, opening/closing and rearming, action scopes and button
combinations, both weapon hands and all three mirroring modes, limb
endpoints/orientations across 25 pole directions, singular
and invalid input, reachable chest/head targets and fallback. The Lua test runs
the actual world-interaction module with game services stubbed: it checks the
close edge, occupied hands, remote rejection, sticky loot prompts and the
disabled profile. It also compiles every changed C++ translation unit, including
the frame loop, XInput merge, initialization, calibration, body solver, menu and registration.

Validation in this workspace: all changed C++ translation units compiled with
MSVC; both CTest suites passed; `git diff --check` passed. The Lua test uses 5.4
with the LuaJIT `math.atan2` alias, not the in-game LuaJIT host. Dependencies used:
OpenXR 1.0.34, ImGui 1.91.1-docking, RED4ext SDK
`d02a1f329744cd4e39c6f7f3fdf63053731e1aff`, and IK
`39f65bef1f46b0ea945dbd46aafbe4c7aaf89b8d`.

Still required on hardware: confirm SteamVR exposes CyberFinger's actual hand
skeleton through OpenXR; save the app binding; check both Menu buttons and all four wrist buttons,
left-handed firing, right-hand support/reloads and switching weapon hand,
charged jump, scanner/menu input capture, nearby loot versus remote loot,
holsters, magazine/support grip, throwing with the release grace, chest lean,
knee/elbow rotation, tracker dropout, recalibration and cutscene suspension.
The 100 ms grab-release grace can affect throwing feel and needs tuning during
that playtest. The preparation workflow has now produced a fully linked Release
DLL using the game-2.5 SDK, and a complete package is available locally at
`dist/CyberpunkVRPort-0.2.3-cyberfinger`. The installer tests cover fresh-source
and fresh-process previews, configuration preservation, rollback and concise CLI errors.
Neither those tests nor the native build constitute an in-game or headset pass.
