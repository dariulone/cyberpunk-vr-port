# Goal 12 validation — 2026-11-12

Branch: `fix/goal12-20261111`. Local commits only; no push.

## Test environment

- Cyberpunk 2077 2.5, H10 entrance, `ManualSave-491`.
- OpenXR Simulator 1.5.0 with the local capture harness. This is not Pimax, Quest/VD or WiVRn hardware coverage.
- Ryzen 7 5800X, RTX 5070 Ti.
- Clean baseline: packaged 0.2.3, optional RED4ext plugins / scripts / tweaks / archives / CET mods isolated by rename, preserving originals and hardlinks. CETBridge/CVRBridge used only as test transports.
- Original configuration and 40 saved installation entries restored with hash/size/link-count verification after testing. Journal: `build/goal12-20261111/clean-originals-2/journal.json`.
- Test actors and capacity modifiers were temporary. No manual save was made. Test processes were closed before restoring the installation.

## Results and limits

| Request | Result |
|---|---|
| Ragdoll grasp, fingers, rotation | Corrected completed-physics wrist rotation, competing script driver, post-correction IK shortening, person finger seating, and closer-limb selection. Live forearm lift/roll: relative translation <=1 mm and rotation <=0.5 degrees in the measured final run. This does not guarantee every clothing mesh or body part fits; capsule approximation and unusual reach remain limitations. |
| Air dash fragments | Actual SprintEvents -> JumpEvents -> DodgeEvents recorded with current code, including a dash-effect stop comparison. No obvious detached arm fragments in inspected sampled frames. Not reproduced, not claimed fixed. The first current-code trial was invalid because the save was encumbered; the valid retry used a temporary capacity modifier, then removed it. |
| #200 long-session FPS and holsters | Clean-install holster failure reproduced and fixed. No general long-session FPS cause established. |
| #201 missed snap turns | Raw-axis threshold/rearm corrected. Live 0.85 input gives 30 degrees, held input gives no extra turn, 0.50 re-arms. Physical controller hardware remains untested. |
| #210 HUD/settings/REDscript | Sprint modes changed through the VR menu's own CONTROLS edit path apply live: click mode does not sprint on the stick alone and sprints after L3; hold mode sprints on full forward travel. Crouch reproduced: the preset's Alternative scheme maps ToggleCrouch to R3, which the game accepts only as a tap, while the stick-down gesture held R3 level -- holds of 0.25/0.5/1.0 s never crouched, only a 0.1 s flick did. Fixed with one 100 ms R3 tap per push: all four hold lengths crouch, a second push stands. The physical right-stick click stays the reload slide release and does not crouch. HUD panel/depth A/B did not reproduce duplicate objective/chatter in sampled views; full dialogue subtitle reproduction is outstanding. Startup script prompt was a migration/compilation prompt; reviewed log had warnings but no compile errors. |
| #188 item statistics/right cyberware slots | Without Codeware, missing native tooltip library dependency reproduced. Fixed asynchronous loading and owner dependency association. Real Victor Vector dialogue opened the VR ripperdoc: all 97 stock tooltips laid out, cyberdeck stats/price visible, Eyes hover hit-test resolved with its tooltip placed beside it. No purchase was made. |
| Pimax Crystal Light / disable_mouse_y | `vrik_settings.ini` is an output bridge the plugin rewrites from `vrport.ini` `xr_disable_mouse_y`; VRIK Lua reads native `GetVRMouseYDisabled()` and uses the file only when the native function is absent. Live VR-menu edits 0/1/0 switch pitch limits (+/-80 vs 0) with the right controller's tracking and grip active in every state; writing `disable_mouse_y=1` into the file did not change the native value. No interaction profile is suggested specifically for Pimax (Touch, Index, Vive, WMR, simple, Steam Frame are). No Pimax hardware reproduction or confirmed root cause; the new profile diagnostics log what the runtime binds. |
| PR #208 | Adapted numeric HUD-quad filtering before xrEndFrame, including cached extras and final per-eye poses. Intentionally did not adopt a second xrEndFrame retry after failure; one end attempt per successful begin. WARP harness and normal simulator launch passed. No WiVRn-specific validation. |
| Launcher switches | Wider launcher with VR Lobby, VR Interface and Hand physics switches. Native window/input/persistence tests pass. Live interface OFF closes/rejects SAO; physics OFF while carrying releases body and Carrying. Re-enable with grip held does not automatically regrab. Live rail climb at the H10 atrium: hold on, a 15.8 cm self-pull, physics OFF mid-grip dropped the hold and collision mask to 0 and V back to the floor, re-enable with grip held stayed released. Ladder grip climbing ignored the master switch; it is now gated (unit tested, not live on a ladder). Hair shutdown not tested live. |
| Reload magazine highlight | Seven source showMagazine track indices corrected. Actual Crusher magazine removal: additive ghost visible with glow, empty well with glow disabled. |
| Map slowdown | Three clean baseline map cycles, then ten 45 s pan cycles on the restored modded install: world p50 21.96 ms before, 22.18-22.26 ms after each close; map 11.1 ms throughout; private memory +0.9 GB in the first cycle only. No persistent slowdown reproduced. GC robustness improved for menu/pause paths, but it is not established as the user's map-performance fix. |
| Holsters on clean install | Missing Codeware GetComponents method stopped update before grip handling. Replaced with native component enumeration, kept grip edges sampled through suspension. Same-position script-only A/B changed failure to successful Overture draw. |

