# CyberpunkVR Port

A 6-DoF **VR mod for Cyberpunk 2077**, built as a **RED4ext plugin** -
`CyberpunkVR_Stereo` is the ONLY native plugin: it drives OpenXR head
tracking, real stereo, the in-headset overlay and the **full-body VR avatar with
motion-controlled hands** that used to live in a second DLL; and a set of CET / redscript mods add VR
weapon aiming, the physical reload, motion melee, hand-to-holster equipping and more. Everything is
configured from an in-headset overlay opened by **holding L3+R3** or pressing **F10**.

Repository: <https://github.com/dariulone/cyberpunk-vr-port>

> ⚠️ Experimental community mod. Not affiliated with CD PROJEKT RED. Use at your
> own risk and keep backups of your saves.

## Features

- **Real stereo, not reprojection.** The second eye is an actual engine view — a
  render-to-texture camera on the player entity that runs the frame graph for its
  own eye, from its own position, with its own projection. It falls back to mono
  automatically whenever that view has nothing fresh to give (menus, loading).
- **OpenXR head tracking** injected into the REDengine render path, with the
  submitted frustum matching the one the engine actually rendered on both axes,
  plus world-scale / IPD controls. Each frame is submitted with its per-eye depth
  (`XR_KHR_composition_layer_depth`), so the runtime's reprojection knows where
  things are; generated frames carry none.
- **Each eye renders its own lens window** (asymmetric projection, on by default,
  *Stereo → Asymmetric projection*): exactly the headset's off-centre field of
  view, at full pixel density and with square pixels, instead of a symmetric
  frustum that renders past the lens. Culling, occlusion, light lists and sun
  cascades follow each eye's window. The screen-space passes that assumed a
  centred monitor camera are corrected by shader twins — volumetric fog,
  screen-space and planar water reflections, cone AO, sun contact shadows,
  particles (they face the eye, not the screen), screens and holograms, and the
  Sandevistan — and the two eyes' reflections, AO and contact shadows are merged
  as one wide display at their inner edges. Exposure is metered from the place
  around you, so it no longer swings when you turn your head, and the sun-shadow
  cascade boundary no longer sweeps across the ground. With the switch off,
  off-axis lenses (Quest 3 and family) get a frustum sized to COVER the panel.
- **Native VR HUD panels** with per-element placement, size and visibility,
  adjustable free-look cones, and separate interaction/dialogue and loot panels
  (head-locked by default). Scanner, CCTV, Basilisk and sniper displays have
  dedicated head-following channels; hit and kill markers have their own
  head-locked window. World markers retain their native depth and are projected
  separately for each eye. Popups that lay one flat picture over the frame
  (phone, radio, vehicle list, messages, shards) no longer double it.
- **Native VR frame generation** with game motion vectors and depth: NVIDIA DLSS
  Frame Generation through the game's own `nvngx_dlssg.dll` (the default),
  FidelityFX with or without optical flow, and NVIDIA OFA + FidelityFX, with
  adaptive pacing and a performance overlay. Inputs come from DLSS, DLAA, native
  TAA, FSR 2, FSR 3, FSR 4 and XeSS; on non-NVIDIA GPUs optical flow falls back to
  FidelityFX. Ray-tracing motion formats and live RT changes are supported. CAS
  sharpening processes real captures before frame generation, not interpolated
  frames. CAS defaults to off and is bypassed automatically while game DLAA is active.
- **Upscalers per eye.** FSR 3, FSR 3.1/4 and XeSS get a context per eye, so
  neither eye accumulates the other's history (blur and an IPD-wide ghost).
- **Optional GPU savers**: fixed foveated DLSS, fixed peripheral VRS, a
  hidden-area mask for the lens edges, and sun shadows rendered once for both eyes.
- **The F10 overlay in both eyes**, so a settings panel no longer costs you
  stereo the way a full-screen game menu does.
- **The two views agree.** Sun cascade shadows, the shader clock, the foliage
  wind volume and the reflection march are all shared engine structures that the
  second view used to rebuild from its own frustum — which is where blinking
  shadows, dead flags, jittering vegetation and a mirror-finish ceiling came
  from. All four are lent from one view now.
