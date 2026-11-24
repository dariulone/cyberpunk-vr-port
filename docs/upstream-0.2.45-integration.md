# Upstream 0.2.45 integration

## Scope

Integration branch: `integration/upstream-0.2.45`, based on `af74b88c`
(`Add VR mod panels and refine CyberFinger controls`). The separate worktree is
`build/upstream-integration` beneath the original checkout. The original `main`
checkout remains unchanged. After preparation, the user requested installation
for headset testing; the complete merged package is now installed with backups.

This is a genuine merge of both upstream commits, preserving their identities:

- [612bd89f — 0.2.4](https://github.com/dariulone/cyberpunk-vr-port/commit/612bd89fc634130aad71d8346a002243a577aafa)
- [6f33af62 — 0.2.45](https://github.com/dariulone/cyberpunk-vr-port/commit/6f33af62c50a85f88eceaa63f776539d8fc6f395)

Once this branch is merged into and pushed as the fork's default branch, GitHub
will recognize those commits as integrated. Keeping the work on this branch
alone does not change the default branch's ahead/behind count.

## Improvements brought forward

| Area | Upstream change | Relevance to this setup |
|---|---|---|
| SteamVR presentation | Dedicated runtime GPU queue, completed capture selection and improved frame-generation pacing | Intended to reduce waits behind game rendering; actual smoothness on Quest Pro/Steam Link still needs headset testing |
| HUD | Select a completed HUD canvas when using a separate queue; avoid stale or unfinished quad submissions | Combines with our HUD visibility controls and loading-screen handling |
| Physics hands | Hands meet world surfaces; push and grab loose props, grasp people/corpses and catch ledges | Substantial new gameplay behavior; nearby-only CyberFinger targeting is retained |
| Tracking | Optical and controller hand-data sources; hand tracking can be enabled during a session | Relevant to Quest/CyberFinger skeletons; live tracking still needs confirmation |
| Driving | Motorcycle handlebar steering, body lean and separate seated offsets; minimap over the vehicle | Useful vehicle improvements; verify grab/release and steering in-game |
| Frame generation | ANR FG option and presentation fixes; NVIDIA optical flow falls back to FidelityFX on AMD | RX 7900 XT cannot use NVIDIA-only acceleration; no FPS gain has been measured from this integration |
| Menus | Multi-sell trading and mirror appearance options; VR interface enable switch | Retains the CyberFinger laser, Trigger/A, Menu/Back and Grip/context conventions |
| Scripting | Bundled stable redscript 0.5.32 replaces the preview compiler | Avoids the preview compiler's migration behavior; also includes the breach indicator fix |
| Rendering | Scope resolution/DLSS/Ray Reconstruction isolation, foveated DLSS hook correction, smooth-turn hand and weapon-wheel fixes | DLSS-specific benefits do not apply to AMD; our eye-tracked VRS feature remains separate |
| Controls and haptics | Scanner click ownership, crouch/snap-turn changes, device takeover trigger fixes, routed haptics | Stock-controller improvements coexist with CyberFinger's existing bindings |

These are upstream implementation changes, not a claim that each feature has
been verified in the headset on this machine.

## Compatibility decisions

- The selected completed frame carries its own scene/menu/loading classification
  into eye pairing, depth, HUD submission and the last-frame record. An older
  capture must not be interpreted using the newest frame's menu state.
- Generated scene endpoints are discarded across menu/loading transitions.
  Loading/notice frames retain our long-stall resubmission behavior.
- Native-menu lasers, F10 and CET panels use the runtime submission queue while
  retaining their producer fences and input ownership/release logic.
- CyberFinger keeps left-stick sprint, calibrated right-stick crouch/dash,
  physical Grip scanner, physical Menu roles, wrist roles, dialogue selection,
  left-handed targeting and the support-Grip melee/fists toggle.
- Upstream's left-ear/L3 scanner and B-button arm-cyberware shortcut apply to
  other controllers. They do not claim CyberFinger sprint or eject/back.
- Both CyberFinger Menu buttons still toggle the HUD when the VR interface is
  disabled; the interface switch gates right-pink menu summon only. F10 remains
  available independently.
- Proximity grabs, fingertip interaction, eight-role SlimeVR, Quest Pro gaze and
  facial tracking remain in the branch. The integration does not alter the
  separate SteamVR driver or its bindings.
- The preparation script now fetches the checksum-pinned 0.2.45 release for
  matching packed assets and compiler. A full package is required for this
  upgrade; replacing only the DLL would leave an incomplete installation.

## Validation

The full Release plugin built against the pinned game-2.5 SDK. The initial long
worktree build path exceeded a Windows shader filename limit; the successful
isolated build is at `build/up245` beneath the original checkout. Dependencies
which generate files were copied into isolated locations; the previous build
and matching symbols were not overwritten.

| Suite | Result |
|---|---:|
| CyberFinger policies, adapters, Lua interaction and melee | 12 passed |
| VR overlay, loading presentation, lasers and GPU panels | 25 passed |
| CET/mod UI input, close/release and ABI | 11 passed |
| HUD layout, completed source, visibility and GPU composition | 43 passed |
| Launcher and cursor/window input | 1 passed |
| Analog controls, crouch and snap turn | 14 passed |
| Scanner ownership | 1 passed |
| Driving/steering and camera-loss grab release | 24 passed |
| Frame-generation policies, timing and FidelityFX GPU tests | 23 passed |
| HUD submission ring and vehicle map | 1 passed |
| Installer backup, rollback, settings and path scenarios | 15 passed |

Two additional frame-generation tests (`hardware` and `gpu_anr`) could not run:
their harness requires NVIDIA hardware and this machine has an RX 7900 XT.
They reported `NVIDIA hardware missing`; they are not counted as passes. No
ANR performance or compatibility claim is made from these results.

The HUD submission fixture still expected the old 12 cm bike-map height;
upstream's production placement explicitly changed it to 8 cm. The fixture now
checks that documented placement. Other fixture changes supply missing
standalone dependencies and follow upstream's new Lua wrapper, preserving the
existing behavior assertions.

All port scripts plus the installed framework/other script mods compiled in
separate workspace sandboxes with **both** the installed compiler and bundled
0.5.32 compiler: **0 errors, 0 warnings**. The game files were read-only during
these checks. Headset behavior and performance remain to be tested.

Evidence relative to the integration worktree:

- `build/integration-tests/<suite>/{configure,build,test}.log` (the added arm
  shortcut test also has its CTest result under `Testing/Temporary`).
- `build/integration-installer-test.log`.
- `build/integration-redscript.log` and `build/integration-redscript-stable.log`.
- `build/integration-package.log`, `build/integration-install-preview.log`,
  `build/integration-install.log`, `build/integration-install-verified.json`.

## Installed candidate

- Package: `dist/CyberpunkVRPort-0.2.45-cyberfinger-upstream` in this worktree;
  1,337 files, 192,837,310 bytes.
- Installed 181 changed files into the GOG game folder, then independently
  verified every installed file against the install receipt.
- `vrport.ini`, `cyberfinger.ini`, `fbt_settings.ini` and CET `bindings.json` are
  byte-for-byte unchanged. No CET or SteamVR driver files were replaced.
- Plugin SHA256:
  `7D78E81EA2856E16C29ABE0797F492CE9ABF9FAB76E5CEA9F71EE7B8328E1A55`.
- Backup and receipt:
  `build/cyberfinger-install-backups/aaf21d0e41b34e3296f106000ddb0c52/`.
  The receipt distinguishes replaced and newly added files for an exact rollback.
- Matching PDB and DLL: original checkout's
  `build/up245/bin/red4ext/plugins/cyberpunkvrport/Release/`.

The source branch remains separate. Installing this candidate does not merge
it into `main` or change the fork's default branch on GitHub.

## Next headset session

1. On Quest Pro through Steam Link, launch from GOG, load a save and check the loading panel,
   stereo stability, hand skeletons and left-handed laser selection.
2. Check movement/sprint, backward crouch, forward dash, Menu tap/hold,
   both-Menu HUD toggle, and right-pink VR menu.
3. Open and close CET, F10, Esc, map and appearance screens. Confirm input
   returns to gameplay and each panel's laser/Trigger/Back controls work.
4. Test nearby grabbing, physical melee with either Trigger as the strong
   modifier, melee/fists switching, then vehicle grabs and steering.
5. Verify eye/face tracking and optional SlimeVR. Compare frame times in a
   repeatable scene before drawing any performance conclusion.
6. If a regression appears, close the game and use the installation receipt
   and backup for rollback before comparing with the previous loading-r1 build.