## Codeware leftovers audit

The port replaced Codeware with its own `VR*` natives (docs/native-frameworks-plan-20261030.md, stage 4); the
holster and tooltip failures were call sites that migration missed. The installed Codeware 1.21.2 scripts give 145
added methods, its own classes and 68 globals; every use of them in `mods/` (421 Lua, 49 redscript files) was
listed and each remaining call checked live on a port-only install (one RED4ext plugin loaded, as in #210's log):

- Missing without Codeware: `Entity:GetComponents()` -- the hands-only mode failed at it twice a second and the body
  stayed drawn; fixed with `Game.VREntityComponents` (30 body components hidden, hand copies on, no errors).
  `inkText:SetContentHAlign()` is missing too, but the mod only calls it on `inkImage`, which has it.
- Present in 2.5 itself: `inkWidget` `GetParentWidget`/`SetUserData`, `inkScrollController:GetScrollPosition`,
  `ItemDisplayContextData:AddTag`, `IsA`/`GetClassName`; `ScriptableService`/`CallbackSystem` (the port's
  redscript compiled with no errors without Codeware). `Game.GetSystemRequestsHandler` is a Lua host alias.
- The port-only session logged no other Lua errors through load and idle at H10. HUD panel/depth A/B in that
  configuration matched the Codeware run; #210's doubling still not reproduced in the sampled view.

## #209 Disasterpiece braindance crash

- Reporter's ReportQueue dump (same 2.5 exe build): write to 0x34 in `Cyberpunk2077.exe+0x18C70C3` on a job worker.
  It is `entTriggerActivatorComponent`'s handler for `enteventsEntityResize` (capsule resize, extents 0.25/0.25/0.9)
  moving its trigger proxy at +0x140, which was null. Owner: a `gamePuppet`; the component in the dump has height 0.5,
  radius 0.1, channels 0x20. No port frame on the crashing thread's stack; neither the port nor game scripts send
  that event (engine native, sub_141861A64 resizes the character capsule and sends it). Streaming was mid-teleport
  into the braindance world (LoadingStage WaitingForUpdate, observer at the braindance position).
- Not reproduced on the reporter's own save (`AutoSave-9`, objective `plug_in`, "[Put on the headset]"): current
  build with the usual frameworks, clean packaged 0.2.3, and 0.2.3 with the head held seated and moving -- the
  braindance loaded each time (`01_analyze_braindance`, braindance active). Physical crouch stayed unrequested
  throughout (vehicle, then braindance), so it is not the resize source here. Simulator only; both reporters use
  real headsets (Pimax/SteamVR, Quest 3 and BSB2 on SteamVR/OpenXR).
- The user's save folder was snapshotted and restored identical (only the foreign save removed, `user.gls`
  reverted); the install was restored from its journal.

## #213 frame-generation pacing phase

- Confirmed in source: with frame generation and adaptive pacing the real-frame release was a free-running timer
  (last release + interval), not aligned to the display cycle the presenter reads at; adaptive pacing stretched it by
  fractions of a period. Now `PhaseLockedPacing` (FramePolicy.hpp): the next Present lands mid-cycle, real frames
  whole periods apart (2, or 3 past a quarter-period stretch), interval fallback without XR cycles.
- Model test (`framegen_tests phase_pacing`, presenter cadence): free-running worst 23.8 % repeated cycles, 5 of 24
  start phases over 5 %; 4 % stretched interval 3.9 %; phase-locked 0.00 % in every case; steady 3-period step and
  slow-game grid also checked. Framegen suite 22/23; `stereo_depth` fails on the existing "foreign producer queue"
  expectation that d27d5ff1 intentionally changed (inputs from another queue are accepted once finished).
- Live on the simulator (framegen on, 10 re-rolls of `xr_framegen_pace` each): the old pacer already shows 0-0.8 %
  there (the simulator does not exhibit the lottery), the new one 0-0.4 %, real 45 / output 90 fps -- no regression,
  not a hardware confirmation. The reporter's GPU-load ceiling (generation sharing the game's queue) is not addressed.