- **The second view builds its own frame, rather than being handed MAIN's bytes.**
  It keeps its own depth range (borrowing MAIN's near emptied the clustered light
  grid and put every lamp out indoors), the engine's own environment-override pass
  is *run* for it instead of copied — which is what brings the scanner's green tint
  back — and its composition is lent so the pass that draws the scanner outline
  executes for it at all. The finished frame is then taken at the node epilogue,
  from the engine's own acquire barrier, instead of mid-chain.
- **Every view is identified by name, never by elimination.** The port used to call
  anything that was not the second eye "MAIN" — including the flat desktop window
  and every reflection-probe face — and that one habit produced a white second eye,
  a hang on ESC in a braindance, and a braindance with no colour grade. Views are
  matched by their virtual-camera name hash now; one that cannot be named is passed
  through untouched.
- **Braindances are stereo.** A braindance renders through a camera no component
  points at, so the two eyes used to be built from different bases, at different
  rates, and the frame was submitted against a pose its pixels were never drawn
  from. One base, one head sample and one pose label serve both eyes, and the eye
  separation goes where the picture is actually rendered from.
- **Cameras you take over are yours.** A surveillance camera or the AV turret keeps
  its own aim as the base and the head composes on top, at the headset's own fov,
  with both eyes on the lens rather than one on the player's body. The turret's
  camera never reaches the hook the port classifies cameras at, so it is identified
  through the entity the script names and driven through the located buffer; there
  is a viewpoint slider for the mission that needs one.
- **Full-body VR avatar** (VRIK) — body under the HMD, arm-length calibration,
  leg IK, real-life squat. Hands are with the controllers, the shoulder girdle
  and elbows hang off the BODY rather than the head, and the solve is clocked by
  the engine's own animation batch. VRIK suspends itself during cutscenes so the
  avatar does not fight an authored scene.
- **Roomscale movement, bending and hybrid body rotation** with paired headset,
  camera and avatar poses, anatomical shoulder reach and preserved foot contacts.
- **Gesture swimming and grip-based ladder climbing**, including water-aware
  body posture, stroke-dependent propulsion and authored ladder hand poses.
- **Third-person and external-camera stereo**, plus HMD aiming and steering for
  the Basilisk and head-driven surveillance cameras and story sniper sequences.
- **Tactical belt and physical grenades**, with carried magazines, authored
  grenade grips and male/female belt assets.
- **Physical body rotation** (optional) — the character turns to follow the
  headset through the game's own motion, independently of the VR view, so the
  mesh, the collision capsule, the aim and the movement direction all move with
  it. The view does not move and the hands stay on the controllers.
- **Physical crouch** — squat in the room and the character crouches (on by
  default, 45 cm, *Controls → Physical crouch depth*); the VR camera stays put
  through weapon changes.
- **Paired animations get the body.** The game's own reload (the Reload button)
  eases both arms and fingers onto its animation and back. Grapples and
  takedowns, picking up, dropping and hiding a body, and finishers hand the body
  to their animation; while carrying, the left arm holds the body and the right
  hand keeps aiming. Roomscale stays as it is.
- **The physical reload.** Grab the magazine with your free hand's grip, pull it
  out along the path the weapon's own well defines, carry it, push it home; rack
  the slide with the other palm and release it with the right stick click. Ammo
  and sound follow the hands, not a timeline. **Thirteen weapons** are tuned
  individually — Unity, Tamayura, Tsunami Nue, Constitutional Liberty, Arasaka
  Kenshin, Militech Lexington, Militech Omaha, Kang Tao Chao, Tsunami Kappa,
  Arasaka Yukimura, Militech Ticon, Silverhand's Malorian, and the Malorian
  Overture revolver, which rolls its cylinder, ejects its cases and reloads from
  a real speedloader.
- **Decoupled VR weapon aim** — bullets follow the real weapon muzzle, not the
  camera. The hit is redirected inside the engine's own `PhysicalRay` evaluator,
  so shotgun spread stays spread and every other shooter in the world keeps
  firing from their own barrel. Projectiles leave the muzzle too. Optional
  barrel dot in both eyes, scope-zoom aware.
