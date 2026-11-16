# CyberFinger / Quest Pro: resume here

## Installed: upstream 0.2.45 integration candidate (2026-11-15)

The user requested a parallel integration branch, then asked to install the
merged candidate for testing. Worktree: `build/upstream-integration` under the
original checkout; branch `integration/upstream-0.2.45`. Original `main` remains
at `af74b88c`. See the [integration report](upstream-0.2.45-integration.md) for
upstream improvements, compatibility decisions and validation.

- Full Release build, 155 controller/render/UI checks and 15 installer scenarios
  passed. Two NVIDIA-only framegen tests cannot run on this AMD machine.
- The complete port plus installed script mods compile with both compilers,
  with no errors or warnings.
- Full package installed: 181 changed files, independently hash-verified.
- VR/CyberFinger/FBT settings and CET bindings are unchanged; CET and the
  SteamVR driver were not replaced.
- Package: `dist/CyberpunkVRPort-0.2.45-cyberfinger-upstream` in this worktree.
- Backup: `build/cyberfinger-install-backups/aaf21d0e41b34e3296f106000ddb0c52/`.
- DLL SHA256: `7D78E81EA2856E16C29ABE0797F492CE9ABF9FAB76E5CEA9F71EE7B8328E1A55`.
- Matching symbols: original checkout's `build/up245/bin/red4ext/plugins/cyberpunkvrport/Release/`.

Next: launch from GOG with SteamVR/Steam Link connected, load a save, check
loading stereo and hands, movement, menus/CET release and HUD toggle, then
combat and physical grabs. No headset or performance result is yet confirmed.
The historical installed loading-r1 notes below describe the pre-merge build;
do not use its DLL/PDB layouts against the newly installed candidate.

## Installed: save-load flicker correction (2026-11-15)

User clarified the failure: loading a saved game shows the transition on the
monitor, but the headset flickers uncomfortably instead of displaying the panel.
The earlier `[loading-screen] up` message proves activation only, not successful
headset presentation. No live reproduction has yet been recorded; game is closed.

Implemented corrections to concrete submission hazards:

- Capture carries Scene/Menu/Loading/Notice/Dark presentation with its pixels.
  XR submission uses that captured classification instead of live flags which
  may already describe the next frame. Loading projections cannot be rewrapped
  as a menu quad while their last image is being repeated.
- Second-eye pairing checks presentation type. Loading/menu/notice/dark require
  the same capture serial; a recently rendered gameplay eye cannot accompany
  a loading image, or vice versa. Normal bounded gameplay reuse is preserved.
- Loading/notice/dark fallback survives stalls longer than one second, until
  a new frame replaces it. Gameplay retains the existing timeout. Fallback is
  rejected after a tracking-origin change or swapchain/space replacement.
- Loading frames suppress separate gameplay HUD layers, depth and framegen.
- New `[xr-presentation]` log records successfully submitted mode transitions.

Full Release build and all 23 overlay tests pass, including loading policy,
a 45-second stall and eye pairing across transitions. Installed and independently
verified with the game closed; only the plugin DLL changed. Eight settings/
binding files, five melee-related Lua scripts and CET ASI are unchanged.

- Package: `dist/CyberpunkVRPort-0.2.3-cyberfinger-loading-r1`.
- Plan, installer, verification and matching DLL/PDB: `build/loading-frame-install/`.
- Backup: `build/loading-frame-install/backup-da90742e9c4644809b08997b59db3603/`.
- Installed DLL SHA256: `11D469AC67FED11E32D8E01B215C0C2AEED44FB5577A5D888DF6EA55553E6EBC`.
- Matching diagnostic layouts: `build/loading-frame-install/types.json`.

Read-only probe prepared at `build/loading-transition-probe.py`; resolves the exact DLL/PDB and
records loading-layer, capture, submit and loading-screen readiness. User was
asked to leave the game at its main menu for a recording, with headset off if
desired. Await reply; do not claim the headset flicker is confirmed fixed. After
resolving the new PID, run the probe with `--pid PID --dll
build/loading-frame-install/symbols/cyberpunkvrport.dll --seconds 45`, then tell
the user to load a save while it is recording. Old melee-r1 symbols no longer
match the installed DLL.

## Loading-screen panel investigation (2026-11-15)

User asked whether between-scene/loading screens can appear as an anchored panel
like Esc/map. Existing `Render/LoadingScreen` already does this for recognized
post-main-menu loading screens: 3 m ahead, 1.8 m tall, anchored on first valid
head pose. `Lobby::LoadingInSpace` and `EngineStereo::LoadingShown` select it;
capture draws the same panel through each eye's pose. Installed configuration
has engine stereo enabled. Latest game log (2026-11-15 18:26) confirms the layer
hook and a post-intro `[loading-screen] up: hung 3.0 m ahead` activation.

Historical investigation before the user's save-load clarification above. The game was
closed. Asked what the user sees (head-following oversized screen, black screen,
or scene/video filling view). Need the affected transition
before changing detection or treating a cinematic as a loading screen. README
now describes the existing behavior; headset behavior on that transition is
not yet verified.

## Installed: physical melee strength modifier (2026-11-15)

The user resolved the attack choice: normal physical jabs/swings need no Trigger;
**either physical Trigger held before a swing makes it strong** with CyberFinger
active. Trigger presses alone never launch punches. Ordinary fists/melee and
arm-cyberware melee share the modifier; other controllers retain the weapon or
attacking hand's Trigger. Firearms and projectile-launcher controls are unchanged.

- Implemented in the Weapon Lua module with a shared `melee_strength.lua` helper.
- 288 offline replays of the actual ordinary/arm-melee detectors pass: 30/90 Hz,
  both hands, either/both/neither Trigger, threshold boundary, active/inactive/
  missing CyberFinger profile. Stationary Trigger presses never start attacks;
  physical strokes preserve contacts and sounds and do not repeat while still.