## Ragdoll details

- World orientation now uses the completed physics pose and each hand's own fixed relation, rather than a pre-step common error with an angle limit.
- The controller still drives the body grip point. The drawn wrist yields to the held part's measured orientation; it is not a kinematic ragdoll pin.
- Native body holding bypasses the legacy Lua velocity/angular driver. Query visibility and timed lost-grip release remain active.
- Arm IK no longer moves a held wrist away from that corrected target via a second minimum/maximum forearm-length clamp. The grip shoulder profile applies; free-arm caps are unchanged.
- A ray hitting the torso through an arm may be replaced only by a closer surface from that same ragdoll, with a 1 cm margin.
- Person wraps use the real FPP mesh's sparse thumb/web samples and a palm-pad seat. Non-person toy grasp output is unchanged in the offline comparison.
- Live two-hand test held two different parts of one body (left trunk, right arm); releasing right left the left hold active, then both released. Same-actor two-hand interaction has unit coverage, not a matching live test.
- A smaller constant shin radius was tested and rejected as inconclusive; no radius inflation/shrink trial was retained.
- Earlier relative-body measurements could read the nearest pre-grab body instead of the actual selected body. The final helper reads the actual grasp proxy/body. Do not generalize those older measurements.

## Follow-up: rotation, scanner and SteamVR null

The earlier ragdoll check measured **hand-to-part slip**, not whether a controller turn actually reached the part.
It did not establish usable wrist rotation under load.

- Reproduced on the simulator: a 60-degree controller turn moved the held forearm/wrist only about 8 degrees after
  settling. The drive capped angular acceleration against inertia artificially multiplied by at least 20 for solver
  stability. It now uses original physical inertia for that cap and a damped angular chase. In the fixed live forearm
  run, the wrist turned about 55–71 degrees relative to its starting pose, with 4–11 degrees of remaining controller
  error under changing load. Grip-point separation was approximately 1–5 mm. This is a weighted dynamic hold, not a
  rigid controller pin. The final surface-profile rerun still showed load-dependent error: forearm controller/wrist
  error 7–34 degrees; shin up to 64 degrees in one loaded direction, although the wrist no longer stayed frozen and
  followed the commanded turns. Do not describe this as exact controller orientation. Offline angular
  follow/overshoot/torque checks pass at 45/60/90/120 Hz.
- Visible surface fit is separate: ragdoll collision radii were allowed to enlarge the anatomical grasp profile, and
  dead-body profiles lacked the living NPC body-type scale. Reuse anatomical arm/leg dimensions and body-type scaling,
  and allow collision surfaces to refine only a narrower radius. Judy's forearm trial changed the effective radius
  from about 55 mm to 35 mm, before the unchanged 6 mm finger-wrap allowance. Arbitrary clothing remains approximate;
  this does not claim mesh-exact surface contact for every NPC.
- Scanner reproduced through the actual keyboard `Vision_Toggle_Button`, aimed at Judy: `Vision=1` and
  `focusMode=true`. A 20 cm controller movement left the drawn wrist 185–239 mm behind after 0.4 s. The fix suspends
  hand physics contacts/grabs during scanner mode while raw pose, hand targets and IK keep updating. Both transitions
  invalidate old snapshots and park/rearm the bodies. Fixed live run removes that slowdown-related lag; an extreme
  right-hand reach still hits the same arm-length limit with scanner on and off. Collision mask 111 -> 0 -> 111.
  Source-derived invalidate events did not activate the scanner in this harness; those attempts were not valid repros.
  Final in-reach run: both hands' model-space tracking error below 0.1 mm at all four sampled scanner positions, with
  actual focusMode active. The native collision mask returned after scanner exit; ordinary body/contact limits still apply.
  Entering scanner while carrying released the native/script hold and Carrying state. Closing it with grip still held
  did not regrab (`scanner-held-final/scan-hold.json`). Two-hand independent release also passed on the final build.
- SteamVR null (90 Hz requested, approximately 135–137 `xrWaitFrame` returns/s): the first phase-lock implementation
  regressed real frames from 45.0 fps / p99 26.1 ms to 40.3 fps / p99 30.9 ms. `CycleClock` now rejects that irregular
  clock and uses interval pacing. Fixed 30 s run: 45.0 fps, p50 22.2 ms, p95 25.0 ms, p99 26.4 ms, no >50 ms frames.
  Actual presenter report: 45 real + 45 generated frames/s, 34–35% repeated cycles against the null driver's excessive
  submission cadence. The generic XR repeat counter counts generated frames as same-producer repeats too; use the
  presenter counters to distinguish these. This repairs the regression, **not a verified Steam Link/Quest 3 fix**.