- **Recoil in the hands, not the camera** — the shot turns the wrist on a damped
  spring, every weapon kicks by its own measured amount (scaled by
  `xr_recoil_scale`, 0.7), and the head is never kicked.
- **A resting hand and a real two-handed hold** — with a weapon out the empty
  hand relaxes instead of clawing at nothing, and bringing it to the weapon
  offers the support grip: squeeze and the wrist locks on, the off hand steers
  the aim from where it was when the grip closed, and the kick drops. Pull the
  hand 60 cm off the weapon and it lets go.
- **Physical optics** — low-magnification scopes are real lenses in the headset
  instead of the flat-screen ADS overlay; sniper rifles look through a scope
  camera bound to the weapon; collimator reticles are placed by angle along the
  sight's own optical axis, so they stay on the bore instead of sliding across
  the glass when you look at the sight from the side. Bring your eye to the sight
  and it comes closer; no aim trigger needed.
- **VR motion melee** — real swings trigger the game's native melee along the
  blade (native damage/reaction/stamina).
- **VR cyberware** — mantis blades, gorilla arms, the monowire and the projectile
  launcher are driven by your arms, with free-hand gestures from the controllers.
  The monowire leaves the forearm housing and its line moves with the body.
- **Physical throws** — hold the left trigger with a throwable melee weapon,
  squeeze the right grip, throw and open the grip. The knife flies nearly
  straight, aimed mostly where you looked just before the throw; aim assistance
  prefers the enemy nearest that gaze and curves the flight on to it.
- **Controller vibration** — every shot by weapon class, melee blocks and parries,
  bullet blocks and deflections, the tactical belt and the grenade pin
  (*Controls → Controller vibration / Vibration strength*).
- **Hand-to-holster** equip/unequip on a grip squeeze — *immersive* (by visual
  holster) or *simple* (fixed weapon slots).
- **VR smoking** — cigarette and lighter as real props, with a captured
  finger grip, a hands-free mouth anchor and the game's own FX and audio.
- **VR controller mapping** merged into XInput: one movement speed whatever the
  stick deflection, sprint on a held detent, dash and crouch on the right stick,
  the scanner as a one-hand gesture, snap or smooth turn, HMD/hand-relative
  locomotion, and a D-pad chord. See **Controls** below.
- **VR driving** — squeeze a grip with your hand where the driving animation
  holds the wheel and that arm is handed back to the animation; the tilt of the
  line through your controllers is the steering, one-handed or two, with
  adjustable deadzone and lock angle. A hand on the hub sounds the horn, and
  drawing a weapon turns the right trigger into the gun while the throttle
  latches so the car keeps rolling.
- **World-map head-lock** — DLSS/NGX handling (the second
  view gets its own upscaler viewport automatically).
- **16 headsets, 125 resolutions**, every ladder shaped to that headset's measured
  eye window, picked before launch: Quest 2 / 3 / 3S, Pico 4 / 4 Ultra, Pimax
  Crystal OG / Light / Super / Ultra Wide and Dream Air, Valve Index, PSVR2,
  Bigscreen Beyond 2/2e, Steam Frame, HP Reverb G2 and MeganeX superlight 8K.
- **HUD placement is built into the port.** The HUD page controls native VR
  panels and their elements. HUDitor is not required and its old layout is not
  installed by the release package.
- **The game's own bindings work in menus.** Every controller remap this port makes
  is for gameplay, so a menu or a braindance now gets the raw pad — nothing of ours
  eats a button the menu needs. And B closes the phone, the radio port and the
  vehicle list even with a weapon in hand, without dropping your magazine: the
  physical reload's own button is held back for a moment after the popup goes.
- **The UI is fitted to the square VR view.** The phone, the vehicle summon panel,
  the radio, the subtitles, the dialogue choice list, the tutorial and welcome
  popups and the loot window are centred and scaled where a 16:9 layout would put
  them at the edge of vision or off it. The loot window's detailed description used
  to hang off the right edge; it sits above the take button now.