- All 11 CyberFinger tests pass. Package:
  `dist/CyberpunkVRPort-0.2.3-cyberfinger-melee-r2`.
- Installed and independently verified with Cyberpunk closed: `ordinary_melee.lua`,
  `cyberware.lua`, and new `melee_strength.lua`. Eight settings/binding files,
  the CET ASI and the VR plugin DLL are unchanged; no driver update is needed.
- Plan/installer/verification: `build/melee-strength-install/`. Two original
  files have verified backups in
  `build/melee-strength-install/backup-52345aecadbd4fae9e3e1560245e8639/`.
- The installed DLL/PDB symbols remain those of melee-r1 below.
- **Headset test still needed:** equip fists/melee, jab without Trigger for a
  normal hit; hold either Trigger before a physical swing for a strong hit.
  Test alongside the installed support-Grip fists/melee toggle below.

## Installed: fists/melee toggle plus laser/HUD fixes (2026-11-15)

**Installed and independently verified after Cyberpunk closed.** Three mod
files: plugin DLL, CyberFinger `init.lua`, and new `melee_toggle.lua`. Eight
settings/binding files and the CET ASI are unchanged. Two replaced files have
verified backups; the third file is new.

- Package: `dist/CyberpunkVRPort-0.2.3-cyberfinger-melee-r1`.
- Plan, installer, verification and matching DLL/PDB: `build/melee-toggle-install/`.
- Backup/verification: `build/melee-toggle-install/backup-53f9cad0a39b4f7da35ea3bd9d86c778/`.
- Installed DLL SHA256: `B8AFF3B1D35B05EEB9B05991EE436EE5BFA16FCBB1F0DD541C4645A293E71669`.
- Full Release build and all 10 CyberFinger tests pass. Includes the prior
  tested native-menu laser, HUD visibility correction and F10 defaults.
- The user clarified Grip must toggle **equipped melee weapon ↔ bare fists**,
  never a gun. Implemented on the support-hand physical Grip (right for left
  dominance, left for right dominance), only with fists/ordinary melee active.
  Remembers last melee while equipped, otherwise uses first eligible weapon
  slot; never falls back to a gun or arm cyberware. No melee slot means no-op.
  Input is claimed until release, with neutral rearming and stale-input checks.
  Menus, interactions, held world objects, scanner, vehicles, swimming, ladders
  and staged/cinematic contexts suppress the toggle. Game-thread requests
  recheck pause, vehicle/device/swim/scene state and equipment restrictions.
- README_CYBRFNGR is updated. No driver update required.
- **Headset playtesting remains:** left-hand native-menu beam meets its cursor;
  closing menus restores gameplay; both Menu hide/show HUD away from prompts;
  equip a melee weapon, right Grip to fists, release, right Grip back to melee.
  Left Menu must retain general weapon cycling and guns' Grip reload actions.
- The later attack-trigger choice and its implementation are recorded above.
  Native automatic melee remains blocked by the port's tracked combat system.
- Old mod-ui-r3 live probes/PID 67968 are stale after restart; resolve symbols
  from the matching DLL/PDB above for the next running game.

## Latest HUD preset and pending combat choices (2026-11-15)

**HUD capture complete; included in the installed update above.**
The user requested the current F10 HUD parameters as defaults before restart.
Read-only capture from PID 67968, exact installed mod-ui-r3 symbols:
`build/hud-defaults-captured.json`. This is the refreshed capture after further
live adjustments, replacing the earlier 58.9-degree / 1.5 m snapshot.

- HUD: 56.2-degree FOV, 1.17 m stereo depth, head/cone follow at 60 degrees.
- Wall clearance on, grow 0.51, hand push-away off (inactive amount still 2x).
- Interactions: enabled, 0.84 m, 57.5-degree FOV, zero interaction/loot cone,
  object-depth anchoring off. Brightness, shadow and glow are 1x.
- All 40 individual elements enabled at neutral position/scale/opacity.
- Complete 59-key preset: `mods/config/cyberfinger-hud.ini.example`, shipped by
  build_dist. CyberFinger installer seeds missing keys, preserving existing
  tuning; explicit `-ResetHudDefaults` reapplies all captured HUD keys.
  F10's per-element Reset controls still restore the neutral element layout.
- All 15 installer integration scenarios pass, including existing preferences,
  captured defaults, explicit reset, preview, game guard and rollback.
- Latest package: `dist/CyberpunkVRPort-0.2.3-hud-r4`. No DLL rebuild needed for
  this preset; includes the tested laser/HUD fix and F10 defaults below.
- Latest install plan: `build/hud-defaults-install/`. Current saved vrport.ini
  already agrees with all captured values, so **only two mod files change**;
  all eight settings/binding files and CET ASI are preserved. Refresh hashes
  with `verify.py --before` only after game exit, then `install.ps1`, then
  `verify.py`. Exact DLL SHA is the same 71A30C5B... listed below. Preview passes.
- Older hud-r1/r2/r3 prepared packages are superseded for this session.

User reconfirmed: native Esc/map/PgUp cursor already follows the hand when the
PC window has focus; the missing feature is visible laser feedback. The laser
fix below is prepared, not yet installed/headset-tested.

**Historical combat request, superseded by the modifier choice above.** User wanted dominant
Trigger = fast attack, opposite Trigger = strong, and opposite Grip to toggle
melee rather than cycle/equip successive weapons (left Menu already cycles).
The Grip clarification is resolved as equipped melee weapon vs bare fists.
The user subsequently chose physical swings with either Trigger as strong modifier.
The existing port intentionally suppresses native button melee while tracked
hands fight (`physicalCyberware` in XInput and `vrport_native_attack_guard.reds`).
Its physical swing scripts use each hand's trigger as that swing's strong flag.
Do not simply remap RT/LT: it would still be blocked and/or double-hit alongside
physical collision. Current right Grip is exposed as R3; left Grip as cf_scanner.
Inspect context claims and handedness before changing either. Preserve menus,
world grabs, firearms and other controller profiles. README combat additions
follow the clarified design above.

