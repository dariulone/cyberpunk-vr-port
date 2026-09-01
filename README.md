# CyberpunkVR Port

A 6-DoF **VR mod for Cyberpunk 2077**, built as a **RED4ext plugin** -
`CyberpunkVR_Stereo` is the ONLY native plugin: it drives OpenXR head
tracking, real stereo, the in-headset overlay and the **full-body VR avatar with
motion-controlled hands** that used to live in a second DLL; and a set of CET / redscript mods add VR
weapon aiming, the physical reload, motion melee, hand-to-holster equipping and more. Everything is
configured from an in-headset **F10** overlay.

Repository: <https://github.com/dariulone/cyberpunk-vr-port>

> ⚠️ Experimental community mod. Not affiliated with CD PROJEKT RED. Use at your
> own risk and keep backups of your saves.

## Credits

- **[iPowerTech](https://github.com/iPowerTech)** — **VR driving.** Grabbing the
  wheel with your own hands, the steering geometry, the deadzone and lock-angle
  controls, the horn on the wheel hub, and shooting while you drive are all his
  work, from [his fork](https://github.com/iPowerTech/cyberpunk-vr-port).
  

## Features

- **Real stereo, not reprojection.** The second eye is an actual engine view — a
  render-to-texture camera on the player entity that runs the frame graph for its
  own eye, from its own position, with its own projection. It falls back to mono
  automatically whenever that view has nothing fresh to give (menus, loading).
- **OpenXR head tracking** injected into the REDengine render path, with the
  submitted frustum matching the one the engine actually rendered on both axes,
  plus world-scale / IPD controls. Off-axis lenses (Quest 3 and family) get a
  frustum sized to COVER the panel rather than match its span, so there is no
  black band down the outer edge.
- **The game HUD in both eyes** — the engine's own HUD composite is ported
  shader-for-shader for the second eye. By default it is pasted at the same
  PIXEL in both eyes so markers land in the same place; the finite-distance
  placement is still there behind one setting, because wide-FOV headsets want it.
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
- **Physical body rotation** (optional) — the character turns to follow the
  headset through the engine's own heading channel, so the mesh, the collision
  capsule, the aim and the movement direction all move with it. The view does not
  move and the hands stay on the controllers.
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
  firing from their own barrel. Optional barrel dot in both eyes, scope-zoom
  aware.
- **Recoil in the hands, not the camera** — the shot turns the wrist on a damped
  spring, and every weapon kicks by its own measured amount.
- **A resting hand and a real two-handed hold** — with a weapon out the empty
  hand relaxes instead of clawing at nothing, and bringing it to the weapon
  offers the support grip: squeeze and the wrist locks on, the off hand steers
  the aim, and the kick drops. Twelve weapons have a captured hold.
- **Collimated reflex sights** — the reticle is placed by angle along the sight's
  own optical axis, so it stays on the bore instead of sliding across the glass
  when you look at the sight from the side.
- **VR motion melee** — real swings trigger the game's native melee along the
  blade (native damage/reaction/stamina).
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
- **13 headsets, 60 resolutions**, every ladder reaching 6000 px, picked before
  launch — PlayStation VR2 and the Bigscreen Beyond 2/2e included.
- **HUD placement is HUDitor**, which moves and scales each
  widget individually. The port shipped its own HUD mod until 2026-08-20 and it
  is gone: it scaled the shared HUD root around screen centre, which is too
  blunt to be comfortable and actively fights a real HUD editor. What the port
  still does for the HUD is the part only it can do -- compositing the engine's
  own HUD into the SECOND eye. The port's own HUDitor setup -- the editor moved
  to **F11**, plus a VR-tuned layout for all 26 widgets -- is saved in
  `mods\config\huditor\`, opt-in rather than installed for you.
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
- **The cascade shadow rows are hidden** from *Graphics → Advanced*. The atlas is
  shared between the two views, so raising them gives you artefacts the port
  cannot fix from its side.
- **In-headset F10 overlay** with tabbed, live, persisted settings.
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
- HUDitor

Recommended but not required: **HUDitor**, **Visible Bullets**, for HUD placement — the port ships a
VR-tuned layout for it in `mods\config\huditor\` and no longer moves HUD widgets
itself.

Install RED4ext, CET and redscript first (the usual Nexus dependencies).

## Installation (drop-in)

Download the release archive and extract its contents into your **Cyberpunk 2077
game root** (the folder that contains `bin\`, `r6\`, `red4ext\`). The files land
as:

```
red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_Stereo.dll     # the VR plugin: OpenXR, stereo, overlay, VRIK
red4ext\plugins\CyberpunkVR_Stereo\CyberpunkVR_Sight*.dxil    # sight shaders, loaded by name at PSO swap
                                                              # NOTE: CyberpunkVR_Hands.dll is GONE -- its
                                                              # code is inside CyberpunkVR_Stereo.dll now.
                                                              # An old copy left in red4ext\plugins makes
                                                              # two plugins detour one address, which
                                                              # crashes with a fault at FFFFFFFFFFFFFFFF.
archive\pc\mod\cyberpunkvrport.archive                        # held props, weapon assets, the VRCAM cameras
archive\pc\mod\VRCigarette.archive.xl, vrport_basketball...   # the smoking and basketball props
bin\x64\plugins\cyber_engine_tweaks\mods\CyberpunkVRPort_*\   # CET mods -- see the table below
r6\scripts\CyberpunkVRPort_*\                                 # redscript mods -- see the table below
r6\input\HUDitor.xml, r6\config\...                           # the HUDitor binding and the port's config
```

Then **start your OpenXR runtime first**, and launch the game.

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
| Left stick **up / down, to the stop** | Page the quickhack list *while the scanner is open* |
| A | Jump (double jump and charge jump unchanged) |
| B, **weapon in hand** | **Drop the magazine** (physical reload) — not dodge |
| B, **holstered** | The game's own B again — close the phone, back out |
| B, **phone / radio / vehicle list open** | Closes it, weapon in hand or not — and cannot drop a magazine for a second afterwards |
| X / Y | Reload·interact / Weapon switch |
| Left menu button | Pause menu |
| Swing a melee weapon | VR motion melee (native attack along the blade) |

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

In **menus** and in **braindances** the pad is vanilla: none of the remaps below
apply there, including the grips-as-shoulders that used to page menu tabs.

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
| Perspective toggle | **Does nothing** — the camera is held in first person |
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
- `F10` / `Insert` — open the in-headset settings overlay
- `F11` — HUDitor's editor, if you installed it (the port rebinds it off F7)

## In-headset overlay (F10)

Four tabs, live, and saved to `vrport.ini` — nothing here needs a restart. The
**driving** block sits under Controls: wheel grab and its grab radius, steering
deadzone and full-lock angle, horn on/off with its hub radius,
trigger-fires-the-gun with its throttle trim rate — plus a live read-out of what
is grabbed, what the steering is doing and whether the horn is being pressed.

- **General** — world scale, IPD scale, stereo separation, VR menu FOV and quad
  size, motion prediction, reuse-last-clean-frame, pose pair-lock, and the head
  offset (X right / Y forward / Z up), with a second set that applies only in a
  vehicle.
- **Controls** — decoupled weapon aim and its laser dot, locomotion source
  (Game / HMD / left hand / right hand), snap turn and angle, immersive holsters,
  the driving block, and the current binding list.
- **Stereo** — the second eye itself: which eye VRCAM is sent to, how stale its
  last frame may get before the submit falls back to mono, the HUD composite, and
  the live counters that say whether the second view is producing, being captured
  and reaching the headset.
- **VRIK** — start/stop tracking, physical body rotation and its free-look cone,
  the cutscene-suspend tier, IK calibration (reach scale, height, elbow
  swing/pole, wrist offset), diagnostics.

The launcher (before the game starts) picks the render resolution and carries a
**DEBUG** tick-box that arms every diagnostic probe at once. Leave it off for
play: it is for diagnosis and it costs both frame time and a very large log.

## Mod components

| Component | Type | Purpose |
|---|---|---|
| `CyberpunkVR_Stereo.dll` | RED4ext plugin | OpenXR head tracking, the second engine view, HUD composite, sight shaders, F10 overlay, XInput merge |
| `CyberpunkVR_Stereo.dll` | (same plugin) | Full-body avatar / hand IK, hand recoil, weapon-aim and muzzle override, smoking and reload poses, shared-memory bridge -- was `CyberpunkVR_Hands.dll` until the single-plugin build |
| `cyberpunkvrport.archive` | archive | The magazine/prop carriers on the player templates (Johnny's replacer body included), the weapon assets the reload drives, the case-ejection effects, the 62 VRCAM camera components, and the UI layouts fitted to the square view — phone, vehicle panel, radio, subtitles, dialogue choices, tutorial popups |
| `CyberpunkVRPort_Stereo` | CET | Enables the VRCAM component the launcher picked |
| `CyberpunkVRPort_VRIK` | CET | Starts hand tracking, bridges calibration, publishes the locomotion state |
| `CyberpunkVRPort_Weapon` | CET | Decoupled weapon aim + VR motion-melee detection |
| `CyberpunkVRPort_Reload` | CET | The physical reload — thirteen per-weapon configs, the magazine, the slide, the revolver cylinder |
| `CyberpunkVRPort_HandCollision` | CET | Hand / finger / weapon push-out. **Off since 0.1.2** (`COLL_ON = false`) — the body capsules came back out of the archive after a vehicle launched the player |
| `CyberpunkVRPort_Crosshair` | CET | Hides the game's own crosshair while decoupled VR aim is on |
| `CyberpunkVRPort_Holster` | CET + reds | Hand-to-holster equip/unequip (immersive / simple) |
| `CyberpunkVRPort_Smoking` | CET + reds | Cigarette / lighter props, FX, audio, auto-puff |
| `CyberpunkVRPort_Basketball` | CET + reds | VR basketball with real ball values (the physics half is redscript) |
| `CyberpunkVRPort_WorldMap` | CET + reds | World-map head-lock |
| `CyberpunkVRPort_Hold` | reds | Puts a real item in a hand — `AddItemToSlot` works from redscript and silently does nothing from CET. This is how the Overture's speedloader is carried |
| `CyberpunkVRPort_Melee` | reds | Native melee along the blade segment |
| `CyberpunkVRPort_WeaponUp` | reds | Stops auto-lower / auto-unequip of a drawn MELEE weapon |
| `CyberpunkVRPort_NoAnims` | reds | Disables VR-fighting animations (keeps gameplay systems); also carries no-auto-reload and the firearm half of no-auto-lower |
| `CyberpunkVRPort_LootUi` | reds | The loot window for the square view — the plate at 0.70 and the detailed description centred above the take button instead of off the right edge |
| `CyberpunkVRPort_BraindanceHud` | CET | The braindance notification plate, whose slot is declared in a HUD asset the port does not ship |
| `CyberpunkVRPort_SettingsGuard` | reds | Removes the two cascade shadow rows from the settings menu |

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