- **Cascade shadow range and resolution remain available** in *Graphics → Advanced*.
  Stereo sampling follows the active cascade count, and both views render their
  own shadow maps when settings recreate resources.
- **Shared MAIN fog with per-eye reprojection** keeps lamp beams fixed in the
  world during head turns. Underwater effects, colour grading and local-light
  radiance are synchronized without replacing the current-eye fog grid.
- **In-headset F10 overlay** with tabbed, live, persisted settings.
- Hold **L3+R3 for one second** to toggle the VR overlay. Hold **L3+left Grip
  for half a second** to activate iconic cyberware in gameplay. L3+R3 takes
  priority when both chords are pressed.
- SteamVR (OpenVR) runtime supported alongside OpenXR; pre-launch resolution
  selector; quiet-by-default logging with a DEBUG toggle in the launcher.

## Requirements

- Cyberpunk 2077 (PC, 2.31).
- Cyber Engine Tweaks
- RED4ext
- ArchiveXL
- TweakXL
- redscript
- Codeware (**1.20 or newer** — older builds fail script compilation)
- Visual Holsters (Automatic Clothes Swap)
- Visible Bullets (Projectile Restoration)
- Equipment-EX
- Nova Optics
- Input Loader
- [More Occluders (Scripted v0.3)](https://www.nexusmods.com/cyberpunk2077/mods/33136)

HUDitor is not required for the native VR HUD and is not bundled.

Install RED4ext, CET and redscript first (the usual Nexus dependencies).

## Installation (drop-in)

Download the release archive and extract its contents into your **Cyberpunk 2077
game root** (the folder that contains `bin\`, `r6\`, `red4ext\`). The files land
as:

```
red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_Stereo.dll     # the VR plugin: OpenXR, stereo, overlay, VRIK
red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_*.dxil         # sight, scope and outline shaders, loaded by name
red4ext\plugins\CyberpunkVR_Stereo\asym*\, view_dir\, screen_fx\  # shader twins for the per-eye lens windows
red4ext\plugins\CyberpunkVR_Stereo\UserSettings.json          # the first-launch VR preset
                                                              # NOTE: CyberpunkVR_Hands.dll is GONE -- its
                                                              # code is inside CyberpunkVR_Stereo.dll now.
                                                              # An old copy left in red4ext\plugins makes
                                                              # two plugins detour one address, which
                                                              # crashes with a fault at FFFFFFFFFFFFFFFF.
archive\pc\mod\cyberpunkvrport.archive                        # ALL packed assets: VRCAM + optics cameras, props, weapons
archive\pc\mod\cyberpunkvrport.archive.xl                     # its ArchiveXL manifest (prop + smoking factories)
                                                              # NOTE: older releases shipped VRCigarette.archive.xl
                                                              # and vrport_*.archive separately; they are merged into
                                                              # cyberpunkvrport.archive now -- delete the old files.
bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_*\   # CET mods -- see the table below
r6\scripts\CyberpunkVRPort_*\                                 # redscript mods -- see the table below
r6\input\CyberpunkVRPort_ScannerHud.xml                      # scanner editor binding for Input Loader
r6\tweaks\vrport\, r6\tweaks\vrcigarette\                     # TweakXL records (belt, props, grenades, smoking)
bin\x64\CyberpunkVR_*Grip*.ini                                # captured hand poses (smoking, rest, two-hand holds)
engine\config\platform\pc\vrcam_cpu_tweaks.ini                # streaming distances and culling for the two views
```

Then **start your OpenXR runtime first**, and launch the game.

On first launch, the VR preset is merged into UserSettings.json with a timestamped
backup. Graphics, all input controls, HUD and comfort values come from the preset;
language, audio, difficulty, telemetry, cloud saves and gallery favorites are
preserved. The merge runs once, not whenever graphics settings change.

When upgrading, remove the retired `CyberpunkVRPort_LootUi` and
`CyberpunkVRPort_SettingsGuard` folders from `r6\scripts`. From 0.1.7 or older,
also delete `VRCigarette.archive.xl`, `vrport_basketball.archive` and
`vrport_mag.archive` from `archive\pc\mod`. QuickBoot is excluded
from the release: remove an old development copy from CET's mods folder if present.
The launcher defaults to visible, but an existing `show_launcher=0` is respected.

> There is no `dxgi.dll` any more — this is a RED4ext plugin. Anything else that
> proxies dxgi (R.E.A.L. VR, for one) must be out of `bin\x64` or the two fight
> over the same engine hooks; `scripts\deploy_stereo.ps1` moves one aside for you.

> Keep only one `.dll` in each `red4ext\plugins\CyberpunkVR_*` folder. RED4ext
> loads **every** DLL it finds there, so a renamed backup beside the real build
> loads as a second copy of the plugin and the two fight over the same hooks.

From a source tree, install with:

```
cmake --build build --config Release --target cyberpunkvrport_stereo
pwsh scripts\deploy_stereo.ps1 -GameRoot "<game root>"
```

## Controls

VR controller input is merged into the native CP2077 gamepad, so the in-game
"Controller" key bindings apply. Default VR mapping, **on foot**:

| Input | Action |
|---|---|
| Left stick | Move — **one speed whatever the deflection**, direction from the stick |
| Left stick **fully forward, held 0.2 s** | **Sprint** — held, and it survives a dash |
| Right stick X | Turn camera (snap or smooth; a snap needs a FULL push) |
| Right stick **fully up** | **Dash** (dodge, direction from the left stick) — once per push |
| Right stick **fully down** | **Crouch** (R3) |
| Right thumb **click** | **Slide release** — racks the weapon (physical reload) |
| Left hand to your **left ear** + **left grip** | **Scanner**, *toggled* — squeeze to open, squeeze again to close |
| Right trigger / Left trigger | Fire / Aim (left trigger is also melee block) |
| Right grip | Hand-to-holster equip / unequip; melee power modifier |
| Left grip | **Grab the magazine** during a reload (at the ear it is the scanner) |
| **L3 + R3, held 1 s** | Toggle the in-headset VR overlay |
| **L3 + left Grip, held 0.5 s** | Activate iconic cyberware in gameplay; requires Input Loader |
| Left stick **up / down, to the stop** | Page the quickhack list *while the scanner is open* |
| A | Jump (double jump and charge jump unchanged) |
| B, **weapon in hand** | **Drop the magazine** (physical reload) — not dodge |
| B, **holstered** | The game's own B again — close the phone, back out |
| B, **phone / radio / vehicle list open** | Closes it, weapon in hand or not — and cannot drop a magazine for a second afterwards |
| X / Y | Reload·interact / Weapon switch |
| Menu button (left on Touch; right on Steam Frame) | Pause menu |
| Swing a melee weapon | VR motion melee (native attack along the blade) |
| Left trigger held + right grip, throw, open the grip | **Physical throw** of a throwable melee weapon |
| Squat in the room | Crouch (physical crouch; the right stick still works) |

Steam Frame uses the native `XR_VALVE_frame_controller_interaction` profile when
available. Right A/B/X/Y, left D-pad, left View, right Menu and both bumpers map
to the matching gamepad controls. Grips retain the port's VR gestures. Touch
emulation remains the fallback when the runtime does not expose the extension.

While the scanner is open the same hand works it: the **left stick to the stop**
pages the quickhack list (below the stop it still walks, so you can read and move),
**X** applies the selected hack, the **right trigger** tags the target instead of
firing, the **right stick click** changes the tab and **left trigger + right stick**
zooms. The list used to sit on X and Y, which meant reaching across the face buttons
with the hand that is already holding a grip at your ear — the very reach a one-hand
scanner exists to avoid.

B is the port's only while a weapon is actually in your hand, because a magazine
drop is the only thing the port needs it for. Holstered, it reaches the game — so
the phone and the radial close normally. And while the phone, the radio port or the
vehicle list is up it reaches the game as well, weapon or no weapon: those three are
overlays rather than menus, which is why they used to be impossible to close with a
gun out. Spamming that button is safe — the magazine drop stays blocked for a second
after the popup closes.

In **menus** and in **braindances** the pad is otherwise vanilla. Quest Touch grips
stand in for LB/RB in menus because the controllers have no physical shoulder buttons;
gameplay remaps stay off there, and grips are not remapped during braindances.

While **driving**, the same controllers do something else:

| Input | Action |
|---|---|
| Grip, hand at the wheel / handlebars | Grab it — that arm goes back to the driving animation |
| Tilt of the line through both controllers | Steering (one hand: that controller against the wheel centre) |
| Hand on the **middle of the wheel** | **Horn** — no grip needed; a hand that is grabbing never honks |
| Right trigger / Left trigger | Throttle / Brake |
| Right trigger **with a weapon drawn** | **Fire.** The throttle latches at the speed it had |
| Left stick forward / back, weapon drawn | Trim the latched throttle |
| **X, held** | **Get out.** B is never the exit in a car, so no stray press can eject you |
| Perspective toggle | Native view toggle; third-person and external cameras retain VR tracking |
| A | **Confirm a dialogue line** (it is X on foot, and X is the exit in here) — the handbrake still fires |

Each hand is independent, so you can hold the wheel with one and keep the other on
a gun. With a weapon equipped the right hand shoots and cannot grab the wheel;
holster it and the wheel is its again — and since reaching for a holster is how you
draw in the first place, that hand is already off the wheel by then. A grip that is
holding the wheel does nothing else: no holster equip, no magazine grab. Nothing
here needs to know which vehicle you are in, because the reference pose is the
game's own driving animation — bikes included.

The exit is a HOLD rather than a tap on purpose. The game's `ExitVehicle_Button`
carries no hold of its own, so the vehicle acts on the first frame it sees the
action — and at speed acting on it means throwing you out of the car.

**D-pad chord.** Hold the **left stick clicked in**, then pick the direction with
the **right stick** — up / down / left / right, **to the stop**, the same full push
every other gesture here asks for so that a resting thumb cannot step a list. While
the chord is held the right stick is taken out of the camera, so selecting a
direction cannot snap-turn you.
Release the left stick *without* having chosen a direction and it emits the normal
L3 press instead, so nothing is lost by using it.

Buttons follow each runtime's interaction profile (Touch / Index / Vive / WMR);
customise the actual actions in the game's *Settings → Key Bindings → Controller*.

Hotkeys:

- `F7` — recenter HMD
- `F8` — toggle the VR menu rectangle between full-HMD and a small panel
- `F10` / `Insert` — toggle the in-headset settings overlay from the keyboard
- `L3` + `R3`, held for 1 s — toggle the same overlay in the headset
- `L3` + left Grip, held for 0.5 s — activate iconic cyberware in gameplay

## In-headset overlay (F10 or L3+R3)

The controller-operated overlay saves settings to `vrport.ini`. Point, select,
scroll and grip-drag the panel without pausing gameplay. Separate pages cover
General, Framegen, HUD, Controls, Stereo, Avatar, Driving and Bindings.

- **General** — world scale, IPD scale, stereo separation, VR menu FOV and quad
  size, motion prediction, reuse-last-clean-frame, pose pair-lock, and the head
  offset (X right / Y forward / Z up), with a second set that applies only in a
  vehicle.
- **Fixed Foveated DLSS** in General — Off, Quality, Balanced and Performance,
  with independent center width and height from 20–100% of each eye image.
  The center follows the game's DLSS/DLAA settings; the profile controls the
  periphery. Horizontal and vertical offsets reposition the region, including
  toward the hands. Slider changes apply one second after release.
  Requires NVIDIA RTX and DLSS. No additional runtime or eye tracker
  is needed. The boundary preview marks the central region.
- Also in **General** — CAS sharpening, shared sun shadows, scope lens
  stabilization, variable rate shading for the periphery and the hidden-area mask
  for the lens edges.
- **Controls** — decoupled weapon aim and its laser dot, locomotion source
  (Game / HMD / left hand / right hand), snap turn and angle, immersive holsters,
  controller vibration and its strength, physical crouch and its depth, the
  driving block, and the current binding list.
- **Stereo** — the second eye itself: the asymmetric projection switch, which eye
  VRCAM is sent to, how stale its last frame may get before the submit falls back
  to mono, the HUD composite, and the live counters that say whether the second
  view is producing, being captured and reaching the headset.
- **Avatar** — start/stop tracking, physical body rotation and its free-look cone,
  the cutscene-suspend tier, IK calibration (reach scale, height, elbow
  swing/pole, wrist offset), diagnostics.
- **Framegen** - generation backend, pacing, performance history and overlay placement.
- **HUD** - per-element controls and independent follow cones for HUD channels.
- **Driving / Bindings** - steering geometry and prediction, wheel grip and horn,
  driving controls and the current controller mapping.

The launcher (before the game starts) picks the render resolution and carries a
**DEBUG** tick-box that arms every diagnostic probe at once. Leave it off for
play: it is for diagnosis and it costs both frame time and a very large log.

## Mod components

| Component | Type | Purpose |
|---|---|---|
| `CyberpunkVR_Stereo.dll` | RED4ext plugin | OpenXR head tracking, the second engine view, HUD composite, sight shaders, F10 overlay, XInput merge |
| `CyberpunkVR_Stereo.dll` | (same plugin) | Full-body avatar / hand IK, hand recoil, weapon-aim and muzzle override, smoking and reload poses, shared-memory bridge -- was `CyberpunkVR_Hands.dll` until the single-plugin build |
| `cyberpunkvrport.archive` | archive | Every packed asset: the magazine/prop carriers on the player templates and on every gameplay replacer, the weapon assets the reload drives, the case-ejection effects, the VRCAM camera components (one per supported resolution, 112) and the optics cameras, basketball, smoking, cyberware and crouch assets, on-screen effects with their picture displacement neutralised, and the UI layouts fitted to the square view — phone, vehicle panel, radio, subtitles, dialogue choices, tutorial popups |
| `CyberpunkVRPort_Stereo` | CET | Enables the VRCAM component the launcher picked; runs the Lua garbage collector in small per-frame steps |
| `CyberpunkVRPort_VRIK` | CET | Starts hand tracking, bridges calibration, publishes the locomotion state |
| `CyberpunkVRPort_Weapon` | CET | Decoupled weapon aim, VR motion-melee detection, cyberware combat and physical throws |
| `CyberpunkVRPort_Optics` | CET + reds | Scope, collimator and sniper-camera optics: path policy, authored profiles, the sight display apart from flat ADS |
| `CyberpunkVRPort_PhysicalCrouch` | CET | Real squats request the game's own crouch |
| `CyberpunkVRPort_WristHud` | CET | The living tattoo readout on the left forearm |
| `CyberpunkVRPort_ForceFPP` | CET | Camera bridge: publishes scene, device and UI state to the camera director (legacy folder name) |
| `CyberpunkVRPort_Reload` | CET | The physical reload — thirteen per-weapon configs, the magazine, the slide, the revolver cylinder |
| `CyberpunkVRPort_HandCollision` | CET | Hand / finger / weapon push-out. **Off since 0.1.2** (`COLL_ON = false`) — the body capsules came back out of the archive after a vehicle launched the player |
| `CyberpunkVRPort_Crosshair` | CET | Hides the game's own crosshair while decoupled VR aim is on |
| `CyberpunkVRPort_Holster` | CET + reds | Hand-to-holster equip/unequip (immersive / simple) |
| `CyberpunkVRPort_Smoking` | CET + reds | Cigarette / lighter props, FX, audio, auto-puff |
| `CyberpunkVRPort_Basketball` | CET + reds | VR basketball with real ball values (the physics half is redscript) |
| `CyberpunkVRPort_WorldMap` | CET + reds | World-map head-lock |
| `CyberpunkVRPort_Hold` | reds | Puts a real item in a hand — `AddItemToSlot` works from redscript and silently does nothing from CET. This is how the Overture's speedloader is carried |
| `CyberpunkVRPort_Melee` | reds | Native melee along the blade segment, enhanced melee and the physical throw |
| `CyberpunkVRPort_Cyberware` | reds | Cyberware combat, launcher aim and selection, monowire line and collision, breach and nameplate markers |
| `CyberpunkVRPort_BodyCarry` / `_Finisher` | reds | Tell the plugin when a carry or a finisher animation owns the arms |
| `CyberpunkVRPort_Haptics` | reds | Reads the guard's block / parry / deflect result for controller vibration |
| `CyberpunkVRPort_PopupVisualState` | reds | Holds back the popups' flat visual state while VR runs |
| `CyberpunkVRPort_DeviceCam` | reds | Computers and monitors do not take the camera away from the head |
| `CyberpunkVRPort_ScannerHud` | reds | Scanner layout editor (Right Shift, through Input Loader) |
| `CyberpunkVRPort_StoryAttention` | reds | Manual transition of the Ofrenda clue (adapted from Ajson44's fork) |
| `CyberpunkVRPort_WeaponUp` | reds | Stops auto-lower / auto-unequip of a drawn MELEE weapon |
| `CyberpunkVRPort_NoAnims` | reds | Disables VR-fighting animations (keeps gameplay systems); also carries no-auto-reload and the firearm half of no-auto-lower |
| `CyberpunkVRPort_BraindanceHud` | CET | The braindance notification plate, whose slot is declared in a HUD asset the port does not ship |
| `CyberpunkVRPort_TacticalBelt` | CET + assets | Belt, magazine props and physical grenade handling |
| `CyberpunkVRPort_Swimming` | CET + native | Water state and gesture-driven swimming |
| `CyberpunkVRPort_Ladder` | CET + native | Grip-based climbing and ladder hand poses |
| `CyberpunkVRPort_Driving` | CET + native | Native steering response while physically holding the wheel |

`CyberpunkVRPort_QuickBoot`, `CyberpunkVRPort_ReloadRecorder` and
`CyberpunkVRPort_WorldMapDiag` are development tools, excluded from the dist.

## Logs

- `Cyberpunk 2077\bin\x64\cyberpunkvrport.log` — the plugin's own log, and the
  right file for a bug report. Quiet by default; tick **DEBUG** in the launcher
  for per-frame diagnostics.
- `Cyberpunk 2077\red4ext\logs\` — script validation and plugin load errors. If
  redscript compilation fails, *every* redscript mod is off, not just the one that
  failed, so check here first when something stops working all at once.
- Per-mod CET logs live in each mod folder; they follow the same DEBUG switch.

## Test hardware used during development

- Headset: PICO 4 (via VDXR)
- CPU: AMD Ryzen 7 5800X
- GPU: NVIDIA RTX 5070 Ti
- RAM: 32 GB DDR4
- OS: Windows 11 Pro 25H2 (26200)

## Donations

Donating is your personal choice. It speeds up development and makes new features
possible — nobody is forcing you to do it.

- <https://boosty.to/dariulone>
- <https://dalink.to/dariulone>

| | |
|---|---|
| USDT TRC20 | `TRgmDeRcFumXvsSRqYV5kQAqRAvoFKXJCt` |
| USDT BEP20 | `0x4638c6580d1e684bdc60a1c415e5cb1522b66942` |
| TRX | `TRgmDeRcFumXvsSRqYV5kQAqRAvoFKXJCt` |
| BTC | `13AfpBwZvaezf36FmpjtENHTXjYcnzEsze` |

## Credits

- **[ClarkCheekyKent / CheekyFoveatedDLSS](https://github.com/ClarkCheekyKent/CheekyFoveatedDLSS)** —
  foveated DLSS research and the initial comparison backend.
- **[iPowerTech](https://github.com/iPowerTech/cyberpunk-vr-port)** — physical
  wheel steering, vehicle controls and shooting while driving.
- **[dabinn](https://github.com/dabinn/cyberpunk-vr-port)** — substantial work on
  weapon aiming, head aim and renderer stability; fixes adapted from his fork.
- **[Ajson44](https://github.com/Ajson44/cyberpunk-vr-port)** — extensive work
  on story progression, scripted cameras and quest interaction research.
- **[tig3rmast3r / OFXR Bridge](https://github.com/tig3rmast3r/OFXR-Bridge)** —
  open VR frame-generation research used as a reference for the native implementation.