## Native-menu laser correction (2026-11-15)

**Built and packaged; awaiting game closure to install.** This includes the
pending HUD/defaults update below; do not install the older hud-r1 package separately.
While Esc was open, live PID 67968 showed the configured left-hand pointer
active, hitting the panel and moving its cursor, with no native laser swapchain
allocated. CET/F10 captures were false. Read-only capture:
`build/menu-laser-samples.json`; symbols match installed mod-ui-r3.

The native beam required linear RGBA8 UNORM while the working F10/CET surfaces
use sRGB. It now prefers sRGB with a linear fallback and uses that same format
for its RTV. The beam follows the native cursor intersection, stays visible
between controls and continues as a short ray off-canvas. Mouse/stick focus,
tracking loss, menu closure and overlay input handoff hide it. This applies to
CF native Esc/inventory/map/journal/mod editor menus; handedness and existing
click/secondary/tab behavior remain intact.

All 8 CyberFinger and 22 overlay tests pass, including GPU creation on an
sRGB-only runtime, linear fallback, unsupported formats, endpoint geometry,
handedness, off-canvas aim and input ownership. README_CYBRFNGR and the mod UI
design notes describe the behavior. After installation, check the visible ray
in Esc, PgUp's native character editor and map/inventory; compare its endpoint
with the cursor, then close the menu and verify normal gameplay resumes.

- Release build: passed; log `build/menu-laser-release.log`.
- Package: `dist/CyberpunkVRPort-0.2.3-hud-r2` (1,218 files).
- Two-file plan/installer/verification and matching symbols: `build/menu-laser-install/`.
- Preview passed: only VR DLL and `lua/CyberpunkVRPort_CyberFinger/init.lua`.
- Prepared DLL SHA256: `71A30C5BF66384EF8E8CA099D3813BC10D8D16C28E628A19AB921695948B5C3F`.
- Live game remains on mod-ui-r3 until this install runs. Wait for Cyberpunk to
  close, refresh preserved hashes with `verify.py --before`, run `install.ps1`
  with existing authorized game-folder access, then run `verify.py`.
- The installer verifies backups and target hashes, preserves eight settings/
  binding files and the installed CET ASI, and rolls back if either copy fails.

## HUD visibility correction in progress (2026-11-15)

Live PID 67968 confirms the Menu chord worked: hidden=true. In menus, Tier2
forced the HUD visible. Back in gameplay (tier=1, menus=0, no visible prompt),
interactionUntil stayed refreshed: the Lua bridge read cached choices without
checking InteractionChoiceHub.active. Snapshot: `build/hud-toggle-gameplay.json`.

The correction checks active prompts, uses visible subtitles/choices instead of
all staged gameplay to reveal HUD, retains cinematic reveal, and adds F10 > HUD
> Hide HUD (CyberFinger) with temporary-reveal status. The F10 defaults below are
also included. **Built, packaged, awaiting game closure to install**; live game
is still on mod-ui-r3. Full Release build and all eight CyberFinger tests pass,
including regressions for cached inactive prompts, staged gameplay, subtitles,
temporary reveal expiry and F10's shared hide preference. The 33 overlay/mod-UI
tests also passed with the new defaults before this HUD-only correction.

- Superseded package: `dist/CyberpunkVRPort-0.2.3-hud-r1`; install hud-r2 above instead.
- Superseded installer, plan, verification and symbols: `build/hud-toggle-install/`.
- Preview changes only plugin DLL and `lua/CyberpunkVRPort_CyberFinger/init.lua`.
- Prepared DLL SHA256: `B6C4431E60F630C4A84B38C83B03010E7B6E28A19FDE22E37E3AFB908DC1483C`.
- Post-install test: away from prompts, both Menu held 0.5 s should hide/show main
  HUD. Approach/leave an interactable object; check automatic reveal then hide.
  Test dialogue/subtitles and F10 > HUD's preference/reveal status.

## F10 defaults captured (2026-11-15)

Source defaults now match the user's saved working placement: distance 1.22 m,
scale 1.23, width 1.55 m, zero X/Y offsets (about 1.91 x 1.43 m total bounds).
Fresh configurations and Reset use these values in the prepared hud-r2 build.
README and shared-panel documentation are updated. Existing installed settings
already contain these values and are preserved by the combined update.

## CET / menu regression correction (2026-11-15)

**Installed and independently verified:** VR `mod-ui-r3` and CET adapter `r2`,
replacing the r2/r1 build below. User verified live hands in V's apartment; the actual
gameplay blocker is CET retaining overlay input after PgDn closes it.

- CET close hides its entire canvas, strip and ray, regardless of background
  mod windows. Input returns after any held controls are released.
- Native Esc/character-editor anchors survive XInput overlay suspension.
- Native/adapted panels support physical mouse takeover and deliberate Trigger
  return to the laser. CET yields its Win32 handler when F10 has focus and lets
  F10/Insert reach the host.
- Opening a mod panel restores the configured hand; CF optical dropout does
  not silently transfer it to the opposite hand.
- Added regression coverage for the real CET adapter lifecycle with an ordinary
  always-running ImGui mod window, desktop/laser handoff, and native suspension.

Both Release builds pass; **41 tests pass** (11 mod UI, 22 overlay, 8 CyberFinger).
Seven prepared CET source files match the compiled isolated source; original CET
integration files are unchanged. Two installed binaries and their backups were
independently hash-verified; eight settings/bindings files are unchanged.

