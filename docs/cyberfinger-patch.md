# CyberFinger / SlimeVR experimental patch

This is a source patch for this checkout, not an installed or hardware-validated
release. It changes the Cyberpunk port. Companion binding assets have also been
added to `D:\build\CyberFinger_SteamVRFusion`; its driver binary and bridge are
unchanged. ResoFBTAutoSetup and all game files remain unchanged.

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
GOG launch key still needs a runtime check; a personal saved binding takes
priority over a driver default. The bindings require this patch with `enabled=1`.
They preserve both grip/aim poses and haptics; finger joints come independently
from `XR_EXT_hand_tracking`, not invented OpenVR skeleton actions.

When editing manually, edit the **CyberpunkVRPort** application's binding. The action
labels below are those created by the port; SteamVR may display per-hand action
variants. Choose the indicated hand for the existing per-hand actions. This table
uses the **left-handed weapon preset**, with movement still on the left stick.

| Physical control | Action to bind | Result |
|---|---|---|
| Left stick axes | Thumbstick, left | Move; full forward retains sprint |
| Right stick axes | Thumbstick, right | Turn; up dash, down crouch |
| Left stick click | Thumbstick Click, left | Existing D-pad modifier/sprint click |
| Right stick click | Primary Button (A/X), right | Jump/confirm; hold supports charge jump |
| Left trigger | Trigger, left | Fire; throttle in vehicles |
| Right trigger | Trigger, right | Aim/block; brake in vehicles |
| Left physical grip (`/input/grip/click`) | CyberFinger Scanner (`cf_scanner`) | Hold scanner |
| Right physical grip (`/input/grip/click`) | Thumbstick Click, right | Existing R3: melee/slide release, context dependent |
| Left Menu button (`/input/b`) | CyberFinger Weapon Wheel (`cf_weapon`) | Tap next weapon, hold weapon wheel |
| Right Menu button (`/input/b`) | CyberFinger Inventory (`cf_inventory`) | Inventory/Back-Select |
| Left black wrist (`/input/a`) | Primary Button (A/X), left | Interact/reload |
| Right black wrist (`/input/a`) | Secondary Button (B/Y), right | Eject magazine/back, using existing context rules |
| Left black wrist, hold (`/input/a_hold`) | Menu Button | Pause |
| Left pink wrist (`/input/pink`) | No game action; existing compositor binding | SteamVR dashboard |
| Right pink wrist (`/input/pink`) | CyberFinger Quick Heal (`cf_heal`) | Use equipped healing consumable (D-pad Up; navigation in menus) |
| Close/open fingers near a target | No SteamVR gesture binding | Grab/release |

Use the **raw** physical grip click, not `/input/grab`: the latter has CyberFinger's
tap-to-hold behavior. Remove the old source assignments when replacing them, so
right stick click does not also emit R3 and Menu does not also emit B. Leave
pinch/fist gestures unassigned in SteamVR; the port evaluates the joints itself.

The resulting VR-settings chord is **left stick click + right physical grip**,
held for half a second (the port still receives L3+R3). The existing cyberware
chord remains left stick click + a left-hand fist. Physical grip and finger grip
are intentionally distinct.

The default leaves left pink for the SteamVR dashboard through its compositor
binding. Optional `*_pink_pause.json` game bindings pair with the driver's
`vrcompositor_cyberfinger_black_hold.json`: left pink then pauses and left black
hold opens the dashboard. Select both together; changing the dashboard layout
affects all apps. Neither optional layout is registered by default. The
bridge's **Right pink button** option must be **SteamVR**, so it reaches the app
instead of being consumed by mic mute/FluxAction. Black wrist short presses are
reported on release by this driver; holding them emits the separate `a_hold`
input, used for pause on the left in the default mapping. No firmware change or rotary dial is
needed: “dial” here means the existing Resonite Menu button. Left and right Menu
are separate physical inputs and can be rebound independently in SteamVR.

## Weapon hand

Use the port's existing **Controls > Left-handed controls** setting for the
actual gun/weapon hand. The CyberFinger preset sets it to **on** and sets
**Left-handed bindings** to **Same as right-handed**. Despite that binding
option's name, the weapon is left-handed: the left trigger fires and the right
trigger aims/blocks. It keeps the sticks and face buttons in the layout above.
The existing left-handed weapon, holster, support-hand, reload and recoil paths
follow the weapon-hand setting.

The equivalent `config/vrport.ini` keys are:

```ini
xr_left_handed=1
xr_left_handed_bindings=0
```

For a right-handed weapon, set `xr_left_handed=0` (or turn the checkbox off).
The right trigger then fires and the left aims/blocks, while the Menu and wrist
bindings remain as listed. Keep both triggers bound to their **physical** hands
in SteamVR; the port performs the swap once. No second handedness setting in
`cyberfinger.ini` overrides the in-game choice.

If desired, `xr_left_handed_bindings=1` also mirrors ordinary face buttons, and
`2` also mirrors sticks/clicks so movement is on the right stick. Those modes
change the jump/wrist mappings in the table. Dedicated `cf_weapon`,
`cf_inventory`, `cf_scanner` and `cf_heal` actions keep their named functions.

## Configuration

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
that playtest. This checkout has no complete native build tree and Cargo was
not available on PATH during this work; the focused harness is not a linked
game DLL or evidence of an in-game pass.