- Inline framegen was inspected, not enabled: it needs conditional midpoint/endpoint pumping, retention of owed real
  endpoints across transient submission failures, and pacing that does not phase-lock to its own inline cycle.
  Merely bypassing the forced-threaded branch is not safe. Current framegen ownership remains unchanged.

Evidence: `rotation-baseline/`, `rotation-drive-fixed/`, `forearm-surface-trial/`, `shin-baseline/`,
`scanner-aim-baseline/`, `scanner-fixed/`, `scanner-final/`, `forearm-final/`, `shin-final/`, `two-hand-final/`,
`steamvr-null/clock-gated.json`, `steamvr-null/gated-cadence/`.

Follow-up Release DLL build and deployment passed; SHA-256:
`2494d4c6b4d3de4caaea5dc69f0a2a630d3f3c6ddbd349f00a957c13de4e61a2`.
SteamVR null configuration and the previous HKLM OpenXR runtime were restored after testing.

## Automated checks

- Earlier main Release DLL build: passed. Its deployed DLL SHA-256 was `0f46a8f7d1e603c006423e9558724b4666c24025f2faef2bc63e6a88d15e24d2`; superseded by the follow-up build above.
- Ladder: 14 passed, including the Hand physics master gate.
- Restored optional-mod installation cold-started successfully, reached H10 and passed stable HUD submission checks. Native cyberware tooltip stats and price were visible with Codeware restored (`final-codeware-tooltip-view_L.jpg`).
- HUD submission WARP test: 1 passed.
- Launcher actual Win32 window/input/persistence: 1 passed.
- Overlay: 18 passed after building all targets. Initial run had 4 Not Run because those executables had not yet been built; no failure was concealed.
- Analog input: 15 passed, including the crouch tap pulse and VRIK bridge test.
- Feature gates Lua: 6 passed.
- Item tooltip Lua: 15 passed; native dependency registration/lifetime test passed.
- Menu GC Lua: 13 passed.
- Holster native-only component/input tests: passed.
- Held pose math: 72 rotation combinations, independent two-hand relations, grip-point preservation, nonfinite rejection, held/free IK pin bounds passed.
- Corpse driver/selection tests: passed. Whole HandCollision Lua source compiled with LuaJIT.
- Grasp real-mesh offline regressions: 35 cases, no assertion failures. All 22 non-person PPM outputs SHA-256 identical to baseline (agent-produced evidence).

## Evidence locations

All live evidence is under `build/goal12-20261111/` unless noted:

- `holster-fixed_L.jpg`
- `crusher-mag-side_L.jpg`, `crusher-mag-no-glow_L.jpg`
- `tooltip-fixed2_L.jpg`
- `launcher/features.jpg`
- `map-baseline/map-results.json`
- `input-live/results.json`
- `dash-fixed4-unencumbered/normal.mp4`, `frames.jpg`, `dashfx-stopped.mp4`
- `hud-ab/` (both-eye panel/depth comparisons)
- `grab-forearm-ik/`, `grab-settled/`, `grab-new-seat/`, `grab-two/`
- `physics-master-hold/`
- `climb-gate/result.json`
- `map-modded-stress/map-results.json`
- `ui-input/results.json`, `ui-input/controls.json`
- `crouch-r3/crouch-matrix.json` (before), `crouch-r3-fixed/crouch-matrix.json` (after)
- `ripper-live-stock/`
- `port-only-probe/` (method probes, hands-only off/on, HUD A/B); isolation journal `port-only-1/journal.json`
- `tools/grasp_tests/out/regression.log` and rendered real-mesh grasp images.

## Commits

- `0c47b82a` — holsters without Codeware and stale grip edges.
- `14b76074` — raw stick snap travel and actual XR profile diagnostics.
- `44386fd9` — seven magazine source rig track indices.
- `98bb672b` — menu/pause Lua collection pacing.
- `b7b4c7c2` — launcher feature gates, native tooltips, guarded HUD submission, ragdoll grip and finger corrections.
- `c16a086b` — ladder grip climbing behind the Hand physics master switch.
- `f050b589` — one R3 tap per right-stick crouch push.
- `2973c6d4` — hands-only body components without Codeware.
- `8872daa9` — frame-generation real frames locked to the display cycle (#213).
- `bdd8accf` — reject irregular XR clocks before phase-locking frame-generation pacing.
- `a284c0e8` — scanner tracking independent of slowed hand physics; corpse angular drive uses original inertia.
- `ebfea442` — corpse surface profiles follow anatomical dimensions/body type, not inflated collision volumes.