- Packages: `dist/CyberpunkVRPort-0.2.3-mod-ui-r3`, `dist/CET-VR-ModUi-1.38.1-r2`.
- CET prepared patch: `build/cet-vr-adapter-r2-final/`.
- Installer/plan/symbols: `build/mod-ui-r3-install/`.
- Backup and receipts: `build/mod-ui-r3-install/backup-dee0c9a74271414ab7a10a99b18aa676/`.
- VR SHA256: `06EFA8DB9DE6FC33AD83A69EA82A5EDD3DBE179806090A57BD30363179455D5D`.
- CET SHA256: `7DAFCC22219558DC29C65C0C895DBFBE20E00B4703F7A9488F46730C1F2DEBAD`.

Headset checks after installation: PgDn open/close leaves no strip or laser and
restores movement/dialogue; Esc and both CPM screens remain anchored; physical
mouse works on native/CPM/F10 panels; opening F10 over CET transfers mouse focus.

## General mod UI integration installed (2026-11-15)

**Installed and verified; headset validation pending.** The [mod UI bridge](mod-ui-vr.md) provides
world-anchored OpenXR canvases and shared laser/Trigger, Grip, Menu, scroll and
opposite-hand tab input. The port's Lua renderer uses it; an isolated CET build
adds the companion renderer adapter. Native CyberFinger modal menus are anchored
while open; existing device surfaces and other controller behavior remain.

Both full Release builds pass, plus **9 mod UI + 22 overlay + 8 CyberFinger tests**.
The original CET source checkout remains unchanged. Generated CET patch
and source hashes: `build/cet-vr-adapter-final/`; tested CET base is
`v1.38.1-11-g20b42c6`. `README_CYBRFNGR.md` documents the controls and limitations.

- VR package: `dist/CyberpunkVRPort-0.2.3-mod-ui-r2` (supersedes r1).
- CET companion: `dist/CET-VR-ModUi-1.38.1-r1`.
- Two-file plan and guarded installer: `build/mod-ui-install/`.
- Installed with explicit user approval. Both installed hashes and both original
  backups verified independently; six VR INIs and CET's bindings/config JSON
  remain byte-for-byte unchanged (eight files).
- Backup/receipts: `build/mod-ui-install/backup-718d7d890870416d80e5d6e290901601/`.
- VR DLL SHA256: `C0072B332E02E2ABB22D95A53650A392E81101CBE4E3625F960B96B755E033EF`.
- CET ASI SHA256: `CB0C16437460688CAA8E796392A85331A9360A33E34096CAA941AB06B3A32952`.

**Next:** start SteamVR/Steam Link and launch Cyberpunk through GOG, then test
CET/CPM/native mod menus in the headset. Page Down opens CET. Check left-handed
pointing, scrolling, tabs, Menu, Grip panel movement,
F10 priority, held-button release, save loading and clean exit. Standard ImGui
tabs support joystick switching; CET's toolbar uses buttons and needs laser
clicks. Custom independent windows and decorative HUD widgets need the coverage
limits in the design doc. The earlier CET watchdog hang remains unproven/fix unconfirmed.

The r2 placement correction fits the complete native SBS canvas into F10's
physical bounds (default max 1.55 x 1.1625 m), shares Panel size with adapted mod
windows, and keeps CF native/F10 panels fixed through head rotation/translation,
tab changes, temporary focus changes and tracking loss. Grip still moves F10/mod
panels. Other controller profiles retain their native/F10 follow behavior.

## Installed controls correction (2026-11-15)

**Installed `controls-r5`**, superseding `wrist-r4`. Right Menu tap opens Inventory
and hold (0.65 s) requests equipped OS cyberware through game-thread commands.
Weapon-hand black hold opens Pause through the native menu function, avoiding
the Start/Back mapping ambiguity. Right-stick left/right changes tabs on the map;
weapon-hand Y still zooms. Both Menu hold (0.5 s) toggles HUD without requiring
both optical hand poses. Interaction prompts and conversations still reveal it.
CyberFinger dash/crouch now defaults to 80% raw stick travel; the user's isolated
full-back capture reached about 85%, below the previous effective 91.8% gate.
Other controller profiles retain their existing behavior.

All **8 CyberFinger + 21 overlay suites pass**, full Release build and packaging
pass, and `git diff --check` passes. Installed **2 files (DLL + CyberFinger Lua)**,
verified both hashes, and verified **all 6 existing INIs unchanged**. No SteamVR
binding or driver update is required; the selected repository v2 binding remains.

- Package: `dist/CyberpunkVRPort-0.2.3-cyberfinger-controls-r5`.
- DLL SHA256: `BD715A367DA31277F3E048EDA409D86B9781707BAC8E49F4FF59E23CE4B51023`.
- Backup: `build/cyberfinger-install-backups/75dca00a98f343bc8b4319a5c5779c7c/`.
- Plan, install log and verification: `build/cyberfinger-controls-r5-install/`.

**User feedback (2026-11-15):** the current setup is largely working. This is
overall gameplay feedback; the specific Menu-hold, HUD-toggle and stick-action
checks below have not yet been individually confirmed. Mirror facial animation
was confirmed working earlier. A foveation frame-rate benefit remains unproven.

**Remaining targeted checks:** load a save and test right Menu tap/hold and
weapon-hand black hold. Test HUD hide/show away from interaction prompts; release
both Menu buttons between holds. On the map, test right-stick tabs and weapon-hand
Y zoom. Confirm right-stick back now toggles crouch and returns to standing after
centering and pushing back again. Saved controller scheme is Alternative; actual
R3-to-crouch behavior still needs this runtime check. Keyboard C restored standing
in the earlier reproduction with physical crouch inactive; the cause of entering
that original crouch remains unknown.

## Previous context-control installation (2026-11-14)

**Installed final wrist revision (`wrist-r4`):** supersedes both prepared `shortcuts-r3`
and `menu-context-r2`. Weapon-hand black tap ejects and hold pauses; opposite
black tap quick-reloads and hold heals, swapping with weapon handedness regardless
of stick/button mirroring. Right pink summons the gameplay VR menu; F10 is on
the keyboard. Both Menu hold still toggles HUD; left-stick + right-Grip stays
retired. Scanner confirmation remains the interaction-hand Trigger.
Weapon-hand hold pauses even in vehicles; keyboard F remains vehicle exit.
All 8 CF + 21 overlay suites pass; 9 binding-selection scenarios and 5 driver
binding tests pass. README, seven-context diagrams, in-game reference, binding
descriptions, and the driver source guide are updated. Full release build and
packaging pass. Installed **7 files (DLL + 6 Lua files)** and verified every
installed hash; **all 6 existing INIs are byte-for-byte unchanged**.
Package: `dist/CyberpunkVRPort-0.2.3-cyberfinger-wrist-r4`.
DLL SHA256: `CF59BA55A7C0AEC9498CAE9935A28F4E18649A267F4CECF78BA403DE393BEE25`.
Backup: `build/cyberfinger-install-backups/f5f30ae72f8d49a2a67ec8bbbbcd0d33/`.
Install plan/log/verification: `build/cyberfinger-wrist-r4-install/`.
SteamVR **selected and verified** `resources/cyberfinger/cyberpunk_openxr_v2.json`.
The prior selection is saved under
`build/cyberfinger-install-backups/binding-selection-39175f005e4042d18f5e6cd353a7903e/`.
Keep the selected repository binding file in place. No driver binary changed.
The selector's default-path error under Windows PowerShell `-File` is fixed and
covered by an added preview scenario.

**Next headset test:** launch from GOG and verify left black tap eject / hold
pause, right black tap reload / hold heal, right pink VR menu, both Menu held
0.5 s HUD toggle, and no old left-stick + right-Grip HUD action. Test that holds
do not also perform taps. Then test the bundled context controls listed below.
Other-controller behavior and every handedness/mirror mode pass offline tests;
the new controls still need live headset verification.

### Superseded shortcut package (not installed)

**Earlier shortcut revision:** superseded the prepared `menu-context-r2` package.
Left black wrist tap now opens/closes the world VR menu; holding both Menu
buttons for 0.5 s toggles HUD visibility. Left-stick + right-Grip is retired.
The chord is consumed until both buttons are released, including when a
computer has focus. Scanner confirmation moves to the interaction-hand Trigger;
left black hold retains pause on foot and uses the deliberate vehicle-exit
route while seated. Physical reload and keyboard R remained available; the later
quick-reload answer is implemented by `wrist-r4` above. Existing bindings expose
all inputs; no driver update is needed.

All **8 CF suites + 21 overlay suites pass** for the revised shortcuts, including
one toggle per hold, partial releases, no old-chord capture, wrist tap vs HUD,
and unchanged non-CF behavior. The full release build and packaging pass.
Prepared package: `dist/CyberpunkVRPort-0.2.3-cyberfinger-shortcuts-r3`.
DLL SHA256: `9D3BBBC5A5C0466B09488756A19C8C572D600FF2E668E90E7946D5EF53AC8D19`.
The installer preview plans **7 changes: DLL + 6 Lua files**, with no settings
changes. Preview/hashed plan: `build/cyberfinger-shortcuts-install/`.
At preparation Cyberpunk was running (PID 29488); this package was never installed.
It is superseded by the installed `wrist-r4` package above.
Headset checks: wrist tap opens/closes VR menu without reload; both Menu hold
toggles HUD once without Back or inventory; release both to re-arm; old
left-stick + right-Grip no longer toggles HUD; scanner Trigger confirms.

The quick reference documents the intended layout, per the user's request.
Installation and validation status belong here.

The following context changes from `menu-context-r2` are also included:

- persistent CF world-menu laser, independent of button hover;
- map dominant-hand right-click waypoint and stick zoom, off-hand Trigger Tab
  and Grip journal; journal off-hand Trigger sorts;
- prompt Take/Interact X and Eat/Use Y on the hand opposite the actual movement
  stick, with hold support and release guards before aim/fire/scanner/melee return;
- HUD reveal for object prompts and loot, independent of dialogue capture;
- the apartment photo selector on a smaller, world-anchored menu panel;
- computer mouse from the hand ray through the native screen mesh picker,
  Trigger click, Grip return within the computer, same-hand stick scroll,
  either Menu exit. Non-CF controls keep their existing path.

The 8 UI policy/bridge suites pass, including handed map/journal roles, picture
panel anchoring, prompt-vs-dialogue distinction, computer ownership, and input
release. The 21 overlay and 40 HUD suites also pass. Six edited Lua modules parse.
The full release build passes. Superseded package:
`dist/CyberpunkVRPort-0.2.3-cyberfinger-menu-context-r2`.
The installer preview plans **7 changes: DLL + 6 Lua files**, preserving all
settings. Preview/hashed plan: `build/cyberfinger-context-install/`.
This behavior still needs headset verification. `wrist-r4` now includes it and
has replaced the old `ui-face-exitfix` DLL.

**Face animation:** user confirmed visible mirror facial tracking works. Keep
tracking-loss/reopen/save-reload and other head/body rigs on the test checklist;
do not keep treating the visible mirror result as unverified.

**Fresh foveation comparison:** the user stood still in gameplay with menus
closed. At 70% x 65%, the first 8-second on sample measured 66.58 game-Present FPS
(15.02 ms), with **0 new foveated/eligible draws out of 766,882 scene draws**.
Every rejected draw failed the pipeline eligibility check. Gaze was fresh for
87.5% of samples. This is not a measured speedup: the sweep stopped before an
off baseline and restored the original INI byte for byte. Results:
`build/cyberfinger-diagnostics/vrs-region-sweep-controls-01/70x65/result.json`.
The reusable `tools/vrs_tests/scan_regions.py` previews by default; `--run`
compares 70x65, 60x55, 50x45, 40x35 against off using on/off/off/on for each,
stops on inactive shading or invalid gaze, and restores configuration. Fix scene
pipeline eligibility before repeating; narrowing the region alone cannot help.

Next live checks: door/mirror/wardrobe interaction including hold prompts;
take versus consume; computer cursor while moving only the head, then only the
hand; scroll, Grip back and Menu exit; map waypoint/zoom and journal Tab;
picture-frame size/anchoring/photo selection; HUD prompt reveal; Alt-F4 cleanup.

## Previous installed build and historical checks

Status updated **2026-11-14**. The combined **UI + face-animation update and
Alt-F4 shutdown fix are installed and verified**. All five existing INI files
are unchanged. Cyberpunk was running at the latest check; the unified-menu
update below awaits closing it. V2 binding selection remains unverified. The foveation
lifetime fix is included; another gameplay eligibility issue remains.

The update makes **left-stick click sprint**; full-forward movement alone does
not start sprint. It also includes physical trigger/Grab menu input, both-Menu
VR-menu access, right-pink F10, a larger/toggleable HUD and conversation controls.
All apply only to an active CyberFinger binding. Other controllers keep their
existing behavior. The updated [quick reference](../README_CYBRFNGR.md) includes
[controller diagrams](controls/cyberfinger-controls.html).

## Prepared unified-menu update

The next package, `dist/CyberpunkVRPort-0.2.3-cyberfinger-menu-unified`, replaces
the prepared `menu-abx` package. Installation awaits the game closing. It
includes all earlier fixes. For active CyberFinger controllers in **all original
Cyberpunk menus**, including the wardrobe:
Trigger selects (A / laser click), Menu sends B on release, and Grip sends X
for as long as it is held. Both-Menu remains the VR-menu chord and emits no B.
In the world-space VR menu, Menu is Back/cancel and Grip opens existing
inventory/backpack item actions or sort options. Unassigned context actions are
no-ops, never primary clicks. F10 receives Grip as an independent right mouse
button (panel options), and Menu dismisses an active popup/edit before closing
settings. Hold-Grip on its title still moves it. Physical right Menu is now
preserved before the inventory/cyberware tap-hold conversion. F10 takes input
priority over a world-space menu behind it. Other controllers retain their
existing bindings. The driver/binding already exposes these controls.

All **8 CyberFinger suites and 21 overlay suites pass**, covering held entry
controls, X hold/release, both Menu buttons, chord suppression, laser click
versus A (no double activation), off-ray A, non-CyberFinger gating, Grip context
targeting by hand, independent right mouse input and popup-aware Back. All five
changed Lua modules parse. README and diagrams distinguish native menus from
VR menus/F10. After installation, test the wardrobe's A/B/X prompts with each
hand, hold-X, VR item/sort contexts, F10 popup cancellation, tabs and both-Menu.

## Resume after dinner

1. **Done:** closed Cyberpunk. The installed package is
   `dist/CyberpunkVRPort-0.2.3-cyberfinger-ui-face-exitfix`.
2. **Done:** installed with backups. To reinstall from the repository root:

   ```powershell
   .\install_cyberfinger.cmd -Package .\dist\CyberpunkVRPort-0.2.3-cyberfinger-ui-face-exitfix
   ```

   The update contains the plugin and changed/new CET menu scripts. Existing
   left-handed, foveation and calibration settings are preserved. The installer
   refuses actual installation while the game is running; `-WhatIf` previews
   without writing even while it runs. Do not use the older face-only package
   for these input changes.
   The r2 installation changed **six files** (the DLL, three existing menu Lua
   files, and two new Lua files), backed up at
   `build/cyberfinger-install-backups/60b5a8888ed94e9795eb8bc0ac4c9741`.
   The subsequent shutdown fix changed **only the DLL**; its installed hash
   matches and all five INI hashes are unchanged. Latest backup and manifest:
   `build/cyberfinger-install-backups/35f9cb62e320438cb320e4debd564929`.
   Installed DLL SHA-256:
   `67BE01A985485F4837993223B0D7A893C248E70FED6723134590DE5CAB0FDF0C`.
3. Launch the new plugin, then run `scripts/select_cyberfinger_binding.ps1` to
   select **v2**. It requires the fifth `cf_settings` action. The prepared binding
   is `resources/cyberfinger/cyberpunk_openxr_v2.json`; keep it in place after
   selection. Until this step, right pink still heals rather than opening F10.
   Test right-stick up/down choice selection plus right-trigger confirmation,
   with left-stick movement and right-stick turning still available. Confirm
   walking does not change the highlighted choice. Then load the save and enable
   **F10 > Avatar > Face tracking > Quest Pro
   face tracking in mirrors**. Open the apartment mirror. Smile, blink each eye,
   open the jaw, then look left/right/up/down with your eyes. Eyebrows can be
   enabled separately. The initial feature defaults to off.
4. Run `tools/face_tests/live_probe.py --pid <game PID>` to check live reception,
   mirror ownership, mapped controls and solver writes without taking OSC port
   9015. Test tracking loss, disabling, closing/reopening the mirror and save
   reload. Native hook execution and the visible face result remain unverified.
5. Use the new VRS rejection counters to identify why gameplay draws remain
   ineligible before another FPS comparison. Their order is pipeline, depth,
   target count, viewport, native-stereo frame, target dimensions, stereo
   viewport layout, legacy viewport layout. Do not claim an FPS gain yet.
6. After loading a save and letting its HUD appear, save progress and close
   with **Alt-F4**. Confirm there is no new crash report. Also check normal
   menu exit. The log should contain `[hud-panel] released native widgets
   before engine shutdown` before plugin unload. This live check is pending.

Shutdown diagnosis: report `Cyberpunk2077-20261114-202724-56936-34404`
predates r2. OpenXR completed shutdown, then the process faulted at game RVA
`0x1C2F8F`, reading a null engine pointer plus `0x250`. An expanded stack scan
with the matching archived DLL/PDB identifies `Handle<ink::Widget>` destruction
from the static `upkeepRoots` vector during the plugin's CRT process detach.
The scan is not a formal unwind; native disassembly confirms the failing
destructor's unguarded engine access. Evidence is in
`build/crash-altf4-56936/`. The fix closes cache writers, restores masks and
releases root/subtitle/repaint handles on Running exit / Shutdown entry while
the engine is alive. It is idempotent; no HUD hooks install if neither cleanup
callback can register. Release compilation and the 40 HUD tests pass; those
tests do not simulate the game's shutdown sequence.

The prior face build compiled and all three face suites passed (OSC parser/freshness,
rig mapping/native fallback, actual Windows UDP receiver/lifecycle). All three
VRS suites also passed after adding rejection diagnostics. Details and engine
compatibility limits: [Quest Pro face tracking](quest-pro-face-tracking.md).

New interface validation: **8 CyberFinger suites, 21 overlay suites and 40 HUD
suites passed**, including held-input entry guards, controller-activity gating,
conversation delivery, both-Menu capture, HUD sizing/hiding, mouse focus and
GPU layer lifetime checks. The binding helper's eight mocked scenarios passed.
Headset usability and native-game laser hit alignment still require playtesting.

## Installed and prepared

- The eye-tracked-foveation plugin is installed. The game configuration now has
  `xr_vrs_eye_tracked=1`, `xr_vrs=1` (2 x 2), center width 70%, height 65%.
  Left-handed weapon and **Same as right-handed** bindings are preserved.
- The corrected CyberFinger layout is installed at
  `C:\Program Files (x86)\Steam\config\openxr\cyberfinger_cyberpunkvrport_binding.json`.
  Its actual application key is
  `system.generated.openxr.cyberpunkvrport.cyberpunk2077.exe`.
- **The older OpenXR binding is selected and verified; v2 is pending.** The helper checked the live
  action manifest and SteamVR's active URL. It replaced the automatic
  `knuckles_12_binding.json` mapping with the installed CyberFinger layout.
  The previous selection and verification are saved in
  `build/cyberfinger-install-backups/binding-selection-a981801c383d4d4a9b8819069b2f76ed`.
  An earlier request also saved an unused legacy-category preference; cleanup
  can wait until SteamVR is closed.
- The protected installed driver profile and binary were not replaced.
  Corrected binding assets are also in `D:\build\CyberFinger_SteamVRFusion`.
- [README_CYBRFNGR.md](../README_CYBRFNGR.md) contains launch steps, the physical
  button table, grabbing, left-handed settings, snap turn, foveation, FBT and
  a one-eye OBS crop recipe.
- After the reported Ultraleap uninstall, the current game log confirms finger
  data from hand tracking for **both hands**. Visible animation and grabs still
  need the user's test in a loaded save.
- The current game log also reports **eye-tracking state=2 (tracking)**. Live
  gaze reception is confirmed. Cumulative VRS counts initially showed maps/draws,
  but the subsequent stationary gameplay test had **zero new foveated draws**
  across eight seconds (72.4 game FPS). That is not a valid on/off comparison.
  Both aborted runs restored the original INI byte for byte.
- The previous build's foveation registry reached its hard **512-command-list limit**.
  A GPU regression reproduced the same failure at the 512th registration.
  Source now ties state ownership to each D3D command list, releases destroyed
  states and prunes weak registry entries. All three VRS suites pass, including
  640 simultaneously live command lists and resource-release checks.
  The release build and the additional create/destroy lifetime test passed.
  The initial lifetime-fix package was
  `dist/CyberpunkVRPort-0.2.3-cyberfinger-questpro-eyegaze-vrsfix`.
  **The correction is installed and its SHA-256 matches the prepared build.**
  The installer changed only the plugin DLL. All four checked configuration
  files are byte-for-byte unchanged, preserving the eye-tracked 2 x 2 settings,
  left-handed weapons and calibration. The next gameplay sample showed the
  registry working (116 to 135 live lists, zero allocation failures), but still
  **zero eligible/foveated draws** across 623,063 scene draws. It measured 68.04
  game FPS, with gaze fresh in 68.75% of samples. The comparison stopped and
  restored the INI byte for byte. This is not evidence of a performance gain.
  Evidence: `build/cyberfinger-diagnostics/vrs-ab-549c56984dbc4f21ac7fe56c6c3cfc90/result.json`.
- New binding helper: eight mocked API scenarios pass, including refusing a
  legacy manifest, missing actions/controllers, and an unverified success reply.
  The helper also completed and verified selection against the live game.
- New face-input diagnostic: six parser/freshness tests pass. The live 30-second
  localhost **9015** test received **76 fresh facial channels**, **18,356 packets**
  and **278,996 validated facial values**, with **zero malformed packets**.
  Smile, eyelid and jaw coefficients changed during the test. The probe has
  exited and released the port. The native facial adapter is now installed
  in the combined r2 update; the first live mirror test remains pending.

Foveation/config backup and actual installation record:
`build/cyberfinger-install-backups/bindings-foveation-4300f7556dbf4812bce389d45d8f6bf0/install-custom.json`.
The earlier `install.json` there describes the rolled-back protected-driver
attempt. The earlier lifetime-fix DLL backup and installation record are in
`build/cyberfinger-install-backups/387004508d7d48f4bedd52becf54d536`.
Previous lifetime-fix DLL SHA-256 (replaced by r2 above):
`912D34973EE72D587C559A5076DCDA7D1FB4D0BE46A3872FDFA87522BB226973`.

## Next session, in order

1. **Done:** connect Quest Pro/CyberFinger and launch Cyberpunk.
2. **Pending for v2:** select and verify the new CyberFinger OpenXR binding after
   installing and launching the new plugin (the older binding was verified).
   Run these commands from the repository root while the game is running:

   ```powershell
   .\scripts\select_cyberfinger_binding.ps1 -WhatIf
   .\scripts\select_cyberfinger_binding.ps1
   ```

   The helper reads the live action manifest, checks all binding outputs, selects
   only this app/controller, and requires SteamVR to report the same OpenXR URL.
   It saves the previous selection under `build/cyberfinger-install-backups`.
   If SteamVR's internal UI API has changed, it fails explicitly; use Controller
   Bindings manually rather than guessing settings keys.
3. Load a save. Verify click-to-sprint, right-stick jump/dash/crouch, left Menu
   weapon wheel, right Menu inventory/hold cyberware, left Grab scanner, right
   pink F10, right black hold heal, and both Menu held for the VR menu. In all
   menus, try either laser/trigger, left-stick highlights, right-stick tabs and
   Menu for Back/cancel and Grip for context/X. Confirm left-handed F10
   preference and Grip on its title handle; Menu must dismiss a popup before F10.
   Try the HUD shortcut (L stick click + R Grab), enter a conversation and check
   automatic HUD visibility, right-stick up/down selection and right-trigger
   confirmation while left-stick walking and right-stick turning still work.
   Check held controls do not dodge/crouch/aim/fire when dialogue closes.
   Check fingers animate and nearby close-fingers grabbing still
   works while button use cannot create accidental grabs.
4. In **F10 > Controls > General > Turning (right stick)**, enable snap turn and
   **Save** if desired. The last inspected config still had snap turn off; this
   installation changed only foveation, not that preference.
5. **Installed:** the `eyegaze-vrsfix` correction. The subsequent comparison
   still found zero eligible draws; diagnose using the installed r2 build's rejection
   counters before repeating the on/off comparison.
   In **F10 > General > Variable Rate Shading**, check that eye tracking reports
   live tracking. Test gaze response and appearance before reducing the 70%/65%
   high-quality area. Loss of gaze should restore normal shading.
   A bounded on/off/off/on comparison is prepared in `tools/vrs_tests/live_ab.py`;
   keep a fixed scene/view for the test. It measures game Present intervals,
   verifies active shading counters and restores the original foveation setting.
   No FPS gain has yet been established against an off baseline.
6. Make a short stream recording using the [one-eye crop](../README_CYBRFNGR.md#streaming-one-eye-at-1920-x-1080).
   Check motion, HUD visibility and the preferred eye. OBS has not been configured.
7. Clean up the unused legacy-category binding preference after the real OpenXR
   selection is verified, using SteamVR's settings interface or a narrow backed-up
   edit while SteamVR is closed. Do not overwrite the whole settings file.

## Quest Pro facial tracking: implemented, pending live verification

V has a facial rig; Cyberpunk's facial system supports mouth, brow and eye
animation. The original developers describe its FACS-like rigs in their
[JALI/CDPR paper](https://history.siggraph.org/wp-content/uploads/2022/08/2020-Talks-Edwards_JALI-Driven-Expressive-Facial-Animation-and-Multilingual-Speech-in-Cyberpunk-2077.pdf).
The [modding documentation](https://wiki.redmodding.org/cyberpunk-2077-modding/modding-guides/npcs/fixing-eye-clipping-in-npvs-by-replacing-facial-rigs)
also describes V's facial rigs and customization-related eye alignment issues.
That establishes the animation machinery, not a ready-made live-tracking API.

Steam Link can send face weights via OSC. Its [protocol reference](https://github.com/danwillm/VRCFT-SteamLink#about-osc-data-from-steamlink)
includes lip-corner pull, eyelid closure, jaw drop, gaze-related weights and
upper/lower-face confidence, spread across multiple bundles. The currently read
SteamVR settings already have `useOSC=true`, `useOSCFace=true`,
`shareEyeTrackingData=true`, `OSCOutPort=9015`; they were not changed for this idea.
Actual face-weight reception is verified by the live diagnostic described
above. The new adapter maps 19 named smile/jaw/blink/eye/brow controls onto the
mirror's native facial solver. The player and mirror root rigs were inspected
read-only and have distinct track buffers and indices. Runtime resolution uses
names and ranges, not the observed numeric indices.

To repeat the live check, with the headset connected and face tracking allowed:

```powershell
python -B .\tools\cyberfinger_tests\quest_face_probe.py --seconds 30
```

Smile, blink, then open your jaw. Watch the corresponding coefficients change.
The probe listens only on localhost, records no files, expires stale values and
reports a port conflict rather than sharing the socket with another receiver.
Close VRCFaceTracking or another port-9015 consumer for this test. See the
[Steam Link/VRCFT setup reference](https://docs.vrcft.io/docs/hardware/vr/meta/quest-pro/steamlink)
for headset sharing; installing VRCFaceTracking is not needed for this probe.

Disable the in-game face feature before running this standalone OSC probe; both
require exclusive ownership of the same port. The native feature has its own
counter-only probe which can run alongside it. Follow the resume steps above
for the first installed test. Visual behavior, both V body types and custom-head
compatibility are still pending; keep this marked experimental.

## Native single-eye desktop output: possible follow-up

The current OBS crop recipe works without a native change. For a built-in
1920 x 1080 desktop view, add **SBS / first eye / second eye** plus crop/fit and
window-size controls to the final desktop pass. Relevant code:
`src/Render/DesktopMirror.cpp` and the ordering in `src/Hooks/SwapChain.cpp`:
OpenXR capture/pump completes before `DesktopMirrorRender`.

Use that final pass so headset buffers retain both complete eyes. Update the
shared `DesktopCoverTransform` mapping used by `src/Hooks/OsInput.cpp` and
`src/Overlay/ImGuiOverlay.cpp` too, including desktop overlay placement. A crop
alone would otherwise make mouse clicks land on the wrong pixels. Test both
native SBS and legacy mono sources, eye switching, resize/DPI, UI hit targets
and actual OBS output. No native mirror implementation was changed in this session.
